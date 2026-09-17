//! The K7-1 X-host gate (features `harness,decision,process`): the group arm
//! hosted by `run_workflow_instance` through the two trait lifecycle hooks,
//! over `WorkflowSpec::default()` on seeds 330..360 with
//! `OutcomeSignal::RoleCoverage`, one fresh policy per seed, against the frozen
//! archive binary's committed run, `docs/runs/K4-archive.log`.
//!
//! - `grp-role` (`GroupAifConfig::default()`): the summary row at line 2032
//!   (median PRIMARY `0.2270`, median churn `173.00`) and the candidate-reach
//!   row at line 2071.
//! - `grp-role-blind` (`CoverageMasks::RoleBlind`): the summary row at line
//!   2036 (median PRIMARY `0.0268`, median churn `185.50`).

use std::collections::HashMap;

use koalisi::decision::{
    AgreementSample, CoverageMasks, GroupAifConfig, GroupAifCounters, GroupAifPolicy,
};
use koalisi::harness::{
    OutcomeSignal, SeedRange, WorkflowInstance, WorkflowResult, WorkflowSpec, median_iqr,
    run_workflow_instance,
};
use koalisi::process::Role;

const SEEDS_330_360: SeedRange = SeedRange {
    start: 330,
    end: 360,
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

/// One fresh `config` arm per seed of 330..360, hosted by
/// `run_workflow_instance`: the per-seed results and counter ledgers.
fn host(config: GroupAifConfig) -> (Vec<WorkflowResult>, Vec<GroupAifCounters>) {
    let spec = WorkflowSpec::default();
    let mut latencies = Vec::new();
    let mut results = Vec::new();
    let mut counters = Vec::new();
    for seed in SEEDS_330_360.iter() {
        let inst = spec
            .generate(seed)
            .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        let policy = GroupAifPolicy::new(seed, config, role_map(&inst))
            .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        let result =
            run_workflow_instance(&policy, &inst, OutcomeSignal::RoleCoverage, &mut latencies)
                .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        let c = policy.counters();
        assert_eq!(
            c.begin_task_rejections, 0,
            "seed {seed}: the hook refused {} tasks",
            c.begin_task_rejections
        );
        assert!(
            c.s_learn_exact(),
            "seed {seed}: update ledger {:?}, {} tasks opened, {} observed",
            c.model_updates,
            c.roster_sizes.len(),
            c.tasks_observed
        );
        results.push(result);
        counters.push(c);
    }
    (results, counters)
}

/// `(median PRIMARY at 4 dp, median churn at 2 dp)`.
fn medians(results: &[WorkflowResult]) -> (String, String) {
    let primary: Vec<f64> = results.iter().map(|r| r.primary).collect();
    let churn: Vec<f64> = results.iter().map(|r| r.churn as f64).collect();
    (
        format!("{:.4}", median_iqr(&primary).0),
        format!("{:.2}", median_iqr(&churn).0),
    )
}

/// The candidate-reach row as the archive binary prints it: zero
/// candidate-sensitive share, mean blind internals, mean blind CW weight
/// share, leave `cfg0 == cfg1` of leave queries, act rate sensitive / blind.
fn reach_row(counters: &[GroupAifCounters]) -> String {
    let samples: Vec<AgreementSample> = counters
        .iter()
        .flat_map(|c| c.agreement.iter().copied())
        .collect();
    let n = samples.len() as f64;
    let zero_sensitive = samples
        .iter()
        .filter(|s| s.candidate_sensitive == 0)
        .count();
    let mean_blind = samples
        .iter()
        .map(|s| s.candidate_blind() as f64)
        .sum::<f64>()
        / n;
    let mean_blind_weight = samples.iter().map(|s| s.blind_weight_share).sum::<f64>() / n;
    let sensitive_total: usize = samples.iter().map(|s| s.candidate_sensitive).sum();
    let sensitive_acts: usize = samples.iter().map(|s| s.votes_for_act_sensitive).sum();
    let blind_total: usize = samples.iter().map(AgreementSample::candidate_blind).sum();
    let blind_acts: usize = samples.iter().map(|s| s.votes_for_act_blind).sum();
    let (queries, identical) = counters.iter().fold((0u64, 0u64), |(q, i), c| {
        (q + c.leave_queries, i + c.leave_queries_identical)
    });
    format!(
        "{:.1} % | {:.2} | {:.3} | {identical} of {queries} | {:.1} % / {:.1} %",
        100.0 * zero_sensitive as f64 / n,
        mean_blind,
        mean_blind_weight,
        100.0 * sensitive_acts as f64 / sensitive_total as f64,
        100.0 * blind_acts as f64 / blind_total as f64,
    )
}

#[test]
fn hosted_grp_role_reproduces_the_archive_rows() {
    let (results, counters) = host(GroupAifConfig::default());
    assert_eq!(results.len(), 30);
    let observed = medians(&results);
    assert_eq!(
        observed,
        ("0.2270".to_string(), "173.00".to_string()),
        "grp-role over 330..360: observed (median PRIMARY, median churn) {observed:?}, \
         docs/runs/K4-archive.log:2032 says (0.2270, 173.00)"
    );
    let observed = reach_row(&counters);
    let expected = "23.0 % | 1.35 | 0.626 | 9671 of 13107 | 80.4 % / 95.1 %";
    assert_eq!(
        observed, expected,
        "grp-role candidate reach over 330..360: observed `{observed}`, \
         docs/runs/K4-archive.log:2071 says `{expected}`"
    );
}

#[test]
fn hosted_grp_role_blind_reproduces_the_archive_row() {
    let (results, _) = host(GroupAifConfig {
        masks: CoverageMasks::RoleBlind,
        ..GroupAifConfig::default()
    });
    assert_eq!(results.len(), 30);
    let observed = medians(&results);
    assert_eq!(
        observed,
        ("0.0268".to_string(), "185.50".to_string()),
        "grp-role-blind over 330..360: observed (median PRIMARY, median churn) {observed:?}, \
         docs/runs/K4-archive.log:2036 says (0.0268, 185.50)"
    );
}
