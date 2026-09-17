//! K7-1 — topology-routed group voting against the candidate-blind group, as
//! registered in `docs/k7/prereg-K7-1-topology-routed-group.md` (Amendment 1
//! included): six `GroupAifPolicy` cells and the `wf-asis` context arm over
//! `WorkflowSpec::default()` with `OutcomeSignal::RoleCoverage`, one fresh
//! policy per seed, every gate computed before any table is printed, and one
//! `VERDICT:` line last.
//!
//! The block is seeds `90..120`. `K7_1_SEEDS=a..b` replaces it with a smoke
//! block disjoint from `90..120`; a smoke run prints a banner and
//! `VERDICT: SMOKE (no verdict)` unless a gate fails.
//!
//! Run: `cargo run --release --features harness,decision,process --example k7_1`.

use std::collections::HashMap;
use std::error::Error;

use koalisi::decision::{
    AgreementSample, CoverageMasks, GroupAifConfig, GroupAifCounters, GroupAifPolicy,
    MagnitudePolicy, ModelLabel, NonVacuity, RoleModulation, VoteRouting, models_moved,
};
use koalisi::harness::{
    OutcomeSignal, SeedRange, TraceEntry, TracedPolicy, WorkflowInstance, WorkflowResult,
    WorkflowSpec, median_iqr, percentile, run_workflow_instance, superior_count,
};
use koalisi::process::Role;

const PREREG: &str = "docs/k7/prereg-K7-1-topology-routed-group.md";
const REGISTERED_SEEDS: SeedRange = SeedRange {
    start: 90,
    end: 120,
};
const SEEDS_ENV: &str = "K7_1_SEEDS";
const SIGNAL: OutcomeSignal = OutcomeSignal::RoleCoverage;

/// XOR applied to the battery seed of the seed-invariance run.
const INVARIANCE_XOR: u64 = 0x9E37_79B9_7F4A_7C15;

/// H-T conjunct 1: median PRIMARY ratio against `grp-role`.
const BAR_RATIO: f64 = 1.25;
/// H-T conjunct 2: strictly superior seeds against `grp-role`.
const BAR_SUPERIOR: usize = 18;

const GRP_ROLE: usize = 0;
const GRP_ROLE_BLIND: usize = 1;
const GRP_TOPO: usize = 2;
const GRP_TOPO_ID: usize = 3;
const GRP_TOPO_Q: usize = 4;
const GRP_TOPO_SOLO: usize = 5;

/// `(label, table role, masks, routing)` of the six group cells, indexed by the
/// `GRP_*` constants.
const GROUP_CELLS: [(&str, &str, CoverageMasks, VoteRouting); 6] = [
    (
        "grp-role",
        "control",
        CoverageMasks::RoleMatched,
        VoteRouting::Off,
    ),
    (
        "grp-role-blind",
        "second control",
        CoverageMasks::RoleBlind,
        VoteRouting::Off,
    ),
    (
        "grp-topo",
        "confirmatory",
        CoverageMasks::RoleMatched,
        VoteRouting::CandidateStar { lambda: 0.5 },
    ),
    (
        "grp-topo-id",
        "X-identity cell",
        CoverageMasks::RoleMatched,
        VoteRouting::CandidateStar { lambda: 0.0 },
    ),
    (
        "grp-topo-q",
        "exploratory",
        CoverageMasks::RoleMatched,
        VoteRouting::CandidateStar { lambda: 0.25 },
    ),
    (
        "grp-topo-solo",
        "exploratory",
        CoverageMasks::RoleMatched,
        VoteRouting::CandidateStar { lambda: 1.0 },
    ),
];

/// The cells whose topology can carry a non-identity row.
const ROUTED_CELLS: [usize; 3] = [GRP_TOPO, GRP_TOPO_Q, GRP_TOPO_SOLO];
/// The cells whose `routed_reads` is `0` by construction.
const UNROUTED_CELLS: [usize; 3] = [GRP_TOPO_ID, GRP_ROLE, GRP_ROLE_BLIND];

/// The registered configuration of group cell `cell`.
fn cell_config(cell: usize) -> GroupAifConfig {
    let (_, _, masks, routing) = GROUP_CELLS[cell];
    GroupAifConfig {
        masks,
        routing,
        ..GroupAifConfig::default()
    }
}

/// One policy over one instance: the metrics and the decision trace.
struct SeedRun {
    result: WorkflowResult,
    trace: Vec<TraceEntry>,
}

/// One group policy's ledger after its instance.
struct Ledger {
    counters: GroupAifCounters,
    moved: NonVacuity,
}

/// One arm over the block: per-seed runs in seed order, per-seed ledgers (empty
/// for a non-group arm), every decision latency in call order.
struct Cell {
    label: &'static str,
    role: &'static str,
    runs: Vec<SeedRun>,
    ledgers: Vec<Ledger>,
    latencies: Vec<f64>,
}

impl Cell {
    fn primaries(&self) -> Vec<f64> {
        self.runs.iter().map(|r| r.result.primary).collect()
    }

    fn median_primary(&self) -> f64 {
        median_iqr(&self.primaries()).0
    }

    fn median_churn(&self) -> f64 {
        let churn: Vec<f64> = self.runs.iter().map(|r| r.result.churn as f64).collect();
        median_iqr(&churn).0
    }

    fn samples(&self) -> Vec<AgreementSample> {
        self.ledgers
            .iter()
            .flat_map(|l| l.counters.agreement.iter().copied())
            .collect()
    }
}

/// A gate's outcome: the failing items name the cell and seed.
struct Gate {
    name: &'static str,
    summary: String,
    failures: Vec<String>,
}

impl Gate {
    fn pass(&self) -> bool {
        self.failures.is_empty()
    }

    fn print(&self) {
        let word = if self.pass() { "PASS" } else { "FAIL" };
        println!("- **{} — {word}.** {}", self.name, self.summary);
        if !self.pass() {
            println!("  - failing: {}", self.failures.join("; "));
        }
    }
}

/// Position-wise divergence of two arms' traces, summed over seeds.
struct Divergence {
    act: usize,
    score_bits: usize,
    seeds_with_act: usize,
    length: usize,
}

/// The block and whether it came from `K7_1_SEEDS`.
fn seed_block() -> Result<(SeedRange, bool), Box<dyn Error>> {
    let text = match std::env::var(SEEDS_ENV) {
        Ok(text) => text,
        Err(std::env::VarError::NotPresent) => return Ok((REGISTERED_SEEDS, false)),
        Err(e) => return Err(e.into()),
    };
    let (start, end) = text
        .split_once("..")
        .ok_or_else(|| format!("{SEEDS_ENV}={text}: expected `a..b`"))?;
    let range = SeedRange {
        start: start.trim().parse()?,
        end: end.trim().parse()?,
    };
    if range.is_empty() {
        return Err(format!("{SEEDS_ENV}={text}: the range holds no seed").into());
    }
    if range.start < REGISTERED_SEEDS.end && REGISTERED_SEEDS.start < range.end {
        return Err(format!(
            "{SEEDS_ENV}={text} overlaps the registered block {REGISTERED_SEEDS}; \
             run the registered block without {SEEDS_ENV}"
        )
        .into());
    }
    Ok((range, true))
}

/// `inst.role_map()` with each role id as a `Role`.
fn role_map(inst: &WorkflowInstance) -> Result<HashMap<usize, Role>, Box<dyn Error>> {
    inst.role_map()
        .into_iter()
        .map(|(id, role)| Ok((id, Role::new(u8::try_from(role)?))))
        .collect()
}

/// `ρ = δ` over `roles` roles.
fn identity_rho(roles: u8) -> RoleModulation {
    let n = usize::from(roles);
    let rows: Vec<Vec<f64>> = (0..n)
        .map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect())
        .collect();
    RoleModulation::new(rows).expect("invariant: a square identity table")
}

/// One fresh `config` policy per instance, seeded `instance.seed ^ seed_xor`.
fn run_group(
    label: &'static str,
    role: &'static str,
    config: GroupAifConfig,
    instances: &[WorkflowInstance],
    seed_xor: u64,
) -> Result<Cell, Box<dyn Error>> {
    let mut cell = Cell {
        label,
        role,
        runs: Vec::with_capacity(instances.len()),
        ledgers: Vec::with_capacity(instances.len()),
        latencies: Vec::new(),
    };
    for inst in instances {
        let policy = GroupAifPolicy::new(inst.seed ^ seed_xor, config, role_map(inst)?)?;
        let before = policy.model_snapshots();
        let traced = TracedPolicy::new(&policy);
        let result = run_workflow_instance(&traced, inst, SIGNAL, &mut cell.latencies)?;
        let trace = traced.entries();
        let counters = policy.counters();
        let moved = models_moved(&policy.model_snapshots(), &before, &counters.model_updates);
        cell.runs.push(SeedRun { result, trace });
        cell.ledgers.push(Ledger { counters, moved });
    }
    Ok(cell)
}

/// `wf-asis`: one fresh typed magnitude policy per instance at `ρ = δ`.
fn run_context(instances: &[WorkflowInstance], roles: u8) -> Result<Cell, Box<dyn Error>> {
    let mut cell = Cell {
        label: "wf-asis",
        role: "context",
        runs: Vec::with_capacity(instances.len()),
        ledgers: Vec::new(),
        latencies: Vec::new(),
    };
    for inst in instances {
        let policy =
            MagnitudePolicy::default().with_role_modulation(inst.role_map(), identity_rho(roles));
        let traced = TracedPolicy::new(&policy);
        let result = run_workflow_instance(&traced, inst, SIGNAL, &mut cell.latencies)?;
        let trace = traced.entries();
        cell.runs.push(SeedRun { result, trace });
    }
    Ok(cell)
}

/// Trace entries (leave flag, act, raw score bits), PRIMARY bits and churn all
/// equal.
fn identical(a: &SeedRun, b: &SeedRun) -> bool {
    a.trace == b.trace
        && a.result.primary.to_bits() == b.result.primary.to_bits()
        && a.result.churn == b.result.churn
}

/// `"<label> <seed>"` for every seed on which `a` and `b` are not [`identical`].
fn differing_seeds(a: &Cell, b: &Cell) -> Vec<String> {
    a.runs
        .iter()
        .zip(&b.runs)
        .filter(|(x, y)| !identical(x, y))
        .map(|(x, _)| format!("`{}` {}", a.label, x.result.seed))
        .collect()
}

fn gate_identity(cells: &[Cell]) -> Gate {
    let failures = differing_seeds(&cells[GRP_TOPO_ID], &cells[GRP_ROLE]);
    let n = cells[GRP_ROLE].runs.len();
    Gate {
        name: "X-identity",
        summary: format!(
            "`grp-topo-id` (λ = 0, the `RoutedAggregator` path) against `grp-role` per seed on \
             trace entries (leave flag, act, raw score bits), PRIMARY bits and churn: {}/{n} \
             seeds identical.",
            n - failures.len()
        ),
        failures,
    }
}

fn gate_determinism(cells: &[Cell], reruns: &[Cell]) -> Gate {
    let mut failures = Vec::new();
    let mut parts = Vec::new();
    for (first, second) in cells.iter().zip(reruns) {
        let differing = differing_seeds(first, second);
        parts.push(format!(
            "`{}` {}/{}",
            first.label,
            first.runs.len() - differing.len(),
            first.runs.len()
        ));
        failures.extend(differing);
    }
    Gate {
        name: "S-determinism",
        summary: format!(
            "Every group cell re-run from scratch and compared per seed on trace entries, \
             PRIMARY bits and churn: {}.",
            parts.join(" · ")
        ),
        failures,
    }
}

fn gate_invariance(cells: &[Cell], reseeded: &Cell) -> Gate {
    let failures = differing_seeds(&cells[GRP_TOPO], reseeded);
    let n = reseeded.runs.len();
    Gate {
        name: "Seed invariance",
        summary: format!(
            "`grp-topo` rebuilt with `battery_seed = seed ^ {INVARIANCE_XOR:#018X}`, same \
             comparison: {}/{n} seeds identical.",
            n - failures.len()
        ),
        failures,
    }
}

fn gate_learn(cells: &[Cell]) -> Gate {
    let mut failures = Vec::new();
    let mut parts = Vec::new();
    for cell in cells {
        let mut ok = 0usize;
        for (run, ledger) in cell.runs.iter().zip(&cell.ledgers) {
            let c = &ledger.counters;
            let mut broken = Vec::new();
            if !c.s_learn_exact() {
                broken.push(format!(
                    "ledger {:?}, {} opened, {} observed",
                    c.model_updates
                        .iter()
                        .map(|m| (m.updates, m.expected))
                        .collect::<Vec<_>>(),
                    c.roster_sizes.len(),
                    c.tasks_observed
                ));
            }
            if !ledger.moved.ok {
                broken.push("a model asked to learn did not move".to_owned());
            }
            if c.begin_task_rejections != 0 {
                broken.push(format!("{} begin_task rejections", c.begin_task_rejections));
            }
            if broken.is_empty() {
                ok += 1;
            } else {
                failures.push(format!(
                    "`{}` {} ({})",
                    cell.label,
                    run.result.seed,
                    broken.join(", ")
                ));
            }
        }
        parts.push(format!("`{}` {ok}/{}", cell.label, cell.runs.len()));
    }
    Gate {
        name: "S-learn (i)",
        summary: format!(
            "Per seed and cell: `s_learn_exact()`, `models_moved(..).ok` with its \
             `expected == 0` exemption, `begin_task_rejections == 0`: {}.",
            parts.join(" · ")
        ),
        failures,
    }
}

/// Successful reads with a candidate-sensitive member and a roster of at least
/// two, from the `AgreementSample` ledger.
fn routable_reads(counters: &GroupAifCounters) -> u64 {
    counters
        .agreement
        .iter()
        .filter(|s| s.candidate_sensitive >= 1 && s.roster >= 2)
        .count() as u64
}

fn gate_route(cells: &[Cell]) -> Gate {
    let mut failures = Vec::new();
    let mut parts = Vec::new();
    for &i in &ROUTED_CELLS {
        let cell = &cells[i];
        let mut total = 0u64;
        for (run, ledger) in cell.runs.iter().zip(&cell.ledgers) {
            let routed = ledger.counters.routed_reads;
            let expected = routable_reads(&ledger.counters);
            total += routed;
            if routed != expected {
                failures.push(format!(
                    "`{}` {} (routed_reads {routed}, ledger {expected})",
                    cell.label, run.result.seed
                ));
            }
        }
        if total == 0 {
            failures.push(format!("`{}` block total 0", cell.label));
        }
        parts.push(format!("`{}` {total}", cell.label));
    }
    for &i in &UNROUTED_CELLS {
        let cell = &cells[i];
        let mut total = 0u64;
        for (run, ledger) in cell.runs.iter().zip(&cell.ledgers) {
            let routed = ledger.counters.routed_reads;
            total += routed;
            if routed != 0 {
                failures.push(format!(
                    "`{}` {} (routed_reads {routed}, expected 0)",
                    cell.label, run.result.seed
                ));
            }
        }
        parts.push(format!("`{}` {total}", cell.label));
    }
    Gate {
        name: "S-route",
        summary: format!(
            "Per seed on `grp-topo`, `grp-topo-q`, `grp-topo-solo`: `routed_reads` == ledger \
             reads with `candidate_sensitive >= 1` and `roster >= 2`, block total > 0; on \
             `grp-topo-id`, `grp-role`, `grp-role-blind`: `routed_reads == 0`. Block totals: {}.",
            parts.join(" · ")
        ),
        failures,
    }
}

fn divergence(a: &Cell, b: &Cell) -> Divergence {
    let mut d = Divergence {
        act: 0,
        score_bits: 0,
        seeds_with_act: 0,
        length: 0,
    };
    for (x, y) in a.runs.iter().zip(&b.runs) {
        let act = x
            .trace
            .iter()
            .zip(&y.trace)
            .filter(|(p, q)| p.act != q.act)
            .count();
        d.act += act;
        d.score_bits += x
            .trace
            .iter()
            .zip(&y.trace)
            .filter(|(p, q)| p.score_bits != q.score_bits)
            .count();
        d.seeds_with_act += usize::from(act > 0);
        d.length += x.trace.len().abs_diff(y.trace.len());
    }
    d
}

/// `n / d` as a percentage at 1 dp, `n/a` for an empty denominator.
fn pct(n: usize, d: usize) -> String {
    if d == 0 {
        "n/a".to_owned()
    } else {
        format!("{:.1} %", 100.0 * n as f64 / d as f64)
    }
}

/// `a / b` at 4 dp, `n/a` for a zero `b`.
fn ratio_text(a: f64, b: f64) -> String {
    if b > 0.0 {
        format!("{:.4}×", a / b)
    } else {
        "n/a".to_owned()
    }
}

fn mean(xs: impl Iterator<Item = f64>) -> Option<f64> {
    let (n, sum) = xs.fold((0usize, 0.0f64), |(n, s), x| (n + 1, s + x));
    (n > 0).then(|| sum / n as f64)
}

fn or_na(x: Option<f64>, dp: usize) -> String {
    x.map_or_else(|| "n/a".to_owned(), |v| format!("{v:.dp$}"))
}

fn print_header(spec: &WorkflowSpec, seeds: SeedRange, smoke: bool) {
    println!("# K7-1 — topology-routed group voting vs the candidate-blind group");
    println!();
    if smoke {
        println!(
            "**SMOKE — NOT THE REGISTERED BLOCK** (`{SEEDS_ENV}={seeds}`; the registered block is {REGISTERED_SEEDS})."
        );
        println!();
    }
    println!("- registration: K7-1 (koalisi #90)");
    println!("- prereg: `{PREREG}` (Amendment 1 included)");
    println!("- koalisi: v{}", env!("CARGO_PKG_VERSION"));
    println!("- seeds: {seeds} ({} seeds)", seeds.len());
    println!("- outcome signal: {SIGNAL:?}");
    println!(
        "- world: `WorkflowSpec::default()` — universe_bits {}, pool {:?}, caps_per_agent {:?}, trust {:?}, tasks {}, required_bits {:?}, roles {}, redraw_cap {}, fanout_denom {}, performance {:?}",
        spec.base.universe_bits,
        spec.base.pool,
        spec.base.caps_per_agent,
        spec.base.trust,
        spec.base.tasks,
        spec.base.required_bits,
        spec.roles,
        spec.redraw_cap,
        spec.fanout_denom,
        spec.performance
    );
    println!();
}

fn print_gates(gates: &[Gate], cells: &[Cell]) {
    println!("## Gates");
    println!();
    println!(
        "- **X-battery** — checked outside this binary: one serial run of `examples/strategy_comparison.rs` diffed against `docs/runs/K4-archive.log` with the latency column stripped (`docs/runs/README.md`)."
    );
    println!(
        "- **X-host** — checked outside this binary: `cargo test --features harness,decision,process --test k7_group_host`."
    );
    for gate in gates {
        gate.print();
    }
    println!("- S-learn (i) exempt models (`expected == 0`), as `(seed, model)`:");
    for cell in cells {
        let mut pairs = Vec::new();
        for (run, ledger) in cell.runs.iter().zip(&cell.ledgers) {
            for label in &ledger.moved.exempt {
                let name = match label {
                    ModelLabel::Role(r) => format!("r{}", r.index()),
                    ModelLabel::Shared => "shared".to_owned(),
                };
                pairs.push(format!("({}, {name})", run.result.seed));
            }
        }
        let listed = if pairs.is_empty() {
            "none".to_owned()
        } else {
            pairs.join(" · ")
        };
        println!("  - `{}`: {listed}", cell.label);
    }
    println!();
}

fn print_arms(cells: &[Cell], context: &Cell) {
    let control = cells[GRP_ROLE].primaries();
    let control_median = cells[GRP_ROLE].median_primary();
    println!("## Arms (pooled)");
    println!();
    println!(
        "| arm | role | median PRIMARY | vs `grp-role` | superior seeds vs `grp-role` | median churn | median µs/decision |"
    );
    println!("|---|---|---:|---:|---:|---:|---:|");
    for cell in cells.iter().chain(std::iter::once(context)) {
        println!(
            "| `{}` | {} | {:.4} | {} | {}/{} | {:.2} | {:.3} |",
            cell.label,
            cell.role,
            cell.median_primary(),
            ratio_text(cell.median_primary(), control_median),
            superior_count(&cell.primaries(), &control),
            control.len(),
            cell.median_churn(),
            median_iqr(&cell.latencies).0
        );
    }
    println!();

    println!("## Per seed");
    println!();
    let labels: Vec<String> = cells
        .iter()
        .chain(std::iter::once(context))
        .map(|c| format!("`{}`", c.label))
        .collect();
    println!(
        "| seed | n | {} | churn `grp-role` | churn `grp-topo` |",
        labels.join(" | ")
    );
    println!("|---:|---:|{}---:|---:|", "---:|".repeat(labels.len()));
    for (i, run) in cells[GRP_ROLE].runs.iter().enumerate() {
        let primaries: Vec<String> = cells
            .iter()
            .chain(std::iter::once(context))
            .map(|c| format!("{:.4}", c.runs[i].result.primary))
            .collect();
        println!(
            "| {} | {} | {} | {} | {} |",
            run.result.seed,
            run.result.n,
            primaries.join(" | "),
            run.result.churn,
            cells[GRP_TOPO].runs[i].result.churn
        );
    }
    println!();
}

fn print_hs(cells: &[Cell]) {
    println!("## H-S (structural answer to part (i) — counted, not a bar)");
    println!();
    println!(
        "| cell | reads | every readout row sensitive | no readout row sensitive | mean blind-origin mass share |"
    );
    println!("|---|---:|---:|---:|---:|");
    for cell in cells {
        let samples = cell.samples();
        let every = samples
            .iter()
            .filter(|s| s.sensitive_rows == s.roster)
            .count();
        let none = samples.iter().filter(|s| s.sensitive_rows == 0).count();
        println!(
            "| `{}` | {} | {} | {} | {} |",
            cell.label,
            samples.len(),
            pct(every, samples.len()),
            pct(none, samples.len()),
            or_na(mean(samples.iter().map(|s| s.blind_origin_share)), 3)
        );
    }
    println!();
    let samples = cells[GRP_TOPO].samples();
    let present: Vec<&AgreementSample> =
        samples.iter().filter(|s| s.centre_vote.is_some()).collect();
    let absent: Vec<&AgreementSample> =
        samples.iter().filter(|s| s.centre_vote.is_none()).collect();
    println!(
        "- `grp-topo` centre-present reads with every readout row sensitive: {} of {}; centre-absent reads with no readout row sensitive: {} of {}.",
        present
            .iter()
            .filter(|s| s.sensitive_rows == s.roster)
            .count(),
        present.len(),
        absent.iter().filter(|s| s.sensitive_rows == 0).count(),
        absent.len()
    );
    println!("- {}", hs_sentence(cells));
    println!();
}

/// §5's pre-committed H-S sentence with the share read from `grp-topo`.
fn hs_sentence(cells: &[Cell]) -> String {
    let samples = cells[GRP_TOPO].samples();
    let none = samples.iter().filter(|s| s.sensitive_rows == 0).count();
    format!(
        "*\"every voter is candidate-sensitive on the centre-present reads and on no centre-absent read; the centre-absent share is {}\"*",
        pct(none, samples.len())
    )
}

fn print_divergence_row(label: &str, against: &str, d: &Divergence, n_seeds: usize) {
    println!(
        "| `{label}` vs `{against}` | {} | {} | {}/{n_seeds} | {} |",
        d.act, d.score_bits, d.seeds_with_act, d.length
    );
}

fn print_live(cells: &[Cell]) {
    let n_seeds = cells[GRP_ROLE].runs.len();
    println!("## S-live (disclosure — compared by position up to the shorter trace)");
    println!();
    println!(
        "| pair | decisions differing by ACT | decisions differing by raw score bits | seeds with any act difference | total length difference |"
    );
    println!("|---|---:|---:|---:|---:|");
    print_divergence_row(
        "grp-topo",
        "grp-role",
        &divergence(&cells[GRP_TOPO], &cells[GRP_ROLE]),
        n_seeds,
    );
    println!();

    println!("## E-λ (registered exploratory, non-gating)");
    println!();
    println!(
        "| pair | decisions differing by ACT | decisions differing by raw score bits | seeds with any act difference | total length difference |"
    );
    println!("|---|---:|---:|---:|---:|");
    for &i in &[GRP_TOPO_Q, GRP_TOPO_SOLO] {
        print_divergence_row(
            cells[i].label,
            "grp-topo",
            &divergence(&cells[i], &cells[GRP_TOPO]),
            n_seeds,
        );
    }
    println!();
    println!(
        "- median PRIMARY: `grp-topo` {:.4} · `grp-topo-q` {:.4} · `grp-topo-solo` {:.4}",
        cells[GRP_TOPO].median_primary(),
        cells[GRP_TOPO_Q].median_primary(),
        cells[GRP_TOPO_SOLO].median_primary()
    );
    println!();
}

fn print_follow(cells: &[Cell]) {
    let cell = &cells[GRP_TOPO];
    let mut reads = 0usize;
    let mut followed = 0usize;
    let mut unaligned = Vec::new();
    for (run, ledger) in cell.runs.iter().zip(&cell.ledgers) {
        let samples = &ledger.counters.agreement;
        if samples.len() != run.trace.len() {
            unaligned.push(run.result.seed.to_string());
            continue;
        }
        for (entry, sample) in run.trace.iter().zip(samples) {
            if let Some(vote) = sample.centre_vote {
                reads += 1;
                followed += usize::from(entry.act == (vote == 1));
            }
        }
    }
    println!("## E-follow (`grp-topo`, centre-present reads)");
    println!();
    println!(
        "- group act == centre's own argmax: {} ({followed} of {reads} reads).",
        pct(followed, reads)
    );
    println!(
        "- seeds left out because a decision without a successful read breaks the trace-to-sample alignment: {}.",
        if unaligned.is_empty() {
            "none".to_owned()
        } else {
            unaligned.join(", ")
        }
    );
    println!();
}

fn print_reach(cells: &[Cell], roles: u8) {
    println!("## Candidate reach");
    println!();
    println!(
        "| cell | zero candidate-sensitive | mean blind internals | blind CW weight share (by member) | blind CW weight share (by origin) | leave `cfg0 == cfg1` | act rate: sensitive / blind |"
    );
    println!("|---|---:|---:|---:|---:|---:|---|");
    for cell in cells {
        let samples = cell.samples();
        let zero = samples
            .iter()
            .filter(|s| s.candidate_sensitive == 0)
            .count();
        let sensitive_total: usize = samples.iter().map(|s| s.candidate_sensitive).sum();
        let sensitive_acts: usize = samples.iter().map(|s| s.votes_for_act_sensitive).sum();
        let blind_total: usize = samples.iter().map(AgreementSample::candidate_blind).sum();
        let blind_acts: usize = samples.iter().map(|s| s.votes_for_act_blind).sum();
        let (queries, same) = cell.ledgers.iter().fold((0u64, 0u64), |(q, i), l| {
            (
                q + l.counters.leave_queries,
                i + l.counters.leave_queries_identical,
            )
        });
        println!(
            "| `{}` | {} | {} | {} | {} | {same} of {queries} | {} / {} |",
            cell.label,
            pct(zero, samples.len()),
            or_na(mean(samples.iter().map(|s| s.candidate_blind() as f64)), 2),
            or_na(mean(samples.iter().map(|s| s.blind_weight_share)), 3),
            or_na(mean(samples.iter().map(|s| s.blind_origin_share)), 3),
            pct(sensitive_acts, sensitive_total),
            pct(blind_acts, blind_total)
        );
    }
    println!();

    println!("## E-agree and the realised roster");
    println!();
    println!(
        "| cell | roster 1 / 2 / 3 | empty role-slots | unanimous reads | margin p25 / median / p75 |"
    );
    println!("|---|---|---:|---:|---|");
    for cell in cells {
        let mut hist = [0usize; 4];
        let mut tasks = 0usize;
        let mut empty = 0u64;
        for ledger in &cell.ledgers {
            for &size in &ledger.counters.roster_sizes {
                if size < hist.len() {
                    hist[size] += 1;
                }
                tasks += 1;
            }
            empty += ledger.counters.empty_role_slots;
        }
        let samples = cell.samples();
        let unanimous = samples.iter().filter(|s| s.unanimous()).count();
        let mut margins: Vec<f64> = samples.iter().map(|s| s.margin).collect();
        margins.sort_by(f64::total_cmp);
        let quartiles = if margins.is_empty() {
            "n/a".to_owned()
        } else {
            format!(
                "{:.4} / {:.4} / {:.4}",
                percentile(&margins, 25.0),
                percentile(&margins, 50.0),
                percentile(&margins, 75.0)
            )
        };
        println!(
            "| `{}` | {} / {} / {} | {empty} of {} | {} of {} | {quartiles} |",
            cell.label,
            hist[1],
            hist[2],
            hist[3],
            tasks * usize::from(roles),
            pct(unanimous, samples.len()),
            samples.len()
        );
    }
    println!();

    println!("## Declines (a zero score is not a decline)");
    println!();
    println!(
        "| cell | decisions | reads | declines_upstream | declines_missing_role | declines_no_demand | begin_task_rejections |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|");
    for cell in cells {
        let sum = |f: fn(&GroupAifCounters) -> u64| -> u64 {
            cell.ledgers.iter().map(|l| f(&l.counters)).sum()
        };
        println!(
            "| `{}` | {} | {} | {} | {} | {} | {} |",
            cell.label,
            sum(|c| c.decisions),
            sum(|c| c.reads),
            sum(|c| c.declines_upstream),
            sum(|c| c.declines_missing_role),
            sum(|c| c.declines_no_demand),
            sum(|c| c.begin_task_rejections)
        );
    }
    println!();
}

fn print_clauses(cells: &[Cell], context: &Cell, gates_ok: bool, ht_pass: bool) {
    let topo = cells[GRP_TOPO].median_primary();
    let control = cells[GRP_ROLE].median_primary();
    let blind = cells[GRP_ROLE_BLIND].median_primary();
    let asis = context.median_primary();
    println!("## Scoped clauses (prereg §6)");
    println!();

    let exceeds = if topo > blind {
        "exceeds"
    } else {
        "does not exceed"
    };
    print!(
        "1. **Against `grp-role-blind`.** `grp-topo`'s median {topo:.4} {exceeds} `grp-role-blind`'s {blind:.4}."
    );
    if topo > blind && topo < control {
        print!(
            " `grp-topo` above `grp-role-blind` and below `grp-role` ({control:.4}): *\"candidate-sensitivity bought by routing costs less than candidate-sensitivity bought by role-blind masks, and still costs\"*."
        );
    }
    println!();

    let second = if !gates_ok {
        "not read: a gate failed.".to_owned()
    } else if ht_pass {
        "does not apply: H-T passes.".to_owned()
    } else if topo < control {
        format!(
            "H-T fails with `grp-topo`'s median {topo:.4} below `grp-role`'s {control:.4}: *\"the blind voters are load-bearing with the masks held fixed\"*."
        )
    } else if control > 0.0 && topo / control < BAR_RATIO {
        format!(
            "H-T fails with the ratio {} in `[1.0, 1.25)`: *\"not worse, not validated\"*.",
            ratio_text(topo, control)
        )
    } else {
        format!(
            "H-T fails with the ratio {}; neither pre-committed reading applies.",
            ratio_text(topo, control)
        )
    };
    println!("2. **A failed H-T.** {second}");

    let third = if !gates_ok {
        "not read: a gate failed.".to_owned()
    } else if !ht_pass {
        "does not apply: H-T fails.".to_owned()
    } else if topo > asis {
        format!("H-T passes and `grp-topo`'s median {topo:.4} exceeds `wf-asis`'s {asis:.4}.")
    } else {
        format!(
            "H-T passes and `grp-topo`'s median {topo:.4} does not exceed `wf-asis`'s {asis:.4}: *\"beats the blind-voter group, not the typed magnitude control\"*."
        )
    };
    println!("3. **Against `wf-asis`.** {third}");
    println!("4. **H-S's sentence.** {}", hs_sentence(cells));
    println!();
}

fn main() -> Result<(), Box<dyn Error>> {
    let (seeds, smoke) = seed_block()?;
    let spec = WorkflowSpec::default();
    let instances = seeds
        .iter()
        .map(|seed| spec.generate(seed))
        .collect::<Result<Vec<_>, _>>()?;

    let run_cells = |seed_xor: u64| -> Result<Vec<Cell>, Box<dyn Error>> {
        (0..GROUP_CELLS.len())
            .map(|i| {
                let (label, role, _, _) = GROUP_CELLS[i];
                run_group(label, role, cell_config(i), &instances, seed_xor)
            })
            .collect()
    };
    let cells = run_cells(0)?;
    let reruns = run_cells(0)?;
    let reseeded = run_group(
        "grp-topo",
        "seed invariance",
        cell_config(GRP_TOPO),
        &instances,
        INVARIANCE_XOR,
    )?;
    let context = run_context(&instances, spec.roles)?;

    let gates = [
        gate_identity(&cells),
        gate_determinism(&cells, &reruns),
        gate_invariance(&cells, &reseeded),
        gate_learn(&cells),
        gate_route(&cells),
    ];
    let gates_ok = gates.iter().all(Gate::pass);

    let topo_median = cells[GRP_TOPO].median_primary();
    let control_median = cells[GRP_ROLE].median_primary();
    let ratio_ok = control_median > 0.0 && topo_median / control_median >= BAR_RATIO;
    let superior = superior_count(&cells[GRP_TOPO].primaries(), &cells[GRP_ROLE].primaries());
    let superior_ok = superior >= BAR_SUPERIOR;
    let ht_pass = ratio_ok && superior_ok;

    print_header(&spec, seeds, smoke);
    print_gates(&gates, &cells);
    print_arms(&cells, &context);

    let word = |ok: bool| if ok { "PASS" } else { "FAIL" };
    println!("## H-T (confirmatory — both conjuncts, against in-battery `grp-role`)");
    println!();
    println!(
        "- conjunct 1 — median PRIMARY ratio ≥ {BAR_RATIO:.2}×: `grp-topo` {topo_median:.4} / `grp-role` {control_median:.4} = {} — **{}**",
        ratio_text(topo_median, control_median),
        word(ratio_ok)
    );
    println!(
        "- conjunct 2 — strictly superior on ≥ {BAR_SUPERIOR}/30 seeds: {superior}/{} — **{}**",
        seeds.len(),
        word(superior_ok)
    );
    println!("- H-T: **{}**", word(ht_pass));
    println!();

    print_hs(&cells);
    print_live(&cells);
    print_follow(&cells);
    print_reach(&cells, spec.roles);

    println!("## Context (`wf-asis`, non-gating)");
    println!();
    let asis = context.primaries();
    println!(
        "- `wf-asis` median PRIMARY: {:.4}",
        context.median_primary()
    );
    for &i in &[GRP_ROLE, GRP_TOPO] {
        println!(
            "- `{}` strictly superior to `wf-asis` on {}/{} seeds",
            cells[i].label,
            superior_count(&cells[i].primaries(), &asis),
            asis.len()
        );
    }
    println!();

    print_clauses(&cells, &context, gates_ok, ht_pass);

    if !gates_ok {
        let failing: Vec<String> = gates
            .iter()
            .filter(|g| !g.pass())
            .map(|g| format!("{} [{}]", g.name, g.failures.join("; ")))
            .collect();
        println!("Gate failures: {}", failing.join(" · "));
        println!();
        println!("VERDICT: RUN-INVALID");
    } else if smoke {
        println!("VERDICT: SMOKE (no verdict)");
    } else if ht_pass {
        println!("VERDICT: VALIDATED (topology-routed group)");
    } else {
        println!("VERDICT: FALSIFIED (topology-routed group)");
    }
    Ok(())
}
