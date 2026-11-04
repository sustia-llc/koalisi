//! Boot and stop of the SurrealDB container, through upstream
//! `sdb::sdb_task`, for the `durable` container tests and the
//! `durable_decisions` example.
//!
//! [`SdbGuard`] runs `sdb_task` on a runtime of its own. Dropping the guard,
//! during a panic or on an early return, cancels the task's token and blocks
//! until the task returns, which is after it stops the container
//! (`surrealdb-live-message` `f60eeca` `src/subsystems/sdb.rs:175-183`), or
//! until [`STOP_TIMEOUT`] elapses.
//!
//! A boot that is not ready within [`BOOT_TIMEOUT`] is outside this: before
//! it reports ready, `sdb_task` does not read its token
//! (`surrealdb-live-message` `f60eeca` `src/subsystems/sdb.rs:150-175`), so
//! the guard's wait ends at [`STOP_TIMEOUT`], the in-flight task is dropped
//! with the runtime, and a container it created can keep running.

#![allow(dead_code)]

use std::future::Future;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use tokio::runtime::{Builder, Runtime};
use tokio::task::{JoinError, JoinHandle};
use tokio::time::{Duration, sleep, timeout};
use tokio_util::sync::CancellationToken;

use surrealdb_live_message::error::{self as slm_error, chain};
use surrealdb_live_message::settings::SETTINGS;
use surrealdb_live_message::subsystems::sdb::{self, SurrealDBWrapper};

pub const BOOT_TIMEOUT: Duration = Duration::from_secs(180);
pub const PING_TIMEOUT: Duration = Duration::from_secs(5);
/// The longest [`SdbGuard`] waits for `sdb_task` to return after the cancel.
pub const STOP_TIMEOUT: Duration = Duration::from_secs(60);

/// `sdb::sdb_task` on a one-worker runtime owned by the guard.
///
/// [`SdbGuard::stop`] and `Drop` cancel the task's token and wait up to
/// [`STOP_TIMEOUT`] for the task to return; `Drop` waits on a thread of its
/// own.
pub struct SdbGuard {
    token: CancellationToken,
    runtime: Option<Runtime>,
    task: Option<JoinHandle<slm_error::Result<()>>>,
}

impl SdbGuard {
    /// Spawn `sdb::sdb_task(token)` on a new runtime.
    pub fn spawn(token: CancellationToken) -> std::io::Result<Self> {
        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("sdb")
            .enable_all()
            .build()?;
        let task = runtime.spawn(sdb::sdb_task(token.clone()));
        Ok(Self {
            token,
            runtime: Some(runtime),
            task: Some(task),
        })
    }

    /// Resolve with the outcome of `sdb_task` once it returns or panics.
    /// After it resolves, the guard has no task left to wait for.
    pub async fn ended(&mut self) -> Result<slm_error::Result<()>, JoinError> {
        let task = self
            .task
            .as_mut()
            .expect("invariant: `ended` resolves at most once per guard");
        let outcome = task.await;
        self.task = None;
        outcome
    }

    /// Cancel the token and wait for `sdb_task` to return. `Err` names the
    /// task's error or panic, or a wait longer than [`STOP_TIMEOUT`].
    pub async fn stop(mut self) -> Result<(), String> {
        self.token.cancel();
        let outcome = match self.task.take() {
            Some(task) => describe(timeout(STOP_TIMEOUT, task).await),
            None => Err("sdb_task had already ended".to_string()),
        };
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
        outcome
    }
}

impl Drop for SdbGuard {
    fn drop(&mut self) {
        self.token.cancel();
        let Some(runtime) = self.runtime.take() else {
            return;
        };
        let task = self.task.take();
        let waiter = std::thread::spawn(move || {
            let outcome = task.map(|task| {
                describe(runtime.block_on(async move { timeout(STOP_TIMEOUT, task).await }))
            });
            drop(runtime);
            outcome
        });
        match waiter.join() {
            Ok(Some(Err(reason))) => eprintln!("SdbGuard drop: {reason}"),
            Ok(_) => {}
            Err(_) => eprintln!("SdbGuard drop: the thread waiting for sdb_task panicked"),
        }
    }
}

/// `Ok` iff `sdb_task` returned `Ok` within the wait; otherwise the reason.
fn describe(
    waited: Result<Result<slm_error::Result<()>, JoinError>, tokio::time::error::Elapsed>,
) -> Result<(), String> {
    match waited {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(e))) => Err(format!("sdb_task failed: {}", chain(&e))),
        Ok(Err(e)) => Err(format!("sdb_task did not finish: {}", chain(&e))),
        Err(_) => Err(format!("sdb_task did not return within {STOP_TIMEOUT:?}")),
    }
}

/// The outcome of [`boot_db`], or of [`boot_db_on`] with a `G`-valued boot.
pub enum Boot<G = SdbGuard> {
    /// The boot returned; for [`boot_db`], SurrealDB is ready and dropping
    /// the guard stops it.
    Ready(G),
    /// The Docker daemon did not answer the ping; the reason is attached.
    DockerUnreachable(String),
}

/// The Unix socket that `sdb::sdb_task` connects to: the path of
/// `DOCKER_HOST` when it starts with `unix://`, else `/var/run/docker.sock`
/// (bollard 0.21.1 `Docker::connect_with_unix_defaults`).
pub fn docker_socket() -> PathBuf {
    let host = std::env::var("DOCKER_HOST")
        .ok()
        .filter(|h| h.starts_with("unix://"));
    let uri = host.as_deref().unwrap_or("unix:///var/run/docker.sock");
    PathBuf::from(uri.replacen("unix://", "", 1))
}

/// The raw HTTP/1.0 reply of the daemon on `socket` to `GET path`, with
/// [`PING_TIMEOUT`] on each read or write; otherwise the reason.
fn docker_get(socket: &Path, path: &str) -> Result<String, String> {
    let at = socket.display();
    let mut stream = UnixStream::connect(socket).map_err(|e| format!("connect {at}: {e}"))?;
    stream
        .set_read_timeout(Some(PING_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(PING_TIMEOUT)))
        .map_err(|e| format!("set timeouts on {at}: {e}"))?;
    stream
        .write_all(format!("GET {path} HTTP/1.0\r\nHost: docker\r\n\r\n").as_bytes())
        .map_err(|e| format!("write GET {path} to {at}: {e}"))?;
    let mut reply = String::new();
    stream
        .read_to_string(&mut reply)
        .map_err(|e| format!("read the {path} reply from {at}: {e}"))?;
    Ok(reply)
}

/// The status code and body of an HTTP reply.
fn status_and_body(reply: &str) -> (Option<&str>, Option<&str>) {
    let status = reply
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1));
    let body = reply.split_once("\r\n\r\n").map(|(_, body)| body);
    (status, body)
}

/// `Ok` iff the daemon on `socket` answers `GET /_ping` with status 200 and
/// body `OK` within `PING_TIMEOUT` per read or write; otherwise the reason.
pub fn docker_ping(socket: &Path) -> Result<(), String> {
    let reply = docker_get(socket, "/_ping")?;
    if status_and_body(&reply) == (Some("200"), Some("OK")) {
        Ok(())
    } else {
        Err(format!(
            "GET /_ping on {} answered {reply:?}",
            socket.display()
        ))
    }
}

/// A socket path under the temp dir, unique to this process and `tag`.
pub fn temp_socket(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("koalisi-ping-{tag}-{}.sock", std::process::id()))
}

/// Serve one connection on `socket` with `reply`, then remove the socket.
pub fn serve_once(socket: &Path, reply: &'static str) -> std::thread::JoinHandle<()> {
    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket).expect("bind the test socket");
    let path = socket.to_path_buf();
    std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().expect("accept the ping");
        let mut request = [0u8; 256];
        let _ = conn.read(&mut request).expect("read the ping");
        conn.write_all(reply.as_bytes()).expect("write the reply");
        drop(conn);
        let _ = std::fs::remove_file(path);
    })
}

/// The JSON body of `GET /containers/json` on `socket` for the containers
/// whose name matches `SETTINGS.sdb.container_name`: running ones only, or
/// with `all`, stopped ones too (the `docker ps [-a] --filter name=` list).
pub fn sdb_containers(socket: &Path, all: bool) -> Result<String, String> {
    let filters = format!("{{\"name\":[\"{}\"]}}", SETTINGS.sdb.container_name);
    let encoded: String = filters
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    let path = format!("/containers/json?all={all}&filters={encoded}");
    let reply = docker_get(socket, &path)?;
    match status_and_body(&reply) {
        (Some("200"), Some(body)) => Ok(body.trim().to_string()),
        _ => Err(format!(
            "GET {path} on {} answered {reply:?}",
            socket.display()
        )),
    }
}

/// `Ok` with no request when `environment` is `production`; otherwise
/// [`docker_ping`] on `socket`.
pub fn docker_check(environment: &str, socket: &Path) -> Result<(), String> {
    if environment == "production" {
        return Ok(());
    }
    docker_ping(socket)
}

/// Boot the DB under a child of `token`. Returns `Boot::DockerUnreachable`
/// iff [`docker_check`] of `SETTINGS.environment` on [`docker_socket`]
/// fails; otherwise returns `Boot::Ready` once SurrealDB is ready, and panics
/// as [`await_boot`] does, after the guard has stopped the task.
pub async fn boot_db(token: &CancellationToken) -> Boot {
    boot_db_on(&SETTINGS.environment, &docker_socket(), || async {
        let mut sdb = SdbGuard::spawn(token.child_token()).expect("build the sdb runtime");
        await_boot(
            SurrealDBWrapper::wait_until_ready(),
            sdb.ended(),
            BOOT_TIMEOUT,
        )
        .await;
        sdb
    })
    .await
}

/// Returns `Boot::DockerUnreachable` iff [`docker_check`] of `environment`
/// on `socket` fails, without calling `boot`; otherwise returns
/// `Boot::Ready` with the output of `boot`.
pub async fn boot_db_on<G, B, F>(environment: &str, socket: &Path, boot: B) -> Boot<G>
where
    B: FnOnce() -> F,
    F: Future<Output = G>,
{
    if let Err(reason) = docker_check(environment, socket) {
        return Boot::DockerUnreachable(reason);
    }
    Boot::Ready(boot().await)
}

/// Return once `ready` resolves `Ok`. Panic, with the error's source chain,
/// when `ready` resolves `Err`, when `sdb` (the `sdb_task` outcome) resolves
/// first with any outcome, or when neither resolves within `limit`.
pub async fn await_boot<R, S>(ready: R, sdb: S, limit: Duration)
where
    R: Future<Output = slm_error::Result<()>>,
    S: Future<Output = Result<slm_error::Result<()>, JoinError>>,
{
    tokio::select! {
        r = ready => {
            if let Err(e) = r {
                panic!("SurrealDB ready signal failed: {}", chain(&e));
            }
        }
        r = sdb => match r {
            Ok(Ok(())) => panic!("sdb_task returned before SurrealDB was ready"),
            Ok(Err(e)) => panic!("sdb_task failed: {}", chain(&e)),
            Err(e) => panic!("sdb_task did not finish: {}", chain(&e)),
        },
        () = sleep(limit) => panic!("SurrealDB not ready within {limit:?}"),
    }
}
