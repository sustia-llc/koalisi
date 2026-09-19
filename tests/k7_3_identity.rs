//! The K7-3 S-id gate (features `harness,decision,process`), on one hand-built
//! fixture driven through the `CoalitionDecisionPolicy` trait hooks; no
//! generated instance is used.
//!
//! The fixture: agents 0, 1 and 4 hold bits 0 and 1 at role 0; agent 2 holds
//! bit 2 at role 1; agent 3 holds bit 0 at role 1; agent 5 holds bit 0 at
//! role 0. The task declares the steps `(0, r0)`, `(1, r0)`, `(2, r1)`, so
//! agents 0 and 1 are role-mates with identical capabilities and agent 3
//! holds no demanded bit of its role.
//!
//! The group reads are the leave reads of agent 0 and of agent 1 out of
//! `{0, 1, 2}`, under candidate-star routing at λ = ½ over the v5 E1 base,
//! on `WorldModelTopology::AgentKeyed` and on
//! `WorldModelTopology::RoleSpecialised`, one fresh `GroupAifPolicy` per
//! read: on a policy that has observed no task, and on a policy that has
//! observed one task in which agents 0..=3 were each read for leave out of
//! `{0, 1, 2, 3}` and then reported as final members, agents 0 and 2
//! performing and agents 1 and 3 not. One further `AgentKeyed` read is
//! pinned: agent 4 out of `{1, 2, 4}` after the same task. The agent models'
//! applied-update counts and pA rows after that task are compared with
//! hand-derived values and with an untouched agent's model.
//!
//! `RefPruneId` is run over five hand-written tasks by the loop
//! `run_workflow_instance` documents (first arrival joins, each later arrival
//! joins iff `should_join` acts, one leave sweep in arrival order), with
//! hand-written member outcomes.

#![cfg(all(feature = "harness", feature = "decision", feature = "process"))]

use std::collections::HashMap;

use koalisi::algorithms::{AgentCapabilities, CapabilityAgent};
use koalisi::decision::{
    CoalitionDecisionPolicy, DecisionContext, GroupAifConfig, GroupAifPolicy, MemberOutcome,
    ModelLabel, TaskStart, VoteRouting, WorldModelTopology,
};
use koalisi::harness::{RefPrune, RefPruneId};
use koalisi::process::Role;

const REQUIRED: u32 = 0b111;
const STEPS: [(u8, u8); 3] = [(0, 0), (1, 0), (2, 1)];
const BATTERY_SEED: u64 = 11;
/// The warm task's per-bit outcome as `OutcomeSignal::Both` scores it: each of
/// the three steps has a final member of its role holding its bit that
/// performed (agent 0 for bits 0 and 1, agent 2 for bit 2).
const OUTCOME: [bool; 8] = [true, true, true, false, false, false, false, false];
/// The warm task's final members: agents 0 and 2 performed, 1 and 3 did not.
const WARM_MEMBERS: [MemberOutcome; 4] = [
    member(0, true),
    member(1, false),
    member(2, true),
    member(3, false),
];

const fn member(agent_id: usize, performed: bool) -> MemberOutcome {
    MemberOutcome {
        agent_id,
        performed,
    }
}

/// The fixture's agents, index = agent id.
fn agents() -> [CapabilityAgent; 6] {
    [
        CapabilityAgent::new(0, 0b011, 50),
        CapabilityAgent::new(1, 0b011, 50),
        CapabilityAgent::new(2, 0b100, 50),
        CapabilityAgent::new(3, 0b001, 50),
        CapabilityAgent::new(4, 0b011, 50),
        CapabilityAgent::new(5, 0b001, 50),
    ]
}

/// The fixture's role map: agents 0, 1, 4, 5 at role 0; agents 2, 3 at role 1.
fn roles() -> HashMap<usize, Role> {
    [(0, 0), (1, 0), (2, 1), (3, 1), (4, 0), (5, 0)]
        .into_iter()
        .map(|(id, r)| (id, Role::new(r)))
        .collect()
}

/// Candidate-star routing at λ = ½ over the default cell with `topology`.
fn routed(topology: WorldModelTopology) -> GroupAifConfig {
    GroupAifConfig {
        topology,
        routing: VoteRouting::CandidateStar { lambda: 0.5 },
        ..GroupAifConfig::default()
    }
}

fn views<'a>(agents: &'a [CapabilityAgent], ids: &[usize]) -> Vec<&'a dyn AgentCapabilities> {
    ids.iter()
        .map(|&i| &agents[i] as &dyn AgentCapabilities)
        .collect()
}

/// `(act, raw score bits)` of one read.
type Measured = (bool, u64);

/// The measured leave read on a policy that has observed no task.
const FRESH: Measured = (true, 0x3fe0_0000_0000_0000);
/// The measured warmed leave read whose centre queried a model that observed
/// bits 0 and 1 succeeding: the role-0 model, or agent 0's model.
const WARM_SUCCESS: Measured = (true, 0x3fdf_ffff_f9df_29c8);
/// The measured warmed leave read whose centre queried a model that observed
/// bits 0 and 1 failing: agent 1's model.
const WARM_FAILURE: Measured = (true, 0x3fdf_ffff_fb2e_8100);
/// The measured warmed leave read whose centre queried a model that observed
/// nothing: agent 4's model.
const WARM_UNOBSERVED: Measured = (true, 0x3fdf_ffff_fb2e_8100);

/// `"(act, score, 0x<bits>)"`.
fn show((act, bits): Measured) -> String {
    format!("({act}, {:?}, {bits:#018x})", f64::from_bits(bits))
}

/// The leave read of `candidate` out of `{candidate, its role-mate, 2}` — the
/// role-mate is agent 1 for candidates 0 and 4, agent 0 for candidate 1 — on
/// a fresh `topology` policy, after the warm task when `warmed`; with it, the
/// policy's `agent_model_updates()` after the read.
fn leave_read(
    topology: WorldModelTopology,
    candidate: usize,
    warmed: bool,
) -> (Measured, Vec<(usize, u64)>) {
    let mate = usize::from(candidate != 1);
    let agents = agents();
    let policy = GroupAifPolicy::new(BATTERY_SEED, routed(topology), roles())
        .unwrap_or_else(|e| panic!("{topology:?}: {e}"));
    let hooks: &dyn CoalitionDecisionPolicy = &policy;
    let task = TaskStart {
        required: REQUIRED,
        steps: &STEPS,
    };
    let ctx = DecisionContext {
        required_capabilities: REQUIRED,
    };
    let mut reads = 0u64;
    if warmed {
        hooks.begin_task(&task);
        let coalition = views(&agents, &[0, 1, 2, 3]);
        for agent in &agents[..4] {
            let _ = hooks.should_leave(agent, &coalition, &ctx);
            reads += 1;
        }
        hooks.observe_outcome(REQUIRED, &OUTCOME, &WARM_MEMBERS);
    }
    hooks.begin_task(&task);
    let mut shown = [candidate, mate, 2];
    shown.sort_unstable();
    let coalition = views(&agents, &shown);
    let decision = hooks.should_leave(&agents[candidate], &coalition, &ctx);
    reads += 1;

    let c = policy.counters();
    assert_eq!(
        (
            c.begin_task_rejections,
            c.decisions,
            c.reads,
            c.routed_reads,
            c.tasks_observed,
            c.outcome_updates_unapplied
        ),
        (0, reads, reads, reads, u64::from(warmed), 0),
        "{topology:?} candidate {candidate} warmed {warmed}: (begin_task_rejections, decisions, \
         reads, routed_reads, tasks_observed, outcome_updates_unapplied); counters {c:?}"
    );
    let sample = c.agreement[c.agreement.len() - 1];
    assert_eq!(
        (
            sample.roster,
            sample.candidate_sensitive,
            sample.leave,
            c.leave_queries,
            c.leave_queries_identical
        ),
        (2, 1, true, 2 * reads, 2 * reads - 2 * u64::from(warmed)),
        "{topology:?} candidate {candidate} warmed {warmed}: (roster, candidate_sensitive, leave) \
         of the read's sample and (leave_queries, leave_queries_identical) — two queries per \
         read, with identical masks but for the role-1 queries of the warm task's reads of \
         agents 2 and 3; sample {sample:?}"
    );
    (
        (decision.act, decision.score.to_bits()),
        policy.agent_model_updates(),
    )
}

#[test]
fn the_agent_keyed_centre_read_separates_role_mates_and_the_role_keyed_read_does_not() {
    use WorldModelTopology::{AgentKeyed, RoleSpecialised};

    let id_fresh = [
        leave_read(AgentKeyed, 0, false),
        leave_read(AgentKeyed, 1, false),
    ];
    let topo_fresh = [
        leave_read(RoleSpecialised, 0, false),
        leave_read(RoleSpecialised, 1, false),
    ];
    let id_warm = [
        leave_read(AgentKeyed, 0, true),
        leave_read(AgentKeyed, 1, true),
    ];
    let topo_warm = [
        leave_read(RoleSpecialised, 0, true),
        leave_read(RoleSpecialised, 1, true),
    ];
    // Agent 4: a role-mate of identical capabilities whose model observed
    // nothing. Pinned as measured; no predicate is stated over it.
    let id_warm_unobserved = leave_read(AgentKeyed, 4, true);

    let mut failures: Vec<String> = Vec::new();

    // S-id: under the agent-keyed store the performer's and the
    // non-performer's leave reads differ in raw score bits.
    if id_warm[0].0.1 == id_warm[1].0.1 {
        failures.push(format!(
            "AgentKeyed: the warmed leave reads of the performer and the non-performer are \
             bit-equal, expected to differ: agent 0 {} agent 1 {}",
            show(id_warm[0].0),
            show(id_warm[1].0)
        ));
    }
    // S-id: under the role-keyed store the two reads are bit-equal.
    if topo_warm[0].0 != topo_warm[1].0 {
        failures.push(format!(
            "RoleSpecialised: the warmed leave reads of the two role-mates differ, expected \
             bit-equal: agent 0 {} agent 1 {}",
            show(topo_warm[0].0),
            show(topo_warm[1].0)
        ));
    }
    // Before any task is observed the two stores read alike.
    for c in 0..2 {
        if id_fresh[c].0 != topo_fresh[c].0 {
            failures.push(format!(
                "fresh leave read of agent {c}: AgentKeyed {} RoleSpecialised {}, expected \
                 bit-equal",
                show(id_fresh[c].0),
                show(topo_fresh[c].0)
            ));
        }
    }

    // One applied update per warm-task member with a non-empty mask: agents
    // 0, 1, 2. Agent 3 holds no demanded bit of its role; agents 4 and 5 were
    // not members.
    let expected_warm: Vec<(usize, u64)> = vec![(0, 1), (1, 1), (2, 1), (3, 0), (4, 0), (5, 0)];
    let expected_fresh: Vec<(usize, u64)> = (0..6).map(|id| (id, 0)).collect();
    for (label, observed, expected) in [
        ("AgentKeyed warmed, agent 0", &id_warm[0].1, &expected_warm),
        ("AgentKeyed warmed, agent 1", &id_warm[1].1, &expected_warm),
        ("AgentKeyed fresh, agent 0", &id_fresh[0].1, &expected_fresh),
        ("AgentKeyed fresh, agent 1", &id_fresh[1].1, &expected_fresh),
        ("RoleSpecialised warmed, agent 0", &topo_warm[0].1, &vec![]),
        ("RoleSpecialised fresh, agent 0", &topo_fresh[0].1, &vec![]),
    ] {
        if observed != expected {
            failures.push(format!(
                "{label}: agent_model_updates observed {observed:?}, expected {expected:?}"
            ));
        }
    }

    let pins: [(&str, Measured, Measured); 9] = [
        (
            "AgentKeyed warmed / agent 4 (never a member)",
            id_warm_unobserved.0,
            WARM_UNOBSERVED,
        ),
        ("AgentKeyed fresh / agent 0", id_fresh[0].0, FRESH),
        ("AgentKeyed fresh / agent 1", id_fresh[1].0, FRESH),
        ("RoleSpecialised fresh / agent 0", topo_fresh[0].0, FRESH),
        ("RoleSpecialised fresh / agent 1", topo_fresh[1].0, FRESH),
        (
            "AgentKeyed warmed / agent 0 (performed)",
            id_warm[0].0,
            WARM_SUCCESS,
        ),
        (
            "AgentKeyed warmed / agent 1 (did not)",
            id_warm[1].0,
            WARM_FAILURE,
        ),
        (
            "RoleSpecialised warmed / agent 0",
            topo_warm[0].0,
            WARM_SUCCESS,
        ),
        (
            "RoleSpecialised warmed / agent 1",
            topo_warm[1].0,
            WARM_SUCCESS,
        ),
    ];
    failures.extend(
        pins.iter()
            .filter(|&&(_, observed, expected)| observed != expected)
            .map(|&(label, observed, expected)| {
                format!(
                    "{label}: observed {}, pinned {}",
                    show(observed),
                    show(expected)
                )
            }),
    );
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

#[test]
fn an_agent_model_counts_one_update_per_observing_final_member() {
    let agents = agents();
    let policy = GroupAifPolicy::new(
        BATTERY_SEED,
        routed(WorldModelTopology::AgentKeyed),
        roles(),
    )
    .unwrap();
    let hooks: &dyn CoalitionDecisionPolicy = &policy;
    let ctx = DecisionContext {
        required_capabilities: REQUIRED,
    };

    // Task 1: agents 0..=3 shown, then reported as final members. Agent 3's
    // mask is empty; agent 4 is in the role map and named as a member but was
    // never shown to a decision; agent 9 is not in the role map.
    hooks.begin_task(&TaskStart {
        required: REQUIRED,
        steps: &STEPS,
    });
    let coalition = views(&agents, &[0, 1, 2, 3]);
    for agent in &agents[..4] {
        let _ = hooks.should_leave(agent, &coalition, &ctx);
    }
    let mut members = WARM_MEMBERS.to_vec();
    members.push(member(4, true));
    members.push(member(9, true));
    hooks.observe_outcome(REQUIRED, &OUTCOME, &members);
    let after_1 = policy.agent_model_updates();

    // Task 2 demands `(2, r1)` alone: role 0 has no demand, so agent 0
    // observes nothing; agent 2 observes; agents 1 and 3 are not members.
    hooks.begin_task(&TaskStart {
        required: 0b100,
        steps: &[(2, 1)],
    });
    hooks.observe_outcome(0b100, &[false; 8], &[member(0, true), member(2, false)]);
    let after_2 = policy.agent_model_updates();

    // A per-bit vector of the wrong width skips the task for every model.
    hooks.observe_outcome(0b100, &[true; 3], &[member(2, true)]);
    let after_3 = policy.agent_model_updates();

    let c = policy.counters();
    let role_ledger: Vec<(ModelLabel, u64, u64)> = c
        .model_updates
        .iter()
        .map(|a| (a.label, a.updates, a.expected))
        .collect();
    assert_eq!(
        (after_1, after_2, after_3),
        (
            vec![(0, 1), (1, 1), (2, 1), (3, 0), (4, 0), (5, 0)],
            vec![(0, 1), (1, 1), (2, 2), (3, 0), (4, 0), (5, 0)],
            vec![(0, 1), (1, 1), (2, 2), (3, 0), (4, 0), (5, 0)],
        ),
        "agent_model_updates after task 1, after task 2, after the wrong-width call: observed \
         (left) vs hand-derived (right)"
    );
    assert_eq!(
        (
            role_ledger,
            c.tasks_observed,
            c.outcome_updates_unapplied,
            c.s_learn_exact()
        ),
        (
            vec![
                (ModelLabel::Role(Role::new(0)), 1, 1),
                (ModelLabel::Role(Role::new(1)), 2, 2),
                (ModelLabel::Role(Role::new(2)), 0, 0),
            ],
            2,
            1,
            true
        ),
        "(role-model ledger as (label, updates, expected), tasks_observed, \
         outcome_updates_unapplied, s_learn_exact)"
    );
}

/// pA row indices of the persistent world model: the observation outcomes.
const ROWS: [&str; 3] = ["success", "failure", "no-observation"];

#[test]
fn an_agent_model_reads_its_mask_as_performed_and_every_other_bit_as_no_observation() {
    let agents = agents();
    let policy = GroupAifPolicy::new(
        BATTERY_SEED,
        routed(WorldModelTopology::AgentKeyed),
        roles(),
    )
    .unwrap();
    let hooks: &dyn CoalitionDecisionPolicy = &policy;
    let ctx = DecisionContext {
        required_capabilities: REQUIRED,
    };
    hooks.begin_task(&TaskStart {
        required: REQUIRED,
        steps: &STEPS,
    });
    let coalition = views(&agents, &[0, 1, 2, 3]);
    for agent in &agents[..4] {
        let _ = hooks.should_leave(agent, &coalition, &ctx);
    }
    hooks.observe_outcome(REQUIRED, &OUTCOME, &WARM_MEMBERS);

    let pa = |id: usize| {
        policy
            .agent_model_snapshot(id)
            .unwrap_or_else(|| panic!("agent {id} has no model"))
            .pa
            .unwrap_or_else(|| panic!("agent {id}'s model carries no pA"))
    };
    // Agent 5 was never shown and never a member: its model is at its prior.
    let prior = pa(5);
    assert_eq!(prior.len(), 8, "one pA matrix per bit");

    // Per agent: the mask it observed and the pA row its mask's bits grow in.
    // Agents 0 and 2 performed (row 0), agent 1 did not (row 1); agents 3
    // (empty mask) and 4 (not a member) observed nothing.
    let expected: [(usize, u32, usize); 5] = [
        (0, 0b011, 0),
        (1, 0b011, 1),
        (2, 0b100, 0),
        (3, 0, 0),
        (4, 0, 0),
    ];
    let mut failures: Vec<String> = Vec::new();
    for (id, mask, performed_row) in expected {
        let after = pa(id);
        for bit in 0..8 {
            // A bit of the mask grows in the performed row; with a non-empty
            // mask every other bit grows in the no-observation row; with an
            // empty mask no row of any bit moves.
            let grows = match (mask, mask >> bit & 1) {
                (0, _) => None,
                (_, 1) => Some(performed_row),
                _ => Some(2),
            };
            for (row, row_name) in ROWS.iter().enumerate() {
                let (a, p) = (after[bit].row(row), prior[bit].row(row));
                let (sum_a, sum_p) = (a.sum(), p.sum());
                let ok = if grows == Some(row) {
                    sum_a > sum_p
                } else {
                    a == p
                };
                if !ok {
                    failures.push(format!(
                        "agent {id} bit {bit} {row_name} row: sum {sum_a:?} against the prior's \
                         {sum_p:?}, expected {}",
                        if grows == Some(row) {
                            "above the prior"
                        } else {
                            "the prior's row, entry for entry"
                        }
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));

    let role_keyed = GroupAifPolicy::new(
        BATTERY_SEED,
        routed(WorldModelTopology::RoleSpecialised),
        roles(),
    )
    .unwrap();
    assert!(
        role_keyed.agent_model_snapshot(0).is_none() && policy.agent_model_snapshot(9).is_none(),
        "agent_model_snapshot is None under RoleSpecialised and for an unmapped agent"
    );
}

/// One hand-written task of the `RefPruneId` sequence.
struct PruneTask {
    arrival: &'static [usize],
    /// Expected `should_join` acts, one per arrival after the first.
    joins: &'static [bool],
    /// Expected `should_leave` acts as `(agent, act)`, in sweep order.
    leaves: &'static [(usize, bool)],
    /// The final members and whether each performed, in membership order.
    outcomes: &'static [(usize, bool)],
    /// Expected `(agent, (p, n))` after the task's `observe_outcome`.
    records: &'static [(usize, (u64, u64))],
}

/// Hand-derived. Records read `(p + 1) / (n + 2)`.
///
/// 1. Every record is 1/2, so the acts are `RefPrune`'s: agent 0 is redundant
///    beside agent 1 and the tie evicts it; agent 3 covers no step and leaves.
/// 2. Agent 1 (1/3) is redundant beside agent 0 (1/2 ≥ 1/3) and leaves.
/// 3. Agent 0 (2/3) is redundant beside agent 1 (1/3 < 2/3) and stays — where
///    `RefPrune` evicts it; agent 1 then leaves (2/3 ≥ 1/3).
/// 4. Agent 0 at `(1, 2)` and agent 4 at `(0, 0)` both read 1/2 — cross
///    products 4 and 4 — and the tie evicts agent 0.
/// 5. Agent 0 (1/2) covers bits 0 and 1. Bit 0 has agent 5 (1/2); bit 1 has
///    only agent 1 (1/3), so agent 0 stays. Agent 1 leaves beside agent 0;
///    agent 5 at `(0, 0)` ties agent 0 at `(1, 2)` and leaves.
const PRUNE_TASKS: [PruneTask; 5] = [
    PruneTask {
        arrival: &[0, 1, 2, 3],
        joins: &[true, true, true],
        leaves: &[(0, true), (1, false), (2, false), (3, true)],
        outcomes: &[(1, false), (2, true)],
        records: &[(0, (0, 0)), (1, (0, 1)), (2, (1, 1)), (3, (0, 0))],
    },
    PruneTask {
        arrival: &[1, 0, 2],
        joins: &[true, true],
        leaves: &[(1, true), (0, false), (2, false)],
        outcomes: &[(0, true), (2, true)],
        records: &[(0, (1, 1)), (1, (0, 1)), (2, (2, 2))],
    },
    PruneTask {
        arrival: &[0, 1, 2],
        joins: &[true, true],
        leaves: &[(0, false), (1, true), (2, false)],
        outcomes: &[(0, false), (2, true)],
        records: &[(0, (1, 2)), (1, (0, 1)), (2, (3, 3))],
    },
    PruneTask {
        arrival: &[0, 4, 2],
        joins: &[true, true],
        leaves: &[(0, true), (4, false), (2, false)],
        outcomes: &[(4, true), (2, false)],
        records: &[(0, (1, 2)), (2, (3, 4)), (4, (1, 1))],
    },
    PruneTask {
        arrival: &[0, 1, 5, 2],
        joins: &[true, true, true],
        leaves: &[(0, false), (1, true), (5, true), (2, false)],
        outcomes: &[(0, true), (2, true)],
        records: &[
            (0, (2, 3)),
            (1, (0, 1)),
            (2, (4, 5)),
            (3, (0, 0)),
            (4, (1, 1)),
            (5, (0, 0)),
        ],
    },
];

/// One task's join acts, leave acts as `(agent, act)` and final members.
type Sweep = (Vec<bool>, Vec<(usize, bool)>, Vec<usize>);

/// One task of the documented loop over `arrival` with [`STEPS`] begun:
/// every score is asserted `0.0`.
fn sweep(
    policy: &dyn CoalitionDecisionPolicy,
    agents: &[CapabilityAgent],
    arrival: &[usize],
) -> Sweep {
    let ctx = DecisionContext {
        required_capabilities: REQUIRED,
    };
    policy.begin_task(&TaskStart {
        required: REQUIRED,
        steps: &STEPS,
    });
    let mut members = vec![arrival[0]];
    let mut joins = Vec::new();
    for &candidate in &arrival[1..] {
        let d = policy.should_join(&agents[candidate], &views(agents, &members), &ctx);
        assert_eq!(
            d.score.to_bits(),
            0.0f64.to_bits(),
            "join of agent {candidate}: score observed {:?}, expected 0.0",
            d.score
        );
        joins.push(d.act);
        if d.act {
            members.push(candidate);
        }
    }
    let mut leaves = Vec::new();
    for &idx in arrival {
        let Some(pos) = members.iter().position(|&m| m == idx) else {
            continue;
        };
        let d = policy.should_leave(&agents[idx], &views(agents, &members), &ctx);
        assert_eq!(
            d.score.to_bits(),
            0.0f64.to_bits(),
            "leave of agent {idx}: score observed {:?}, expected 0.0",
            d.score
        );
        leaves.push((idx, d.act));
        if d.act {
            members.remove(pos);
        }
    }
    (joins, leaves, members)
}

#[test]
fn ref_prune_id_acts_and_records_match_the_hand_derivation() {
    let agents = agents();
    let policy = RefPruneId::new(roles());
    let ctx = DecisionContext {
        required_capabilities: REQUIRED,
    };

    // Before any `begin_task` there is no step: every leave acts.
    let before = policy.should_leave(&agents[0], &views(&agents, &[0]), &ctx);
    assert!(before.act, "leave before any begin_task");

    for (t, task) in PRUNE_TASKS.iter().enumerate() {
        let (joins, leaves, members) = sweep(&policy, &agents, task.arrival);
        let expected_members: Vec<usize> = task.outcomes.iter().map(|&(id, _)| id).collect();
        assert_eq!(
            (joins.as_slice(), leaves.as_slice(), &members),
            (task.joins, task.leaves, &expected_members),
            "task {}: (join acts, leave acts as (agent, act), final members), observed vs \
             hand-derived",
            t + 1
        );
        if t == 0 {
            // A policy that has observed no task takes `RefPrune`'s acts.
            let prune = sweep(&RefPrune::new(roles()), &agents, task.arrival);
            assert_eq!(
                (joins, leaves, members),
                prune,
                "task 1: RefPruneId with every record at (0, 0) vs RefPrune"
            );
        }
        let outcomes: Vec<MemberOutcome> = task
            .outcomes
            .iter()
            .map(|&(id, performed)| member(id, performed))
            .collect();
        policy.observe_outcome(REQUIRED, &OUTCOME, &outcomes);
        let observed: Vec<(usize, (u64, u64))> = task
            .records
            .iter()
            .map(|&(id, _)| (id, policy.record(id)))
            .collect();
        assert_eq!(
            observed.as_slice(),
            task.records,
            "task {}: (agent, (p, n)) after observe_outcome, observed vs hand-derived",
            t + 1
        );
    }

    // Task 3's first leave read is where the two prunes part: `RefPrune`
    // evicts agent 0 out of `{0, 1, 2}`.
    let prune = RefPrune::new(roles());
    prune.begin_task(&TaskStart {
        required: REQUIRED,
        steps: &STEPS,
    });
    assert!(
        prune
            .should_leave(&agents[0], &views(&agents, &[0, 1, 2]), &ctx)
            .act,
        "RefPrune's leave of agent 0 out of {{0, 1, 2}}"
    );
}

#[test]
fn ref_prune_id_counts_nothing_for_an_empty_mask_an_unshown_or_an_unmapped_member() {
    let agents = agents();
    let policy = RefPruneId::new(roles());
    let ctx = DecisionContext {
        required_capabilities: REQUIRED,
    };
    policy.begin_task(&TaskStart {
        required: REQUIRED,
        steps: &STEPS,
    });
    // Shows agents 0 and 3. Agent 3 holds no demanded bit of its role; agent 4
    // is mapped and never shown; agent 9 is not in the role map.
    let _ = policy.should_join(&agents[3], &views(&agents, &[0]), &ctx);
    policy.observe_outcome(
        REQUIRED,
        &OUTCOME,
        &[
            member(0, true),
            member(3, true),
            member(4, true),
            member(9, true),
        ],
    );
    let observed: Vec<(u64, u64)> = [0, 3, 4, 9].iter().map(|&id| policy.record(id)).collect();
    assert_eq!(
        observed,
        vec![(1, 1), (0, 0), (0, 0), (0, 0)],
        "(p, n) of agents 0, 3, 4, 9"
    );
}
