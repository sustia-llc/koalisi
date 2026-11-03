//! Durable coalition decision messaging survives restart — through koalisi's
//! seam. (feature `durable`)
//!
//! Modeled on the upstream `scenario_durable_restart` proof, but driven through
//! koalisi's `CoalitionService` decision tap + `spawn_decision_forwarder`:
//!
//! 1. Boot SurrealDB (upstream container harness). If the Docker daemon does
//!    not answer `GET /_ping` on the socket `sdb_task` connects to, print a
//!    `SKIP` line and pass. Any later boot failure or timeout panics with the
//!    error's source chain.
//! 2. Round 1: stand up the durable bus (`recorder` + `decision_log`) so
//!    `decision_log` snapshots a "from-now" cursor against the empty log, then
//!    shut it down — `decision_log` is now OFFLINE, cursor persisted.
//! 3. Drive TWO policy-gated joins through the service while `decision_log` is
//!    down; the tap → forwarder publishes both as durable edges with no live
//!    subscriber. Drain the tap (drop the service, await the forwarder) so both
//!    are durably written.
//! 4. Round 2: recreate the bus. `decision_log` resumes from its persisted
//!    cursor and the `SHOW CHANGES` catch-up replays BOTH events — the property
//!    plain at-most-once `LIVE` lacks.
//! 5. A log sink that is not an agent of the bus is refused with
//!    `DurableError::UnknownLogSink`, and a producer that is not an agent of
//!    the bus, or a bus agent renamed to another bus agent's name, with
//!    `DurableError::UnknownProducer`; none spawns a task.
//! 6. On a separate bus, two forwarded sends fail: a join whose `RELATE`
//!    fails a field assertion, then a leave whose sink `agent` record is
//!    deleted after the sink check. Each failure is logged once at error
//!    level with the decision's fields and the error's source chain.
//! 7. A forwarder whose token is cancelled forwards the records already
//!    buffered in its receiver, then stops while the sender is still open.
//! 8. A bus from `DurableDecisionBus::new_with_parent` stops its agents when
//!    the parent token is cancelled: its inbox closes.
//!
//! The hot path never touches the DB; only the *decisions* are forwarded.

mod common;

use std::collections::HashMap;
use std::future::{Future, pending, ready};
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use tokio::sync::mpsc;
use tokio::task::JoinError;
use tokio::time::{Duration, sleep, timeout};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::Level;

use common::log_capture::Capture;
use koalisi::algorithms::{AdditiveCalculator, AgentCapabilities};
use koalisi::core::config::setup_logging;
use koalisi::decision::{DecisionContext, ThresholdPolicy};
use koalisi::subsystems::coalition_actor::{CoalitionService, DecisionKind, DecisionRecord};
use koalisi::subsystems::durable::{DurableDecisionBus, DurableError, spawn_decision_forwarder};
use koalisi::topology::CoalitionManager;

use surrealdb_live_message::error::{self as slm_error, chain};
use surrealdb_live_message::subsystems::agents::Agent;
use surrealdb_live_message::subsystems::sdb::{self, SurrealDBWrapper};

const RECORDER: &str = "recorder";
const DECISION_LOG: &str = "decision_log";
const OUTSIDER: &str = "outsider";
const BOOT_TIMEOUT: Duration = Duration::from_secs(180);
const RECV_TIMEOUT: Duration = Duration::from_secs(10);
const PING_TIMEOUT: Duration = Duration::from_secs(5);

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
async fn durable_decisions_survive_restart_through_the_seam() {
    setup_logging();

    let token = CancellationToken::new();
    let tracker = TaskTracker::new();

    if let Boot::DockerUnreachable(reason) = boot_db(&tracker, &token).await {
        eprintln!("SKIP durable_decisions_survive_restart_through_the_seam: {reason}");
        token.cancel();
        tracker.close();
        tracker.wait().await;
        return;
    }
    // Cancels the sdb token on every exit from here, a panic included.
    let _sdb_guard = token.clone().drop_guard();

    // --- Round 1: pin decision_log's "from-now" cursor against an empty log,
    //     then take it offline. recorder's Agent handle stays usable for sends
    //     (send only needs the global connection). ---
    let bus1 = DurableDecisionBus::new(vec![RECORDER.to_string(), DECISION_LOG.to_string()])
        .await
        .expect("round-1 durable bus");
    let recorder = bus1.agent(RECORDER).await.expect("recorder agent");
    bus1.shutdown().await; // decision_log offline; cursor persisted at start

    // --- koalisi service with a decision tap + forwarder (recorder → decision_log). ---
    let manager = CoalitionManager::<Worker, ()>::empty(common::executor());
    let seed = manager
        .add_agent(Worker {
            id: 2,
            caps: 0b010,
            trust: 90,
        })
        .await
        .expect("add seed");
    let coalition = manager.form_coalition(vec![seed], ()).await.expect("form");
    let c1 = manager
        .add_agent(Worker {
            id: 1,
            caps: 0b001,
            trust: 80,
        })
        .await
        .expect("add c1");
    let c2 = manager
        .add_agent(Worker {
            id: 3,
            caps: 0b100,
            trust: 70,
        })
        .await
        .expect("add c2");

    let (tap_tx, tap_rx) = tokio::sync::mpsc::channel(64);
    let service = CoalitionService::spawn_with_tap(
        manager,
        Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
        DecisionContext::default(),
        tap_tx,
        &tracker,
        token.child_token(),
    );
    let forwarder = spawn_decision_forwarder(
        &bus1,
        recorder,
        DECISION_LOG.to_string(),
        tap_rx,
        &tracker,
        token.child_token(),
    )
    .await
    .expect("decision_log is an agent of the bus");

    // --- Drive two joins while decision_log is DOWN. Each `join().await`
    //     returns only after the tap emit, so both records are queued. ---
    let d1 = service.join(c1, coalition).await.expect("join c1");
    let d2 = service.join(c2, coalition).await.expect("join c2");
    assert!(d1.act && d2.act, "positive marginal ⇒ both join");

    // Drop the service so its tap Sender closes; the forwarder drains the two
    // buffered records, publishes them durably, then exits on the closed rx.
    // Awaiting the forwarder guarantees both durable edges are written before
    // the restart.
    drop(service);
    forwarder.await.expect("forwarder join");

    // --- Round 2: restart. decision_log resumes from its cursor and the
    //     catch-up replays BOTH events. ---
    let bus2 = DurableDecisionBus::new(vec![RECORDER.to_string(), DECISION_LOG.to_string()])
        .await
        .expect("round-2 durable bus (restart)");
    let inbox = bus2.coalition().inbox();

    let mut seen: HashMap<u64, (String, bool)> = HashMap::new();
    for _ in 0..2 {
        let d = timeout(RECV_TIMEOUT, inbox.recv())
            .await
            .expect("durable replay timed out — a decision sent while down was lost")
            .expect("inbox bus closed unexpectedly");
        assert_eq!(
            d.recipient, DECISION_LOG,
            "events are addressed to the log sink"
        );
        let e = &d.message.payload;
        seen.insert(e.agent_id, (e.kind.clone(), e.act));
    }

    // Both decisions replayed, keyed by the topology vertex index (usize→u64).
    let c1_id = usize::from(c1) as u64;
    let c2_id = usize::from(c2) as u64;
    assert_eq!(
        seen.get(&c1_id),
        Some(&("join".to_string(), true)),
        "c1's join decision must be durably replayed"
    );
    assert_eq!(
        seen.get(&c2_id),
        Some(&("join".to_string(), true)),
        "c2's join decision must be durably replayed"
    );

    // --- A producer or a log sink that is not an agent of the bus is refused,
    //     and nothing is spawned. ---
    let recorder2 = bus2.agent(RECORDER).await.expect("recorder agent");
    let outsider = Agent::new(OUTSIDER).await.expect("outsider agent");
    // The bus's decision_log agent renamed to recorder: a bus agent has the
    // name, but its record id is agent:decision_log, not agent:recorder.
    let mut impostor = bus2.agent(DECISION_LOG).await.expect("decision_log agent");
    impostor.name = RECORDER.to_string();
    for (producer, log_sink, want) in [
        (
            recorder2,
            "no_such_sink",
            DurableError::UnknownLogSink {
                log_sink: "no_such_sink".to_string(),
            },
        ),
        (
            outsider,
            DECISION_LOG,
            DurableError::UnknownProducer {
                producer: OUTSIDER.to_string(),
            },
        ),
        (
            impostor,
            DECISION_LOG,
            DurableError::UnknownProducer {
                producer: RECORDER.to_string(),
            },
        ),
    ] {
        let refuse_tracker = TaskTracker::new();
        let (_unused_tx, unused_rx) = mpsc::channel(1);
        let refused = spawn_decision_forwarder(
            &bus2,
            producer,
            log_sink.to_string(),
            unused_rx,
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

    bus2.shutdown().await;

    failed_send_logs_at_error(&tracker, &token).await;
    cancelled_forwarder_drains_buffered_records(&tracker, &token).await;
    parented_bus_stops_when_parent_is_cancelled(&token).await;

    token.cancel();
    tracker.close();
    tracker.wait().await;
}

const FAIL_PRODUCER: &str = "fail_producer";
const FAIL_SINK: &str = "fail_sink";
const FAILED_SEND_MESSAGE: &str =
    "durable decision forward failed; the decision may be missing from the log";
/// Makes every `RELATE` into `message` fail its field assertion.
const FAIL_CREATED: &str = "DEFINE FIELD OVERWRITE created ON message TYPE datetime ASSERT false;";
/// The `created` field as upstream `define_schema` defines it.
const RESTORE_CREATED: &str = "DEFINE FIELD OVERWRITE created ON message TYPE datetime;";

/// Run one SurrealQL statement on the upstream connection.
async fn run_query(query: &str) {
    SurrealDBWrapper::connection()
        .await
        .query(query)
        .await
        .and_then(|r| r.check())
        .unwrap_or_else(|e| panic!("query {query:?} failed: {e}"));
}

/// Wait until `capture` holds `n` events with message `FAILED_SEND_MESSAGE`.
async fn wait_for_failed_sends(capture: &Capture, n: usize) {
    timeout(RECV_TIMEOUT, async {
        while capture.matching(FAILED_SEND_MESSAGE).len() < n {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "timed out waiting for {n} failed-send log(s); saw {:?}",
            capture.matching(FAILED_SEND_MESSAGE)
        )
    });
}

/// The forwarder logs each failed `Agent::send` once, at error level, with the
/// decision's fields and the error's source chain. The first send (a join)
/// fails in the `RELATE`, because the `created` field of `message` is
/// redefined with `ASSERT false` (upstream `Agent::send` returns `Error::Send`,
/// whose source is the database error). The second send (a leave) fails
/// because the sink's `agent` record is deleted after the sink check passed
/// (upstream `Agent::send` returns `UnknownRecipient`, which has no source).
async fn failed_send_logs_at_error(tracker: &TaskTracker, token: &CancellationToken) {
    let bus = DurableDecisionBus::new(vec![FAIL_PRODUCER.to_string(), FAIL_SINK.to_string()])
        .await
        .expect("failure bus");
    let producer = bus.agent(FAIL_PRODUCER).await.expect("producer agent");
    let (tx, rx) = mpsc::channel(4);
    let forwarder = spawn_decision_forwarder(
        &bus,
        producer,
        FAIL_SINK.to_string(),
        rx,
        tracker,
        token.child_token(),
    )
    .await
    .expect("fail_sink is an agent of the bus");

    let capture = Capture::default();
    let guard = capture.install();

    run_query(FAIL_CREATED).await;
    tx.send(DecisionRecord {
        coalition: "coalition-7".to_string(),
        agent_id: 10,
        kind: DecisionKind::Join,
        act: true,
        score: 1.0,
    })
    .await
    .expect("queue the join");
    wait_for_failed_sends(&capture, 1).await;
    run_query(RESTORE_CREATED).await;

    run_query(&format!("DELETE agent:{FAIL_SINK}")).await;
    tx.send(DecisionRecord {
        coalition: "coalition-7".to_string(),
        agent_id: 11,
        kind: DecisionKind::Leave,
        act: false,
        score: 0.0,
    })
    .await
    .expect("queue the leave");
    drop(tx);
    forwarder.await.expect("forwarder join");
    drop(guard);

    let events = capture.matching("durable decision forward");
    let messages: Vec<Option<&str>> = events
        .iter()
        .map(|e| e.fields.get("message").map(String::as_str))
        .collect();
    assert_eq!(
        messages,
        vec![Some(FAILED_SEND_MESSAGE); 2],
        "two failed-send logs; events {events:?}"
    );

    let sourced_prefix = format!("failed to send message to '{FAIL_SINK}': ");
    for (e, agent_id, kind) in [(&events[0], "10", "join"), (&events[1], "11", "leave")] {
        assert_eq!(e.level, Level::ERROR, "{kind}: level was {}", e.level);
        for (field, want) in [
            ("coalition", "coalition-7"),
            ("agent_id", agent_id),
            ("kind", kind),
            ("to", FAIL_SINK),
        ] {
            assert_eq!(
                e.fields.get(field).map(String::as_str),
                Some(want),
                "{kind}: field {field}; fields {:?}",
                e.fields
            );
        }
    }

    // The join's error has a source: `err` is the top-level message, ": ",
    // then the database error.
    let join_err = events[0].fields.get("err").map_or("", String::as_str);
    let source = join_err.strip_prefix(&sourced_prefix);
    assert!(
        source.is_some_and(|s| s.starts_with("Found ")
            && s.contains("for field `created`")
            && s.ends_with("but field must conform to: false")),
        "join: err must be {sourced_prefix:?} followed by the assertion error; err {join_err:?}"
    );
    assert_eq!(
        events[1].fields.get("err").map(String::as_str),
        Some("recipient agent 'fail_sink' does not exist"),
        "leave: err; fields {:?}",
        events[1].fields
    );

    bus.shutdown().await;
}

const DRAIN_PRODUCER: &str = "drain_producer";
const DRAIN_SINK: &str = "drain_sink";
const DRAINED_IDS: [usize; 2] = [20, 21];

/// A forwarder spawned with a cancelled token over a receiver that already
/// buffers two records forwards both to the sink, then ends while the sender
/// is still open.
async fn cancelled_forwarder_drains_buffered_records(
    tracker: &TaskTracker,
    token: &CancellationToken,
) {
    let bus = DurableDecisionBus::new(vec![DRAIN_PRODUCER.to_string(), DRAIN_SINK.to_string()])
        .await
        .expect("drain bus");
    let producer = bus.agent(DRAIN_PRODUCER).await.expect("producer agent");
    let inbox = bus.coalition().inbox();

    let (tx, rx) = mpsc::channel(DRAINED_IDS.len());
    for agent_id in DRAINED_IDS {
        tx.send(DecisionRecord {
            coalition: "coalition-9".to_string(),
            agent_id,
            kind: DecisionKind::Join,
            act: true,
            score: 1.0,
        })
        .await
        .expect("buffer a record");
    }
    let forward_token = token.child_token();
    forward_token.cancel();
    let forwarder = spawn_decision_forwarder(
        &bus,
        producer,
        DRAIN_SINK.to_string(),
        rx,
        tracker,
        forward_token,
    )
    .await
    .expect("drain_sink is an agent of the bus");
    timeout(RECV_TIMEOUT, forwarder)
        .await
        .expect("a cancelled forwarder must end while its sender is open")
        .expect("forwarder join");

    let mut delivered = Vec::new();
    for i in 0..DRAINED_IDS.len() {
        let d = timeout(RECV_TIMEOUT, inbox.recv())
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "buffered record {} of {} was not forwarded after the cancel; \
                     delivered {delivered:?}",
                    i + 1,
                    DRAINED_IDS.len()
                )
            })
            .expect("inbox bus closed unexpectedly");
        assert_eq!(d.recipient, DRAIN_SINK, "addressed to the drain sink");
        delivered.push(d.message.payload.agent_id);
    }
    delivered.sort_unstable();
    let want: Vec<u64> = DRAINED_IDS.iter().map(|&id| id as u64).collect();
    assert_eq!(delivered, want, "the buffered records, each once");

    drop(tx);
    bus.shutdown().await;
}

const PARENTED_A: &str = "parented_a";
const PARENTED_B: &str = "parented_b";

/// Cancelling the parent of a `new_with_parent` bus cancels the bus's root
/// token and ends its listen loops, so its inbox closes.
async fn parented_bus_stops_when_parent_is_cancelled(token: &CancellationToken) {
    let parent = token.child_token();
    let bus = DurableDecisionBus::new_with_parent(
        vec![PARENTED_A.to_string(), PARENTED_B.to_string()],
        &parent,
    )
    .await
    .expect("parented bus");
    let inbox = bus.coalition().inbox();

    parent.cancel();
    let closed = timeout(RECV_TIMEOUT, inbox.recv()).await;
    assert!(
        matches!(closed, Ok(Err(_))),
        "the inbox must close within {RECV_TIMEOUT:?} of the parent cancel; got {closed:?}"
    );
    assert!(
        bus.coalition().cancellation_token().is_cancelled(),
        "the bus's root token must be cancelled with its parent"
    );
    bus.shutdown().await;
}

/// A socket path under the temp dir, unique to this process and `tag`.
fn temp_socket(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("koalisi-ping-{tag}-{}.sock", std::process::id()))
}

/// Serve one connection on `socket` with `reply`, then remove the socket.
fn serve_once(socket: &Path, reply: &'static str) -> std::thread::JoinHandle<()> {
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

#[test]
fn docker_ping_fails_on_a_socket_with_no_listener() {
    let socket = temp_socket("absent");
    let _ = std::fs::remove_file(&socket);
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
