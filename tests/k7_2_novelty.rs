//! The K7-2 S-nov gate (features `harness,decision,process`): on a hand-built
//! fixture, `query_novelty` on against off under candidate-star routing at
//! λ = ½, through the `CoalitionDecisionPolicy` trait hooks, one fresh
//! `GroupAifPolicy` per read.
//!
//! The fixture: agent 0 holds bits 0 and 1 at role 0, agent 1 holds bit 0 at
//! role 0, agent 2 holds bit 2 at role 1; the task declares the steps
//! `(0, r0)`, `(1, r0)`, `(2, r1)`. The leave read is agent 1 out of
//! `{0, 1, 2}` — its removal changes no covered step. The join read is agent
//! 0 into `{1, 2}` — it adds the covered step `(1, r0)`.

#![cfg(all(feature = "harness", feature = "decision", feature = "process"))]

use std::collections::HashMap;

use koalisi::algorithms::{AgentCapabilities, CapabilityAgent};
use koalisi::decision::{
    CoalitionDecisionPolicy, DecisionContext, GroupAifConfig, GroupAifPolicy, PersistentAifConfig,
    TaskStart, VoteRouting, v5_e1_base,
};
use koalisi::process::Role;

const REQUIRED: u32 = 0b111;
const STEPS: [(u8, u8); 3] = [(0, 0), (1, 0), (2, 1)];
const BATTERY_SEED: u64 = 11;

/// Which of the fixture's two reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Read {
    /// Agent 1 out of `{0, 1, 2}`.
    LeaveRedundant,
    /// Agent 0 into `{1, 2}`.
    JoinAddsStep,
}

/// Candidate-star routing at λ = ½ over the v5 E1 base with `query_novelty`
/// as given.
fn routed(query_novelty: bool) -> GroupAifConfig {
    GroupAifConfig {
        routing: VoteRouting::CandidateStar { lambda: 0.5 },
        base: PersistentAifConfig {
            query_novelty,
            ..v5_e1_base()
        },
        ..GroupAifConfig::default()
    }
}

/// `(act, raw score bits)` of `read` on a fresh `config` policy with the
/// fixture's task begun through the trait hook.
fn measure(config: GroupAifConfig, read: Read) -> (bool, u64) {
    let agents = [
        CapabilityAgent::new(0, 0b011, 50),
        CapabilityAgent::new(1, 0b001, 50),
        CapabilityAgent::new(2, 0b100, 50),
    ];
    let roles: HashMap<usize, Role> = [(0, Role::new(0)), (1, Role::new(0)), (2, Role::new(1))]
        .into_iter()
        .collect();
    let policy = GroupAifPolicy::new(BATTERY_SEED, config, roles)
        .unwrap_or_else(|e| panic!("{read:?}: {e}"));
    let hooks: &dyn CoalitionDecisionPolicy = &policy;
    hooks.begin_task(&TaskStart {
        required: REQUIRED,
        steps: &STEPS,
    });
    let ctx = DecisionContext {
        required_capabilities: REQUIRED,
    };
    let decision = match read {
        Read::LeaveRedundant => {
            let coalition: [&dyn AgentCapabilities; 3] = [&agents[0], &agents[1], &agents[2]];
            hooks.should_leave(&agents[1], &coalition, &ctx)
        }
        Read::JoinAddsStep => {
            let coalition: [&dyn AgentCapabilities; 2] = [&agents[1], &agents[2]];
            hooks.should_join(&agents[0], &coalition, &ctx)
        }
    };

    let c = policy.counters();
    assert_eq!(
        (
            c.begin_task_rejections,
            c.decisions,
            c.reads,
            c.routed_reads
        ),
        (0, 1, 1, 1),
        "{read:?}: (begin_task_rejections, decisions, reads, routed_reads) — the read must reach \
         the engine through a non-identity topology; counters {c:?}"
    );
    let sample = c.agreement[0];
    assert_eq!(
        (sample.roster, sample.candidate_sensitive, sample.leave),
        (2, 1, read == Read::LeaveRedundant),
        "{read:?}: (roster, candidate_sensitive, leave) of the read's sample {sample:?}"
    );
    (decision.act, decision.score.to_bits())
}

/// `(act, raw score bits)` of one read.
type Measured = (bool, u64);

/// `"(act, score, 0x<bits>)"`.
fn show((act, bits): Measured) -> String {
    format!("({act}, {:?}, {bits:#018x})", f64::from_bits(bits))
}

#[test]
fn novelty_reaches_the_routed_read_and_the_four_values_are_pinned() {
    let leave_on = measure(routed(true), Read::LeaveRedundant);
    let leave_off = measure(routed(false), Read::LeaveRedundant);
    let join_on = measure(routed(true), Read::JoinAddsStep);
    let join_off = measure(routed(false), Read::JoinAddsStep);

    let differing = usize::from(leave_on.1 != leave_off.1) + usize::from(join_on.1 != join_off.1);
    assert!(
        differing >= 1,
        "raw score bits differ between novelty on and off on {differing} of 2 reads, expected \
         >= 1: leave on {} off {}, join on {} off {}",
        show(leave_on),
        show(leave_off),
        show(join_on),
        show(join_off)
    );

    let pins: [(&str, Measured, Measured); 4] = [
        (
            "leave / novelty on",
            leave_on,
            (true, 0x3fe0_0000_0000_0000),
        ),
        (
            "leave / novelty off",
            leave_off,
            (true, 0x3fdf_fffd_4d04_8564),
        ),
        ("join / novelty on", join_on, (true, 0x3fe0_0000_0000_0000)),
        (
            "join / novelty off",
            join_off,
            (true, 0x3fdf_ffff_837f_4f34),
        ),
    ];
    let report: Vec<String> = pins
        .iter()
        .map(|&(label, observed, expected)| {
            format!(
                "{label}: observed {}, pinned {}",
                show(observed),
                show(expected)
            )
        })
        .collect();
    assert!(
        pins.iter()
            .all(|&(_, observed, expected)| observed == expected),
        "{}",
        report.join("; ")
    );
}
