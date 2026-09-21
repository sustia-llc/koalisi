//! `harness::RefPruneId` (features `harness,process`): its join and leave acts
//! and `(p, n)` records over five hand-written tasks on a six-agent fixture,
//! driven by the loop `run_workflow_instance` documents (first arrival joins,
//! each later arrival joins iff `should_join` acts, one leave sweep in arrival
//! order) with hand-written member outcomes; the records it keeps for an
//! empty-mask, an unshown and an unmapped member; and, over the instances
//! `WorkflowSpec::default()` with the performance draw `0.7 / 0.05 / 0.40`
//! generates on seeds 9000..9030, one fresh `RefPruneId` and one fresh
//! `harness::RefPrune` per seed, each traced under `OutcomeSignal::Both` and
//! its trace reconstructed, the order of the two median performance-scored
//! PRIMARYs.

use std::cmp::Ordering;
use std::collections::HashMap;

use koalisi::algorithms::{AgentCapabilities, CapabilityAgent};
use koalisi::decision::{CoalitionDecisionPolicy, DecisionContext, MemberOutcome, TaskStart};
use koalisi::harness::{
    OutcomeSignal, PerformanceSpec, Recon, RefPrune, RefPruneId, SeedRange, TracedPolicy,
    WorkflowInstance, WorkflowSpec, median_iqr, member_set_identity, reconstruct,
    run_workflow_instance, superior_count,
};
use koalisi::process::Role;

const REQUIRED: u32 = 0b111;
const STEPS: [(u8, u8); 3] = [(0, 0), (1, 0), (2, 1)];
/// The per-bit outcome every `observe_outcome` call here passes: bits 0, 1
/// and 2 succeed, the other five fail.
const OUTCOME: [bool; 8] = [true, true, true, false, false, false, false, false];

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

fn views<'a>(agents: &'a [CapabilityAgent], ids: &[usize]) -> Vec<&'a dyn AgentCapabilities> {
    ids.iter()
        .map(|&i| &agents[i] as &dyn AgentCapabilities)
        .collect()
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

/// The R0 block.
const R0_SEEDS: SeedRange = SeedRange {
    start: 9000,
    end: 9030,
};

/// `inst.role_map()` with each role id as a `Role`.
fn role_map(inst: &WorkflowInstance) -> HashMap<usize, Role> {
    inst.role_map()
        .into_iter()
        .map(|(id, role)| {
            let index = u8::try_from(role).expect("invariant: the spec draws roles below a u8");
            (id, Role::new(index))
        })
        .collect()
}

/// `policy` traced over `inst` under `OutcomeSignal::Both`: the run's
/// performance-scored PRIMARY and the trace's reconstruction.
fn traced_run(
    label: &str,
    policy: &dyn CoalitionDecisionPolicy,
    inst: &WorkflowInstance,
) -> (f64, Recon) {
    let seed = inst.seed;
    let traced = TracedPolicy::new(policy);
    let mut lat = Vec::new();
    let result = run_workflow_instance(&traced, inst, OutcomeSignal::Both, &mut lat)
        .unwrap_or_else(|e| panic!("{label} seed {seed}: {e}"));
    let scored = result
        .performance_scored
        .unwrap_or_else(|| panic!("{label} seed {seed}: performance_scored is None"));
    let recon = reconstruct(inst, &traced.entries())
        .unwrap_or_else(|e| panic!("{label} seed {seed}: the trace does not reconstruct: {e}"));
    (scored.primary, recon)
}

#[test]
fn r0_ref_prune_id_against_ref_prune_off_block() {
    let spec = WorkflowSpec {
        performance: Some(PerformanceSpec {
            reliable_prob: 0.7,
            rho_reliable: 0.05,
            rho_flaky: 0.40,
        }),
        ..WorkflowSpec::default()
    };
    let mut p_id: Vec<f64> = Vec::new();
    let mut p_prune: Vec<f64> = Vec::new();
    let (mut same_tasks, mut tasks) = (0usize, 0usize);
    for seed in R0_SEEDS.iter() {
        let inst = spec
            .generate(seed)
            .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        let (primary_id, recon_id) =
            traced_run("ref-prune-id", &RefPruneId::new(role_map(&inst)), &inst);
        let (primary_prune, recon_prune) =
            traced_run("ref-prune", &RefPrune::new(role_map(&inst)), &inst);
        let (same, total) = member_set_identity(&recon_id, &recon_prune);
        same_tasks += same;
        tasks += total;
        p_id.push(primary_id);
        p_prune.push(primary_prune);
    }
    let median_id = median_iqr(&p_id).0;
    let median_prune = median_iqr(&p_prune).0;
    let same_seeds = p_id
        .iter()
        .zip(&p_prune)
        .filter(|(a, b)| a.to_bits() == b.to_bits())
        .count();
    let sup_id = superior_count(&p_id, &p_prune);
    let sup_prune = superior_count(&p_prune, &p_id);
    let line = format!(
        "median performance-scored PRIMARY ref-prune-id {median_id:.4} ({:#018x}) ref-prune \
         {median_prune:.4} ({:#018x}); final member sets identical on {same_tasks} of {tasks} \
         tasks; PRIMARY bit-identical on {same_seeds} of {} seeds; ref-prune-id above on \
         {sup_id} seeds, ref-prune above on {sup_prune}",
        median_id.to_bits(),
        median_prune.to_bits(),
        R0_SEEDS.len()
    );
    println!("{line}");
    assert_eq!(
        median_id.partial_cmp(&median_prune),
        Some(Ordering::Greater),
        "{line}"
    );
}
