//! Integration: policy-driven coalition membership through the live
//! `CoalitionActor` seam (issue #1).
//!
//! Exercises the decision *through the actor* — `JoinRequest`/`LeaveRequest`
//! consult a `CoalitionDecisionPolicy` before the `CoalitionManager` mutates
//! membership. Feature-off covers the always-available `ThresholdPolicy` (both
//! the apply and decline paths); feature-on covers `AifDecisionPolicy`
//! non-degeneracy (an agent covering a new required bit joins; a redundant
//! clone does not).

use std::fmt::{Display, Formatter};

use kameo::prelude::*;
use koalisi::algorithms::AgentCapabilities;
use koalisi::decision::DecisionContext;
use koalisi::subsystems::coalition_actor::{
    CoalitionActor, CoalitionActorArgs, JoinRequest, LeaveRequest, Members,
};
use koalisi::topology::CoalitionManager;

// ---------------------------------------------------------------------------
// Fixture types: a vertex that is both a valid graph weight and an agent, and
// a hyperedge weight usable as a coalition.
// ---------------------------------------------------------------------------

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
struct Worker {
    id: usize,
    capabilities: u32,
    trust: u32,
}

impl Worker {
    const fn new(id: usize, capabilities: u32, trust: u32) -> Self {
        Self {
            id,
            capabilities,
            trust,
        }
    }
}

impl Display for Worker {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Worker({})", self.id)
    }
}

impl AgentCapabilities for Worker {
    fn agent_id(&self) -> usize {
        self.id
    }
    fn capabilities(&self) -> u32 {
        self.capabilities
    }
    fn trust_level(&self) -> u32 {
        self.trust
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
struct Team {
    name: &'static str,
    cost: usize,
}

impl Display for Team {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Team({})", self.name)
    }
}

impl From<Team> for usize {
    fn from(t: Team) -> Self {
        t.cost
    }
}

/// Build a manager with a one-member coalition and return (manager, members…).
async fn manager_with_seed_coalition() -> (
    CoalitionManager<Worker, Team>,
    koalisi::topology::VertexIndex, // seed member (caps 0b010)
    koalisi::topology::HyperedgeIndex,
) {
    let manager = CoalitionManager::<Worker, Team>::empty();
    let seed = manager
        .add_agent(Worker::new(2, 0b010, 90))
        .await
        .expect("add seed agent");
    let coalition = manager
        .form_coalition(vec![seed], Team::new_team())
        .await
        .expect("form coalition");
    (manager, seed, coalition)
}

impl Team {
    fn new_team() -> Self {
        Self {
            name: "alpha",
            cost: 10,
        }
    }
}

// ---------------------------------------------------------------------------
// Feature-off: ThresholdPolicy through the actor (apply + decline paths)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn threshold_policy_applies_join_through_actor() {
    use koalisi::algorithms::AdditiveCalculator;
    use koalisi::decision::ThresholdPolicy;

    let (manager, _seed, coalition) = manager_with_seed_coalition().await;
    let candidate = manager
        .add_agent(Worker::new(1, 0b001, 80))
        .await
        .expect("add candidate");

    // join_threshold 0.0 ⇒ any positive marginal value joins.
    let actor = CoalitionActor::spawn(CoalitionActorArgs {
        manager,
        policy: Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
        ctx: DecisionContext::default(),
    });

    let decision = actor
        .ask(JoinRequest {
            agent: candidate,
            coalition,
        })
        .await
        .expect("decision");
    assert!(decision.act, "positive marginal ⇒ join");

    let members = actor
        .ask(Members { coalition })
        .await
        .expect("members");
    assert_eq!(members.len(), 2, "candidate was actually added");
    assert!(members.contains(&candidate));
}

#[tokio::test]
async fn threshold_policy_declines_join_through_actor() {
    use koalisi::algorithms::AdditiveCalculator;
    use koalisi::decision::ThresholdPolicy;

    let (manager, _seed, coalition) = manager_with_seed_coalition().await;
    let candidate = manager
        .add_agent(Worker::new(1, 0b001, 80))
        .await
        .expect("add candidate");

    // An unreachable join_threshold ⇒ the actor must NOT mutate membership.
    let actor = CoalitionActor::spawn(CoalitionActorArgs {
        manager,
        policy: Box::new(ThresholdPolicy::new(AdditiveCalculator, 1.0e9, 0.0)),
        ctx: DecisionContext::default(),
    });

    let decision = actor
        .ask(JoinRequest {
            agent: candidate,
            coalition,
        })
        .await
        .expect("decision");
    assert!(!decision.act, "unreachable threshold ⇒ decline");

    let members = actor
        .ask(Members { coalition })
        .await
        .expect("members");
    assert_eq!(members.len(), 1, "declined join left membership unchanged");
    assert!(!members.contains(&candidate));
}

#[tokio::test]
async fn threshold_policy_forces_leave_through_actor() {
    use koalisi::algorithms::AdditiveCalculator;
    use koalisi::decision::ThresholdPolicy;

    let (manager, seed, coalition) = manager_with_seed_coalition().await;
    let other = manager
        .add_agent(Worker::new(1, 0b001, 80))
        .await
        .expect("add other");
    manager
        .join_coalition(other, coalition)
        .await
        .expect("seed second member");

    // A high leave_threshold forces leaving even a contributing member.
    let actor = CoalitionActor::spawn(CoalitionActorArgs {
        manager,
        policy: Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 1.0e9)),
        ctx: DecisionContext::default(),
    });

    let decision = actor
        .ask(LeaveRequest {
            agent: seed,
            coalition,
        })
        .await
        .expect("decision");
    assert!(decision.act, "high leave threshold ⇒ leave");

    let members = actor
        .ask(Members { coalition })
        .await
        .expect("members");
    assert_eq!(members.len(), 1, "seed actually removed");
    assert!(!members.contains(&seed));
}

// ---------------------------------------------------------------------------
// Manager primitive directly (no actor) — guards against the actor layer
// masking a `try_join_coalition` / `try_leave_coalition` bug.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn manager_try_join_apply_and_decline() {
    use koalisi::algorithms::AdditiveCalculator;
    use koalisi::decision::ThresholdPolicy;

    let (manager, _seed, coalition) = manager_with_seed_coalition().await;
    let candidate = manager
        .add_agent(Worker::new(1, 0b001, 80))
        .await
        .expect("add candidate");
    let ctx = DecisionContext::default();

    // Decline path: unreachable threshold ⇒ no mutation.
    let decline = ThresholdPolicy::new(AdditiveCalculator, 1.0e9, 0.0);
    let d = manager
        .try_join_coalition(candidate, coalition, &decline, &ctx)
        .await
        .expect("try_join");
    assert!(!d.act);
    assert_eq!(manager.coalition_members(coalition).await.unwrap().len(), 1);

    // Apply path: zero threshold ⇒ join applied.
    let apply = ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0);
    let d = manager
        .try_join_coalition(candidate, coalition, &apply, &ctx)
        .await
        .expect("try_join");
    assert!(d.act);
    let members = manager.coalition_members(coalition).await.unwrap();
    assert_eq!(members.len(), 2);
    assert!(members.contains(&candidate));
}

// ---------------------------------------------------------------------------
// Feature-on: AifDecisionPolicy non-degeneracy through the actor
// ---------------------------------------------------------------------------

#[cfg(feature = "decision")]
#[tokio::test]
async fn aif_policy_join_non_degenerate_through_actor() {
    use koalisi::decision::AifDecisionPolicy;

    // required = 0b111 (3 capability bits). The seed member covers 0b010.
    let ctx = DecisionContext {
        required_capabilities: 0b111,
    };

    // Candidate covering a NEW required bit (0b001) broadens coalition coverage
    // 1/3 → 2/3 ⇒ joins.
    {
        let (manager, _seed, coalition) = manager_with_seed_coalition().await;
        let candidate = manager
            .add_agent(Worker::new(1, 0b001, 80))
            .await
            .expect("add candidate");
        let actor = CoalitionActor::spawn(CoalitionActorArgs {
            manager,
            policy: Box::new(AifDecisionPolicy::default()),
            ctx,
        });
        let decision = actor
            .ask(JoinRequest {
                agent: candidate,
                coalition,
            })
            .await
            .expect("decision");
        assert!(decision.act, "covering a new required bit lowers G ⇒ join");
        let members = actor
            .ask(Members { coalition })
            .await
            .expect("members");
        assert_eq!(members.len(), 2, "candidate added");
    }

    // Redundant candidate (same caps 0b010 as the seed) adds no coverage ⇒
    // does NOT join, and membership is unchanged.
    {
        let (manager, _seed, coalition) = manager_with_seed_coalition().await;
        let redundant = manager
            .add_agent(Worker::new(9, 0b010, 80))
            .await
            .expect("add redundant");
        let actor = CoalitionActor::spawn(CoalitionActorArgs {
            manager,
            policy: Box::new(AifDecisionPolicy::default()),
            ctx,
        });
        let decision = actor
            .ask(JoinRequest {
                agent: redundant,
                coalition,
            })
            .await
            .expect("decision");
        assert!(!decision.act, "redundant coverage ⇒ decline");
        let members = actor
            .ask(Members { coalition })
            .await
            .expect("members");
        assert_eq!(members.len(), 1, "declined join left membership unchanged");
    }
}

/// Issue #2 through the live #1 seam: belief structures (trust / compatibility)
/// fold into the decision. A capability-redundant candidate that the
/// pure-coverage policy declines now JOINS through the actor once the policy is
/// belief-aware and the partner is well-trusted.
#[cfg(feature = "decision")]
#[tokio::test]
async fn belief_aware_join_through_actor() {
    use koalisi::decision::{
        AifDecisionPolicy, BridgeParams, CompatibilityBeliefs, CoalitionHistory, TrustBeliefs,
    };

    let ctx = DecisionContext {
        required_capabilities: 0b111,
    };
    // Seed member has agent_id 2 (Worker::new(2, 0b010, 90)); the candidate
    // (agent_id 9) is capability-redundant with it.
    let (manager, _seed, coalition) = manager_with_seed_coalition().await;
    let redundant = manager
        .add_agent(Worker::new(9, 0b010, 80))
        .await
        .expect("add redundant");

    // Strong trust in partner 2 and high pairwise compatibility (9,2).
    let mut trust = TrustBeliefs::new();
    for _ in 0..200 {
        trust.update(2, 0.95);
    }
    let mut compat = CompatibilityBeliefs::new();
    compat.set(9, 2, 0.95);

    let policy = AifDecisionPolicy::with_beliefs(
        BridgeParams {
            belief_weight: 0.9,
            ..BridgeParams::default()
        },
        0.0,
        trust,
        compat,
        CoalitionHistory::new(),
    );

    let actor = CoalitionActor::spawn(CoalitionActorArgs {
        manager,
        policy: Box::new(policy),
        ctx,
    });

    let decision = actor
        .ask(JoinRequest {
            agent: redundant,
            coalition,
        })
        .await
        .expect("decision");
    assert!(
        decision.act,
        "high trust+compat lets the redundant agent join through the actor"
    );
    let members = actor
        .ask(Members { coalition })
        .await
        .expect("members");
    assert_eq!(members.len(), 2, "belief-aware join was applied");
    assert!(members.contains(&redundant));
}
