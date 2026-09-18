//! K7-2 — `query_novelty` on against off on the topology-routed group, as
//! registered in `docs/k7/prereg-K7-2-novelty-routed-group.md` (Amendments
//! 1–4 included): four `GroupAifPolicy` cells and the `ref-prune`,
//! `ref-first` and `ref-keep` reference cells over `WorkflowSpec::default()`
//! with `OutcomeSignal::RoleCoverage`, one fresh policy per seed, the header
//! printed before any cell runs, every gate computed before the report is
//! rendered, and one `VERDICT:` line last.
//!
//! The registered block, seeds `150..180`, runs with `K7_2_OFFICIAL=1`, without
//! `K7_2_SEEDS`, on a build whose package version is `0.40.0`. `K7_2_SEEDS`
//! accepts `6000..6003` and `6000..6030` and refuses every other value; under
//! it the binary runs every cell and gate, renders the report into a buffer it
//! does not print, and prints the header, the gate lines (pass counts), one
//! fixed line and `VERDICT: SMOKE (no verdict)` — `VERDICT: RUN-INVALID` when
//! a gate fails. With neither variable, with both, with `K7_2_OFFICIAL` other
//! than `1`, or with any other environment variable whose name starts `K7_`,
//! the binary exits 2 before generating an instance.
//!
//! Run: `K7_2_OFFICIAL=1 cargo run --release --features
//! harness,decision,process --example k7_2`.

use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fmt::{self, Write as _};
use std::io::Write as _;
use std::process::ExitCode;

use koalisi::algorithms::AgentCapabilities;
use koalisi::decision::{
    AgreementSample, CoalitionDecisionPolicy, GroupAifConfig, GroupAifCounters, GroupAifPolicy,
    ModelLabel, NonVacuity, PersistentAifConfig, S_LEARN_VACUITY_TOL, VoteRouting, models_moved,
    v5_e1_base,
};
use koalisi::harness::{
    OutcomeSignal, Recon, ReconError, RefFirst, RefKeep, RefPrune, SeedRange, TraceEntry,
    TracedPolicy, WorkflowInstance, WorkflowResult, WorkflowSpec, WorkflowTask, median_iqr,
    member_set_identity, reconstruct, roster_decomposition, run_workflow_instance, step_covered,
    superior_count,
};
use koalisi::process::Role;

const PREREG: &str = "docs/k7/prereg-K7-2-novelty-routed-group.md";
const REGISTERED_SEEDS: SeedRange = SeedRange {
    start: 150,
    end: 180,
};
const SEEDS_ENV: &str = "K7_2_SEEDS";
const OFFICIAL_ENV: &str = "K7_2_OFFICIAL";
/// Environment variable names with this prefix are read by [`seed_block`].
const ENV_PREFIX: &str = "K7_";
/// The package version the registered block runs on.
const OFFICIAL_VERSION: &str = "0.40.0";
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

const REF_PRUNE: usize = 0;
const REF_FIRST: usize = 1;
const REF_KEEP: usize = 2;

/// The reference cells' labels, indexed by the `REF_*` constants.
const REFERENCE_LABELS: [&str; 3] = ["ref-prune", "ref-first", "ref-keep"];

/// Amendment A3.3's pre-committed readings of `grp-topo-nonov` against a
/// reference cell, as `(reference index, reading)`.
const REFERENCE_READINGS: [(usize, &str); 2] = [
    (
        REF_KEEP,
        "without the term the routed arm admits as the prune does and evicts no one; the eviction of redundant members is what the term carried",
    ),
    (
        REF_FIRST,
        "without the term the routed arm takes no act after the first arrival",
    ),
];

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

/// What the centre's query sees on one centre-present read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct CentreClass {
    /// `cfg0 & required_r == cfg1 & required_r` for the candidate's role `r`.
    identical: bool,
    /// Either of role `r`'s last two on-roster tasks ended with a covered bit
    /// that `r` requires on this task.
    window: bool,
}

/// One decision of a replayed trace.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Read {
    task: usize,
    leave: bool,
    act: bool,
    /// `None` when the candidate's role has no step in the task's demand.
    centre: Option<CentreClass>,
}

/// A trace replayed over its instance, one [`Read`] per consumed entry in trace
/// order, with the leave-path mask counts of the `RoleMatched` mirror:
/// `leave_queries` is one per rostered role per leave read, `leave_identical`
/// those of them with `cfg0 == cfg1` over the whole mask.
#[derive(Default, PartialEq, Eq, Debug)]
struct Replay {
    reads: Vec<Read>,
    leave_queries: u64,
    leave_identical: u64,
}

/// `(cfg0, cfg1)` of the role-`role` internal for a read about `agent`, as
/// `GroupAifPolicy` builds them under `CoverageMasks::RoleMatched`: the union
/// is over the role-`role` members other than a leaving `agent`, and `agent`'s
/// own capabilities enter iff its role is `role`. `members` includes `agent` on
/// a leave read and excludes it on a join read.
fn role_masks(
    inst: &WorkflowInstance,
    role: usize,
    agent: usize,
    members: &[usize],
    leave: bool,
) -> (u32, u32) {
    let has_role = |i: usize| {
        inst.roles
            .get(i)
            .is_some_and(|r| usize::from(r.index()) == role)
    };
    let caps = |i: usize| inst.agents.get(i).map_or(0, |a| a.capabilities());
    let union = members
        .iter()
        .filter(|&&m| has_role(m) && !(leave && m == agent))
        .fold(0u32, |acc, &m| acc | caps(m));
    let own = if has_role(agent) { caps(agent) } else { 0 };
    if leave {
        (union | own, union)
    } else {
        (own, own | union)
    }
}

/// `required_r` per role index below `n_roles`, over the task's distinct steps
/// with a bit below `n_bits`.
fn required_by_role(task: &WorkflowTask, n_roles: usize, n_bits: usize) -> Vec<u32> {
    let mut required = vec![0u32; n_roles];
    for step in task.demand.distinct() {
        if usize::from(step.bit) >= n_bits {
            continue;
        }
        if let (Some(slot), Some(mask)) = (
            required.get_mut(usize::from(step.role.index())),
            step.capability_mask(),
        ) {
            *slot |= mask;
        }
    }
    required
}

/// Replay `trace` over `inst` in `run_workflow_instance`'s call order, stopping
/// at the end of the trace. `windows[r]` holds, oldest first, the covered
/// required bits of role `r`'s last two on-roster tasks.
fn replay(inst: &WorkflowInstance, trace: &[TraceEntry], n_roles: usize, n_bits: usize) -> Replay {
    let mut out = Replay::default();
    let mut entries = trace.iter();
    let mut windows: Vec<Vec<u32>> = vec![Vec::new(); n_roles];
    'tasks: for (t, task) in inst.tasks.iter().enumerate() {
        let required = required_by_role(task, n_roles, n_bits);
        let roster: Vec<usize> = (0..n_roles).filter(|&r| required[r] != 0).collect();
        let mut read = |out: &mut Replay, agent: usize, members: &[usize], leave: bool| {
            let entry = entries.next()?;
            if leave {
                out.leave_queries += roster.len() as u64;
                out.leave_identical += roster
                    .iter()
                    .filter(|&&r| {
                        let (cfg0, cfg1) = role_masks(inst, r, agent, members, leave);
                        cfg0 == cfg1
                    })
                    .count() as u64;
            }
            let centre = inst
                .roles
                .get(agent)
                .map(|r| usize::from(r.index()))
                .filter(|r| roster.contains(r))
                .map(|r| {
                    let (cfg0, cfg1) = role_masks(inst, r, agent, members, leave);
                    CentreClass {
                        identical: cfg0 & required[r] == cfg1 & required[r],
                        window: windows[r].iter().any(|&seen| seen & required[r] != 0),
                    }
                });
            out.reads.push(Read {
                task: t,
                leave,
                act: entry.act,
                centre,
            });
            Some(entry.act)
        };

        let mut members: Vec<usize> = Vec::with_capacity(inst.agents.len());
        let mut arrivals = task.arrival.iter().copied();
        if let Some(first) = arrivals.next() {
            members.push(first);
        }
        for candidate in arrivals {
            let Some(act) = read(&mut out, candidate, &members, false) else {
                break 'tasks;
            };
            if act {
                members.push(candidate);
            }
        }
        for &idx in &task.arrival {
            let Some(pos) = members.iter().position(|&m| m == idx) else {
                continue;
            };
            let Some(act) = read(&mut out, idx, &members, true) else {
                break 'tasks;
            };
            if act {
                members.remove(pos);
            }
        }

        let covered = task
            .demand
            .distinct()
            .filter(|&s| usize::from(s.bit) < task.tags.len() && step_covered(inst, &members, s))
            .fold(0u32, |acc, s| acc | s.capability_mask().unwrap_or(0));
        for &r in &roster {
            let window = &mut windows[r];
            window.push(required[r] & covered);
            if window.len() > 2 {
                window.remove(0);
            }
        }
    }
    out
}

/// One policy over one instance: the metrics, the decision trace, the
/// reconstruction of the trace and its replay.
struct SeedRun {
    result: WorkflowResult,
    trace: Vec<TraceEntry>,
    recon: Result<Recon, ReconError>,
    replay: Replay,
}

impl SeedRun {
    fn recon(&self) -> Option<&Recon> {
        self.recon.as_ref().ok()
    }
}

/// One group policy's ledger after its instance.
struct Ledger {
    counters: GroupAifCounters,
    moved: NonVacuity,
}

/// One arm over the block: per-seed runs in seed order, per-seed ledgers (empty
/// for a reference cell), every decision latency in call order.
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

/// The block and whether it is a smoke block, from `vars` — the environment
/// variables whose name starts [`ENV_PREFIX`], as `(name, value)` — and the
/// package `version`. `K7_2_SEEDS` alone selects a block of [`SMOKE_BLOCKS`];
/// `K7_2_OFFICIAL=1` alone at `version == "0.40.0"` selects the registered
/// block.
///
/// # Errors
///
/// The refusal message: a name other than the two above, neither or both of
/// them, a `K7_2_SEEDS` value outside [`SMOKE_BLOCKS`], a `K7_2_OFFICIAL`
/// value other than `1`, or `K7_2_OFFICIAL=1` at another `version`.
fn seed_block(vars: &[(String, String)], version: &str) -> Result<(SeedRange, bool), String> {
    let foreign: Vec<String> = vars
        .iter()
        .filter(|(name, _)| name != SEEDS_ENV && name != OFFICIAL_ENV)
        .map(|(name, _)| format!("`{name}`"))
        .collect();
    if !foreign.is_empty() {
        return Err(format!(
            "refused: {} set; this binary runs with {SEEDS_ENV} or {OFFICIAL_ENV} and no other \
             environment variable whose name starts `{ENV_PREFIX}` (prereg A3.2)",
            foreign.join(", ")
        ));
    }
    let value = |wanted: &str| {
        vars.iter()
            .find(|(name, _)| name == wanted)
            .map(|(_, value)| value.as_str())
    };
    match (value(SEEDS_ENV), value(OFFICIAL_ENV)) {
        (None, None) => Err(format!(
            "refused: neither {SEEDS_ENV} nor {OFFICIAL_ENV} is set; the registered block \
             {REGISTERED_SEEDS} runs only with {OFFICIAL_ENV}=1 (prereg A3.2)"
        )),
        (Some(_), Some(_)) => Err(format!(
            "refused: {SEEDS_ENV} and {OFFICIAL_ENV} are both set (prereg A3.2)"
        )),
        (Some(text), None) => SMOKE_BLOCKS
            .iter()
            .find(|(accepted, _)| *accepted == text)
            .map(|&(_, range)| (range, true))
            .ok_or_else(|| {
                let accepted: Vec<String> = SMOKE_BLOCKS
                    .iter()
                    .map(|(text, _)| format!("`{text}`"))
                    .collect();
                format!(
                    "{SEEDS_ENV}={text} refused: the accepted values are {} (prereg §8)",
                    accepted.join(" and ")
                )
            }),
        (None, Some("1")) if version == OFFICIAL_VERSION => Ok((REGISTERED_SEEDS, false)),
        (None, Some("1")) => Err(format!(
            "{OFFICIAL_ENV}=1 refused: the registered block runs on koalisi v{OFFICIAL_VERSION} \
             and this build is v{version} (prereg §9, A3.2)"
        )),
        (None, Some(other)) => Err(format!(
            "{OFFICIAL_ENV}={other} refused: the accepted value is `1` (prereg A3.2)"
        )),
    }
}

/// Run `policy` traced over `inst`, reconstruct its trace and replay it under
/// the group cells' role count and universe width.
fn run_traced(
    policy: &dyn CoalitionDecisionPolicy,
    inst: &WorkflowInstance,
    latencies: &mut Vec<f64>,
) -> Result<SeedRun, Box<dyn Error>> {
    let traced = TracedPolicy::new(policy);
    let result = run_workflow_instance(&traced, inst, SIGNAL, latencies)?;
    let trace = traced.entries();
    let recon = reconstruct(inst, &trace);
    let config = cell_config(GRP_TOPO);
    let replayed = replay(inst, &trace, config.n_roles, config.base.n_bits);
    Ok(SeedRun {
        result,
        trace,
        recon,
        replay: replayed,
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

/// Reference cell `which` of [`REFERENCE_LABELS`]: one fresh policy per
/// instance — [`RefPrune`] over the instance's role map, [`RefFirst`] or
/// [`RefKeep`].
fn run_reference(which: usize, instances: &[WorkflowInstance]) -> Result<Cell, Box<dyn Error>> {
    let mut cell = Cell {
        label: REFERENCE_LABELS[which],
        role: "reference",
        runs: Vec::with_capacity(instances.len()),
        ledgers: Vec::new(),
        latencies: Vec::new(),
    };
    for inst in instances {
        let policy: Box<dyn CoalitionDecisionPolicy> = match which {
            REF_PRUNE => Box::new(RefPrune::new(role_map(inst)?)),
            REF_FIRST => Box::new(RefFirst),
            _ => Box::new(RefKeep),
        };
        let run = run_traced(policy.as_ref(), inst, &mut cell.latencies)?;
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
            match &run.recon {
                Ok(recon) => {
                    churn_equal += usize::from(recon.churn == run.result.churn);
                    if recon.primary.to_bits() == run.result.primary.to_bits() {
                        ok += 1;
                    } else {
                        failures.push(format!(
                            "`{}` {} (recomputed {:?}, harness {:?})",
                            cell.label, run.result.seed, recon.primary, run.result.primary
                        ));
                    }
                }
                Err(e) => failures.push(format!(
                    "`{}` {} (the trace does not replay: {e})",
                    cell.label, run.result.seed
                )),
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
    /// The task index of the read at `position` in the novelty-on cell's
    /// replay; `None` when the replay ended before `position`.
    task: Option<usize>,
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
                        task: x.replay.reads.get(position).map(|r| r.task),
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
    /// Seeds per task index of the first act difference.
    by_task: BTreeMap<usize, usize>,
}

impl FirstPool {
    /// The leave reads on which the novelty-on cell acted.
    fn leave_on_acted(&self) -> usize {
        self.counts[1][1]
    }

    /// Whether [`FirstPool::leave_on_acted`] is strictly the largest of the
    /// four buckets.
    fn leave_on_acted_is_modal(&self) -> bool {
        let k = self.leave_on_acted();
        [self.counts[0][0], self.counts[0][1], self.counts[1][0]]
            .iter()
            .all(|&other| k > other)
    }
}

fn first_pool(per_seed: &[SeedDivergence]) -> FirstPool {
    let mut pool = FirstPool {
        counts: [[0; 2]; 2],
        none: 0,
        by_task: BTreeMap::new(),
    };
    for d in per_seed {
        match d.first {
            Some(f) => {
                pool.counts[usize::from(f.leave)][usize::from(f.on_acted)] += 1;
                if let Some(task) = f.task {
                    *pool.by_task.entry(task).or_insert(0) += 1;
                }
            }
            None => pool.none += 1,
        }
    }
    pool
}

/// `"task <t>: n"` for every task index holding a first act difference.
fn first_tasks_text(pool: &FirstPool) -> String {
    if pool.by_task.is_empty() {
        return "none".to_owned();
    }
    let parts: Vec<String> = pool
        .by_task
        .iter()
        .map(|(task, seeds)| format!("task {task}: {seeds}"))
        .collect();
    parts.join(" · ")
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
    /// `(covered-step count, final size)` are equal.
    differing_same_shape: usize,
}

/// A seed without a reconstruction on either side adds nothing.
fn against(cell: &Cell, base: &Cell) -> Against {
    let mut out = Against {
        same_tasks: 0,
        tasks: 0,
        same_seeds: 0,
        seeds: cell.runs.len().max(base.runs.len()),
        differing_same_shape: 0,
    };
    for (run, other) in cell.runs.iter().zip(&base.runs) {
        let (Some(recon), Some(other_recon)) = (run.recon(), other.recon()) else {
            continue;
        };
        let (same, total) = member_set_identity(recon, other_recon);
        out.same_tasks += same;
        out.tasks += total;
        out.differing_same_shape += recon
            .tasks
            .iter()
            .zip(&other_recon.tasks)
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

    /// Whether `other`'s median PRIMARY is not above `0`, which leaves the
    /// ratio undefined.
    fn ratio_undefined(&self) -> bool {
        self.other_median <= 0.0 || self.other_median.is_nan()
    }
}

struct Criterion {
    live: Live,
    /// Indexed by [`GRP_TOPO`] and [`GRP_TOPO_NONOV`].
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
    writeln!(out, "- prereg: `{PREREG}` (Amendments 1–4 included)")?;
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

/// `all` is every cell in table order: the four group cells, then the three
/// reference cells.
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
            "| `{}` vs `{}`, seed | position of the first act difference | task index | read kind | cell that acted |",
            off.label, on.label
        )?;
        writeln!(out, "|---:|---:|---:|---|---|")?;
        for d in &per_seed {
            match d.first {
                Some(f) => writeln!(
                    out,
                    "| {} | {} | {} | {} | `{}` |",
                    d.seed,
                    f.position,
                    f.task.map_or_else(|| "—".to_owned(), |t| t.to_string()),
                    if f.leave { "leave" } else { "join" },
                    if f.on_acted { on.label } else { off.label }
                )?,
                None => writeln!(out, "| {} | — | — | — | — |", d.seed)?,
            }
        }
        writeln!(out)?;
        let pool = first_pool(&per_seed);
        writeln!(
            out,
            "- pooled, `{}` vs `{}`: {}",
            off.label,
            on.label,
            first_pool_text(&pool, on.label, off.label)
        )?;
        writeln!(
            out,
            "- pooled, `{}` vs `{}`, seeds by the task index of the first act difference: {}",
            off.label,
            on.label,
            first_tasks_text(&pool)
        )?;
        writeln!(out)?;
    }
    Ok(())
}

fn render_against(out: &mut String, cells: &[Cell], references: &[Cell]) -> fmt::Result {
    for reference in references {
        let name = reference.label;
        writeln!(out, "## Against `{name}` (disclosure)")?;
        writeln!(out)?;
        writeln!(
            out,
            "| cell | tasks with final member set identical to `{name}`'s | seeds with PRIMARY bit-identical to `{name}`'s | differing tasks whose (covered-step count, final size) equal `{name}`'s |"
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
    }

    writeln!(
        out,
        "### Amendment A3.3's pre-committed readings (non-gating)"
    )?;
    writeln!(out)?;
    let nonov = &cells[GRP_TOPO_NONOV];
    for &(which, reading) in &REFERENCE_READINGS {
        let reference = &references[which];
        let a = against(nonov, reference);
        let bar = at_least(a.tasks, DEAD_TASKS);
        let applies = a.same_tasks >= bar;
        writeln!(
            out,
            "- `{}` matches `{}` on {} of {} tasks ({} {bar}, 95 % of tasks rounded up): {}",
            nonov.label,
            reference.label,
            a.same_tasks,
            a.tasks,
            if applies { "≥" } else { "<" },
            if applies {
                format!("*\"{reading}\"*")
            } else {
                "the reading does not apply.".to_owned()
            }
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

/// `(leave, roster, centre vote, blind act votes)` of one read.
type ReadKey = (bool, usize, Option<usize>, usize);

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
        let rows = roster_decomposition(
            cell.runs.iter().filter_map(SeedRun::recon),
            usize::from(roles),
        );
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
    writeln!(out)?;

    writeln!(
        out,
        "### (read kind, roster, centre vote, blind act votes) → group acts of n"
    )?;
    writeln!(out)?;
    // Key → per-cell `(group acts, reads)`.
    let mut table: BTreeMap<ReadKey, Vec<(usize, usize)>> = BTreeMap::new();
    for (i, cell) in cells.iter().enumerate() {
        for s in cell.samples() {
            let row = table
                .entry((s.leave, s.roster, s.centre_vote, s.votes_for_act_blind))
                .or_insert_with(|| vec![(0, 0); cells.len()]);
            row[i].0 += usize::from(s.group_act);
            row[i].1 += 1;
        }
    }
    let labels: Vec<String> = cells.iter().map(|c| format!("`{}`", c.label)).collect();
    writeln!(
        out,
        "| read | roster | centre vote | blind act votes | {} |",
        labels.join(" | ")
    )?;
    writeln!(out, "|---|---:|---|---:|{}", "---:|".repeat(labels.len()))?;
    for ((leave, roster, centre, blind), row) in &table {
        let centre = match centre {
            None => "absent",
            Some(1) => "act",
            Some(_) => "decline",
        };
        let counts: Vec<String> = row
            .iter()
            .map(|&(acts, n)| {
                if n == 0 {
                    "—".to_owned()
                } else {
                    format!("{acts} of {n}")
                }
            })
            .collect();
        writeln!(
            out,
            "| {} | {roster} | {centre} | {blind} | {} |",
            if *leave { "leave" } else { "join" },
            counts.join(" | ")
        )?;
    }
    writeln!(out)
}

/// `(acts, reads)` of one cell's replayed reads, indexed
/// `[leave][identical][window]` over the centre-present reads.
type CentreCounts = [[[(usize, usize); 2]; 2]; 2];

fn centre_counts(cell: &Cell) -> CentreCounts {
    let mut counts: CentreCounts = [[[(0, 0); 2]; 2]; 2];
    for read in cell.runs.iter().flat_map(|r| &r.replay.reads) {
        if let Some(class) = read.centre {
            let slot = &mut counts[usize::from(read.leave)][usize::from(class.identical)]
                [usize::from(class.window)];
            slot.0 += usize::from(read.act);
            slot.1 += 1;
        }
    }
    counts
}

/// One seed on which a cell's replay and its ledger disagree on the leave-path
/// counts, each as `(leave queries, those with cfg0 == cfg1)`.
#[derive(Debug, PartialEq, Eq)]
struct Mismatch {
    seed: u64,
    mirror: (u64, u64),
    ledger: (u64, u64),
}

/// The integrity check of one seed: `None` when the replay's leave-path counts
/// equal the ledger's.
fn mismatch(seed: u64, replay: &Replay, counters: &GroupAifCounters) -> Option<Mismatch> {
    let mirror = (replay.leave_queries, replay.leave_identical);
    let ledger = (counters.leave_queries, counters.leave_queries_identical);
    (mirror != ledger).then_some(Mismatch {
        seed,
        mirror,
        ledger,
    })
}

/// Every seed of a group cell failing [`mismatch`].
fn mismatches(cell: &Cell) -> Vec<Mismatch> {
    cell.runs
        .iter()
        .zip(&cell.ledgers)
        .filter_map(|(run, ledger)| mismatch(run.result.seed, &run.replay, &ledger.counters))
        .collect()
}

/// `rows` is `(cell label, seeds checked, counts, mismatching seeds)` per group
/// cell. A cell with a mismatching seed gets no table row.
fn render_centre(
    out: &mut String,
    rows: &[(&str, usize, CentreCounts, Vec<Mismatch>)],
) -> fmt::Result {
    writeln!(
        out,
        "## Centre-present reads by what the centre's query sees (disclosure)"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "Computed in this binary from the instance and the replayed trace. *Masks identical*: `cfg0 & required_r == cfg1 & required_r` for the candidate's role `r`, the masks as `coverage_masks` builds them under `RoleMatched`. *Window*: either of role `r`'s last two on-roster tasks ended with `RoleCoverage` `true` on a bit `r` requires now — a world-side stand-in for the engine's two-task replay window, not a read of it. Cells are group acts of reads."
    )?;
    writeln!(out)?;
    let checked: Vec<String> = rows
        .iter()
        .map(|(label, seeds, _, bad)| format!("`{label}` {}/{seeds}", seeds - bad.len()))
        .collect();
    writeln!(
        out,
        "Integrity check — per seed, the mirror's leave-query count and its unrestricted `cfg0 == cfg1` count equal `GroupAifCounters::leave_queries` and `leave_queries_identical`; seeds equal per cell: {}.",
        checked.join(" · ")
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "| cell | read | masks identical, window true | masks identical, window false | masks differ, window true | masks differ, window false |"
    )?;
    writeln!(out, "|---|---|---:|---:|---:|---:|")?;
    for (label, _, counts, bad) in rows {
        if !bad.is_empty() {
            continue;
        }
        for (leave, kind) in [(false, "join"), (true, "leave")] {
            let cell = |identical: bool, window: bool| {
                let (acts, reads) =
                    counts[usize::from(leave)][usize::from(identical)][usize::from(window)];
                share(acts, reads)
            };
            writeln!(
                out,
                "| `{label}` | {kind} | {} | {} | {} | {} |",
                cell(true, true),
                cell(true, false),
                cell(false, true),
                cell(false, false)
            )?;
        }
    }
    writeln!(out)?;
    for (label, _, _, bad) in rows {
        for m in bad {
            writeln!(
                out,
                "- `{label}` withheld: seed {} — mirror {} leave queries, {} with `cfg0 == cfg1`; ledger {} and {}.",
                m.seed, m.mirror.0, m.mirror.1, m.ledger.0, m.ledger.1
            )?;
        }
    }
    writeln!(out)
}

/// `all` is every cell in table order.
fn render_task_split(out: &mut String, all: &[&Cell]) -> fmt::Result {
    writeln!(
        out,
        "## Task 0 against tasks ≥ 1 (disclosure; no task has been observed before task 0)"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "| cell | tasks | join-act rate | leave-act rate | success rate | mean final size |"
    )?;
    writeln!(out, "|---|---|---:|---:|---:|---:|")?;
    for cell in all {
        for (first, name) in [(true, "task 0"), (false, "tasks ≥ 1")] {
            let reads = || {
                cell.runs
                    .iter()
                    .flat_map(|r| &r.replay.reads)
                    .filter(|r| (r.task == 0) == first)
            };
            let rate = |leave: bool| {
                share(
                    reads().filter(|r| r.leave == leave && r.act).count(),
                    reads().filter(|r| r.leave == leave).count(),
                )
            };
            let ends = || {
                cell.runs
                    .iter()
                    .filter_map(SeedRun::recon)
                    .flat_map(|r| r.tasks.iter().enumerate())
                    .filter(|(t, _)| (*t == 0) == first)
                    .map(|(_, end)| end)
            };
            writeln!(
                out,
                "| `{}` | {name} | {} | {} | {} | {} |",
                cell.label,
                rate(false),
                rate(true),
                pct(ends().filter(|e| e.success()).count(), ends().count()),
                or_na(mean(ends().map(|e| e.members.len() as f64)), 2)
            )?;
        }
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
        "1. **The premise.** *\"the contrast is the centre's query with and without the term only where the centre decides\"*. E-follow over all centre-present reads: {}.",
        premise.join(" · ")
    )?;
    for sentence in &departed {
        write!(out, " {sentence}")?;
    }
    writeln!(
        out,
        " E-follow does not see a centre whose soft read coincides with the blind vote; the votes table (A3.4) is where that shows."
    )?;

    let median = |i: usize| cells[i].median_primary();
    writeln!(
        out,
        "2. **Against EQ5b's pair.** *\"the ratio of medians novelty-off / novelty-on is {} on the unrouted pair and {} on the routed pair on this block.\"* (`grp-role-nonov` {:.4} / `grp-role` {:.4}; `grp-topo-nonov` {:.4} / `grp-topo` {:.4}.)",
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
    writeln!(out, "5. **§4's note.**")?;
    writeln!(
        out,
        "   - **5a.** S-nov's redundant-member leave read: novelty-off declines where novelty-on acts on 0 of 1 fresh and 1 of 1 warmed; exact ties 0 of 1."
    )?;
    writeln!(
        out,
        "   - **5b.** *\"{} of {diverged} seeds with an act difference have as first divergence a leave read on which `grp-topo` acts and `grp-topo-nonov` declines\"*. {}",
        routed.leave_on_acted(),
        if routed.leave_on_acted_is_modal() {
            "That bucket is the modal one of the four."
        } else {
            "That bucket is not strictly the largest of the four."
        }
    )?;
    let nonov = &c.dead[GRP_TOPO_NONOV];
    writeln!(
        out,
        "   - **5c.** §4's consequence, *the lock's dead at the outcome fails*, is **{}**: `{}` {} its H-dead conjuncts (final member sets {}, PRIMARY bits {}).",
        if nonov.pass() {
            "contradicted"
        } else {
            "confirmed"
        },
        nonov.label,
        if nonov.pass() {
            "passes both of"
        } else {
            "fails one or both of"
        },
        word(nonov.tasks_ok()),
        word(nonov.seeds_ok())
    )?;
    writeln!(out)
}

/// §6's reading of `label` as Amendment A3.6 amends it, with H-live's score
/// conjunct as given.
fn render_one_reading(out: &mut String, label: &str, c: &Criterion, score_ok: bool) -> fmt::Result {
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
        LABEL_NOT_LIVE if score_ok => "*\"live in the score, dead at the decision\"*".to_owned(),
        LABEL_NOT_LIVE => "*\"inert on the routed arm\"*".to_owned(),
        LABEL_VALIDATED => "*\"on v2w under `RoleCoverage` the v5 novelty term changes what the routed group computes and which acts it takes, and not where it ends: with it and without it the arm ends where a learning-free arrival-order redundancy prune ends.\"*".to_owned(),
        LABEL_MOVES => {
            // The label is reached only with a passing leg; a smoke block
            // renders it for every criterion outcome.
            let leg = c.move_pass().unwrap_or(&c.moves[0]);
            let mut text = format!(
                "*\"novelty {} is the better routed cell at the lineage bar ({}, {}/{})\"*. H-dead conjuncts failed: {}.",
                leg.novelty,
                ratio_text(leg.better_median, leg.other_median),
                leg.superior,
                leg.seeds,
                dead_failures()
            );
            if c.dead[GRP_TOPO].pass() {
                text.push_str(" *\"the better cell ends where the engine-free prune ends; the contrast measures the query without the term, not a gain over a rule with no engine.\"*");
            }
            text
        }
        _ => {
            let mut text = format!(
                "H-live passes, H-dead and H-move fail. H-dead conjuncts failed: {}. The *against `ref-prune`* and *inside each pair* tables above carry the disclosures this verdict is reported with.",
                dead_failures()
            );
            for leg in c.moves.iter().filter(|leg| leg.ratio_undefined()) {
                write!(
                    text,
                    " *\"`{}`'s median PRIMARY is 0; the ratio is undefined; `{}` is strictly superior on {}/{} seeds.\"*",
                    leg.other, leg.better, leg.superior, leg.seeds
                )?;
            }
            text
        }
    };
    writeln!(out, "`{label}` — {reading}")?;
    writeln!(out)
}

/// The registered block renders the reading of `label`. A smoke block renders
/// the reading of every label, and of both ways H-live fails, whatever the
/// criterion found. The continuation (Amendment A3.7) follows unless `label`
/// is `RUN-INVALID`.
fn render_reading(out: &mut String, label: &str, c: &Criterion, smoke: bool) -> fmt::Result {
    writeln!(out, "## Reading")?;
    writeln!(out)?;
    if smoke {
        writeln!(
            out,
            "Smoke block: every label's reading is rendered and none is a verdict."
        )?;
        writeln!(out)?;
        render_one_reading(out, LABEL_INVALID, c, c.live.score_ok())?;
        for score_ok in [true, false] {
            render_one_reading(out, LABEL_NOT_LIVE, c, score_ok)?;
        }
        for label in [LABEL_VALIDATED, LABEL_MOVES, LABEL_NOT_DEAD] {
            render_one_reading(out, label, c, c.live.score_ok())?;
        }
    } else {
        render_one_reading(out, label, c, c.live.score_ok())?;
    }
    if label == LABEL_INVALID {
        return Ok(());
    }
    let routed = &c.dead[GRP_TOPO];
    let text = if routed.pass() {
        "the next registration moves the world (`OutcomeSignal::Performance` / `Both` with the harness's performance draw), on its own issue, next free K7 number, fresh seed block."
    } else {
        "K7-3 is locked next, and its lock reads this report first."
    };
    writeln!(
        out,
        "Continuation (Amendment A3.7) — `{}` {} its H-dead conjuncts: {text}",
        routed.label,
        if routed.pass() {
            "passes both of"
        } else {
            "fails one or both of"
        }
    )?;
    writeln!(out)
}

/// The smoke block's line in place of the report.
const SMOKE_BODY: &str = "smoke: the report is rendered and not printed\n\n";

/// What one execution writes to stdout after the header: `gate_lines`, then
/// `report` on the registered block or [`SMOKE_BODY`] under `K7_2_SEEDS`, then
/// the `VERDICT:` line.
struct Rendered {
    gate_lines: String,
    report: String,
    verdict: &'static str,
}

impl Rendered {
    fn stdout(&self, smoke: bool) -> String {
        let body = if smoke { SMOKE_BODY } else { &self.report };
        format!("{}{body}VERDICT: {}\n", self.gate_lines, self.verdict)
    }
}

/// Run every cell and gate over `instances`, then render. When a trace does
/// not replay, X-recon fails and the report holds the gate detail and the arms
/// table alone.
fn run(
    spec: &WorkflowSpec,
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
    let references = (0..REFERENCE_LABELS.len())
        .map(|which| run_reference(which, instances))
        .collect::<Result<Vec<_>, _>>()?;
    let all: Vec<&Cell> = cells.iter().chain(&references).collect();

    let gates = [
        gate_recon(&all),
        gate_determinism(&cells, &reruns),
        gate_invariance(&cells, &reseeded),
        gate_learn(&cells),
        gate_route(&cells),
    ];
    let gates_ok = gates.iter().all(Gate::pass);
    let mut gate_lines = String::new();
    render_gate_lines(&mut gate_lines, &gates)?;

    let mut report = String::new();
    render_gate_detail(&mut report, &gates)?;
    render_arms(&mut report, &cells, &all)?;

    let replayed = all
        .iter()
        .all(|cell| cell.runs.iter().all(|run| run.recon.is_ok()));
    if !replayed {
        writeln!(
            report,
            "A trace did not replay (X-recon above); the criterion, the disclosures, the scoped clauses and the reading are not rendered."
        )?;
        writeln!(report)?;
        return Ok(Rendered {
            gate_lines,
            report,
            verdict: LABEL_INVALID,
        });
    }

    let found = criterion(&cells, &references[REF_PRUNE]);
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

    render_criterion(&mut report, &found)?;
    render_live(&mut report, &cells)?;
    render_first(&mut report, &cells)?;
    render_against(&mut report, &cells, &references)?;
    render_follow(&mut report, &cells)?;
    render_decomposition(&mut report, &cells, &all, spec.roles)?;
    render_task_split(&mut report, &all)?;
    let centre: Vec<(&str, usize, CentreCounts, Vec<Mismatch>)> = cells
        .iter()
        .map(|cell| {
            (
                cell.label,
                cell.runs.len(),
                centre_counts(cell),
                mismatches(cell),
            )
        })
        .collect();
    render_centre(&mut report, &centre)?;
    render_reach(&mut report, &cells)?;
    render_clauses(&mut report, &cells, &found)?;
    render_reading(&mut report, label, &found, smoke)?;

    Ok(Rendered {
        gate_lines,
        report,
        verdict,
    })
}

fn main() -> ExitCode {
    let vars: Vec<(String, String)> = std::env::vars_os()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .filter(|(name, _)| name.starts_with(ENV_PREFIX))
        .collect();
    let (seeds, smoke) = match seed_block(&vars, env!("CARGO_PKG_VERSION")) {
        Ok(block) => block,
        Err(refusal) => {
            eprintln!("k7_2: {refusal}");
            return ExitCode::from(2);
        }
    };
    let spec = WorkflowSpec::default();
    let mut header = String::new();
    if render_header(&mut header, &spec, seeds, smoke).is_err() {
        eprintln!("k7_2: the header failed to render before any cell ran");
        return ExitCode::FAILURE;
    }
    print!("{header}");
    if std::io::stdout().flush().is_err() {
        eprintln!("k7_2: stdout failed to flush before any cell ran");
        return ExitCode::FAILURE;
    }
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
    match run(&spec, smoke, &instances) {
        Ok(rendered) => {
            print!("{}", rendered.stdout(smoke));
            ExitCode::SUCCESS
        }
        Err(e) => {
            if smoke {
                eprintln!(
                    "k7_2: a cell failed to run; the error text is not printed under {SEEDS_ENV}"
                );
            } else {
                eprintln!("k7_2: a cell failed to run: {e}");
            }
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use catgraph_applied::prop::colored::ColoredExpr;
    use catgraph_applied::prop::{Free, PropExpr};
    use koalisi::algorithms::CapabilityAgent;
    use koalisi::process::{StaffingTable, Step, WorkflowGen, chain, demand, step_expr};

    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|&(name, value)| (name.to_owned(), value.to_owned()))
            .collect()
    }

    #[test]
    fn seed_block_accepts_the_two_smoke_blocks_and_the_official_opt_in_at_the_release_version() {
        for (text, range) in SMOKE_BLOCKS {
            for version in ["0.39.0", OFFICIAL_VERSION] {
                assert_eq!(
                    seed_block(&vars(&[(SEEDS_ENV, text)]), version),
                    Ok((range, true)),
                    "{text} at {version}"
                );
            }
        }
        assert_eq!(
            seed_block(&vars(&[(OFFICIAL_ENV, "1")]), OFFICIAL_VERSION),
            Ok((REGISTERED_SEEDS, false))
        );
    }

    #[test]
    fn seed_block_refuses_every_other_environment() {
        let refused: [&[(&str, &str)]; 7] = [
            &[],
            &[(OFFICIAL_ENV, "1"), (SEEDS_ENV, "6000..6003")],
            &[(SEEDS_ENV, "150..180")],
            &[(SEEDS_ENV, "6000..6004")],
            &[(SEEDS_ENV, "")],
            &[(OFFICIAL_ENV, "0")],
            &[(OFFICIAL_ENV, "true")],
        ];
        for pairs in refused {
            let outcome = seed_block(&vars(pairs), OFFICIAL_VERSION);
            assert!(
                outcome.is_err(),
                "{pairs:?} at {OFFICIAL_VERSION}: {outcome:?}"
            );
        }
        for version in ["0.39.0", "0.40.1", ""] {
            let outcome = seed_block(&vars(&[(OFFICIAL_ENV, "1")]), version);
            assert!(outcome.is_err(), "official at {version:?}: {outcome:?}");
        }
        // A foreign `K7_` name is refused by name, beside an accepted variable
        // and alone.
        let foreign: [(&[(&str, &str)], &str); 4] = [
            (&[("K7_2_SEED", "6000..6003")], "`K7_2_SEED`"),
            (
                &[("K7_1_SEEDS", "5000..5003"), (SEEDS_ENV, "6000..6003")],
                "`K7_1_SEEDS`",
            ),
            (
                &[("K7_WRITE_FIXTURE", "1"), (OFFICIAL_ENV, "1")],
                "`K7_WRITE_FIXTURE`",
            ),
            (&[("K7_", "")], "`K7_`"),
        ];
        for (pairs, name) in foreign {
            let outcome = seed_block(&vars(pairs), OFFICIAL_VERSION);
            assert!(
                outcome.as_ref().is_err_and(|text| text.contains(name)),
                "{pairs:?}: {outcome:?}"
            );
        }
    }

    /// One role's steps on `bits` as a sequential chain.
    fn leg(role: Role, bits: &[u8]) -> PropExpr<WorkflowGen> {
        chain(
            bits.iter()
                .map(|&b| step_expr(Step::new(b, role)))
                .collect(),
        )
        .unwrap()
    }

    /// A task demanding the steps of `legs = [(role index, bits)]` over a
    /// three-bit universe, arriving in `arrival` order.
    fn task(legs: &[(u8, &[u8])], arrival: &[usize]) -> WorkflowTask {
        let source: Vec<Role> = legs.iter().map(|&(r, _)| Role::new(r)).collect();
        let expr = legs
            .iter()
            .map(|&(r, bits)| leg(Role::new(r), bits))
            .reduce(Free::tensor)
            .unwrap();
        let written = ColoredExpr::new(source, expr).unwrap();
        let demand = demand(&written);
        let required = legs
            .iter()
            .flat_map(|&(_, bits)| bits)
            .fold(0u32, |acc, &b| acc | (1u32 << b));
        let mut tags = vec![Role::new(0); 3];
        for &(r, bits) in legs {
            for &b in bits {
                tags[usize::from(b)] = Role::new(r);
            }
        }
        WorkflowTask {
            required,
            tags,
            arrival: arrival.to_vec(),
            written,
            demand,
        }
    }

    /// Agent 0 holds bit 0 at role 0; agent 1 bits 0 and 1 at role 0; agent 2
    /// bit 2 at role 1; agent 3 bit 2 at role 2. Task 0 demands `(0, r0)`,
    /// `(1, r0)`, `(2, r1)` with arrival 0, 1, 2, 3; task 1 `(1, r0)`,
    /// `(2, r2)` with arrival 3, 2, 0, 1; task 2 `(1, r0)` and task 3
    /// `(0, r0)`, both with arrival 1, 0.
    fn four_task_instance() -> WorkflowInstance {
        let agents = vec![
            CapabilityAgent::new(0, 0b001, 50),
            CapabilityAgent::new(1, 0b011, 50),
            CapabilityAgent::new(2, 0b100, 50),
            CapabilityAgent::new(3, 0b100, 50),
        ];
        let roles = vec![Role::new(0), Role::new(0), Role::new(1), Role::new(2)];
        let table = StaffingTable::from_pool(
            agents
                .iter()
                .zip(&roles)
                .map(|(a, &r)| (a.capabilities(), r)),
        );
        let tasks = vec![
            task(&[(0, &[0, 1]), (1, &[2])], &[0, 1, 2, 3]),
            task(&[(0, &[1]), (2, &[2])], &[3, 2, 0, 1]),
            task(&[(0, &[1])], &[1, 0]),
            task(&[(0, &[0])], &[1, 0]),
        ];
        WorkflowInstance {
            seed: 7,
            agents,
            roles,
            prefix_required: tasks.iter().map(|t| t.required).collect(),
            tasks,
            table,
            redraws: 0,
            max_attempts: 1,
            performance: None,
        }
    }

    fn entry(leave: bool, act: bool) -> TraceEntry {
        TraceEntry {
            leave,
            act,
            score_bits: 0,
        }
    }

    /// Task 0: agents 1, 2, 3 join, then agents 0 and 2 leave. Task 1: agent 2
    /// does not join, agents 0 and 1 do, then agent 0 leaves. Task 2: agent 0
    /// joins and leaves. Task 3: agent 0 joins, nobody leaves.
    fn four_task_trace() -> Vec<TraceEntry> {
        let (join, leave) = (false, true);
        vec![
            entry(join, true),
            entry(join, true),
            entry(join, true),
            entry(leave, true),
            entry(leave, false),
            entry(leave, true),
            entry(leave, false),
            entry(join, false),
            entry(join, true),
            entry(join, true),
            entry(leave, false),
            entry(leave, true),
            entry(leave, false),
            entry(join, true),
            entry(leave, false),
            entry(leave, true),
            entry(join, true),
            entry(leave, false),
            entry(leave, false),
        ]
    }

    #[test]
    fn replay_of_the_hand_instance_gives_the_hand_derived_classes_and_counts() {
        let class = |identical, window| Some(CentreClass { identical, window });
        let read = |task, leave, act, centre| Read {
            task,
            leave,
            act,
            centre,
        };
        let (join, leave) = (false, true);
        // Masks are (cfg0, cfg1) of the candidate's role; `req` is that
        // role's required bits on the task.
        let expected = vec![
            // Task 0, req r0 = 0b011, r1 = 0b100, r2 off the roster; no window.
            read(0, join, true, class(true, false)), // agent 1: (0b011, 0b011)
            read(0, join, true, class(true, false)), // agent 2: (0b100, 0b100)
            read(0, join, true, None),               // agent 3: r2
            read(0, leave, true, class(true, false)), // agent 0: (0b011, 0b011)
            read(0, leave, false, class(false, false)), // agent 1: (0b011, 0)
            read(0, leave, true, class(false, false)), // agent 2: (0b100, 0)
            read(0, leave, false, None),             // agent 3: r2
            // Task 0 ends on {1, 3}: r0's window holds 0b011, r1's holds 0.
            // Task 1, req r0 = 0b010, r2 = 0b100, r1 off the roster.
            read(1, join, false, None),                 // agent 2: r1
            read(1, join, true, class(true, true)),     // agent 0: (0b001, 0b001)
            read(1, join, true, class(true, true)),     // agent 1: (0b011, 0b011)
            read(1, leave, false, class(false, false)), // agent 3: (0b100, 0)
            read(1, leave, true, class(true, true)),    // agent 0: (0b011, 0b011)
            read(1, leave, false, class(false, true)),  // agent 1: (0b011, 0)
            // Task 1 ends on {1, 3}: r0's window holds 0b011, 0b010.
            // Task 2, req r0 = 0b010.
            read(2, join, true, class(false, true)), // agent 0: (0b001, 0b011)
            read(2, leave, false, class(false, true)), // agent 1: (0b011, 0b001)
            read(2, leave, true, class(true, true)), // agent 0: (0b011, 0b011)
            // Task 2 ends on {1}: r0's window holds 0b010, 0b010.
            // Task 3, req r0 = 0b001: masks identical on it, not over the
            // whole mask, on agent 0's join and agent 1's leave.
            read(3, join, true, class(true, false)), // agent 0: (0b001, 0b011)
            read(3, leave, false, class(true, false)), // agent 1: (0b011, 0b001)
            read(3, leave, false, class(true, false)), // agent 0: (0b011, 0b011)
        ];
        let replayed = replay(&four_task_instance(), &four_task_trace(), 3, 8);
        assert_eq!(replayed.reads, expected);
        // Leave queries: 4 reads × 2 roles, 3 × 2, 2 × 1, 2 × 1. Identical
        // over the whole mask: task 0 — 2, 1, 1, 2; task 1 — 1, 2, 1; task 2
        // — 0, 1; task 3 — 0, 1.
        assert_eq!(
            (replayed.leave_queries, replayed.leave_identical),
            (18, 12),
            "hand values 18 and 12"
        );
    }

    #[test]
    fn replay_stops_at_the_end_of_a_short_trace() {
        let trace = four_task_trace();
        let replayed = replay(&four_task_instance(), &trace[..5], 3, 8);
        assert_eq!(replayed.reads.len(), 5);
        assert_eq!((replayed.leave_queries, replayed.leave_identical), (4, 3));
    }

    #[test]
    fn mismatch_is_none_on_equal_counts_and_carries_both_pairs_otherwise() {
        let replayed = Replay {
            reads: Vec::new(),
            leave_queries: 18,
            leave_identical: 12,
        };
        let mut counters = GroupAifCounters {
            leave_queries: 18,
            leave_queries_identical: 12,
            ..GroupAifCounters::default()
        };
        assert_eq!(mismatch(7, &replayed, &counters), None);
        counters.leave_queries_identical = 13;
        assert_eq!(
            mismatch(7, &replayed, &counters),
            Some(Mismatch {
                seed: 7,
                mirror: (18, 12),
                ledger: (18, 13),
            })
        );
        counters.leave_queries_identical = 12;
        counters.leave_queries = 17;
        assert!(mismatch(7, &replayed, &counters).is_some());
    }

    #[test]
    fn render_centre_withholds_a_cell_with_a_mismatching_seed() {
        let mut counts: CentreCounts = [[[(0, 0); 2]; 2]; 2];
        counts[1][1][0] = (3, 4);
        let bad = vec![Mismatch {
            seed: 6001,
            mirror: (18, 13),
            ledger: (18, 12),
        }];
        let mut out = String::new();
        render_centre(
            &mut out,
            &[("kept", 3, counts, Vec::new()), ("dropped", 3, counts, bad)],
        )
        .unwrap();
        assert!(
            out.contains("| `kept` | leave | n/a (0 of 0) | 75.0 % (3 of 4) |"),
            "{out}"
        );
        assert!(!out.contains("| `dropped` |"), "{out}");
        assert!(
            out.contains(
                "- `dropped` withheld: seed 6001 — mirror 18 leave queries, 13 with `cfg0 == cfg1`; ledger 18 and 12."
            ),
            "{out}"
        );
        assert!(out.contains("`kept` 3/3 · `dropped` 2/3"), "{out}");
    }

    /// `grp-topo` and `grp-role` hosted over the first three seeds of the
    /// consumed block 90..120.
    #[test]
    fn replay_counts_equal_the_ledger_on_consumed_seeds() {
        let spec = WorkflowSpec::default();
        let instances: Vec<WorkflowInstance> =
            (90..93).map(|seed| spec.generate(seed).unwrap()).collect();
        for cell in [GRP_TOPO, GRP_ROLE] {
            let hosted = run_group(cell, cell_config(cell), &instances, 0).unwrap();
            let mut leave_queries = 0;
            for (run, ledger) in hosted.runs.iter().zip(&hosted.ledgers) {
                assert_eq!(
                    mismatch(run.result.seed, &run.replay, &ledger.counters),
                    None,
                    "`{}`",
                    hosted.label
                );
                assert_eq!(run.replay.reads.len(), run.trace.len());
                leave_queries += run.replay.leave_queries;
            }
            assert!(leave_queries > 0, "`{}`: no leave query", hosted.label);
            let present = hosted
                .runs
                .iter()
                .flat_map(|r| &r.replay.reads)
                .filter(|r| r.centre.is_some())
                .count();
            let ledger_present = hosted
                .samples()
                .iter()
                .filter(|s| s.centre_vote.is_some())
                .count();
            assert_eq!(
                present, ledger_present,
                "`{}`: centre-present reads, replay against the `AgreementSample` ledger",
                hosted.label
            );
        }
    }
}
