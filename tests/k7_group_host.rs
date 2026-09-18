//! The K7 X-host and X-carry gates (features `harness,decision,process`):
//! policies hosted by `run_workflow_instance` through the two trait lifecycle
//! hooks, over `WorkflowSpec::default()` with `OutcomeSignal::RoleCoverage`,
//! one fresh policy per seed, against committed runs.
//!
//! X-host, seeds 330..360 against `docs/runs/K4-archive.log`:
//!
//! - `grp-role` (`GroupAifConfig::default()`): the summary row at line 2032
//!   (median PRIMARY `0.2270`, median churn `173.00`) and the candidate-reach
//!   row at line 2071.
//! - `grp-role-blind` (`CoverageMasks::RoleBlind`): the summary row at line
//!   2036 (median PRIMARY `0.0268`, median churn `185.50`).
//! - `grp-role-nonov` (`query_novelty: false` over `v5_e1_base()`): the
//!   summary row at line 2037 (median PRIMARY `0.1125`, median churn
//!   `140.00`) and the candidate-reach row at line 2076.
//!
//! X-carry, seeds 90..120 against `docs/runs/K7-1.log`, each policy traced by
//! `TracedPolicy` and its trace replayed by `harness::reconstruct`:
//!
//! - `grp-role`, `grp-topo` (`VoteRouting::CandidateStar { lambda: 0.5 }`)
//!   and `harness::RefPrune`: the per-seed PRIMARY columns of lines 45–74 at
//!   4 dp and the summary rows at lines 32, 34 and 38 (median PRIMARY, median
//!   churn), with PRIMARY recomputed from the reconstruction bit-equal to
//!   `WorkflowResult::primary` on every seed.
//! - `grp-topo` against `ref-prune`: the identity row at line 164 (final
//!   member sets identical on 598 of 600 tasks, PRIMARY bit-identical on 28
//!   of 30 seeds).
//! - `harness::roster_decomposition` over `grp-topo`'s and `ref-prune`'s
//!   reconstructions: the roster rows all / 1 / 2 / 3 at lines 185–188 and
//!   201–204, every column.

use std::collections::HashMap;

use koalisi::decision::{
    AgreementSample, CoalitionDecisionPolicy, CoverageMasks, GroupAifConfig, GroupAifCounters,
    GroupAifPolicy, PersistentAifConfig, VoteRouting, v5_e1_base,
};
use koalisi::harness::{
    OutcomeSignal, Recon, RefPrune, SeedRange, TracedPolicy, WorkflowInstance, WorkflowResult,
    WorkflowSpec, median_iqr, member_set_identity, reconstruct, roster_decomposition,
    run_workflow_instance,
};
use koalisi::process::Role;

const SEEDS_330_360: SeedRange = SeedRange {
    start: 330,
    end: 360,
};

const SEEDS_90_120: SeedRange = SeedRange {
    start: 90,
    end: 120,
};

/// `docs/runs/K7-1.log` line of seed 90's per-seed row; seed `s` is on line
/// `K7_1_FIRST_SEED_LINE + (s - 90)`.
const K7_1_FIRST_SEED_LINE: u64 = 45;

/// `docs/runs/K7-1.log:45`–`:74`, column `grp-role`, seeds 90..120.
const K7_1_GRP_ROLE: [&str; 30] = [
    "0.1292", "0.3150", "0.1105", "0.1850", "0.0814", "0.3269", "0.5115", "0.0211", "0.2085",
    "0.1678", "0.3135", "0.0860", "0.2144", "0.1216", "0.0711", "0.1082", "0.0400", "0.4394",
    "0.0957", "0.5621", "0.0612", "0.3462", "0.1415", "0.1948", "0.6000", "0.1594", "0.5078",
    "0.2627", "0.3375", "0.0240",
];

/// `docs/runs/K7-1.log:45`–`:74`, column `grp-topo`, seeds 90..120.
const K7_1_GRP_TOPO: [&str; 30] = [
    "0.3825", "0.4392", "0.3625", "0.3767", "0.3867", "0.4333", "0.5708", "0.3325", "0.4473",
    "0.4575", "0.4667", "0.3442", "0.4933", "0.4183", "0.3508", "0.3758", "0.3867", "0.4625",
    "0.4142", "0.5917", "0.2961", "0.4475", "0.3933", "0.4767", "0.6833", "0.3767", "0.5958",
    "0.4458", "0.4517", "0.3217",
];

/// `docs/runs/K7-1.log:45`–`:74`, column `ref-prune`, seeds 90..120.
const K7_1_REF_PRUNE: [&str; 30] = [
    "0.3825", "0.4392", "0.3625", "0.3767", "0.3867", "0.4333", "0.5708", "0.3325", "0.4708",
    "0.4575", "0.4667", "0.3442", "0.4933", "0.4183", "0.3508", "0.3758", "0.3867", "0.4625",
    "0.4142", "0.5917", "0.3142", "0.4475", "0.3933", "0.4767", "0.6833", "0.3767", "0.5958",
    "0.4458", "0.4517", "0.3217",
];

/// `docs/runs/K7-1.log:185`–`:188`, cell `grp-topo`, rosters all / 1 / 2 / 3:
/// tasks, success rate, mean covered fraction, mean cov_eff, mean final size,
/// tasks ending empty, final members whose role has no demand.
const K7_1_GRP_TOPO_ROSTER: [&str; 4] = [
    "600 | 99.7 % | 0.9982 | 0.4340 | 2.73 | 0 | 0 of 1636",
    "98 | 100.0 % | 1.0000 | 0.7313 | 1.58 | 0 | 0 of 155",
    "343 | 99.7 % | 0.9981 | 0.4281 | 2.48 | 0 | 0 of 849",
    "159 | 99.4 % | 0.9975 | 0.2635 | 3.97 | 0 | 0 of 632",
];

/// `docs/runs/K7-1.log:201`–`:204`, cell `ref-prune`, the same columns.
const K7_1_REF_PRUNE_ROSTER: [&str; 4] = [
    "600 | 100.0 % | 1.0000 | 0.4341 | 2.73 | 0 | 0 of 1639",
    "98 | 100.0 % | 1.0000 | 0.7313 | 1.58 | 0 | 0 of 155",
    "343 | 100.0 % | 1.0000 | 0.4281 | 2.48 | 0 | 0 of 851",
    "159 | 100.0 % | 1.0000 | 0.2638 | 3.98 | 0 | 0 of 633",
];

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

/// One policy's traced run over one instance, with its trace replayed.
struct Carried {
    result: WorkflowResult,
    recon: Recon,
}

/// `policy` traced over `inst`, its trace replayed by `reconstruct`.
fn carry(label: &str, policy: &dyn CoalitionDecisionPolicy, inst: &WorkflowInstance) -> Carried {
    let seed = inst.seed;
    let traced = TracedPolicy::new(policy);
    let mut latencies = Vec::new();
    let result = run_workflow_instance(&traced, inst, OutcomeSignal::RoleCoverage, &mut latencies)
        .unwrap_or_else(|e| panic!("{label} seed {seed}: {e}"));
    let recon =
        reconstruct(inst, &traced.entries()).unwrap_or_else(|e| panic!("{label} seed {seed}: {e}"));
    Carried { result, recon }
}

/// The instances of 90..120, in seed order.
fn instances_90_120() -> Vec<WorkflowInstance> {
    let spec = WorkflowSpec::default();
    SEEDS_90_120
        .iter()
        .map(|seed| {
            spec.generate(seed)
                .unwrap_or_else(|e| panic!("seed {seed}: {e}"))
        })
        .collect()
}

/// One fresh `config` group arm per instance, traced and replayed.
fn carry_group(
    label: &str,
    config: GroupAifConfig,
    instances: &[WorkflowInstance],
) -> Vec<Carried> {
    instances
        .iter()
        .map(|inst| {
            let seed = inst.seed;
            let policy = GroupAifPolicy::new(seed, config, role_map(inst))
                .unwrap_or_else(|e| panic!("{label} seed {seed}: {e}"));
            let carried = carry(label, &policy, inst);
            let rejections = policy.counters().begin_task_rejections;
            assert_eq!(
                rejections, 0,
                "{label} seed {seed}: the hook refused {rejections} tasks"
            );
            carried
        })
        .collect()
}

/// `runs` against one per-seed PRIMARY column and one summary row of
/// `docs/runs/K7-1.log`, and the recomputed PRIMARY and churn against the
/// harness's.
fn assert_carried(
    label: &str,
    runs: &[Carried],
    per_seed: &[&str; 30],
    summary_line: u32,
    summary: (&str, &str),
) {
    assert_eq!(runs.len(), 30, "{label}: {} seeds", runs.len());
    for (run, expected) in runs.iter().zip(per_seed) {
        let seed = run.result.seed;
        let observed = format!("{:.4}", run.result.primary);
        assert_eq!(
            &observed,
            expected,
            "{label} seed {seed}: observed PRIMARY {observed}, docs/runs/K7-1.log:{} says {expected}",
            K7_1_FIRST_SEED_LINE + (seed - SEEDS_90_120.start)
        );
        assert_eq!(
            run.recon.primary.to_bits(),
            run.result.primary.to_bits(),
            "{label} seed {seed}: PRIMARY recomputed from the reconstruction {:?}, \
             WorkflowResult::primary {:?}",
            run.recon.primary,
            run.result.primary
        );
        assert_eq!(
            run.recon.churn, run.result.churn,
            "{label} seed {seed}: churn recomputed from the reconstruction {}, \
             WorkflowResult::churn {}",
            run.recon.churn, run.result.churn
        );
    }
    let results: Vec<WorkflowResult> = runs.iter().map(|r| r.result.clone()).collect();
    let observed = medians(&results);
    assert_eq!(
        observed,
        (summary.0.to_string(), summary.1.to_string()),
        "{label} over 90..120: observed (median PRIMARY, median churn) {observed:?}, \
         docs/runs/K7-1.log:{summary_line} says {summary:?}"
    );
}

/// `roster_decomposition` over `runs` against the four roster rows (all, 1, 2,
/// 3) of one cell of `docs/runs/K7-1.log`, the first of them on `first_line`,
/// each rendered at the log's precision.
fn assert_roster_rows(label: &str, runs: &[Carried], first_line: usize, expected: &[&str; 4]) {
    let roles = usize::from(WorkflowSpec::default().roles);
    let rows = roster_decomposition(runs.iter().map(|r| &r.recon), roles);
    assert_eq!(rows.len(), expected.len(), "{label}: {} rows", rows.len());
    let at =
        |x: Option<f64>, dp: usize| x.map_or_else(|| "n/a".to_owned(), |v| format!("{v:.dp$}"));
    for (i, (row, expected)) in rows.iter().zip(expected).enumerate() {
        let observed = format!(
            "{} | {} % | {} | {} | {} | {} | {} of {}",
            row.tasks,
            at(
                (row.tasks > 0).then(|| 100.0 * row.successes as f64 / row.tasks as f64),
                1
            ),
            at(row.mean_covered_fraction(), 4),
            at(row.mean_cov_eff(), 4),
            at(row.mean_final_size(), 2),
            row.empty,
            row.off_demand,
            row.final_members
        );
        assert_eq!(
            &observed,
            expected,
            "{label} roster row {:?} over 90..120: observed `{observed}`, docs/runs/K7-1.log:{} \
             says `{expected}`",
            row.roster,
            first_line + i
        );
    }
}

#[test]
fn hosted_grp_role_nonov_reproduces_the_archive_rows() {
    let (results, counters) = host(GroupAifConfig {
        base: PersistentAifConfig {
            query_novelty: false,
            ..v5_e1_base()
        },
        ..GroupAifConfig::default()
    });
    assert_eq!(results.len(), 30);
    let observed = medians(&results);
    assert_eq!(
        observed,
        ("0.1125".to_string(), "140.00".to_string()),
        "grp-role-nonov over 330..360: observed (median PRIMARY, median churn) {observed:?}, \
         docs/runs/K4-archive.log:2037 says (0.1125, 140.00)"
    );
    let observed = reach_row(&counters);
    let expected = "23.0 % | 1.35 | 0.621 | 8201 of 11173 | 67.1 % / 73.6 %";
    assert_eq!(
        observed, expected,
        "grp-role-nonov candidate reach over 330..360: observed `{observed}`, \
         docs/runs/K4-archive.log:2076 says `{expected}`"
    );
}

#[test]
fn carried_grp_role_reproduces_the_k7_1_rows() {
    let instances = instances_90_120();
    let runs = carry_group("grp-role", GroupAifConfig::default(), &instances);
    assert_carried("grp-role", &runs, &K7_1_GRP_ROLE, 32, ("0.1764", "195.00"));
}

#[test]
fn carried_grp_topo_and_ref_prune_reproduce_the_k7_1_rows_and_identity() {
    let instances = instances_90_120();
    let topo = carry_group(
        "grp-topo",
        GroupAifConfig {
            routing: VoteRouting::CandidateStar { lambda: 0.5 },
            ..GroupAifConfig::default()
        },
        &instances,
    );
    let prune: Vec<Carried> = instances
        .iter()
        .map(|inst| carry("ref-prune", &RefPrune::new(role_map(inst)), inst))
        .collect();
    assert_carried("grp-topo", &topo, &K7_1_GRP_TOPO, 34, ("0.4258", "156.00"));
    assert_carried(
        "ref-prune",
        &prune,
        &K7_1_REF_PRUNE,
        38,
        ("0.4258", "167.00"),
    );
    assert_roster_rows("grp-topo", &topo, 185, &K7_1_GRP_TOPO_ROSTER);
    assert_roster_rows("ref-prune", &prune, 201, &K7_1_REF_PRUNE_ROSTER);

    let mut same_tasks = 0usize;
    let mut tasks = 0usize;
    let mut differing_seeds = Vec::new();
    for (a, b) in topo.iter().zip(&prune) {
        let (same, total) = member_set_identity(&a.recon, &b.recon);
        same_tasks += same;
        tasks += total;
        if a.result.primary.to_bits() != b.result.primary.to_bits() {
            differing_seeds.push(a.result.seed);
        }
    }
    assert_eq!(
        (same_tasks, tasks),
        (598, 600),
        "grp-topo vs ref-prune over 90..120: final member sets identical on {same_tasks} of \
         {tasks} tasks, docs/runs/K7-1.log:164 says 598 of 600"
    );
    assert_eq!(
        30 - differing_seeds.len(),
        28,
        "grp-topo vs ref-prune over 90..120: PRIMARY bit-identical on {} of 30 seeds (differing \
         {differing_seeds:?}), docs/runs/K7-1.log:164 says 28 of 30",
        30 - differing_seeds.len()
    );
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
