//! `spawn_decision_log_bus_tee` (features `persistence` + `durable`): every
//! trace from `CoalitionService::spawn_with_trace_tap` reaches both the
//! `Decisions` stream of a `FileEventStore` and the durable bus inbox.
//!
//! Container-backed like `tests/durable_integration.rs`: boots SurrealDB via
//! upstream's `sdb::sdb_task`. If the Docker daemon does not answer
//! `GET /_ping` on the socket `sdb_task` connects to, it prints a `SKIP` line
//! and passes; any later boot failure or timeout panics with the error's
//! source chain. A log sink that is not an agent of the bus is refused with
//! `DurableError::UnknownLogSink`, and a producer that is not an agent of the
//! bus, or a bus agent renamed to another bus agent's name, with
//! `DurableError::UnknownProducer`; none spawns a task.
//!
//! Fixture arithmetic (`ThresholdPolicy(AdditiveCalculator, 0, 0)`): an agent
//! with trust `t` and `p` capability bits contributes `50 + 10·p + t`, so the
//! join is taken, the member's leave is declined, and the non-member's leave
//! returns `act == false`, `score == 0.0` without consulting the policy.

use std::fs;
use std::future::{Future, pending, ready};
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinError;
use tokio::time::{Duration, sleep, timeout};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use koalisi::algorithms::{AdditiveCalculator, AgentCapabilities};
use koalisi::core::Executor;
use koalisi::core::config::setup_logging;
use koalisi::decision::{DecisionContext, ThresholdPolicy};
use koalisi::persistence::{
    EventRef, EventStore, FileEventStore, FileStoreConfig, Payload, Record, SequenceNo, StreamId,
    WireDecision, spawn_store_writer,
};
use koalisi::subsystems::coalition_actor::{
    CoalitionService, DecisionKind, DecisionRecord, DecisionTrace,
};
use koalisi::subsystems::durable::{
    DecisionEvent, DurableDecisionBus, DurableError, spawn_decision_log_bus_tee,
};
use koalisi::topology::CoalitionManager;

use surrealdb_live_message::error::{self as slm_error, chain};
use surrealdb_live_message::subsystems::agents::Agent;
use surrealdb_live_message::subsystems::sdb::{self, SurrealDBWrapper};

const PRODUCER: &str = "tee_producer";
const LOG_SINK: &str = "tee_log";
const OUTSIDER: &str = "tee_outsider";
const BOOT_TIMEOUT: Duration = Duration::from_secs(180);
const RECV_TIMEOUT: Duration = Duration::from_secs(10);
const PING_TIMEOUT: Duration = Duration::from_secs(5);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "koalisi-decision-log-bus-tee-{}",
            std::process::id()
        ));
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Domain agent — a valid graph vertex weight and an `AgentCapabilities`.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct Worker {
    id: usize,
    caps: u32,
    trust: u32,
}

impl AgentCapabilities for Worker {
    fn agent_id(&self) -> usize {
        self.id
    }
    fn capabilities(&self) -> u32 {
        self.caps
    }
    fn trust_level(&self) -> u32 {
        self.trust
    }
}

/// The outcome of [`boot_db`].
enum Boot {
    /// SurrealDB is ready.
    Ready,
    /// The Docker daemon did not answer the ping; the reason is attached.
    DockerUnreachable(String),
}

/// The Unix socket that `sdb::sdb_task` connects to: the path of
/// `DOCKER_HOST` when it starts with `unix://`, else `/var/run/docker.sock`
/// (bollard 0.21.1 `Docker::connect_with_unix_defaults`).
fn docker_socket() -> PathBuf {
    let host = std::env::var("DOCKER_HOST")
        .ok()
        .filter(|h| h.starts_with("unix://"));
    let uri = host.as_deref().unwrap_or("unix:///var/run/docker.sock");
    PathBuf::from(uri.replacen("unix://", "", 1))
}

/// `Ok` iff the daemon on `socket` answers `GET /_ping` with status 200 and
/// body `OK` within `PING_TIMEOUT` per read or write; otherwise the reason.
fn docker_ping(socket: &Path) -> Result<(), String> {
    let at = socket.display();
    let mut stream = UnixStream::connect(socket).map_err(|e| format!("connect {at}: {e}"))?;
    stream
        .set_read_timeout(Some(PING_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(PING_TIMEOUT)))
        .map_err(|e| format!("set timeouts on {at}: {e}"))?;
    stream
        .write_all(b"GET /_ping HTTP/1.0\r\nHost: docker\r\n\r\n")
        .map_err(|e| format!("write GET /_ping to {at}: {e}"))?;
    let mut reply = String::new();
    stream
        .read_to_string(&mut reply)
        .map_err(|e| format!("read the /_ping reply from {at}: {e}"))?;
    let status_ok = reply
        .lines()
        .next()
        .is_some_and(|line| line.split_whitespace().nth(1) == Some("200"));
    let body_ok = reply.split_once("\r\n\r\n").map(|(_, body)| body) == Some("OK");
    if status_ok && body_ok {
        Ok(())
    } else {
        Err(format!("GET /_ping on {at} answered {reply:?}"))
    }
}

/// Boot the DB under a child token. Returns `Boot::DockerUnreachable` iff
/// [`docker_ping`] fails on [`docker_socket`]; otherwise returns
/// `Boot::Ready` once SurrealDB is ready, and panics as [`await_boot`] does.
async fn boot_db(tracker: &TaskTracker, token: &CancellationToken) -> Boot {
    if let Err(reason) = docker_ping(&docker_socket()) {
        return Boot::DockerUnreachable(reason);
    }
    let sdb = tracker.spawn(sdb::sdb_task(token.child_token()));
    await_boot(SurrealDBWrapper::wait_until_ready(), sdb, BOOT_TIMEOUT).await;
    Boot::Ready
}

/// Return once `ready` resolves `Ok`. Panic, with the error's source chain,
/// when `ready` resolves `Err`, when `sdb` (the `sdb_task` handle) ends first
/// with any outcome, or when neither resolves within `limit`.
async fn await_boot<R, S>(ready: R, sdb: S, limit: Duration)
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

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn every_trace_reaches_the_decisions_stream_and_the_bus() {
    setup_logging();

    let token = CancellationToken::new();
    let db_tracker = TaskTracker::new();

    if let Boot::DockerUnreachable(reason) = boot_db(&db_tracker, &token).await {
        eprintln!("SKIP every_trace_reaches_the_decisions_stream_and_the_bus: {reason}");
        token.cancel();
        db_tracker.close();
        db_tracker.wait().await;
        return;
    }
    // Cancels the sdb token on every exit from here, a panic included.
    let _sdb_guard = token.clone().drop_guard();

    let bus = DurableDecisionBus::new(vec![PRODUCER.to_string(), LOG_SINK.to_string()])
        .await
        .expect("durable bus");
    let producer = bus.agent(PRODUCER).await.expect("producer agent");
    let inbox = bus.coalition().inbox();

    let tmp = TempDir::new();
    let store: Arc<dyn EventStore> =
        Arc::new(FileEventStore::open(tmp.path(), FileStoreConfig::default()).unwrap());
    let pipe_tracker = TaskTracker::new();
    let (writer_tx, writer_rx) = mpsc::channel::<(StreamId, Record)>(16);
    spawn_store_writer(
        Arc::clone(&store),
        writer_rx,
        &pipe_tracker,
        token.child_token(),
    );
    // A producer or a log sink that is not an agent of the bus is refused, and
    // nothing is spawned.
    let outsider = Agent::new(OUTSIDER).await.expect("outsider agent");
    // The bus's tee_log agent renamed to tee_producer: a bus agent has the
    // name, but its record id is agent:tee_log, not agent:tee_producer.
    let mut impostor = bus.agent(LOG_SINK).await.expect("tee_log agent");
    impostor.name = PRODUCER.to_string();
    for (producer, log_sink, want) in [
        (
            producer.clone(),
            "no_such_sink",
            DurableError::UnknownLogSink {
                log_sink: "no_such_sink".to_string(),
            },
        ),
        (
            outsider,
            LOG_SINK,
            DurableError::UnknownProducer {
                producer: OUTSIDER.to_string(),
            },
        ),
        (
            impostor,
            LOG_SINK,
            DurableError::UnknownProducer {
                producer: PRODUCER.to_string(),
            },
        ),
    ] {
        let refuse_tracker = TaskTracker::new();
        let (_unused_tx, unused_rx) = mpsc::channel::<DecisionTrace>(1);
        let refused = spawn_decision_log_bus_tee(
            &bus,
            unused_rx,
            writer_tx.clone(),
            producer,
            log_sink.to_string(),
            &refuse_tracker,
            token.child_token(),
        )
        .await;
        let spawned = refuse_tracker.len();
        assert_eq!(
            refused.as_ref().err(),
            Some(&want),
            "{want}: must be refused; got Ok: {}",
            refused.is_ok()
        );
        assert_eq!(spawned, 0, "{want}: a refused spawn left {spawned} task(s)");
        refuse_tracker.close();
    }

    let (trace_tx, trace_rx) = mpsc::channel::<DecisionTrace>(16);
    spawn_decision_log_bus_tee(
        &bus,
        trace_rx,
        writer_tx,
        producer,
        LOG_SINK.to_string(),
        &pipe_tracker,
        token.child_token(),
    )
    .await
    .expect("tee_log is an agent of the bus");

    // Setup: 4 events (3 agents + the coalition) ⇒ log length 4, clock 4.
    let manager =
        CoalitionManager::<Worker, ()>::empty(Arc::new(Executor::with_num_threads(2).unwrap()));
    let s = manager
        .add_agent(Worker {
            id: 0,
            caps: 0b001,
            trust: 50,
        })
        .await
        .unwrap();
    let j1 = manager
        .add_agent(Worker {
            id: 1,
            caps: 0b010,
            trust: 50,
        })
        .await
        .unwrap();
    let j2 = manager
        .add_agent(Worker {
            id: 2,
            caps: 0b100,
            trust: 50,
        })
        .await
        .unwrap();
    let c = manager.form_coalition(vec![s], ()).await.unwrap();

    let service = CoalitionService::spawn_with_trace_tap(
        manager,
        Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
        DecisionContext::default(),
        trace_tx,
        &pipe_tracker,
        token.child_token(),
    );

    // Hand-derived (record, Topology parent seq):
    //   join j1   pos (4, 4): +110 ⇒ join (event 4)  → parent 3
    //   leave j1  pos (5, 5): staying +110 ⇒ stay    → parent 4
    //   leave j2  pos (5, 5): non-member ⇒ act false, score 0 → parent 4
    let label = format!("coalition-{}", usize::from(c));
    let record = |agent, kind, act, score| DecisionRecord {
        coalition: label.clone(),
        agent_id: usize::from(agent),
        kind,
        act,
        score,
    };
    let expected = [
        (record(j1, DecisionKind::Join, true, 110.0), 3),
        (record(j1, DecisionKind::Leave, false, 110.0), 4),
        (record(j2, DecisionKind::Leave, false, 0.0), 4),
    ];

    service.join(j1, c).await.unwrap();
    service.leave(j1, c).await.unwrap();
    service.leave(j2, c).await.unwrap();

    // Lossless drain: the service exits on the dropped handle, dropping the
    // trace tap; the tee drains (log, then bus, per trace), then the writer.
    drop(service);
    pipe_tracker.close();
    pipe_tracker.wait().await;

    // --- the Decisions stream ---
    let read_store = Arc::clone(&store);
    let decisions = tokio::task::spawn_blocking(move || {
        read_store.read_from(StreamId::Decisions, SequenceNo(0), usize::MAX)
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(decisions.len(), expected.len(), "one record per trace");
    for (i, (stored, (want, parent))) in decisions.iter().zip(&expected).enumerate() {
        let Payload::Plain(bytes) = &stored.record.payload else {
            panic!("d{i}: expected a Plain payload");
        };
        let wire: WireDecision = ciborium::from_reader(bytes.as_slice()).unwrap();
        assert_eq!(&wire.try_into_record().unwrap(), want, "d{i}: payload");
        assert_eq!(
            stored.record.parents,
            vec![EventRef {
                stream: StreamId::Topology,
                seq: SequenceNo(*parent),
            }],
            "d{i}: parent"
        );
    }

    // --- the bus inbox ---
    let mut seen: Vec<DecisionEvent> = Vec::new();
    for i in 0..expected.len() {
        let delivery = timeout(RECV_TIMEOUT, inbox.recv())
            .await
            .unwrap_or_else(|_| panic!("bus delivery {i} of {} timed out", expected.len()))
            .expect("inbox bus closed unexpectedly");
        assert_eq!(delivery.recipient, LOG_SINK, "addressed to the log sink");
        seen.push(delivery.message.payload);
    }
    let mut want: Vec<DecisionEvent> = expected
        .iter()
        .map(|(r, _)| DecisionEvent::from(r.clone()))
        .collect();
    want.sort_by_key(|e| (e.agent_id, e.kind.clone()));
    seen.sort_by_key(|e| (e.agent_id, e.kind.clone()));
    assert_eq!(seen, want, "every trace published to the bus");

    bus.shutdown().await;
    token.cancel();
    db_tracker.close();
    db_tracker.wait().await;
}

/// A socket path under the temp dir, unique to this process and `tag`.
fn temp_socket(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "koalisi-tee-ping-{tag}-{}.sock",
        std::process::id()
    ))
}

/// Serve one connection on `socket` with `reply`, then remove the socket.
fn serve_once(socket: &Path, reply: &'static str) -> std::thread::JoinHandle<()> {
    let _ = fs::remove_file(socket);
    let listener = UnixListener::bind(socket).expect("bind the test socket");
    let path = socket.to_path_buf();
    std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().expect("accept the ping");
        let mut request = [0u8; 256];
        let _ = conn.read(&mut request).expect("read the ping");
        conn.write_all(reply.as_bytes()).expect("write the reply");
        drop(conn);
        let _ = fs::remove_file(path);
    })
}

#[test]
fn docker_ping_fails_on_a_socket_with_no_listener() {
    let socket = temp_socket("absent");
    let _ = fs::remove_file(&socket);
    let got = docker_ping(&socket);
    let want_prefix = format!("connect {}: ", socket.display());
    assert!(
        got.as_ref()
            .is_err_and(|reason| reason.starts_with(&want_prefix)),
        "expected Err starting with {want_prefix:?}; got {got:?}"
    );
}

#[test]
fn docker_ping_passes_on_a_200_ok_answer() {
    let socket = temp_socket("ok");
    let server = serve_once(&socket, "HTTP/1.0 200 OK\r\nContent-Length: 2\r\n\r\nOK");
    let got = docker_ping(&socket);
    server.join().expect("server thread");
    assert_eq!(got, Ok(()));
}

#[test]
fn docker_ping_fails_on_a_500_answer() {
    let socket = temp_socket("err");
    let server = serve_once(&socket, "HTTP/1.0 500 Internal Server Error\r\n\r\nno");
    let got = docker_ping(&socket);
    server.join().expect("server thread");
    let want_prefix = format!("GET /_ping on {} answered ", socket.display());
    assert!(
        got.as_ref()
            .is_err_and(|reason| reason.starts_with(&want_prefix)),
        "expected Err starting with {want_prefix:?}; got {got:?}"
    );
}

#[tokio::test]
#[should_panic(expected = "sdb_task failed: SurrealDB health check failed after all attempts")]
async fn await_boot_panics_when_sdb_task_fails() {
    await_boot(
        pending(),
        ready(Ok(Err(slm_error::Error::HealthCheck))),
        BOOT_TIMEOUT,
    )
    .await;
}

#[tokio::test]
#[should_panic(expected = "panicked with message \"container create failed\"")]
async fn await_boot_panics_when_sdb_task_panics() {
    async fn panicking_sdb_task() -> slm_error::Result<()> {
        panic!("container create failed")
    }
    let sdb = tokio::spawn(panicking_sdb_task());
    await_boot(pending(), sdb, BOOT_TIMEOUT).await;
}

#[tokio::test]
#[should_panic(expected = "SurrealDB not ready within 10ms")]
async fn await_boot_panics_on_timeout() {
    await_boot(
        pending(),
        pending::<Result<slm_error::Result<()>, JoinError>>(),
        Duration::from_millis(10),
    )
    .await;
}
