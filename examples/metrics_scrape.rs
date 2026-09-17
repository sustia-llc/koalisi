//! Prometheus metrics over koalisi's three tap surfaces, scraped and checked
//! in one process (feature `metrics`).
//!
//! Surfaces and the series each consumer records:
//! 1. The decision tap of a policy-gated `CoalitionService`, fanned out by
//!    `spawn_decision_tee` to two sinks: `koalisi_decisions_total{kind,act}`
//!    and the `koalisi_decision_score` histogram.
//! 2. The outcome tap drained by `spawn_outcome_forwarder` into two
//!    `OutcomeSink`s: `koalisi_task_outcomes_total{success}` and the
//!    `koalisi_outcome_members` histogram.
//! 3. The topology event tap installed by `TemporalHypergraph::with_event_tap`
//!    before the first mutation: `koalisi_topology_events_total{event_type}`,
//!    labelled by `TemporalEvent::event_type()`.
//!
//! Each surface is also counted without the recorder: the second tee sink, a
//! second `OutcomeSink` over atomics, and a map returned by the topology
//! consumer task. A scripted workload drives all three surfaces, the pipeline
//! shuts down losslessly, and the example then issues one `GET /metrics`
//! against its own Prometheus listener over a raw `TcpStream`, prints the
//! body, and asserts every scraped counter, histogram count and histogram sum
//! against the recorder-free count and against the value the script fixes.
//!
//! The listener binds `KOALISI_METRICS_ADDR` when set, otherwise a free
//! `127.0.0.1` port. Run with
//! `cargo run --features metrics --example metrics_scrape`.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use metrics::{counter, histogram};
use metrics_exporter_prometheus::{BuildError, Matcher, PrometheusBuilder};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use koalisi::algorithms::{AdditiveCalculator, CapabilityAgent};
use koalisi::decision::{DecisionContext, ThresholdPolicy};
use koalisi::subsystems::coalition_actor::{CoalitionService, DecisionRecord, spawn_decision_tee};
use koalisi::subsystems::outcome::{
    OutcomeSink, TaskOutcome, emit_outcome, spawn_outcome_forwarder,
};
use koalisi::topology::{CoalitionManager, TemporalEvent, TemporalHypergraph};

const DECISIONS_TOTAL: &str = "koalisi_decisions_total";
const DECISION_SCORE: &str = "koalisi_decision_score";
const OUTCOMES_TOTAL: &str = "koalisi_task_outcomes_total";
const OUTCOME_MEMBERS: &str = "koalisi_outcome_members";
const TOPOLOGY_EVENTS_TOTAL: &str = "koalisi_topology_events_total";

const CHANNEL_CAPACITY: usize = 64;
const ENV_ADDR: &str = "KOALISI_METRICS_ADDR";
/// Connect attempts of the self-scrape, 20 ms apart.
const CONNECT_ATTEMPTS: usize = 100;
/// Probe-and-bind attempts for the default loopback listener.
const LISTENER_ATTEMPTS: usize = 8;

/// Join and leave threshold of the scripted `ThresholdPolicy`. The additive
/// marginal of one agent is `50 + 10 * popcount(caps) + trust`.
const THRESHOLD: f64 = 100.0;

/// Recorder-free tally of one histogram-backed surface: records per label
/// value, number of observations, sum of observations.
#[derive(Debug, Default)]
struct Tally {
    by_label: BTreeMap<String, u64>,
    observations: u64,
    sum: f64,
}

/// Recorder-free outcome tally, shared between the sink and `main`.
#[derive(Default)]
struct OutcomeTally {
    success: AtomicU64,
    failure: AtomicU64,
    member_sum: AtomicU64,
}

fn decision_label(kind: &str, act: bool) -> String {
    format!("act=\"{act}\",kind=\"{kind}\"")
}

/// Metrics consumer of the decision surface (tee sink 0).
async fn record_decisions(mut rx: mpsc::Receiver<DecisionRecord>) {
    while let Some(record) = rx.recv().await {
        counter!(
            DECISIONS_TOTAL,
            "kind" => record.kind.as_str(),
            "act" => record.act.to_string()
        )
        .increment(1);
        histogram!(DECISION_SCORE).record(record.score);
    }
}

/// Recorder-free consumer of the decision surface (tee sink 1).
async fn tally_decisions(mut rx: mpsc::Receiver<DecisionRecord>) -> Tally {
    let mut tally = Tally::default();
    while let Some(record) = rx.recv().await {
        *tally
            .by_label
            .entry(decision_label(record.kind.as_str(), record.act))
            .or_insert(0) += 1;
        tally.observations += 1;
        tally.sum += record.score;
    }
    tally
}

/// Consumer of the topology surface: records the counter and returns its own
/// per-`event_type` map.
async fn consume_topology(
    mut rx: mpsc::Receiver<TemporalEvent<CapabilityAgent, u32>>,
) -> BTreeMap<String, u64> {
    let mut seen = BTreeMap::new();
    while let Some(event) = rx.recv().await {
        let event_type = event.event_type();
        counter!(TOPOLOGY_EVENTS_TOTAL, "event_type" => event_type).increment(1);
        *seen
            .entry(format!("event_type=\"{event_type}\""))
            .or_insert(0) += 1;
    }
    seen
}

/// Metrics consumer of the outcome surface.
fn record_outcome_metrics(outcome: &TaskOutcome) {
    counter!(OUTCOMES_TOTAL, "success" => outcome.success.to_string()).increment(1);
    histogram!(OUTCOME_MEMBERS).record(as_f64(outcome.members.len() as u64));
}

/// Install the Prometheus recorder with its HTTP listener on `addr`.
fn install_recorder(addr: SocketAddr) -> Result<(), BuildError> {
    PrometheusBuilder::new()
        .with_http_listener(addr)
        .set_buckets_for_metric(
            Matcher::Full(DECISION_SCORE.to_string()),
            &[75.0, 100.0, 150.0],
        )?
        .set_buckets_for_metric(Matcher::Full(OUTCOME_MEMBERS.to_string()), &[1.0, 2.0, 4.0])?
        .install()
}

/// Install the recorder and return the listener address: the env override,
/// or a free loopback port found by binding port 0 and releasing it. The
/// exporter binds before it sets the global recorder, so a probed port taken
/// by another process between the release and the exporter's bind is retried
/// on a fresh port.
async fn install_on_listener() -> Result<SocketAddr> {
    if let Ok(raw) = std::env::var(ENV_ADDR) {
        let addr: SocketAddr = raw
            .parse()
            .with_context(|| format!("{ENV_ADDR}={raw} is not a socket address"))?;
        install_recorder(addr).with_context(|| format!("install the recorder on {addr}"))?;
        return Ok(addr);
    }
    let mut last = None;
    for _ in 0..LISTENER_ATTEMPTS {
        let addr = {
            let probe = TcpListener::bind("127.0.0.1:0")
                .await
                .context("probe a free loopback port")?;
            probe.local_addr().context("probe local_addr")?
        };
        match install_recorder(addr) {
            Ok(()) => return Ok(addr),
            Err(BuildError::FailedToCreateHTTPListener(cause)) => last = Some((addr, cause)),
            Err(other) => return Err(other).context("install the Prometheus recorder"),
        }
    }
    match last {
        Some((addr, cause)) => {
            bail!("no loopback listener after {LISTENER_ATTEMPTS} attempts; last {addr}: {cause}")
        }
        None => bail!("LISTENER_ATTEMPTS is 0"),
    }
}

/// One `GET /metrics` over a raw HTTP/1.1 connection; returns the body.
async fn scrape(addr: SocketAddr) -> Result<String> {
    let mut attempt = TcpStream::connect(addr).await;
    for _ in 1..CONNECT_ATTEMPTS {
        if attempt.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        attempt = TcpStream::connect(addr).await;
    }
    let mut stream =
        attempt.with_context(|| format!("connect to {addr} ({CONNECT_ATTEMPTS} attempts)"))?;
    let request = format!("GET /metrics HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .context("write request")?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .await
        .context("read response")?;
    let response = String::from_utf8(raw).context("response is not UTF-8")?;

    let (head, body) = response
        .split_once("\r\n\r\n")
        .context("response has no header terminator")?;
    let status = head.lines().next().unwrap_or_default();
    if !status.starts_with("HTTP/1.1 200") {
        bail!("unexpected status line: {status}");
    }
    if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        bail!("chunked response is not handled by this scraper");
    }
    Ok(body.to_string())
}

/// Parse the sample lines of a Prometheus text body into
/// `name{sorted labels}` → value.
fn parse_samples(body: &str) -> Result<BTreeMap<String, f64>> {
    let mut samples = BTreeMap::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, labels, rest) = split_series(line)?;
        // `value [timestamp]`: the value is the first token after the series.
        let value: f64 = rest
            .split_whitespace()
            .next()
            .with_context(|| format!("sample line without a value: {line}"))?
            .parse()
            .with_context(|| format!("sample value is not a number: {line}"))?;
        let key = if labels.is_empty() {
            name.to_string()
        } else {
            let mut labels = labels;
            labels.sort_unstable();
            format!("{name}{{{}}}", labels.join(","))
        };
        samples.insert(key, value);
    }
    Ok(samples)
}

/// Split one sample line into its metric name, its `key="value"` label pairs
/// and the text after the series. Commas, braces and spaces inside a quoted
/// label value, and `\"` / `\\` escapes, stay inside that pair.
fn split_series(line: &str) -> Result<(&str, Vec<&str>, &str)> {
    let name_end = line
        .find(|c: char| c == '{' || c.is_whitespace())
        .with_context(|| format!("sample line without a value: {line}"))?;
    let name = &line[..name_end];
    if !line[name_end..].starts_with('{') {
        return Ok((name, Vec::new(), &line[name_end..]));
    }

    let mut labels = Vec::new();
    let first = name_end + 1;
    let mut start = first;
    let mut quoted = false;
    let mut escaped = false;
    for (i, c) in line.char_indices().skip_while(|(i, _)| *i < first) {
        if escaped {
            escaped = false;
        } else if quoted {
            match c {
                '\\' => escaped = true,
                '"' => quoted = false,
                _ => {}
            }
        } else {
            match c {
                '"' => quoted = true,
                ',' | '}' => {
                    if start < i {
                        labels.push(&line[start..i]);
                    }
                    if c == '}' {
                        return Ok((name, labels, &line[i + 1..]));
                    }
                    start = i + 1;
                }
                _ => {}
            }
        }
    }
    bail!("unterminated label set: {line}")
}

/// Assert one scraped sample against the recorder-free value and the value
/// the script fixes. Every value is a small integer-valued `f64`, so equality
/// is exact.
#[allow(clippy::float_cmp)]
fn check(samples: &BTreeMap<String, f64>, series: &str, independent: f64, scripted: f64) {
    assert_eq!(
        independent, scripted,
        "{series}: independent count {independent} != scripted value {scripted}"
    );
    let Some(scraped) = samples.get(series).copied() else {
        panic!("{series}: absent from the scrape (independent count {independent})");
    };
    assert_eq!(
        scraped, independent,
        "{series}: scraped {scraped} != independent count {independent}"
    );
    println!("  ok {series} = {scraped}");
}

/// Counts in this example stay far below 2^53, where `u64 → f64` is exact.
#[allow(clippy::cast_precision_loss)]
fn as_f64(n: u64) -> f64 {
    n as f64
}

/// Assert a labelled counter family: every scripted series matches, and the
/// scrape carries no series of the family outside the scripted set.
fn check_family(
    samples: &BTreeMap<String, f64>,
    name: &str,
    independent: &BTreeMap<String, u64>,
    scripted: &[(String, u64)],
) {
    for (labels, want) in scripted {
        let got = independent.get(labels).copied().unwrap_or(0);
        check(
            samples,
            &format!("{name}{{{labels}}}"),
            as_f64(got),
            as_f64(*want),
        );
    }
    let prefix = format!("{name}{{");
    // `samples` and `independent` are `BTreeMap`s, so their keys arrive sorted.
    let scraped: Vec<&str> = samples
        .keys()
        .filter(|k| k.starts_with(&prefix))
        .map(String::as_str)
        .collect();
    let mut expected: Vec<String> = scripted
        .iter()
        .map(|(labels, _)| format!("{name}{{{labels}}}"))
        .collect();
    expected.sort_unstable();
    assert_eq!(
        scraped, expected,
        "{name}: scraped series != scripted series"
    );
    let counted: Vec<&str> = independent.keys().map(String::as_str).collect();
    let mut scripted_labels: Vec<&str> = scripted.iter().map(|(l, _)| l.as_str()).collect();
    scripted_labels.sort_unstable();
    assert_eq!(
        counted, scripted_labels,
        "{name}: independent series != scripted series"
    );
}

#[tokio::main]
#[allow(clippy::too_many_lines)] // one linear script: wire, drive, drain, scrape, assert
async fn main() -> Result<()> {
    let tracker = TaskTracker::new();
    let token = CancellationToken::new();

    // =====================================================================
    // 1. Prometheus recorder + HTTP listener on loopback.
    // =====================================================================
    let addr = install_on_listener().await?;
    println!("prometheus listener on http://{addr}/metrics");

    // =====================================================================
    // 2. Topology surface: tap installed before the first mutation.
    // =====================================================================
    let (topo_tx, topo_rx) = mpsc::channel(CHANNEL_CAPACITY);
    let topology = tracker.spawn(consume_topology(topo_rx));
    let graph = TemporalHypergraph::<CapabilityAgent, u32>::new().with_event_tap(topo_tx);
    let manager = CoalitionManager::new(graph);

    // Seed (marginal 80), three strong candidates (140 each), one weak
    // candidate (70), one agent that is updated and then removed.
    let seed = manager
        .add_agent(CapabilityAgent::new(0, 0b0001, 20))
        .await?;
    let mut strong = Vec::new();
    for (id, caps) in [(1usize, 0b0010u32), (2, 0b0100), (3, 0b1000)] {
        strong.push(
            manager
                .add_agent(CapabilityAgent::new(id, caps, 80))
                .await?,
        );
    }
    let weak = manager
        .add_agent(CapabilityAgent::new(4, 0b0001, 10))
        .await?;
    let transient = manager
        .add_agent(CapabilityAgent::new(5, 0b0001, 50))
        .await?;

    let coalition = manager.form_coalition(vec![seed], 1).await?;
    let scratch = manager.form_coalition(vec![transient, weak], 2).await?;
    manager
        .update_agent(transient, CapabilityAgent::new(5, 0b0011, 50))
        .await?;
    manager.update_coalition(scratch, 3).await?;
    manager.graph().reverse_hyperedge(scratch).await?;
    manager.dissolve_coalition(scratch).await?;
    manager.remove_agent(transient).await?;
    manager.create_snapshot().await;

    // =====================================================================
    // 3. Decision surface: service tap → tee → two sinks.
    // =====================================================================
    let (tap_tx, tap_rx) = mpsc::channel(CHANNEL_CAPACITY);
    let (metrics_tx, metrics_rx) = mpsc::channel(CHANNEL_CAPACITY);
    let (tally_tx, tally_rx) = mpsc::channel(CHANNEL_CAPACITY);
    spawn_decision_tee(
        tap_rx,
        vec![metrics_tx, tally_tx],
        &tracker,
        token.child_token(),
    );
    tracker.spawn(record_decisions(metrics_rx));
    let decisions = tracker.spawn(tally_decisions(tally_rx));

    let service = CoalitionService::spawn_with_tap(
        manager,
        Box::new(ThresholdPolicy::new(
            AdditiveCalculator,
            THRESHOLD,
            THRESHOLD,
        )),
        DecisionContext {
            required_capabilities: 0b1111,
        },
        tap_tx,
    );

    for candidate in &strong {
        let d = service.join(*candidate, coalition).await?;
        println!("join strong: act={} score={}", d.act, d.score);
    }
    let d = service.join(weak, coalition).await?;
    println!("join weak:   act={} score={}", d.act, d.score);
    let d = service.leave(seed, coalition).await?;
    println!("leave seed:  act={} score={}", d.act, d.score);
    let d = service.leave(strong[0], coalition).await?;
    println!("leave strong: act={} score={}", d.act, d.score);

    // =====================================================================
    // 4. Outcome surface: tap → forwarder → two sinks.
    // =====================================================================
    let outcome_tally = Arc::new(OutcomeTally::default());
    let sink_tally = Arc::clone(&outcome_tally);
    let tally_sink: Box<dyn OutcomeSink> = Box::new(move |outcome: &TaskOutcome| {
        let slot = if outcome.success {
            &sink_tally.success
        } else {
            &sink_tally.failure
        };
        slot.fetch_add(1, Ordering::SeqCst);
        sink_tally
            .member_sum
            .fetch_add(outcome.members.len() as u64, Ordering::SeqCst);
    });
    let metrics_sink: Box<dyn OutcomeSink> = Box::new(record_outcome_metrics);

    let (outcome_tx, outcome_rx) = mpsc::channel(CHANNEL_CAPACITY);
    spawn_outcome_forwarder(
        outcome_rx,
        vec![metrics_sink, tally_sink],
        &tracker,
        token.child_token(),
    );
    for (members, success) in [
        (vec![1, 2, 3], true),
        (vec![1, 2], false),
        (vec![0, 1, 2, 3], true),
        (vec![4], false),
        (vec![1, 2, 3], true),
    ] {
        emit_outcome(
            Some(&outcome_tx),
            TaskOutcome {
                required: 0b1111,
                members,
                success,
            },
        );
    }

    // =====================================================================
    // 5. Lossless shutdown: drop every tap sender, let each consumer drain to
    //    `None`, then wait. The token is never cancelled.
    // =====================================================================
    drop(service); // service task ends → decision tap + topology tap close
    drop(outcome_tx);
    tracker.close();
    let topology_seen = topology.await.context("topology consumer")?;
    let decision_tally = decisions.await.context("decision tally consumer")?;
    tracker.wait().await;

    // =====================================================================
    // 6. Self-scrape and assert.
    // =====================================================================
    let body = scrape(addr).await?;
    println!("\n--- GET /metrics ---\n{body}--- end ---\n");
    let samples = parse_samples(&body)?;

    let scripted_decisions = [
        (decision_label("join", true), 3),
        (decision_label("join", false), 1),
        (decision_label("leave", true), 1),
        (decision_label("leave", false), 1),
    ];
    check_family(
        &samples,
        DECISIONS_TOTAL,
        &decision_tally.by_label,
        &scripted_decisions,
    );
    check(
        &samples,
        &format!("{DECISION_SCORE}_count"),
        as_f64(decision_tally.observations),
        6.0,
    );
    // 3 × 140 (strong joins) + 70 (weak join) + 80 (seed leave) + 140 (strong leave).
    check(
        &samples,
        &format!("{DECISION_SCORE}_sum"),
        decision_tally.sum,
        710.0,
    );

    let successes = outcome_tally.success.load(Ordering::SeqCst);
    let failures = outcome_tally.failure.load(Ordering::SeqCst);
    let member_sum = outcome_tally.member_sum.load(Ordering::SeqCst);
    let outcome_counts = BTreeMap::from([
        ("success=\"true\"".to_string(), successes),
        ("success=\"false\"".to_string(), failures),
    ]);
    check_family(
        &samples,
        OUTCOMES_TOTAL,
        &outcome_counts,
        &[
            ("success=\"true\"".to_string(), 3),
            ("success=\"false\"".to_string(), 2),
        ],
    );
    check(
        &samples,
        &format!("{OUTCOME_MEMBERS}_count"),
        as_f64(successes + failures),
        5.0,
    );
    check(
        &samples,
        &format!("{OUTCOME_MEMBERS}_sum"),
        as_f64(member_sum),
        13.0,
    );

    let scripted_topology: Vec<(String, u64)> = [
        ("VertexAdded", 6),
        ("HyperedgeAdded", 2),
        ("VertexWeightUpdated", 1),
        ("HyperedgeWeightUpdated", 1),
        ("HyperedgeReversed", 1),
        ("HyperedgeRemoved", 1),
        ("VertexRemoved", 1),
        ("SnapshotMarker", 1),
        // Three applied joins + one applied leave, through the service.
        ("HyperedgeVerticesUpdated", 4),
    ]
    .into_iter()
    .map(|(event_type, n)| (format!("event_type=\"{event_type}\""), n))
    .collect();
    check_family(
        &samples,
        TOPOLOGY_EVENTS_TOTAL,
        &topology_seen,
        &scripted_topology,
    );

    println!("\nevery scraped series matches its independent count");
    Ok(())
}
