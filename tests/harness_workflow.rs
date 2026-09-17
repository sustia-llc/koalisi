//! The H0 identity gate (features `harness,process`, koa#92): the v2w draw
//! and the role-matched scorer against the frozen archive binary's committed
//! run, `docs/runs/K4-archive.log`.
//!
//! - Per-seed rows: seeds 270..300 under the `wf-asis` control reproduce the
//!   thirty `(n, PRIMARY, churn asis)` rows at lines 1787–1816.
//! - Medians: seeds 330..360 reproduce the control's summary row at line
//!   2031 (median PRIMARY `0.1806`, median churn `7.50`).
//! - Fixture: `tests/fixtures/k7-workflow-v2w.txt` holds one printed record
//!   per seed of 270..300 and 330..360; regenerate with
//!   `K7_WRITE_FIXTURE=1 cargo test --features harness,process --test
//!   harness_workflow fixture_matches_the_committed_records`.
//! - Correction 2: the flat spec at `required_bits: 2..=8` reproduces the
//!   fixture's pool, arrivals and pre-re-draw required masks at seed 270.

use std::path::PathBuf;
use std::sync::Mutex;

use catgraph_applied::prop::colored::ColoredExpr;
use koalisi::algorithms::{AgentCapabilities, CapabilityAgent};
use koalisi::decision::{
    CoalitionDecisionPolicy, Decision, DecisionContext, MagnitudePolicy, RoleModulation,
};
use koalisi::harness::{
    InstanceSpec, OutcomeSignal, SeedRange, WorkflowArm, WorkflowInstance, WorkflowSpec,
    WorkflowTask, median_iqr, run_workflow_battery, run_workflow_instance,
};
use koalisi::process::{Role, StaffingTable, Step, demand, step_expr};

/// `docs/runs/K4-archive.log:1787–1816`: `(seed, n, wf-asis, churn asis)`.
const EXPECTED_270_300: [(u64, usize, &str, usize); 30] = [
    (270, 13, "0.1827", 10),
    (271, 7, "0.2404", 5),
    (272, 7, "0.1966", 6),
    (273, 6, "0.3018", 8),
    (274, 4, "0.2758", 2),
    (275, 15, "0.1210", 8),
    (276, 16, "0.1033", 14),
    (277, 13, "0.1668", 7),
    (278, 6, "0.2180", 1),
    (279, 15, "0.0873", 13),
    (280, 13, "0.0566", 3),
    (281, 8, "0.2899", 6),
    (282, 16, "0.2050", 8),
    (283, 5, "0.4592", 7),
    (284, 12, "0.1547", 5),
    (285, 15, "0.2148", 12),
    (286, 14, "0.1992", 9),
    (287, 6, "0.2004", 4),
    (288, 5, "0.3444", 4),
    (289, 15, "0.1815", 7),
    (290, 11, "0.2955", 2),
    (291, 8, "0.1701", 8),
    (292, 10, "0.1586", 3),
    (293, 4, "0.3440", 5),
    (294, 9, "0.1647", 6),
    (295, 7, "0.2985", 9),
    (296, 14, "0.1987", 6),
    (297, 6, "0.2197", 5),
    (298, 15, "0.0837", 24),
    (299, 12, "0.1555", 7),
];

/// `docs/runs/K4-archive.log:2031`: `wf-asis` over 330..360.
const EXPECTED_330_360_MEDIAN_PRIMARY: &str = "0.1806";
const EXPECTED_330_360_MEDIAN_CHURN: &str = "7.50";

const SEEDS_270_300: SeedRange = SeedRange {
    start: 270,
    end: 300,
};
const SEEDS_330_360: SeedRange = SeedRange {
    start: 330,
    end: 360,
};

/// `ρ = δ` over the spec's roles.
fn identity_rho(roles: u8) -> RoleModulation {
    let n = usize::from(roles);
    let rows: Vec<Vec<f64>> = (0..n)
        .map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect())
        .collect();
    RoleModulation::new(rows).expect("invariant: a square identity table")
}

/// The `wf-asis` control: the typed magnitude policy over the instance's role
/// map at `ρ = δ`.
fn control_arm(roles: u8) -> WorkflowArm {
    WorkflowArm {
        label: "wf-asis".into(),
        make: Box::new(move |inst: &WorkflowInstance| {
            Box::new(
                MagnitudePolicy::default()
                    .with_role_modulation(inst.role_map(), identity_rho(roles)),
            )
        }),
    }
}

#[test]
fn identity_gate_270_300_per_seed_rows() {
    let spec = WorkflowSpec::default();
    let result = run_workflow_battery(
        &control_arm(spec.roles),
        &spec,
        SEEDS_270_300,
        OutcomeSignal::RoleCoverage,
    )
    .expect("the registered spec generates every seed of 270..300");
    assert_eq!(result.per_seed.len(), 30);
    let mut mismatches = Vec::new();
    for (r, &(seed, n, primary, churn)) in result.per_seed.iter().zip(&EXPECTED_270_300) {
        let observed = (r.seed, r.n, format!("{:.4}", r.primary), r.churn);
        if observed != (seed, n, primary.to_string(), churn) {
            mismatches.push(format!(
                "seed {seed}: expected (n {n}, primary {primary}, churn {churn}), observed (n {}, primary {}, churn {})",
                observed.1, observed.2, observed.3
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of 30 rows differ from docs/runs/K4-archive.log:1787-1816:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

#[test]
fn identity_gate_330_360_medians() {
    let spec = WorkflowSpec::default();
    let result = run_workflow_battery(
        &control_arm(spec.roles),
        &spec,
        SEEDS_330_360,
        OutcomeSignal::RoleCoverage,
    )
    .expect("the registered spec generates every seed of 330..360");
    assert_eq!(result.per_seed.len(), 30);
    let primary: Vec<f64> = result.per_seed.iter().map(|r| r.primary).collect();
    let churn: Vec<f64> = result.per_seed.iter().map(|r| r.churn as f64).collect();
    let observed_primary = format!("{:.4}", median_iqr(&primary).0);
    let observed_churn = format!("{:.2}", median_iqr(&churn).0);
    assert_eq!(
        observed_primary, EXPECTED_330_360_MEDIAN_PRIMARY,
        "median PRIMARY over 330..360: observed {observed_primary}, docs/runs/K4-archive.log:2031 says {EXPECTED_330_360_MEDIAN_PRIMARY}"
    );
    assert_eq!(
        observed_churn, EXPECTED_330_360_MEDIAN_CHURN,
        "median churn over 330..360: observed {observed_churn}, docs/runs/K4-archive.log:2031 says {EXPECTED_330_360_MEDIAN_CHURN}"
    );
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("k7-workflow-v2w.txt")
}

/// The fixture's content as generated now: every record of 270..300 then
/// 330..360, each followed by one blank line.
fn generate_records() -> String {
    let spec = WorkflowSpec::default();
    let mut out = String::new();
    for seed in SEEDS_270_300.iter().chain(SEEDS_330_360.iter()) {
        let inst = spec
            .generate(seed)
            .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        out.push_str(&inst.to_string());
        out.push('\n');
    }
    out
}

#[test]
fn fixture_matches_the_committed_records() {
    let generated = generate_records();
    let path = fixture_path();
    if std::env::var_os("K7_WRITE_FIXTURE").is_some() {
        std::fs::write(&path, &generated).expect("write the fixture");
        return;
    }
    let committed =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    if committed != generated {
        let first = committed
            .lines()
            .zip(generated.lines())
            .position(|(a, b)| a != b);
        let (line_no, committed_line, generated_line) = match first {
            Some(i) => (
                i + 1,
                committed.lines().nth(i).unwrap_or(""),
                generated.lines().nth(i).unwrap_or(""),
            ),
            None => (
                committed.lines().count().min(generated.lines().count()) + 1,
                "<end>",
                "<end>",
            ),
        };
        panic!(
            "fixture {} differs from the generated records (committed {} lines, generated {} lines); first difference at line {line_no}:\n  committed: {committed_line}\n  generated: {generated_line}",
            path.display(),
            committed.lines().count(),
            generated.lines().count()
        );
    }
}

/// The lines of the fixture's record for `seed`.
fn fixture_record(seed: u64) -> Vec<String> {
    let committed = std::fs::read_to_string(fixture_path()).expect("read the fixture");
    let header = format!("seed {seed}");
    committed
        .split("\n\n")
        .find(|block| block.lines().next() == Some(header.as_str()))
        .unwrap_or_else(|| panic!("no record for seed {seed} in the fixture"))
        .lines()
        .map(str::to_string)
        .collect()
}

/// The `n`-th whitespace field of `line` as a `T`.
fn field<T: std::str::FromStr>(line: &str, n: usize) -> T
where
    T::Err: std::fmt::Debug,
{
    line.split_whitespace()
        .nth(n)
        .unwrap_or_else(|| panic!("field {n} missing in `{line}`"))
        .parse()
        .unwrap_or_else(|e| panic!("field {n} of `{line}`: {e:?}"))
}

#[test]
fn correction_2_flat_spec_reproduces_the_fixture_pool_and_tasks_at_seed_270() {
    let record = fixture_record(270);
    let prefix_required: Vec<u32> = field::<String>(&record[2], 1)
        .split(',')
        .map(|s| s.parse().expect("prefix_required mask"))
        .collect();
    let flat = InstanceSpec {
        required_bits: 2..=8,
        ..InstanceSpec::default()
    }
    .generate(270);

    let fixture_agents: Vec<(usize, u32, u32)> = record
        .iter()
        .filter(|l| l.starts_with("agent "))
        .map(|l| (field(l, 1), field(l, 3), field(l, 5)))
        .collect();
    let flat_agents: Vec<(usize, u32, u32)> = flat
        .agents
        .iter()
        .map(|a| (a.agent_id(), a.capabilities(), a.trust_level()))
        .collect();
    assert_eq!(
        flat_agents, fixture_agents,
        "seed 270: the flat pool differs from the fixture's"
    );

    let fixture_arrivals: Vec<Vec<usize>> = record
        .iter()
        .filter(|l| l.starts_with("task "))
        .map(|l| {
            field::<String>(l, 7)
                .split(',')
                .map(|s| s.parse().expect("arrival index"))
                .collect()
        })
        .collect();
    assert_eq!(fixture_arrivals.len(), flat.tasks.len());
    assert_eq!(prefix_required.len(), flat.tasks.len());
    for (t, task) in flat.tasks.iter().enumerate() {
        assert_eq!(
            (task.required, &task.arrival),
            (prefix_required[t], &fixture_arrivals[t]),
            "seed 270, task {t}: flat (required, arrival) differs from the fixture's (prefix_required, arrival)"
        );
    }
}

/// Joins every candidate, never leaves; records every `observe_outcome`.
#[derive(Default)]
struct AlwaysJoin {
    seen: Mutex<Vec<(u32, Vec<bool>)>>,
}

impl CoalitionDecisionPolicy for AlwaysJoin {
    fn observe_outcome(&self, required: u32, per_bit_success: &[bool]) {
        self.seen
            .lock()
            .expect("invariant: no panic holds the lock")
            .push((required, per_bit_success.to_vec()));
    }

    fn should_join(
        &self,
        _agent: &dyn AgentCapabilities,
        _coalition: &[&dyn AgentCapabilities],
        _ctx: &DecisionContext,
    ) -> Decision {
        Decision {
            act: true,
            score: 0.0,
        }
    }

    fn should_leave(
        &self,
        _agent: &dyn AgentCapabilities,
        _coalition: &[&dyn AgentCapabilities],
        _ctx: &DecisionContext,
    ) -> Decision {
        Decision {
            act: false,
            score: 0.0,
        }
    }
}

/// Two agents, one task demanding `s0_r1` alone: agent 0 holds bit 0 at
/// `holder_role`, agent 1 holds bit 1 at role 1.
fn two_agent_instance(holder_role: u8) -> WorkflowInstance {
    let r1 = Role::new(1);
    let agents = vec![
        CapabilityAgent::new(0, 0b01, 50),
        CapabilityAgent::new(1, 0b10, 50),
    ];
    let roles = vec![Role::new(holder_role), r1];
    let table = StaffingTable::from_pool(
        agents
            .iter()
            .zip(&roles)
            .map(|(a, &r)| (a.capabilities(), r)),
    );
    let written = ColoredExpr::new(vec![r1], step_expr(Step::new(0, r1)))
        .expect("invariant: a single r1 -> r1 step pins on [r1]");
    let demand = demand(&written);
    WorkflowInstance {
        seed: 0,
        agents,
        roles,
        tasks: vec![WorkflowTask {
            required: 0b01,
            tags: vec![r1, Role::new(0)],
            arrival: vec![0, 1],
            written,
            demand,
        }],
        prefix_required: vec![0b01],
        table,
        redraws: 0,
        max_attempts: 1,
        performance: None,
    }
}

#[test]
fn hand_derived_role_mismatched_holder_is_not_covered() {
    // Both agents join. Agent 0 holds bit 0 but at role 0; the demanded step
    // wants role 1, so nothing is covered: success 0, cov_eff 0/1/2 = 0.
    let policy = AlwaysJoin::default();
    let mut lat = Vec::new();
    let mismatched = run_workflow_instance(
        &policy,
        &two_agent_instance(0),
        OutcomeSignal::RoleCoverage,
        &mut lat,
    )
    .expect("no performance signal, so no error");
    assert_eq!(
        (
            mismatched.success_rate,
            mismatched.mean_cov_eff,
            mismatched.primary
        ),
        (0.0, 0.0, 0.0),
        "role-mismatched holder: observed {mismatched:?}"
    );
    assert_eq!(
        *policy
            .seen
            .lock()
            .expect("invariant: no panic holds the lock"),
        vec![(0b01, vec![false, false])],
        "per-bit signal"
    );

    // Same pool with agent 0 at role 1: covered 1 of 1 over 2 members.
    let policy = AlwaysJoin::default();
    let matched = run_workflow_instance(
        &policy,
        &two_agent_instance(1),
        OutcomeSignal::RoleCoverage,
        &mut lat,
    )
    .expect("no performance signal, so no error");
    assert_eq!(
        (matched.success_rate, matched.mean_cov_eff, matched.primary),
        (1.0, 0.5, 0.5),
        "role-matched holder: observed {matched:?}"
    );
    assert_eq!(
        *policy
            .seen
            .lock()
            .expect("invariant: no panic holds the lock"),
        vec![(0b01, vec![true, false])],
        "per-bit signal"
    );
    assert_eq!(lat.len(), 2 * 3, "one join and two leave calls per run");
}
