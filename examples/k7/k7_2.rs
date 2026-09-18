//! K7-2 — `query_novelty` on against off on the topology-routed group, as
//! registered in `docs/k7/prereg-K7-2-novelty-routed-group.md` (Amendments 1
//! and 2 included): four `GroupAifPolicy` cells and the `ref-prune` reference
//! cell over `WorkflowSpec::default()` with `OutcomeSignal::RoleCoverage`, one
//! fresh policy per seed, every gate computed before the report is rendered,
//! and one `VERDICT:` line last.
//!
//! The block is seeds `150..180`. `K7_2_SEEDS` accepts `6000..6003` and
//! `6000..6030` and refuses every other value. Under it the binary runs every
//! cell and gate, renders the report into a buffer it does not print, and
//! prints the header, the gate lines (pass counts per cell), the buffer's byte
//! count and `VERDICT: SMOKE (no verdict)` — `VERDICT: RUN-INVALID` when a gate
//! fails.
//!
//! Run: `cargo run --release --features harness,decision,process --example k7_2`.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Write as _};
use std::process::ExitCode;

use koalisi::decision::{
    AgreementSample, CoalitionDecisionPolicy, GroupAifConfig, GroupAifCounters, GroupAifPolicy,
    ModelLabel, NonVacuity, PersistentAifConfig, S_LEARN_VACUITY_TOL, VoteRouting, models_moved,
    v5_e1_base,
};
use koalisi::harness::{
    OutcomeSignal, Recon, RefPrune, SeedRange, TraceEntry, TracedPolicy, WorkflowInstance,
    WorkflowResult, WorkflowSpec, median_iqr, member_set_identity, reconstruct,
    roster_decomposition, run_workflow_instance, superior_count,
};
use koalisi::process::Role;

const PREREG: &str = "docs/k7/prereg-K7-2-novelty-routed-group.md";
const REGISTERED_SEEDS: SeedRange = SeedRange {
    start: 150,
    end: 180,
};
const SEEDS_ENV: &str = "K7_2_SEEDS";
/// The values `K7_2_SEEDS` accepts, compared as text.
const SMOKE_BLOCKS: [(&str, SeedRange); 2] = [
    (
        "6000..6003",
        SeedRange {
            start: 6000,
            end: 6003,
        },
    ),
    (
        "6000..6030",
        SeedRange {
            start: 6000,
            end: 6030,
        },
    ),
];
const SIGNAL: OutcomeSignal = OutcomeSignal::RoleCoverage;

/// XOR applied to the battery seed of the seed-invariance runs.
const INVARIANCE_XOR: u64 = 0x9E37_79B9_7F4A_7C15;

/// H-move: ratio of median PRIMARY.
const BAR_RATIO: f64 = 1.25;
/// H-live and H-move: seeds, as a fraction of the block (18 of 30).
const BAR_SEEDS: (usize, usize) = (3, 5);
/// H-dead: tasks with an identical final member set, as a fraction of the
/// block's tasks (570 of 600).
const DEAD_TASKS: (usize, usize) = (95, 100);
/// H-dead: seeds with bit-identical PRIMARY, as a fraction of the block (24 of
/// 30).
const DEAD_SEEDS: (usize, usize) = (4, 5);

const LABEL_INVALID: &str = "RUN-INVALID";
const LABEL_SMOKE: &str = "SMOKE (no verdict)";
const LABEL_NOT_LIVE: &str = "FALSIFIED (novelty not live)";
const LABEL_VALIDATED: &str = "VALIDATED (novelty live, dead at the outcome)";
const LABEL_MOVES: &str = "FALSIFIED (novelty moves the outcome)";
const LABEL_NOT_DEAD: &str = "FALSIFIED (not dead by count, no effect at the bar)";

const GRP_TOPO: usize = 0;
const GRP_TOPO_NONOV: usize = 1;
const GRP_ROLE: usize = 2;
const GRP_ROLE_NONOV: usize = 3;

/// `(label, table role, routing, query_novelty)` of the four group cells,
/// indexed by the `GRP_*` constants.
const GROUP_CELLS: [(&str, &str, VoteRouting, bool); 4] = [
    (
        "grp-topo",
        "contrast, novelty on",
        VoteRouting::CandidateStar { lambda: 0.5 },
        true,
    ),
    (
        "grp-topo-nonov",
        "contrast, novelty off",
        VoteRouting::CandidateStar { lambda: 0.5 },
        false,
    ),
    ("grp-role", "reference", VoteRouting::Off, true),
    ("grp-role-nonov", "reference", VoteRouting::Off, false),
];

/// `(novelty on, novelty off)` cell indices of the two pairs.
const PAIRS: [(usize, usize); 2] = [(GRP_TOPO, GRP_TOPO_NONOV), (GRP_ROLE, GRP_ROLE_NONOV)];
/// The cells whose topology carries a non-identity row.
const ROUTED_CELLS: [usize; 2] = [GRP_TOPO, GRP_TOPO_NONOV];
/// The cells whose `routed_reads` is `0` by construction.
const UNROUTED_CELLS: [usize; 2] = [GRP_ROLE, GRP_ROLE_NONOV];

/// The registered configuration of group cell `cell`.
fn cell_config(cell: usize) -> GroupAifConfig {
    let (_, _, routing, query_novelty) = GROUP_CELLS[cell];
    GroupAifConfig {
        routing,
        base: PersistentAifConfig {
            query_novelty,
            ..v5_e1_base()
        },
        ..GroupAifConfig::default()
    }
}

/// One policy over one instance: the metrics, the decision trace and the
/// reconstruction of the trace.
struct SeedRun {
    result: WorkflowResult,
    trace: Vec<TraceEntry>,
    recon: Recon,
}

/// One group policy's ledger after its instance.
struct Ledger {
    counters: GroupAifCounters,
    moved: NonVacuity,
}

/// One arm over the block: per-seed runs in seed order, per-seed ledgers (empty
/// for `ref-prune`), every decision latency in call order.
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

/// One pass count of a gate: `ok` of `n` items of `label` satisfy the
/// predicate.
struct Part {
    label: String,
    ok: usize,
    n: usize,
}

/// A gate's outcome. [`Gate::line`] is the printed gate line: the name, the
/// word, the predicate's fixed text and the pass counts. `failures` and
/// `detail` are rendered into the report buffer only.
struct Gate {
    name: &'static str,
    predicate: String,
    parts: Vec<Part>,
    failures: Vec<String>,
    detail: Vec<String>,
}

impl Gate {
    fn pass(&self) -> bool {
        self.parts.iter().all(|p| p.ok == p.n)
    }

    fn line(&self) -> String {
        let word = if self.pass() { "PASS" } else { "FAIL" };
        let counts: Vec<String> = self
            .parts
            .iter()
            .map(|p| format!("{} {}/{}", p.label, p.ok, p.n))
            .collect();
        format!(
            "- **{} — {word}.** {}: {}.",
            self.name,
            self.predicate,
            counts.join(" · ")
        )
    }
}

/// The block and whether it came from `K7_2_SEEDS`.
///
/// # Errors
///
/// The refusal message for a `K7_2_SEEDS` value outside [`SMOKE_BLOCKS`].
fn seed_block() -> Result<(SeedRange, bool), String> {
    let refusal = |value: &str| {
        let accepted: Vec<String> = SMOKE_BLOCKS
            .iter()
            .map(|(text, _)| format!("`{text}`"))
            .collect();
        format!(
            "{SEEDS_ENV}={value} refused: the accepted values are {} (prereg §8); the registered \
             block {REGISTERED_SEEDS} runs without {SEEDS_ENV}",
            accepted.join(" and ")
        )
    };
    match std::env::var(SEEDS_ENV) {
        Err(std::env::VarError::NotPresent) => Ok((REGISTERED_SEEDS, false)),
        Err(std::env::VarError::NotUnicode(_)) => Err(refusal("<not unicode>")),
        Ok(text) => SMOKE_BLOCKS
            .iter()
            .find(|(accepted, _)| *accepted == text)
            .map(|&(_, range)| (range, true))
            .ok_or_else(|| refusal(&text)),
    }
}

/// Run `policy` traced over `inst` and reconstruct its trace.
fn run_traced(
    policy: &dyn CoalitionDecisionPolicy,
    inst: &WorkflowInstance,
    latencies: &mut Vec<f64>,
) -> Result<SeedRun, Box<dyn Error>> {
    let traced = TracedPolicy::new(policy);
    let result = run_workflow_instance(&traced, inst, SIGNAL, latencies)?;
    let trace = traced.entries();
    let recon = reconstruct(inst, &trace)?;
    Ok(SeedRun {
        result,
        trace,
        recon,
    })
}

/// `inst.role_map()` with each role id as a `Role`.
fn role_map(inst: &WorkflowInstance) -> Result<HashMap<usize, Role>, Box<dyn Error>> {
    inst.role_map()
        .into_iter()
        .map(|(id, role)| Ok((id, Role::new(u8::try_from(role)?))))
        .collect()
}

/// One fresh `config` policy per instance, seeded `instance.seed ^ seed_xor`.
fn run_group(
    cell: usize,
    config: GroupAifConfig,
    instances: &[WorkflowInstance],
    seed_xor: u64,
) -> Result<Cell, Box<dyn Error>> {
    let (label, role, _, _) = GROUP_CELLS[cell];
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
        let run = run_traced(&policy, inst, &mut cell.latencies)?;
        let counters = policy.counters();
        let moved = models_moved(&policy.model_snapshots(), &before, &counters.model_updates);
        cell.runs.push(run);
        cell.ledgers.push(Ledger { counters, moved });
    }
    Ok(cell)
}

/// `ref-prune`: one fresh [`RefPrune`] per instance over the instance's role
/// map.
fn run_reference(instances: &[WorkflowInstance]) -> Result<Cell, Box<dyn Error>> {
    let mut cell = Cell {
        label: "ref-prune",
        role: "reference",
        runs: Vec::with_capacity(instances.len()),
        ledgers: Vec::new(),
        latencies: Vec::new(),
    };
    for inst in instances {
        let policy = RefPrune::new(role_map(inst)?);
        let run = run_traced(&policy, inst, &mut cell.latencies)?;
        cell.runs.push(run);
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

/// The pass count of `first` against `second` under [`identical`], and the
/// differing seeds.
fn identity_part(first: &Cell, second: &Cell) -> (Part, Vec<String>) {
    let differing = differing_seeds(first, second);
    let n = first.runs.len().max(second.runs.len());
    let part = Part {
        label: format!("`{}`", first.label),
        ok: first
            .runs
            .iter()
            .zip(&second.runs)
            .filter(|(x, y)| identical(x, y))
            .count(),
        n,
    };
    (part, differing)
}

fn gate_recon(all: &[&Cell]) -> Gate {
    let mut failures = Vec::new();
    let mut parts = Vec::new();
    let mut churn_equal = 0usize;
    let mut total = 0usize;
    for cell in all {
        let mut ok = 0usize;
        for run in &cell.runs {
            total += 1;
            churn_equal += usize::from(run.recon.churn == run.result.churn);
            if run.recon.primary.to_bits() == run.result.primary.to_bits() {
                ok += 1;
            } else {
                failures.push(format!(
                    "`{}` {} (recomputed {:?}, harness {:?})",
                    cell.label, run.result.seed, run.recon.primary, run.result.primary
                ));
            }
        }
        parts.push(Part {
            label: format!("`{}`", cell.label),
            ok,
            n: cell.runs.len(),
        });
    }
    Gate {
        name: "X-recon",
        predicate: "PRIMARY recomputed from the final member sets reconstructed from the arrival \
                    orders and the trace (every trace consumed exactly) against \
                    `WorkflowResult::primary`, bitwise, seeds passing per cell"
            .to_owned(),
        parts,
        failures,
        detail: vec![format!(
            "X-recon disclosure, not part of the predicate: recomputed churn equals \
             `WorkflowResult::churn` on {churn_equal}/{total} cell-seeds."
        )],
    }
}

fn gate_determinism(cells: &[Cell], reruns: &[Cell]) -> Gate {
    let mut failures = Vec::new();
    let mut parts = Vec::new();
    for (first, second) in cells.iter().zip(reruns) {
        let (part, differing) = identity_part(first, second);
        parts.push(part);
        failures.extend(differing);
    }
    Gate {
        name: "S-determinism",
        predicate: "every group cell re-run from scratch and compared per seed on trace entries \
                    (leave flag, act, raw score bits), PRIMARY bits and churn, seeds identical \
                    per cell"
            .to_owned(),
        parts,
        failures,
        detail: Vec::new(),
    }
}

/// `reseeded` holds one cell per entry of [`ROUTED_CELLS`], in that order.
fn gate_invariance(cells: &[Cell], reseeded: &[Cell]) -> Gate {
    let mut failures = Vec::new();
    let mut parts = Vec::new();
    for (&i, second) in ROUTED_CELLS.iter().zip(reseeded) {
        let (part, differing) = identity_part(&cells[i], second);
        parts.push(part);
        failures.extend(differing);
    }
    Gate {
        name: "Seed invariance",
        predicate: format!(
            "`grp-topo` and `grp-topo-nonov` rebuilt with `battery_seed = seed ^ \
             {INVARIANCE_XOR:#018X}`, same comparison, seeds identical per cell"
        ),
        parts,
        failures,
        detail: Vec::new(),
    }
}

fn gate_learn(cells: &[Cell]) -> Gate {
    let mut failures = Vec::new();
    let mut parts = Vec::new();
    let mut detail =
        vec!["S-learn (i) exempt models (`expected == 0`), as `(seed, model)`:".to_owned()];
    for cell in cells {
        let mut ok = 0usize;
        let mut exempt = Vec::new();
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
            for label in &ledger.moved.exempt {
                let name = match label {
                    ModelLabel::Role(r) => format!("r{}", r.index()),
                    ModelLabel::Shared => "shared".to_owned(),
                };
                exempt.push(format!("({}, {name})", run.result.seed));
            }
        }
        parts.push(Part {
            label: format!("`{}`", cell.label),
            ok,
            n: cell.runs.len(),
        });
        let listed = if exempt.is_empty() {
            "none".to_owned()
        } else {
            exempt.join(" · ")
        };
        detail.push(format!("  `{}`: {listed}", cell.label));
    }
    Gate {
        name: "S-learn (i)",
        predicate: format!(
            "per seed and group cell `s_learn_exact()`, `models_moved(..).ok` with its \
             `expected == 0` exemption at `S_LEARN_VACUITY_TOL` = {S_LEARN_VACUITY_TOL:e}, \
             `begin_task_rejections == 0`, seeds passing per cell"
        ),
        parts,
        failures,
        detail,
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
    let mut totals = Vec::new();
    let mut positive = 0usize;
    for &i in &ROUTED_CELLS {
        let cell = &cells[i];
        let mut total = 0u64;
        let mut ok = 0usize;
        for (run, ledger) in cell.runs.iter().zip(&cell.ledgers) {
            let routed = ledger.counters.routed_reads;
            let expected = routable_reads(&ledger.counters);
            total += routed;
            if routed == expected {
                ok += 1;
            } else {
                failures.push(format!(
                    "`{}` {} (routed_reads {routed}, ledger {expected})",
                    cell.label, run.result.seed
                ));
            }
        }
        if total > 0 {
            positive += 1;
        } else {
            failures.push(format!("`{}` block total 0", cell.label));
        }
        parts.push(Part {
            label: format!("`{}`", cell.label),
            ok,
            n: cell.runs.len(),
        });
        totals.push(format!("`{}` {total}", cell.label));
    }
    for &i in &UNROUTED_CELLS {
        let cell = &cells[i];
        let mut total = 0u64;
        let mut ok = 0usize;
        for (run, ledger) in cell.runs.iter().zip(&cell.ledgers) {
            let routed = ledger.counters.routed_reads;
            total += routed;
            if routed == 0 {
                ok += 1;
            } else {
                failures.push(format!(
                    "`{}` {} (routed_reads {routed}, expected 0)",
                    cell.label, run.result.seed
                ));
            }
        }
        parts.push(Part {
            label: format!("`{}`", cell.label),
            ok,
            n: cell.runs.len(),
        });
        totals.push(format!("`{}` {total}", cell.label));
    }
    parts.push(Part {
        label: "routed cells with block total > 0".to_owned(),
        ok: positive,
        n: ROUTED_CELLS.len(),
    });
    Gate {
        name: "S-route",
        predicate: "per seed on `grp-topo` and `grp-topo-nonov` `routed_reads` == ledger reads \
                    with `candidate_sensitive >= 1` and `roster >= 2`, block total > 0; on \
                    `grp-role` and `grp-role-nonov` `routed_reads == 0`; seeds passing per cell"
            .to_owned(),
        parts,
        failures,
        detail: vec![format!("S-route block totals: {}.", totals.join(" · "))],
    }
}

/// The first position at which a pair's traces differ by act.
#[derive(Clone, Copy)]
struct First {
    position: usize,
    /// The entry's read kind.
    leave: bool,
    /// `true` when the novelty-on cell acted and the novelty-off cell declined.
    on_acted: bool,
}

/// Position-wise divergence of a pair's traces on one seed, compared up to the
/// shorter trace.
struct SeedDivergence {
    seed: u64,
    act: usize,
    score_bits: usize,
    length: usize,
    first: Option<First>,
}

/// `on` is the pair's novelty-on cell, `off` its novelty-off cell.
fn seed_divergences(on: &Cell, off: &Cell) -> Vec<SeedDivergence> {
    on.runs
        .iter()
        .zip(&off.runs)
        .map(|(x, y)| {
            let pairs = || x.trace.iter().zip(&y.trace);
            SeedDivergence {
                seed: x.result.seed,
                act: pairs().filter(|(p, q)| p.act != q.act).count(),
                score_bits: pairs()
                    .filter(|(p, q)| p.score_bits != q.score_bits)
                    .count(),
                length: x.trace.len().abs_diff(y.trace.len()),
                first: pairs().enumerate().find(|(_, (p, q))| p.act != q.act).map(
                    |(position, (p, _))| First {
                        position,
                        leave: p.leave,
                        on_acted: p.act,
                    },
                ),
            }
        })
        .collect()
}

/// Pooled first-divergence counts, indexed `[leave][on_acted]`, and the seeds
/// without an act difference.
struct FirstPool {
    counts: [[usize; 2]; 2],
    none: usize,
}

fn first_pool(per_seed: &[SeedDivergence]) -> FirstPool {
    let mut pool = FirstPool {
        counts: [[0; 2]; 2],
        none: 0,
    };
    for d in per_seed {
        match d.first {
            Some(f) => pool.counts[usize::from(f.leave)][usize::from(f.on_acted)] += 1,
            None => pool.none += 1,
        }
    }
    pool
}

/// `"<kind>, `<cell>` acted: n"` for the four buckets, then the seeds without a
/// divergence.
fn first_pool_text(pool: &FirstPool, on: &str, off: &str) -> String {
    let mut parts = Vec::new();
    for (leave, kind) in [(false, "join"), (true, "leave")] {
        for (on_acted, cell) in [(true, on), (false, off)] {
            parts.push(format!(
                "{kind} read, `{cell}` acted: {}",
                pool.counts[usize::from(leave)][usize::from(on_acted)]
            ));
        }
    }
    parts.push(format!("no act difference: {}", pool.none));
    parts.join(" · ")
}

/// One cell against another over the block, from the reconstructions and the
/// harness PRIMARY.
struct Against {
    same_tasks: usize,
    tasks: usize,
    same_seeds: usize,
    seeds: usize,
    /// Of the tasks whose final member sets differ, those whose
    /// `(covered steps, final size)` are equal.
    differing_same_shape: usize,
}

fn against(cell: &Cell, base: &Cell) -> Against {
    let mut out = Against {
        same_tasks: 0,
        tasks: 0,
        same_seeds: 0,
        seeds: cell.runs.len().max(base.runs.len()),
        differing_same_shape: 0,
    };
    for (run, other) in cell.runs.iter().zip(&base.runs) {
        let (same, total) = member_set_identity(&run.recon, &other.recon);
        out.same_tasks += same;
        out.tasks += total;
        out.differing_same_shape += run
            .recon
            .tasks
            .iter()
            .zip(&other.recon.tasks)
            .filter(|(a, b)| {
                a.members != b.members
                    && (a.covered, a.members.len()) == (b.covered, b.members.len())
            })
            .count();
        out.same_seeds +=
            usize::from(run.result.primary.to_bits() == other.result.primary.to_bits());
    }
    out
}

/// `ceil(n * num / den)`.
fn at_least(n: usize, (num, den): (usize, usize)) -> usize {
    (n * num).div_ceil(den)
}

/// H-live: `grp-topo-nonov` against `grp-topo`.
struct Live {
    score_seeds: usize,
    act_seeds: usize,
    seeds: usize,
    bar: usize,
}

impl Live {
    fn score_ok(&self) -> bool {
        self.score_seeds >= self.bar
    }

    fn act_ok(&self) -> bool {
        self.act_seeds >= self.bar
    }

    fn pass(&self) -> bool {
        self.score_ok() && self.act_ok()
    }
}

/// H-dead's two conjuncts for one routed cell against `ref-prune`.
struct DeadLeg {
    label: &'static str,
    found: Against,
    tasks_bar: usize,
    seeds_bar: usize,
}

impl DeadLeg {
    fn tasks_ok(&self) -> bool {
        self.found.same_tasks >= self.tasks_bar
    }

    fn seeds_ok(&self) -> bool {
        self.found.same_seeds >= self.seeds_bar
    }

    fn pass(&self) -> bool {
        self.tasks_ok() && self.seeds_ok()
    }
}

/// H-move read in one direction: `better` against `other`.
struct MoveLeg {
    better: &'static str,
    other: &'static str,
    /// `"on"` or `"off"`: the novelty setting of `better`.
    novelty: &'static str,
    better_median: f64,
    other_median: f64,
    superior: usize,
    seeds: usize,
    bar: usize,
}

impl MoveLeg {
    fn ratio_ok(&self) -> bool {
        self.other_median > 0.0 && self.better_median / self.other_median >= BAR_RATIO
    }

    fn superior_ok(&self) -> bool {
        self.superior >= self.bar
    }

    fn pass(&self) -> bool {
        self.ratio_ok() && self.superior_ok()
    }
}

struct Criterion {
    live: Live,
    dead: [DeadLeg; 2],
    moves: [MoveLeg; 2],
}

impl Criterion {
    fn dead_pass(&self) -> bool {
        self.dead.iter().all(DeadLeg::pass)
    }

    fn move_pass(&self) -> Option<&MoveLeg> {
        self.moves.iter().find(|m| m.pass())
    }

    /// §6's label for a run whose gates hold, in precedence order.
    fn label(&self) -> &'static str {
        if !self.live.pass() {
            LABEL_NOT_LIVE
        } else if self.dead_pass() {
            LABEL_VALIDATED
        } else if self.move_pass().is_some() {
            LABEL_MOVES
        } else {
            LABEL_NOT_DEAD
        }
    }
}

fn criterion(cells: &[Cell], reference: &Cell) -> Criterion {
    let on = &cells[GRP_TOPO];
    let off = &cells[GRP_TOPO_NONOV];
    let seeds = on.runs.len();
    let per_seed = seed_divergences(on, off);
    let live = Live {
        score_seeds: per_seed.iter().filter(|d| d.score_bits > 0).count(),
        act_seeds: per_seed.iter().filter(|d| d.act > 0).count(),
        seeds,
        bar: at_least(seeds, BAR_SEEDS),
    };
    let dead_leg = |cell: &Cell| {
        let found = against(cell, reference);
        DeadLeg {
            label: cell.label,
            tasks_bar: at_least(found.tasks, DEAD_TASKS),
            seeds_bar: at_least(found.seeds, DEAD_SEEDS),
            found,
        }
    };
    let move_leg = |better: &Cell, other: &Cell, novelty: &'static str| MoveLeg {
        better: better.label,
        other: other.label,
        novelty,
        better_median: better.median_primary(),
        other_median: other.median_primary(),
        superior: superior_count(&better.primaries(), &other.primaries()),
        seeds,
        bar: at_least(seeds, BAR_SEEDS),
    };
    Criterion {
        live,
        dead: [dead_leg(on), dead_leg(off)],
        moves: [move_leg(on, off, "on"), move_leg(off, on, "off")],
    }
}

/// `"<pct> (<n> of <d>)"`.
fn share(n: usize, d: usize) -> String {
    format!("{} ({n} of {d})", pct(n, d))
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

fn word(ok: bool) -> &'static str {
    if ok { "PASS" } else { "FAIL" }
}

fn render_header(
    out: &mut String,
    spec: &WorkflowSpec,
    seeds: SeedRange,
    smoke: bool,
) -> fmt::Result {
    writeln!(out, "# K7-2 — novelty on/off on the topology-routed group")?;
    writeln!(out)?;
    if smoke {
        writeln!(
            out,
            "**SMOKE — NOT THE REGISTERED BLOCK** (`{SEEDS_ENV}={seeds}`; the registered block is {REGISTERED_SEEDS}). The report is rendered and not printed."
        )?;
        writeln!(out)?;
    }
    writeln!(out, "- registration: K7-2 (koalisi #97)")?;
    writeln!(out, "- prereg: `{PREREG}` (Amendments 1 and 2 included)")?;
    writeln!(out, "- koalisi: v{}", env!("CARGO_PKG_VERSION"))?;
    writeln!(out, "- seeds: {seeds} ({} seeds)", seeds.len())?;
    writeln!(out, "- outcome signal: {SIGNAL:?}")?;
    writeln!(
        out,
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
    )?;
    writeln!(out)
}

/// The gate lines: the four gates checked outside the binary, then each
/// in-binary gate's [`Gate::line`].
fn render_gate_lines(out: &mut String, gates: &[Gate]) -> fmt::Result {
    writeln!(out, "## Gates")?;
    writeln!(out)?;
    writeln!(
        out,
        "- **X-battery** — checked outside this binary, conditional on the diff (prereg §5): `git diff --name-only v0.39.0..HEAD -- src Cargo.toml Cargo.lock`; a path under `src/` outside `src/harness/`, a dependency line in `Cargo.toml` or a foreign stanza in `Cargo.lock` puts on one serial run of `examples/strategy_comparison.rs` diffed against `docs/runs/K4-archive.log` with the latency column stripped (`docs/runs/README.md`); otherwise recorded not run."
    )?;
    writeln!(
        out,
        "- **X-host** — checked outside this binary: `cargo test --features harness,decision,process --test k7_group_host` (330..360 against `docs/runs/K4-archive.log`)."
    )?;
    writeln!(
        out,
        "- **X-carry** — checked outside this binary: the same test binary (90..120 against `docs/runs/K7-1.log`)."
    )?;
    writeln!(
        out,
        "- **S-nov** — checked outside this binary: `cargo test --features harness,decision,process --test k7_2_novelty` (hand-built fixture)."
    )?;
    for gate in gates {
        writeln!(out, "{}", gate.line())?;
    }
    writeln!(out)
}

fn render_gate_detail(out: &mut String, gates: &[Gate]) -> fmt::Result {
    writeln!(out, "## Gate detail")?;
    writeln!(out)?;
    for gate in gates {
        for line in &gate.detail {
            writeln!(out, "- {line}")?;
        }
        if !gate.pass() {
            writeln!(out, "- {} failing: {}", gate.name, gate.failures.join("; "))?;
        }
    }
    writeln!(out)
}

/// `all` is every cell in table order: the four group cells, `ref-prune`.
fn render_arms(out: &mut String, cells: &[Cell], all: &[&Cell]) -> fmt::Result {
    writeln!(out, "## Arms (pooled; E-lat is record-only)")?;
    writeln!(out)?;
    writeln!(
        out,
        "| arm | role | median PRIMARY | median churn | E-lat: median µs/decision |"
    )?;
    writeln!(out, "|---|---|---:|---:|---:|")?;
    for cell in all {
        writeln!(
            out,
            "| `{}` | {} | {:.4} | {:.2} | {:.3} |",
            cell.label,
            cell.role,
            cell.median_primary(),
            cell.median_churn(),
            median_iqr(&cell.latencies).0
        )?;
    }
    writeln!(out)?;

    writeln!(out, "## Per seed")?;
    writeln!(out)?;
    let labels: Vec<String> = all.iter().map(|c| format!("`{}`", c.label)).collect();
    writeln!(
        out,
        "| seed | n | {} | churn `grp-topo` | churn `grp-topo-nonov` |",
        labels.join(" | ")
    )?;
    writeln!(out, "|---:|---:|{}---:|---:|", "---:|".repeat(labels.len()))?;
    for (i, run) in cells[GRP_TOPO].runs.iter().enumerate() {
        let primaries: Vec<String> = all
            .iter()
            .map(|c| format!("{:.4}", c.runs[i].result.primary))
            .collect();
        writeln!(
            out,
            "| {} | {} | {} | {} | {} |",
            run.result.seed,
            run.result.n,
            primaries.join(" | "),
            run.result.churn,
            cells[GRP_TOPO_NONOV].runs[i].result.churn
        )?;
    }
    writeln!(out)
}

fn render_criterion(out: &mut String, c: &Criterion) -> fmt::Result {
    writeln!(
        out,
        "## The counted criterion (against cells of this battery)"
    )?;
    writeln!(out)?;
    let live = &c.live;
    writeln!(
        out,
        "- H-live conjunct 1 — `grp-topo-nonov` vs `grp-topo`, seeds with ≥ 1 raw-score-bit difference ≥ {}/{}: {}/{} — **{}**",
        live.bar,
        live.seeds,
        live.score_seeds,
        live.seeds,
        word(live.score_ok())
    )?;
    writeln!(
        out,
        "- H-live conjunct 2 — seeds with ≥ 1 act difference ≥ {}/{}: {}/{} — **{}**",
        live.bar,
        live.seeds,
        live.act_seeds,
        live.seeds,
        word(live.act_ok())
    )?;
    writeln!(out, "- H-live: **{}**", word(live.pass()))?;
    for leg in &c.dead {
        writeln!(
            out,
            "- H-dead, `{}` vs `ref-prune` — final member set identical on ≥ {} of {} tasks: {} — **{}**; PRIMARY bit-identical on ≥ {}/{} seeds: {}/{} — **{}**",
            leg.label,
            leg.tasks_bar,
            leg.found.tasks,
            leg.found.same_tasks,
            word(leg.tasks_ok()),
            leg.seeds_bar,
            leg.found.seeds,
            leg.found.same_seeds,
            leg.found.seeds,
            word(leg.seeds_ok())
        )?;
    }
    writeln!(out, "- H-dead: **{}**", word(c.dead_pass()))?;
    let read = if c.dead_pass() {
        "not read: H-dead passes; the numbers are a disclosure"
    } else {
        "read: H-dead fails"
    };
    writeln!(out, "- H-move ({read}):")?;
    for leg in &c.moves {
        writeln!(
            out,
            "  - `{}` (novelty {}) over `{}` — other's median {:.4} > 0 and ratio of medians ≥ {BAR_RATIO:.2}×: {:.4} / {:.4} = {} — **{}**; strictly superior on ≥ {}/{} seeds: {}/{} — **{}**",
            leg.better,
            leg.novelty,
            leg.other,
            leg.other_median,
            leg.better_median,
            leg.other_median,
            ratio_text(leg.better_median, leg.other_median),
            word(leg.ratio_ok()),
            leg.bar,
            leg.seeds,
            leg.superior,
            leg.seeds,
            word(leg.superior_ok())
        )?;
    }
    writeln!(out, "- H-move: **{}**", word(c.move_pass().is_some()))?;
    writeln!(out)
}

fn render_live(out: &mut String, cells: &[Cell]) -> fmt::Result {
    writeln!(
        out,
        "## S-live (disclosure — compared by position up to the shorter trace)"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "The ACT and score-bit counts are positional; an upper bound after the first divergence. *Seeds with any difference* is uncontaminated."
    )?;
    writeln!(out)?;
    let mut pooled = Vec::new();
    for &(on, off) in &PAIRS {
        let (on, off) = (&cells[on], &cells[off]);
        let per_seed = seed_divergences(on, off);
        writeln!(
            out,
            "| `{}` vs `{}`, seed | decisions differing by ACT | decisions differing by raw score bits | length difference |",
            off.label, on.label
        )?;
        writeln!(out, "|---:|---:|---:|---:|")?;
        for d in &per_seed {
            writeln!(
                out,
                "| {} | {} | {} | {} |",
                d.seed, d.act, d.score_bits, d.length
            )?;
        }
        writeln!(out)?;
        pooled.push(format!(
            "| `{}` vs `{}` | {} | {} | {}/{n} | {}/{n} | {} |",
            off.label,
            on.label,
            per_seed.iter().map(|d| d.act).sum::<usize>(),
            per_seed.iter().map(|d| d.score_bits).sum::<usize>(),
            per_seed.iter().filter(|d| d.act > 0).count(),
            per_seed.iter().filter(|d| d.score_bits > 0).count(),
            per_seed.iter().map(|d| d.length).sum::<usize>(),
            n = per_seed.len()
        ));
    }
    writeln!(
        out,
        "| pair (pooled) | decisions differing by ACT | decisions differing by raw score bits | seeds with any act difference | seeds with any score-bit difference | total length difference |"
    )?;
    writeln!(out, "|---|---:|---:|---:|---:|---:|")?;
    for row in pooled {
        writeln!(out, "{row}")?;
    }
    writeln!(out)
}

fn render_first(out: &mut String, cells: &[Cell]) -> fmt::Result {
    writeln!(out, "## First divergence (disclosure)")?;
    writeln!(out)?;
    for &(on, off) in &PAIRS {
        let (on, off) = (&cells[on], &cells[off]);
        let per_seed = seed_divergences(on, off);
        writeln!(
            out,
            "| `{}` vs `{}`, seed | position of the first act difference | read kind | cell that acted |",
            off.label, on.label
        )?;
        writeln!(out, "|---:|---:|---|---|")?;
        for d in &per_seed {
            match d.first {
                Some(f) => writeln!(
                    out,
                    "| {} | {} | {} | `{}` |",
                    d.seed,
                    f.position,
                    if f.leave { "leave" } else { "join" },
                    if f.on_acted { on.label } else { off.label }
                )?,
                None => writeln!(out, "| {} | — | — | — |", d.seed)?,
            }
        }
        writeln!(out)?;
        writeln!(
            out,
            "- pooled, `{}` vs `{}`: {}",
            off.label,
            on.label,
            first_pool_text(&first_pool(&per_seed), on.label, off.label)
        )?;
        writeln!(out)?;
    }
    Ok(())
}

fn render_against(out: &mut String, cells: &[Cell], reference: &Cell) -> fmt::Result {
    writeln!(out, "## Against `ref-prune` (disclosure)")?;
    writeln!(out)?;
    writeln!(
        out,
        "| cell | tasks with final member set identical to `ref-prune`'s | seeds with PRIMARY bit-identical to `ref-prune`'s | differing tasks whose (covered steps, final size) equal `ref-prune`'s |"
    )?;
    writeln!(out, "|---|---:|---:|---:|")?;
    for cell in cells {
        let a = against(cell, reference);
        writeln!(
            out,
            "| `{}` | {} | {} of {} | {} of {} |",
            cell.label,
            share(a.same_tasks, a.tasks),
            a.same_seeds,
            a.seeds,
            a.differing_same_shape,
            a.tasks - a.same_tasks
        )?;
    }
    writeln!(out)?;

    writeln!(out, "## Inside each pair (disclosure)")?;
    writeln!(out)?;
    writeln!(
        out,
        "| pair | tasks with identical final member set | seeds with bit-identical PRIMARY | median PRIMARY off / on | ratio of medians off / on | seeds off strictly superior | seeds on strictly superior |"
    )?;
    writeln!(out, "|---|---:|---:|---:|---:|---:|---:|")?;
    for &(on, off) in &PAIRS {
        let (on, off) = (&cells[on], &cells[off]);
        let a = against(off, on);
        writeln!(
            out,
            "| `{}` vs `{}` | {} | {} of {} | {:.4} / {:.4} | {} | {}/{} | {}/{} |",
            off.label,
            on.label,
            share(a.same_tasks, a.tasks),
            a.same_seeds,
            a.seeds,
            off.median_primary(),
            on.median_primary(),
            ratio_text(off.median_primary(), on.median_primary()),
            superior_count(&off.primaries(), &on.primaries()),
            a.seeds,
            superior_count(&on.primaries(), &off.primaries()),
            a.seeds
        )?;
    }
    writeln!(out)
}

/// Whether the read's group act equals the centre's own argmax (`1` is act);
/// `None` on a centre-absent read.
fn follows(s: &AgreementSample) -> Option<bool> {
    s.centre_vote.map(|vote| s.group_act == (vote == 1))
}

/// `(centre-present reads, those on which the group act differs from the
/// centre's own argmax)` of `cell`.
fn centre_departures(cell: &Cell) -> (usize, usize) {
    let outcomes: Vec<bool> = cell.samples().iter().filter_map(follows).collect();
    (outcomes.len(), outcomes.iter().filter(|&&f| !f).count())
}

fn render_follow(out: &mut String, cells: &[Cell]) -> fmt::Result {
    writeln!(
        out,
        "## E-follow (every group cell, all centre-present reads, from the `AgreementSample` ledger)"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "| cell | join reads: group act == centre's own argmax | leave reads | all reads | mean blind-origin mass share (all reads) |"
    )?;
    writeln!(out, "|---|---:|---:|---:|---:|")?;
    for cell in cells {
        let samples = cell.samples();
        let tally = |kind: Option<bool>| {
            let outcomes: Vec<bool> = samples
                .iter()
                .filter(|s| kind.is_none_or(|leave| s.leave == leave))
                .filter_map(follows)
                .collect();
            share(outcomes.iter().filter(|&&f| f).count(), outcomes.len())
        };
        writeln!(
            out,
            "| `{}` | {} | {} | {} | {} |",
            cell.label,
            tally(Some(false)),
            tally(Some(true)),
            tally(None),
            or_na(mean(samples.iter().map(|s| s.blind_origin_share)), 3)
        )?;
    }
    writeln!(out)
}

/// `all` is every cell in table order; `roles` bounds the realised roster.
fn render_decomposition(out: &mut String, cells: &[Cell], all: &[&Cell], roles: u8) -> fmt::Result {
    writeln!(
        out,
        "## Outcome decomposition (K7-1 A2.3's; from the reconstruction)"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "| cell | roster | tasks | success rate | mean covered fraction | mean cov_eff | mean final size | tasks ending empty | final members whose role has no demand |"
    )?;
    writeln!(out, "|---|---|---:|---:|---:|---:|---:|---:|---:|")?;
    for cell in all {
        let rows = roster_decomposition(cell.runs.iter().map(|r| &r.recon), usize::from(roles));
        for row in &rows {
            let roster = row
                .roster
                .map_or_else(|| "all".to_owned(), |r| r.to_string());
            writeln!(
                out,
                "| `{}` | {roster} | {} | {} | {} | {} | {} | {} | {} of {} |",
                cell.label,
                row.tasks,
                pct(row.successes, row.tasks),
                or_na(row.mean_covered_fraction(), 4),
                or_na(row.mean_cov_eff(), 4),
                or_na(row.mean_final_size(), 2),
                row.empty,
                row.off_demand,
                row.final_members
            )?;
        }
    }
    writeln!(out)?;

    writeln!(
        out,
        "### Act rates and churn by centre (group cells, from the `AgreementSample` ledger)"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "| cell | join act: centre-present | join act: centre-absent | leave act: centre-present | leave act: centre-absent | churn: centre-present | churn: centre-absent | churn (harness) |"
    )?;
    writeln!(out, "|---|---:|---:|---:|---:|---:|---:|---:|")?;
    for cell in cells {
        let samples = cell.samples();
        let count = |leave: bool, present: bool, acted: Option<bool>| {
            samples
                .iter()
                .filter(|s| s.leave == leave && s.centre_vote.is_some() == present)
                .filter(|s| acted.is_none_or(|a| s.group_act == a))
                .count()
        };
        let rate = |leave: bool, present: bool| {
            share(
                count(leave, present, Some(true)),
                count(leave, present, None),
            )
        };
        let churn: usize = cell.runs.iter().map(|r| r.result.churn).sum();
        writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} | {} | {churn} |",
            cell.label,
            rate(false, true),
            rate(false, false),
            rate(true, true),
            rate(true, false),
            count(true, true, Some(true)),
            count(true, false, Some(true))
        )?;
    }
    writeln!(out)
}

fn render_reach(out: &mut String, cells: &[Cell]) -> fmt::Result {
    writeln!(out, "## Candidate reach")?;
    writeln!(out)?;
    writeln!(
        out,
        "| cell | zero candidate-sensitive | mean blind internals | blind CW weight share (by member) | blind CW weight share (by origin) | leave `cfg0 == cfg1` | act rate: sensitive / blind |"
    )?;
    writeln!(out, "|---|---:|---:|---:|---:|---:|---|")?;
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
        writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {same} of {queries} | {} / {} |",
            cell.label,
            pct(zero, samples.len()),
            or_na(mean(samples.iter().map(|s| s.candidate_blind() as f64)), 2),
            or_na(mean(samples.iter().map(|s| s.blind_weight_share)), 3),
            or_na(mean(samples.iter().map(|s| s.blind_origin_share)), 3),
            pct(sensitive_acts, sensitive_total),
            pct(blind_acts, blind_total)
        )?;
    }
    writeln!(out)?;

    writeln!(out, "## Declines (a zero score is not a decline)")?;
    writeln!(out)?;
    writeln!(
        out,
        "| cell | decisions | reads | declines_upstream | declines_missing_role | declines_no_demand | begin_task_rejections |"
    )?;
    writeln!(out, "|---|---:|---:|---:|---:|---:|---:|")?;
    for cell in cells {
        let sum = |f: fn(&GroupAifCounters) -> u64| -> u64 {
            cell.ledgers.iter().map(|l| f(&l.counters)).sum()
        };
        writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} | {} |",
            cell.label,
            sum(|c| c.decisions),
            sum(|c| c.reads),
            sum(|c| c.declines_upstream),
            sum(|c| c.declines_missing_role),
            sum(|c| c.declines_no_demand),
            sum(|c| c.begin_task_rejections)
        )?;
    }
    writeln!(out)
}

fn render_clauses(out: &mut String, cells: &[Cell], c: &Criterion) -> fmt::Result {
    writeln!(
        out,
        "## Scoped clauses (prereg §6, reported under every verdict)"
    )?;
    writeln!(out)?;

    let mut premise = Vec::new();
    let mut departed = Vec::new();
    for &i in &ROUTED_CELLS {
        let cell = &cells[i];
        let (reads, differing) = centre_departures(cell);
        premise.push(format!(
            "`{}` {}",
            cell.label,
            share(reads - differing, reads)
        ));
        if differing > 0 {
            departed.push(format!(
                "*\"on `{}` the group's act differed from the centre's argmax on {differing} of {reads} centre-present reads; on those reads the contrast is not the centre's query alone.\"*",
                cell.label
            ));
        }
    }
    write!(
        out,
        "1. **The premise.** E-follow over all centre-present reads: {}.",
        premise.join(" · ")
    )?;
    for sentence in &departed {
        write!(out, " {sentence}")?;
    }
    writeln!(out)?;

    let median = |i: usize| cells[i].median_primary();
    writeln!(
        out,
        "2. **Against EQ5b's pair.** *\"novelty-off costs the unrouted group {} and the routed group {} on this block.\"* (`grp-role-nonov` {:.4} / `grp-role` {:.4}; `grp-topo-nonov` {:.4} / `grp-topo` {:.4}.)",
        ratio_text(median(GRP_ROLE_NONOV), median(GRP_ROLE)),
        ratio_text(median(GRP_TOPO_NONOV), median(GRP_TOPO)),
        median(GRP_ROLE_NONOV),
        median(GRP_ROLE),
        median(GRP_TOPO_NONOV),
        median(GRP_TOPO)
    )?;

    let pools: Vec<(FirstPool, &str, &str)> = PAIRS
        .iter()
        .map(|&(on, off)| {
            (
                first_pool(&seed_divergences(&cells[on], &cells[off])),
                cells[on].label,
                cells[off].label,
            )
        })
        .collect();
    let quoted: Vec<String> = pools
        .iter()
        .map(|(pool, on, off)| format!("`{off}` vs `{on}` — {}", first_pool_text(pool, on, off)))
        .collect();
    writeln!(
        out,
        "3. **Where the first divergence falls.** Pooled first-divergence seeds by read kind and acting cell: {}.",
        quoted.join("; ")
    )?;

    let failing: Vec<&DeadLeg> = c.dead.iter().filter(|leg| !leg.pass()).collect();
    let fourth = match failing.as_slice() {
        [leg] => format!(
            "`{}` left `ref-prune` on {} of {} tasks (PRIMARY bit-identical on {} of {} seeds); the other routed cell passes both conjuncts.",
            leg.label,
            leg.found.tasks - leg.found.same_tasks,
            leg.found.tasks,
            leg.found.same_seeds,
            leg.found.seeds
        ),
        [] => "does not apply: H-dead passes on both routed cells.".to_owned(),
        _ => "does not apply: H-dead fails on both routed cells.".to_owned(),
    };
    writeln!(out, "4. **H-dead failing on one cell only.** {fourth}")?;

    let (routed, _, _) = &pools[0];
    let diverged = c.live.seeds - routed.none;
    writeln!(
        out,
        "5. **§4's note.** S-nov's pinned values are outside this binary (`tests/k7_2_novelty.rs`; Amendments A1.1 and A2.2, with A1.3's correction of the note). The first-divergence table against the note's derived kind — a leave read on which `grp-topo` acts and `grp-topo-nonov` declines: {} of {diverged} seeds with an act difference; a join read on which `grp-topo` acts and `grp-topo-nonov` declines (Amendment A1.2's second difference): {} of {diverged}.",
        routed.counts[1][1], routed.counts[0][1]
    )?;
    writeln!(out)
}

/// §6's reading of `label` and the lock's continuation line, with H-live's two
/// conjuncts as given.
fn render_one_reading(
    out: &mut String,
    label: &str,
    c: &Criterion,
    (score_ok, act_ok): (bool, bool),
) -> fmt::Result {
    let dead_failures = || {
        let mut failed = Vec::new();
        for leg in &c.dead {
            if !leg.tasks_ok() {
                failed.push(format!(
                    "`{}` final member sets ({} of {}, bar {})",
                    leg.label, leg.found.same_tasks, leg.found.tasks, leg.tasks_bar
                ));
            }
            if !leg.seeds_ok() {
                failed.push(format!(
                    "`{}` PRIMARY bits ({} of {} seeds, bar {})",
                    leg.label, leg.found.same_seeds, leg.found.seeds, leg.seeds_bar
                ));
            }
        }
        failed.join("; ")
    };
    let reading = match label {
        LABEL_INVALID => "A gate failed; the criterion is not read.".to_owned(),
        LABEL_NOT_LIVE => match (score_ok, act_ok) {
            (true, false) => "*\"live in the score, dead at the decision\"*".to_owned(),
            (false, false) => "*\"inert on the routed arm\"*".to_owned(),
            _ => "H-live fails its score conjunct with the act conjunct passing; §6 pre-commits no reading for this case.".to_owned(),
        },
        LABEL_VALIDATED => "*\"on v2w under `RoleCoverage` the v5 novelty term changes what the routed group computes and which acts it takes, and not where it ends: with it and without it the arm ends where a learning-free arrival-order redundancy prune ends.\"*".to_owned(),
        LABEL_MOVES => {
            // The label is reached only with a passing leg; a smoke block
            // renders it for every criterion outcome.
            let leg = c.move_pass().unwrap_or(&c.moves[0]);
            format!(
                "*\"novelty {} is the better routed cell at the lineage bar ({}, {}/{})\"*. H-dead conjuncts failed: {}.",
                leg.novelty,
                ratio_text(leg.better_median, leg.other_median),
                leg.superior,
                leg.seeds,
                dead_failures()
            )
        }
        _ => format!(
            "H-live passes, H-dead and H-move fail. H-dead conjuncts failed: {}. The *against `ref-prune`* and *inside each pair* tables above carry the disclosures this verdict is reported with.",
            dead_failures()
        ),
    };
    writeln!(out, "`{label}` — {reading}")?;
    writeln!(out)?;
    let continuation = match label {
        LABEL_INVALID => None,
        LABEL_VALIDATED => Some(
            "the next registration moves the world (`OutcomeSignal::Performance` / `Both` with the harness's performance draw), on its own issue, next free K7 number, fresh seed block.",
        ),
        _ => Some("K7-3 is locked next, and its lock reads this report first."),
    };
    if let Some(text) = continuation {
        writeln!(out, "Continuation (the lock's): {text}")?;
        writeln!(out)?;
    }
    Ok(())
}

/// The registered block renders the reading of `label`. A smoke block renders
/// the reading of every label, and of each way H-live fails, whatever the
/// criterion found.
fn render_reading(out: &mut String, label: &str, c: &Criterion, smoke: bool) -> fmt::Result {
    writeln!(out, "## Reading")?;
    writeln!(out)?;
    let live = (c.live.score_ok(), c.live.act_ok());
    if !smoke {
        return render_one_reading(out, label, c, live);
    }
    writeln!(
        out,
        "Smoke block: every label's reading is rendered and none is a verdict."
    )?;
    writeln!(out)?;
    render_one_reading(out, LABEL_INVALID, c, live)?;
    for conjuncts in [(true, false), (false, false), (false, true)] {
        render_one_reading(out, LABEL_NOT_LIVE, c, conjuncts)?;
    }
    for label in [LABEL_VALIDATED, LABEL_MOVES, LABEL_NOT_DEAD] {
        render_one_reading(out, label, c, live)?;
    }
    Ok(())
}

/// What one execution writes to stdout: `header`, `gate_lines`, then `report`
/// on the registered block or its byte count under `K7_2_SEEDS`, then the
/// `VERDICT:` line.
struct Rendered {
    header: String,
    gate_lines: String,
    report: String,
    verdict: &'static str,
}

impl Rendered {
    fn stdout(&self, smoke: bool) -> String {
        let body = if smoke {
            format!(
                "smoke: {} bytes of report rendered and suppressed\n\n",
                self.report.len()
            )
        } else {
            self.report.clone()
        };
        format!(
            "{}{}{body}VERDICT: {}\n",
            self.header, self.gate_lines, self.verdict
        )
    }
}

/// Run every cell and gate over `instances`, then render.
fn run(
    spec: &WorkflowSpec,
    seeds: SeedRange,
    smoke: bool,
    instances: &[WorkflowInstance],
) -> Result<Rendered, Box<dyn Error>> {
    let run_cells = |seed_xor: u64| -> Result<Vec<Cell>, Box<dyn Error>> {
        (0..GROUP_CELLS.len())
            .map(|i| run_group(i, cell_config(i), instances, seed_xor))
            .collect()
    };
    let cells = run_cells(0)?;
    let reruns = run_cells(0)?;
    let reseeded = ROUTED_CELLS
        .iter()
        .map(|&i| run_group(i, cell_config(i), instances, INVARIANCE_XOR))
        .collect::<Result<Vec<_>, _>>()?;
    let reference = run_reference(instances)?;
    let all: Vec<&Cell> = cells.iter().chain([&reference]).collect();

    let gates = [
        gate_recon(&all),
        gate_determinism(&cells, &reruns),
        gate_invariance(&cells, &reseeded),
        gate_learn(&cells),
        gate_route(&cells),
    ];
    let gates_ok = gates.iter().all(Gate::pass);
    let found = criterion(&cells, &reference);
    let label = if gates_ok {
        found.label()
    } else {
        LABEL_INVALID
    };
    let verdict = if !gates_ok {
        LABEL_INVALID
    } else if smoke {
        LABEL_SMOKE
    } else {
        label
    };

    let mut header = String::new();
    render_header(&mut header, spec, seeds, smoke)?;
    let mut gate_lines = String::new();
    render_gate_lines(&mut gate_lines, &gates)?;

    let mut report = String::new();
    render_gate_detail(&mut report, &gates)?;
    render_arms(&mut report, &cells, &all)?;
    render_criterion(&mut report, &found)?;
    render_live(&mut report, &cells)?;
    render_first(&mut report, &cells)?;
    render_against(&mut report, &cells, &reference)?;
    render_follow(&mut report, &cells)?;
    render_decomposition(&mut report, &cells, &all, spec.roles)?;
    render_reach(&mut report, &cells)?;
    render_clauses(&mut report, &cells, &found)?;
    render_reading(&mut report, label, &found, smoke)?;

    Ok(Rendered {
        header,
        gate_lines,
        report,
        verdict,
    })
}

fn main() -> ExitCode {
    let (seeds, smoke) = match seed_block() {
        Ok(block) => block,
        Err(refusal) => {
            eprintln!("{refusal}");
            return ExitCode::from(2);
        }
    };
    let spec = WorkflowSpec::default();
    let instances = match seeds
        .iter()
        .map(|seed| spec.generate(seed))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(instances) => instances,
        Err(e) => {
            eprintln!("k7_2: instance generation failed before any cell ran: {e}");
            return ExitCode::FAILURE;
        }
    };
    match run(&spec, seeds, smoke, &instances) {
        Ok(rendered) => {
            print!("{}", rendered.stdout(smoke));
            ExitCode::SUCCESS
        }
        Err(e) => {
            let text = e.to_string();
            if smoke {
                eprintln!(
                    "k7_2: a cell failed to run; {} bytes of error text suppressed under {SEEDS_ENV}",
                    text.len()
                );
            } else {
                eprintln!("k7_2: a cell failed to run: {text}");
            }
            ExitCode::FAILURE
        }
    }
}
