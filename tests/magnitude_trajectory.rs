//! Trajectory-query integration tests for `TemporalAnalytics::magnitude_history`
//! (feature `magnitude`, koalisi #18).
//!
//! Each test drives a `TemporalHypergraph` directly and checks the emitted
//! magnitude trajectory against hand-computed expectations under the coupling
//! model `A(i→j) = |rel_i ∩ rel_j| / |rel_i|`, `rel = caps & required`:
//! disjoint specialists ⇒ `Mag = m`, mutual-1.0 clones skeletalize to one, a
//! subsumed agent gets Möbius weight 0.

mod common;

use common::topology::Coalition;
use koalisi::topology::{
    HyperedgeIndex, TemporalAnalytics, TemporalHypergraph, TimeRange, Timestamp,
};

/// Local vertex fixture — the shared `common::topology::Agent` does not
/// implement `AgentCapabilities`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Worker {
    id: usize,
    caps: u32,
}

impl koalisi::algorithms::AgentCapabilities for Worker {
    fn agent_id(&self) -> usize {
        self.id
    }
    fn capabilities(&self) -> u32 {
        self.caps
    }
    fn trust_level(&self) -> u32 {
        50
    }
}

const EPS: f64 = 1e-9;

fn coalition() -> Coalition {
    Coalition::new("c", 0, 0)
}

/// Assert the trajectory's magnitudes equal `expected` within `EPS`.
fn assert_mags(points: &[koalisi::topology::MagnitudePoint], expected: &[f64]) {
    let mags: Vec<f64> = points.iter().map(|p| p.magnitude).collect();
    assert_eq!(mags.len(), expected.len(), "point count: got {mags:?}");
    for (i, (g, e)) in mags.iter().zip(expected).enumerate() {
        assert!((g - e).abs() < EPS, "point {i}: got {g}, expected {e}");
    }
}

// (a) Joins raise magnitude, a leave lowers it; counts and timestamps track.
#[tokio::test]
async fn trajectory_rises_on_joins_and_drops_on_leave() {
    let g = TemporalHypergraph::<Worker, Coalition>::new();
    let v0 = g.add_vertex(Worker { id: 0, caps: 0b001 }).await.unwrap();
    let v1 = g.add_vertex(Worker { id: 1, caps: 0b010 }).await.unwrap();
    let v2 = g.add_vertex(Worker { id: 2, caps: 0b100 }).await.unwrap();
    let he = g.add_hyperedge(vec![v0, v1], coalition()).await.unwrap();
    g.update_hyperedge_vertices(he, vec![v0, v1, v2])
        .await
        .unwrap();
    g.update_hyperedge_vertices(he, vec![v1, v2]).await.unwrap();

    let points =
        TemporalAnalytics::magnitude_history(g.events_ref(), he, 0b111, &TimeRange::all()).await;

    assert_mags(&points, &[2.0, 3.0, 2.0]);
    let counts: Vec<usize> = points.iter().map(|p| p.member_count).collect();
    assert_eq!(counts, vec![2, 3, 2]);
    for w in points.windows(2) {
        assert!(
            w[1].timestamp > w[0].timestamp,
            "timestamps must strictly increase"
        );
    }
}

// (b) A redundant clone joining is a flat point: count rises, magnitude does not.
#[tokio::test]
async fn redundant_clone_join_is_flat_skeletalization() {
    let g = TemporalHypergraph::<Worker, Coalition>::new();
    let v0 = g.add_vertex(Worker { id: 0, caps: 0b001 }).await.unwrap();
    let v1 = g.add_vertex(Worker { id: 1, caps: 0b010 }).await.unwrap();
    let v2 = g.add_vertex(Worker { id: 2, caps: 0b001 }).await.unwrap(); // clone of v0's caps
    let he = g.add_hyperedge(vec![v0, v1], coalition()).await.unwrap();
    g.update_hyperedge_vertices(he, vec![v0, v1, v2])
        .await
        .unwrap();

    let points =
        TemporalAnalytics::magnitude_history(g.events_ref(), he, 0b111, &TimeRange::all()).await;

    assert_mags(&points, &[2.0, 2.0]);
    assert_eq!(points[0].member_count, 2);
    assert_eq!(points[1].member_count, 3);
}

// (c) A member's weight update is a sample point; a non-member's is not.
#[tokio::test]
async fn member_weight_update_is_a_sample_point() {
    let g = TemporalHypergraph::<Worker, Coalition>::new();
    let v0 = g.add_vertex(Worker { id: 0, caps: 0b001 }).await.unwrap();
    let v1 = g.add_vertex(Worker { id: 1, caps: 0b001 }).await.unwrap();
    let vx = g.add_vertex(Worker { id: 2, caps: 0b100 }).await.unwrap(); // never a member
    let he = g.add_hyperedge(vec![v0, v1], coalition()).await.unwrap();
    // v0 is a member: relevant ⇒ new point.
    g.update_vertex_weight(v0, Worker { id: 0, caps: 0b010 })
        .await
        .unwrap();
    // vx is not a member: not relevant ⇒ no point.
    g.update_vertex_weight(vx, Worker { id: 2, caps: 0b001 })
        .await
        .unwrap();

    let points =
        TemporalAnalytics::magnitude_history(g.events_ref(), he, 0b011, &TimeRange::all()).await;

    // Clones {0b001, 0b001} ⇒ 1.0; after v0 → 0b010, disjoint {0b010, 0b001} ⇒ 2.0.
    assert_mags(&points, &[1.0, 2.0]);
}

// (d) A task-irrelevant bystander is excluded (not collapsed); required == 0 zeros out.
#[tokio::test]
async fn irrelevant_bystander_and_zero_required() {
    let g = TemporalHypergraph::<Worker, Coalition>::new();
    let v0 = g.add_vertex(Worker { id: 0, caps: 0b001 }).await.unwrap();
    let v1 = g.add_vertex(Worker { id: 1, caps: 0b010 }).await.unwrap();
    let v2 = g.add_vertex(Worker { id: 2, caps: 0b100 }).await.unwrap(); // bystander under 0b011
    let he = g
        .add_hyperedge(vec![v0, v1, v2], coalition())
        .await
        .unwrap();

    // required 0b011: bystander 0b100 excluded ⇒ {0b001, 0b010} disjoint ⇒ 2.0, count 3.
    let points =
        TemporalAnalytics::magnitude_history(g.events_ref(), he, 0b011, &TimeRange::all()).await;
    assert_mags(&points, &[2.0]);
    assert_eq!(points[0].member_count, 3);

    // required 0: every agent irrelevant ⇒ every point 0.0.
    let zero = TemporalAnalytics::magnitude_history(g.events_ref(), he, 0, &TimeRange::all()).await;
    assert!(!zero.is_empty());
    assert!(zero.iter().all(|p| p.magnitude.abs() < EPS));
}

// (e) Window baseline for a coalition formed before the window; dissolution
//     terminal point; empty cases (unknown hyperedge, pre-formation window).
#[tokio::test]
async fn window_baseline_and_dissolution() {
    let g = TemporalHypergraph::<Worker, Coalition>::new();
    let v0 = g.add_vertex(Worker { id: 0, caps: 0b001 }).await.unwrap(); // ts 0
    let v1 = g.add_vertex(Worker { id: 1, caps: 0b010 }).await.unwrap(); // ts 1
    let he = g.add_hyperedge(vec![v0, v1], coalition()).await.unwrap(); // ts 2, Mag 2.0
    let _v2 = g.add_vertex(Worker { id: 2, caps: 0b100 }).await.unwrap(); // ts 3, non-member
    g.remove_hyperedge(he).await.unwrap(); // ts 4, dissolution

    // Window opened at ts 3 (after formation): baseline at start + terminal 0.0.
    let points = TemporalAnalytics::magnitude_history(
        g.events_ref(),
        he,
        0b111,
        &TimeRange::from(Timestamp::new(3)),
    )
    .await;
    assert_eq!(points.len(), 2);
    assert_eq!(
        points[0].timestamp,
        Timestamp::new(3),
        "baseline sits at window start"
    );
    assert!((points[0].magnitude - 2.0).abs() < EPS);
    assert_eq!(points[0].member_count, 2);
    assert!(points[1].magnitude.abs() < EPS, "dissolution ⇒ 0.0");
    assert_eq!(points[1].member_count, 0);

    // A hyperedge index that never existed ⇒ empty trajectory.
    let unknown = TemporalAnalytics::magnitude_history(
        g.events_ref(),
        HyperedgeIndex(999),
        0b111,
        &TimeRange::all(),
    )
    .await;
    assert!(unknown.is_empty());

    // A window entirely before formation (ts 2) ⇒ empty trajectory.
    let before = TemporalAnalytics::magnitude_history(
        g.events_ref(),
        he,
        0b111,
        &TimeRange::between(Timestamp::new(0), Timestamp::new(1)),
    )
    .await;
    assert!(before.is_empty());
}

// (f) A subsumed member counts as one effective agent.
#[tokio::test]
async fn subsumption_pair_counts_one() {
    let g = TemporalHypergraph::<Worker, Coalition>::new();
    let v0 = g.add_vertex(Worker { id: 0, caps: 0b001 }).await.unwrap();
    let v1 = g.add_vertex(Worker { id: 1, caps: 0b011 }).await.unwrap(); // subsumes v0
    let he = g.add_hyperedge(vec![v0, v1], coalition()).await.unwrap();

    let points =
        TemporalAnalytics::magnitude_history(g.events_ref(), he, 0b111, &TimeRange::all()).await;

    // A(0→1)=1.0, A(1→0)=0.5 ⇒ w=(0,1) ⇒ Mag = 1.0.
    assert_mags(&points, &[1.0]);
}
