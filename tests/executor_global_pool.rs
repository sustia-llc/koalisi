//! Every async path of the topology, decision and analytics layers, run on an
//! `Executor`, leaves rayon's global pool unbuilt: after the 18 async
//! `TemporalHypergraph` methods, `try_join_coalition` / `try_leave_coalition`
//! and direct `should_join_async` / `should_leave_async` calls for
//! `AifDecisionPolicy`, `AifMmDecisionPolicy` and `MagnitudePolicy` (untyped
//! and typed), and `TemporalAnalytics::magnitude_history`,
//! `ThreadPoolBuilder::build_global` returns `Ok`. One test per binary: the
//! global pool is process state.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use koalisi::algorithms::{AgentCapabilities, CapabilityAgent};
use koalisi::core::Executor;
use koalisi::decision::{
    AifDecisionPolicy, AifMmDecisionPolicy, CoalitionDecisionPolicy, DecisionContext,
    MagnitudePolicy, RoleModulation,
};
use koalisi::topology::{CoalitionManager, TemporalAnalytics, TemporalHypergraph, TimeRange};
use tokio_rayon::rayon;

/// Every async `TemporalHypergraph` method, each checked against its expected
/// result.
async fn graph_paths(exec: &Arc<Executor>) -> Result<()> {
    let g = TemporalHypergraph::<u32, u32>::new(Arc::clone(exec));
    let v0 = g.add_vertex(10).await?;
    let v1 = g.add_vertex(11).await?;
    let v2 = g.add_vertex(12).await?;
    let v3 = g.add_vertex(13).await?;
    g.update_vertex_weight(v3, 14).await?;
    assert_eq!(g.get_vertex_weight(v3).await?, 14, "updated vertex weight");
    g.remove_vertex(v3).await?;

    let a = g.add_hyperedge(vec![v0, v1], 1).await?;
    let b = g.add_hyperedge(vec![v2], 2).await?;
    let c = g.add_hyperedge(vec![v2], 3).await?;
    g.update_hyperedge_weight(a, 5).await?;
    assert_eq!(
        g.get_hyperedge_weight(a).await?,
        5,
        "updated hyperedge weight"
    );
    g.update_hyperedge_vertices(a, vec![v0, v1, v2]).await?;
    g.update_hyperedge_vertices_try(a, Ok).await?;
    let reversed = g.reverse_hyperedge(a).await?;
    assert_eq!(reversed, vec![v2, v1, v0], "reversed members");
    g.contract_hyperedge_vertices(a, vec![v1, v2], v1).await?;
    g.join_hyperedges(vec![b, c]).await?;
    assert_eq!(
        g.get_hyperedge_vertices(b).await?,
        vec![v2, v2],
        "joined members"
    );
    g.remove_hyperedge(b).await?;
    assert_eq!(
        (g.count_vertices().await, g.count_hyperedges().await),
        (3, 1),
        "(vertices, hyperedges) before the clears"
    );
    g.clear_hyperedges().await?;
    g.clear().await?;
    assert_eq!(g.count_vertices().await, 0, "vertices after clear");
    Ok(())
}

/// Direct async calls, and the manager's policy-gated join and leave, for one
/// policy; each async decision equals its sync decision.
async fn policy_paths(
    exec: &Arc<Executor>,
    label: &str,
    policy: &dyn CoalitionDecisionPolicy,
) -> Result<()> {
    let ctx = DecisionContext {
        required_capabilities: 0b111,
    };
    let a0 = CapabilityAgent::new(0, 0b001, 50);
    let a1 = CapabilityAgent::new(1, 0b010, 50);
    let a2 = CapabilityAgent::new(2, 0b100, 50);
    let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2];
    let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
    assert_eq!(
        policy.should_join_async(&a0, &coalition, &ctx, exec).await,
        policy.should_join(&a0, &coalition, &ctx),
        "{label}: async vs sync join"
    );
    assert_eq!(
        policy.should_leave_async(&a0, &full, &ctx, exec).await,
        policy.should_leave(&a0, &full, &ctx),
        "{label}: async vs sync leave"
    );

    let manager = CoalitionManager::<CapabilityAgent, u32>::empty(Arc::clone(exec));
    let m1 = manager.add_agent(a1).await?;
    let m2 = manager.add_agent(a2).await?;
    let m0 = manager.add_agent(a0).await?;
    let he = manager.form_coalition(vec![m1, m2], 1).await?;
    let join = manager.try_join_coalition(m0, he, policy, &ctx).await?;
    assert_eq!(
        join,
        policy.should_join(&a0, &coalition, &ctx),
        "{label}: manager join vs sync join"
    );
    if join.act {
        let leave = manager.try_leave_coalition(m0, he, policy, &ctx).await?;
        let seated: Vec<&dyn AgentCapabilities> = vec![&a1, &a2, &a0];
        assert_eq!(
            leave,
            policy.should_leave(&a0, &seated, &ctx),
            "{label}: manager leave vs sync leave"
        );
    }
    Ok(())
}

async fn analytics_path(exec: &Arc<Executor>) -> Result<()> {
    let g = TemporalHypergraph::<CapabilityAgent, u32>::new(Arc::clone(exec));
    let v0 = g.add_vertex(CapabilityAgent::new(0, 0b001, 50)).await?;
    let v1 = g.add_vertex(CapabilityAgent::new(1, 0b010, 50)).await?;
    let he = g.add_hyperedge(vec![v0, v1], 1).await?;
    let points =
        TemporalAnalytics::magnitude_history(g.events_ref(), he, 0b011, &TimeRange::all(), exec)
            .await;
    let mags: Vec<f64> = points.iter().map(|p| p.magnitude).collect();
    assert!(
        mags.len() == 1 && (mags[0] - 2.0).abs() < 1e-9,
        "magnitude_history: expected [2.0], observed {mags:?}"
    );
    Ok(())
}

#[tokio::test]
async fn async_paths_leave_the_global_pool_unbuilt() -> Result<()> {
    let exec = Arc::new(Executor::with_num_threads(2)?);
    graph_paths(&exec).await?;
    policy_paths(&exec, "AifDecisionPolicy", &AifDecisionPolicy::default()).await?;
    policy_paths(
        &exec,
        "AifMmDecisionPolicy",
        &AifMmDecisionPolicy::default(),
    )
    .await?;
    policy_paths(&exec, "MagnitudePolicy", &MagnitudePolicy::default()).await?;
    let roles: HashMap<usize, usize> = [(0, 0), (1, 1), (2, 2)].into();
    let rho = RoleModulation::new(vec![vec![1.0; 3]; 3]).expect("an all-ones table is valid");
    policy_paths(
        &exec,
        "typed MagnitudePolicy",
        &MagnitudePolicy::new(0.0).with_role_modulation(roles, rho),
    )
    .await?;
    analytics_path(&exec).await?;

    let global = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build_global();
    assert!(
        global.is_ok(),
        "expected build_global() to succeed because no path started the global pool, observed {global:?}"
    );
    Ok(())
}
