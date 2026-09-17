//! K7-1 — topology-routed group voting against the candidate-blind group, as
//! registered in `docs/k7/prereg-K7-1-topology-routed-group.md` (Amendments 1
//! and 2 included): six `GroupAifPolicy` cells, the `ref-prune` reference cell
//! and the `wf-asis` context arm over `WorkflowSpec::default()` with
//! `OutcomeSignal::RoleCoverage`, one fresh policy per seed, every gate computed
//! before any table is printed, and one `VERDICT:` line last.
//!
//! The block is seeds `90..120`. `K7_1_SEEDS=a..b` replaces it with a smoke
//! block disjoint from `0..480`; a smoke run prints a banner and
//! `VERDICT: SMOKE (no verdict)` unless a gate fails.
//!
//! Run: `cargo run --release --features harness,decision,process --example k7_1`.

use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::sync::Mutex;

use koalisi::algorithms::AgentCapabilities;
use koalisi::decision::{
    AgreementSample, CoalitionDecisionPolicy, CoverageMasks, Decision, DecisionContext,
    GroupAifConfig, GroupAifCounters, GroupAifPolicy, MagnitudePolicy, ModelLabel, NonVacuity,
    RoleModulation, S_LEARN_VACUITY_TOL, TaskStart, VoteRouting, models_moved,
};
use koalisi::harness::{
    OutcomeSignal, SeedRange, TraceEntry, TracedPolicy, WorkflowInstance, WorkflowResult,
    WorkflowSpec, median_iqr, percentile, run_workflow_instance, superior_count,
};
use koalisi::process::{Role, Step};

const PREREG: &str = "docs/k7/prereg-K7-1-topology-routed-group.md";
const REGISTERED_SEEDS: SeedRange = SeedRange {
    start: 90,
    end: 120,
};
/// Every block of the seed ledger and every block assigned to a later K7
/// registration; `K7_1_SEEDS` refuses a range overlapping it.
const LEDGER_SEEDS: SeedRange = SeedRange { start: 0, end: 480 };
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

/// One task's end state, reconstructed from the arrival order and the trace.
struct TaskEnd {
    /// Distinct roles in the task's demand.
    roster: usize,
    /// Distinct demanded `(bit, role)` steps.
    steps: usize,
    /// Of `steps`, those covered by a final member of the step's role.
    covered: usize,
    /// Final members, ascending agent id.
    members: Vec<usize>,
    /// Final members whose role has no step in the task's demand.
    off_demand: usize,
    /// Covered fraction divided by the final member count; `0` for an empty
    /// coalition or an empty demand.
    cov_eff: f64,
}

impl TaskEnd {
    fn success(&self) -> bool {
        self.steps > 0 && self.covered == self.steps
    }

    fn covered_fraction(&self) -> f64 {
        if self.steps == 0 {
            0.0
        } else {
            self.covered as f64 / self.steps as f64
        }
    }
}

/// One instance's reconstructed task ends, with PRIMARY and churn recomputed
/// from them.
struct Recon {
    tasks: Vec<TaskEnd>,
    primary: f64,
    churn: usize,
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

/// Position-wise divergence of two arms' traces on one seed, compared up to the
/// shorter trace.
struct SeedDivergence {
    seed: u64,
    act: usize,
    score_bits: usize,
    length: usize,
}

/// [`SeedDivergence`] summed over seeds.
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
    if range.start < LEDGER_SEEDS.end && LEDGER_SEEDS.start < range.end {
        return Err(format!(
            "{SEEDS_ENV}={text} overlaps {LEDGER_SEEDS}, the seed ledger's blocks and the \
             blocks assigned to later K7 registrations; the registered block \
             {REGISTERED_SEEDS} runs without {SEEDS_ENV}"
        )
        .into());
    }
    Ok((range, true))
}

/// The `ref-prune` reference cell: `should_join` always acts; `should_leave`
/// acts iff every `TaskStart` step covered by the coalition shown stays covered
/// without the agent, a step `(bit, role)` being covered by a member of `role`
/// (per `roles`) holding `bit`. Every score is `0.0`.
struct RefPrune {
    roles: HashMap<usize, Role>,
    steps: Mutex<Vec<(u8, u8)>>,
}

impl RefPrune {
    fn new(roles: HashMap<usize, Role>) -> Self {
        Self {
            roles,
            steps: Mutex::new(Vec::new()),
        }
    }

    /// Some member of `coalition` other than agent id `without`, of role index
    /// `role`, holds `bit`.
    fn covered(
        &self,
        coalition: &[&dyn AgentCapabilities],
        without: Option<usize>,
        (bit, role): (u8, u8),
    ) -> bool {
        let Some(mask) = 1u32.checked_shl(u32::from(bit)) else {
            return false;
        };
        coalition.iter().any(|m| {
            Some(m.agent_id()) != without
                && self
                    .roles
                    .get(&m.agent_id())
                    .is_some_and(|r| r.index() == role)
                && m.capabilities() & mask != 0
        })
    }
}

impl CoalitionDecisionPolicy for RefPrune {
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
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        _ctx: &DecisionContext,
    ) -> Decision {
        let steps = self
            .steps
            .lock()
            .expect("invariant: no panic holds the steps lock");
        let redundant = steps.iter().all(|&step| {
            !self.covered(coalition, None, step)
                || self.covered(coalition, Some(agent.agent_id()), step)
        });
        Decision {
            act: redundant,
            score: 0.0,
        }
    }

    fn begin_task(&self, task: &TaskStart<'_>) {
        *self
            .steps
            .lock()
            .expect("invariant: no panic holds the steps lock") = task.steps.to_vec();
    }
}

/// Some member of `step.role` holds `step.bit`.
fn step_covered(inst: &WorkflowInstance, members: &[usize], step: Step) -> bool {
    let Some(mask) = step.capability_mask() else {
        return false;
    };
    members.iter().any(|&i| {
        inst.roles.get(i).is_some_and(|&r| r == step.role)
            && inst.agents[i].capabilities() & mask != 0
    })
}

/// Replay `trace` over `inst` as `run_workflow_instance` produced it: per task
/// the first arrival joins, each later arrival consumes one join entry, then
/// each arrival that is a member consumes one leave entry, in arrival order.
///
/// # Errors
///
/// A trace that ends early, an entry of the wrong kind, or entries left over
/// after the last task.
fn reconstruct(inst: &WorkflowInstance, trace: &[TraceEntry]) -> Result<Recon, String> {
    let seed = inst.seed;
    let mut entries = trace.iter();
    let mut next = |task: usize, leave: bool| -> Result<bool, String> {
        let entry = entries
            .next()
            .ok_or_else(|| format!("seed {seed}, task {task}: the trace ends inside the task"))?;
        if entry.leave != leave {
            return Err(format!(
                "seed {seed}, task {task}: expected an entry with leave = {leave}, found leave = {}",
                entry.leave
            ));
        }
        Ok(entry.act)
    };

    let mut tasks = Vec::with_capacity(inst.tasks.len());
    let mut success_count = 0usize;
    let mut cov_eff_sum = 0.0f64;
    let mut churn = 0usize;
    for (t, task) in inst.tasks.iter().enumerate() {
        let mut members: Vec<usize> = Vec::with_capacity(inst.agents.len());
        let mut arrivals = task.arrival.iter().copied();
        if let Some(first) = arrivals.next() {
            members.push(first);
        }
        for candidate in arrivals {
            if next(t, false)? {
                members.push(candidate);
            }
        }
        for &idx in &task.arrival {
            let Some(pos) = members.iter().position(|&m| m == idx) else {
                continue;
            };
            if next(t, true)? {
                members.remove(pos);
                churn += 1;
            }
        }

        let steps = task.demand.distinct_len();
        let covered = task
            .demand
            .distinct()
            .filter(|&s| step_covered(inst, &members, s))
            .count();
        let cov_eff = if members.is_empty() || steps == 0 {
            0.0
        } else {
            (covered as f64 / steps as f64) / members.len() as f64
        };
        if steps > 0 && covered == steps {
            success_count += 1;
        }
        cov_eff_sum += cov_eff;

        let mut demanded: Vec<Role> = task.demand.distinct().map(|s| s.role).collect();
        demanded.sort_unstable_by_key(|r| r.index());
        demanded.dedup();
        let off_demand = members
            .iter()
            .filter(|&&i| inst.roles.get(i).is_none_or(|r| !demanded.contains(r)))
            .count();
        members.sort_unstable();
        tasks.push(TaskEnd {
            roster: demanded.len(),
            steps,
            covered,
            members,
            off_demand,
            cov_eff,
        });
    }
    let left = entries.count();
    if left != 0 {
        return Err(format!(
            "seed {seed}: {left} trace entries left after the last task"
        ));
    }

    let n_tasks = inst.tasks.len();
    let (success_rate, mean_cov_eff) = if n_tasks == 0 {
        (0.0, 0.0)
    } else {
        (
            success_count as f64 / n_tasks as f64,
            cov_eff_sum / n_tasks as f64,
        )
    };
    Ok(Recon {
        tasks,
        primary: success_rate * mean_cov_eff,
        churn,
    })
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
        parts.push(format!("`{}` {ok}/{}", cell.label, cell.runs.len()));
    }
    Gate {
        name: "X-recon",
        summary: format!(
            "PRIMARY recomputed from the final member sets reconstructed from the arrival \
             orders and the trace (every trace consumed exactly) against \
             `WorkflowResult::primary`, bitwise, per cell and seed: {}. Disclosure, not part \
             of the predicate: recomputed churn equals `WorkflowResult::churn` on \
             {churn_equal}/{total} cell-seeds.",
            parts.join(" · ")
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
             `expected == 0` exemption and non-vacuity tolerance `S_LEARN_VACUITY_TOL` = \
             {S_LEARN_VACUITY_TOL:e}, `begin_task_rejections == 0`: {}.",
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

fn seed_divergences(a: &Cell, b: &Cell) -> Vec<SeedDivergence> {
    a.runs
        .iter()
        .zip(&b.runs)
        .map(|(x, y)| {
            let pairs = || x.trace.iter().zip(&y.trace);
            SeedDivergence {
                seed: x.result.seed,
                act: pairs().filter(|(p, q)| p.act != q.act).count(),
                score_bits: pairs()
                    .filter(|(p, q)| p.score_bits != q.score_bits)
                    .count(),
                length: x.trace.len().abs_diff(y.trace.len()),
            }
        })
        .collect()
}

fn divergence(a: &Cell, b: &Cell) -> Divergence {
    let per_seed = seed_divergences(a, b);
    Divergence {
        act: per_seed.iter().map(|d| d.act).sum(),
        score_bits: per_seed.iter().map(|d| d.score_bits).sum(),
        seeds_with_act: per_seed.iter().filter(|d| d.act > 0).count(),
        length: per_seed.iter().map(|d| d.length).sum(),
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
    println!("- prereg: `{PREREG}` (Amendments 1 and 2 included)");
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

/// `all` is every cell in table order: the six group cells, `ref-prune`,
/// `wf-asis`.
fn print_arms(cells: &[Cell], all: &[&Cell]) {
    let control = cells[GRP_ROLE].primaries();
    let control_median = cells[GRP_ROLE].median_primary();
    println!("## Arms (pooled)");
    println!();
    println!(
        "| arm | role | median PRIMARY | vs `grp-role` | superior seeds vs `grp-role` | median churn | median µs/decision |"
    );
    println!("|---|---|---:|---:|---:|---:|---:|");
    for cell in all {
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
    let labels: Vec<String> = all.iter().map(|c| format!("`{}`", c.label)).collect();
    println!(
        "| seed | n | {} | churn `grp-role` | churn `grp-topo` |",
        labels.join(" | ")
    );
    println!("|---:|---:|{}---:|---:|", "---:|".repeat(labels.len()));
    for (i, run) in cells[GRP_ROLE].runs.iter().enumerate() {
        let primaries: Vec<String> = all
            .iter()
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
    let none: Vec<&AgreementSample> = samples.iter().filter(|s| s.sensitive_rows == 0).collect();
    println!(
        "- X is the share of `grp-topo`'s successful reads with `sensitive_rows == 0`: {}; of those {} reads the centre is off the roster on {}. Every cell's share is the table's *no readout row sensitive* column.",
        share(none.len(), samples.len()),
        none.len(),
        none.iter().filter(|s| s.centre_vote.is_none()).count()
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
        "The ACT and score-bit counts are positional; an upper bound after the first divergence. The uncontaminated readings are *seeds with any act difference* and E-follow."
    );
    println!();
    println!(
        "| `grp-topo` vs `grp-role`, seed | decisions differing by ACT (positional; an upper bound after the first divergence) | decisions differing by raw score bits (positional; an upper bound after the first divergence) | length difference |"
    );
    println!("|---:|---:|---:|---:|");
    for d in seed_divergences(&cells[GRP_TOPO], &cells[GRP_ROLE]) {
        println!(
            "| {} | {} | {} | {} |",
            d.seed, d.act, d.score_bits, d.length
        );
    }
    println!();
    println!(
        "| pair (pooled) | decisions differing by ACT (positional; an upper bound after the first divergence) | decisions differing by raw score bits (positional; an upper bound after the first divergence) | seeds with any act difference | total length difference |"
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
        "| pair (pooled) | decisions differing by ACT (positional; an upper bound after the first divergence) | decisions differing by raw score bits (positional; an upper bound after the first divergence) | seeds with any act difference | total length difference |"
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

fn print_follow(cells: &[Cell]) {
    println!(
        "## E-follow (every group cell, all centre-present reads, from the `AgreementSample` ledger)"
    );
    println!();
    println!("| cell | join reads: group act == centre's own argmax | leave reads | all reads |");
    println!("|---|---:|---:|---:|");
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
        println!(
            "| `{}` | {} | {} | {} |",
            cell.label,
            tally(Some(false)),
            tally(Some(true)),
            tally(None)
        );
    }
    println!();
}

fn print_reference(cells: &[Cell], reference: &Cell) {
    println!("## `ref-prune` identity (Amendment A2.2, non-gating)");
    println!();
    println!(
        "| cell | tasks with final member set identical to `ref-prune`'s | seeds with PRIMARY bit-identical to `ref-prune`'s |"
    );
    println!("|---|---:|---:|");
    let mut topo = (0usize, 0usize);
    for (i, cell) in cells.iter().enumerate() {
        let mut tasks = 0usize;
        let mut same_tasks = 0usize;
        let mut same_seeds = 0usize;
        for (run, base) in cell.runs.iter().zip(&reference.runs) {
            for (a, b) in run.recon.tasks.iter().zip(&base.recon.tasks) {
                tasks += 1;
                same_tasks += usize::from(a.members == b.members);
            }
            same_seeds +=
                usize::from(run.result.primary.to_bits() == base.result.primary.to_bits());
        }
        if i == GRP_TOPO {
            topo = (same_tasks, tasks);
        }
        println!(
            "| `{}` | {} | {same_seeds} of {} |",
            cell.label,
            share(same_tasks, tasks),
            cell.runs.len()
        );
    }
    println!();
    println!("### A2.2 pre-committed reading");
    println!();
    let (same, tasks) = topo;
    if tasks > 0 && same * 100 >= tasks * 95 {
        println!(
            "`grp-topo` matches `ref-prune` on {same} of {tasks} tasks (≥ 95 %): *\"`grp-topo`'s PRIMARY is the PRIMARY of a learning-free arrival-order redundancy prune; S-learn (i) certifies that the models learn, not that learning changes an outcome.\"*"
        );
    } else {
        println!(
            "Does not apply: `grp-topo` matches `ref-prune` on {same} of {tasks} tasks (< 95 %)."
        );
    }
    println!();
}

/// Sums over a set of reconstructed task ends.
#[derive(Default, Clone)]
struct Bucket {
    tasks: usize,
    successes: usize,
    covered_fraction: f64,
    cov_eff: f64,
    size: usize,
    empty: usize,
    off_demand: usize,
}

/// `(leave, roster, centre vote, blind act votes)` of one read.
type ReadKey = (bool, usize, Option<usize>, usize);

/// `all` is every cell in table order; `roles` bounds the realised roster.
fn print_decomposition(cells: &[Cell], all: &[&Cell], roles: u8) {
    println!("## Outcome decomposition (Amendment A2.3, non-gating; from the reconstruction)");
    println!();
    println!(
        "| cell | roster | tasks | success rate | mean covered fraction | mean cov_eff | mean final size | tasks ending empty | final members whose role has no demand |"
    );
    println!("|---|---|---:|---:|---:|---:|---:|---:|---:|");
    for cell in all {
        // Index 0 pools every roster; index `r` holds roster `r`.
        let mut buckets = vec![Bucket::default(); usize::from(roles) + 1];
        for end in cell.runs.iter().flat_map(|r| &r.recon.tasks) {
            for slot in [0, end.roster] {
                let Some(b) = buckets.get_mut(slot) else {
                    continue;
                };
                b.tasks += 1;
                b.successes += usize::from(end.success());
                b.covered_fraction += end.covered_fraction();
                b.cov_eff += end.cov_eff;
                b.size += end.members.len();
                b.empty += usize::from(end.members.is_empty());
                b.off_demand += end.off_demand;
            }
        }
        for (slot, b) in buckets.iter().enumerate() {
            let roster = if slot == 0 {
                "all".to_owned()
            } else {
                slot.to_string()
            };
            let per_task = |sum: f64| (b.tasks > 0).then(|| sum / b.tasks as f64);
            println!(
                "| `{}` | {roster} | {} | {} | {} | {} | {} | {} | {} of {} |",
                cell.label,
                b.tasks,
                pct(b.successes, b.tasks),
                or_na(per_task(b.covered_fraction), 4),
                or_na(per_task(b.cov_eff), 4),
                or_na(per_task(b.size as f64), 2),
                b.empty,
                b.off_demand,
                b.size
            );
        }
    }
    println!();

    println!("### Act rates and churn by centre (group cells, from the `AgreementSample` ledger)");
    println!();
    println!(
        "| cell | join act: centre-present | join act: centre-absent | leave act: centre-present | leave act: centre-absent | churn: centre-present | churn: centre-absent | churn (harness) |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|---:|");
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
        println!(
            "| `{}` | {} | {} | {} | {} | {} | {} | {churn} |",
            cell.label,
            rate(false, true),
            rate(false, false),
            rate(true, true),
            rate(true, false),
            count(true, true, Some(true)),
            count(true, false, Some(true))
        );
    }
    println!();

    println!("### (read kind, roster, centre vote, blind act votes) → group acts of n");
    println!();
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
    println!(
        "| read | roster | centre vote | blind act votes | {} |",
        labels.join(" | ")
    );
    println!("|---|---:|---|---:|{}", "---:|".repeat(labels.len()));
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
        println!(
            "| {} | {roster} | {centre} | {blind} | {} |",
            if *leave { "leave" } else { "join" },
            counts.join(" | ")
        );
    }
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

/// H-T's two conjuncts and the gates' conjunction.
#[derive(Clone, Copy)]
struct Outcome {
    gates_ok: bool,
    ratio_ok: bool,
    superior_ok: bool,
}

fn print_clauses(cells: &[Cell], context: &Cell, outcome: Outcome) {
    let Outcome {
        gates_ok,
        ratio_ok,
        superior_ok,
    } = outcome;
    let ht_pass = ratio_ok && superior_ok;
    let topo = cells[GRP_TOPO].median_primary();
    let control = cells[GRP_ROLE].median_primary();
    let blind = cells[GRP_ROLE_BLIND].median_primary();
    let asis = context.median_primary();
    println!("## Scoped clauses (prereg §6 and Amendment A2.4)");
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
    } else if ratio_ok {
        format!(
            "H-T fails with the ratio {} at or above {BAR_RATIO:.2}: read under clause 7.",
            ratio_text(topo, control)
        )
    } else {
        format!(
            "H-T fails at a zero control median (ratio {}, Amendment A2.5); no reading of this clause applies.",
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

    let fifth = if !gates_ok {
        "not read: a gate failed.".to_owned()
    } else if topo >= control {
        format!(
            "`grp-topo`'s median {topo:.4} is at or above `grp-role`'s {control:.4}: *\"with the masks held fixed the candidate-blind voters are not load-bearing; EQ5b §3.2's reading was carried by its mask change\"*."
        )
    } else {
        format!("does not apply: `grp-topo`'s median {topo:.4} is below `grp-role`'s {control:.4}.")
    };
    println!("5. **The mirror of clause 2.** {fifth}");

    let solo = divergence(&cells[GRP_TOPO_SOLO], &cells[GRP_TOPO]);
    let departures: Vec<String> = ROUTED_CELLS
        .iter()
        .map(|&i| {
            let (reads, differing) = centre_departures(&cells[i]);
            format!("`{}` {differing} of {reads}", cells[i].label)
        })
        .collect();
    let sixth = if solo.act == 0 {
        format!(
            "`grp-topo-solo` differs from `grp-topo` on 0 acts (positional, total length difference {}): *\"on centre-present reads the routed group's act is the act of the candidate's own role query alone; the other voters decided no read; the result concerns who decides, not the aggregation of more than one candidate-informed opinion, and shows no group deliberating about a candidate.\"* Centre-present reads whose group act differs from the centre's argmax: {}.",
            solo.length,
            departures.join(" · ")
        )
    } else {
        format!(
            "`grp-topo-solo` differs from `grp-topo` on {} acts (positional; an upper bound after the first divergence), so the sentence does not apply. Centre-present reads whose group act differs from the centre's argmax (E-follow): {}.",
            solo.act,
            departures.join(" · ")
        )
    };
    println!("6. **Who decides.** {sixth}");

    let seventh = if !gates_ok {
        "not read: a gate failed.".to_owned()
    } else if ratio_ok && !superior_ok {
        format!(
            "H-T fails conjunct 2 with the ratio {} at or above {BAR_RATIO:.2}: *\"ratio cleared, superiority did not\"*; the verdict is `FALSIFIED`.",
            ratio_text(topo, control)
        )
    } else {
        "does not apply.".to_owned()
    };
    println!("7. **Ratio without superiority.** {seventh}");
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
    let reference = run_reference(&instances)?;
    let context = run_context(&instances, spec.roles)?;
    let all: Vec<&Cell> = cells.iter().chain([&reference, &context]).collect();

    let gates = [
        gate_identity(&cells),
        gate_recon(&all),
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
    print_arms(&cells, &all);

    let word = |ok: bool| if ok { "PASS" } else { "FAIL" };
    println!("## H-T (confirmatory — both conjuncts, against in-battery `grp-role`)");
    println!();
    println!(
        "- conjunct 1 — control median > 0 and median PRIMARY ratio ≥ {BAR_RATIO:.2}×: `grp-topo` {topo_median:.4} / `grp-role` {control_median:.4} = {} — **{}**",
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
    print_reference(&cells, &reference);
    print_decomposition(&cells, &all, spec.roles);
    print_reach(&cells, spec.roles);

    println!("## Context (`wf-asis`, non-gating)");
    println!();
    let asis = context.primaries();
    println!(
        "- `wf-asis` median PRIMARY: {:.4}",
        context.median_primary()
    );
    for cell in &cells {
        println!(
            "- `{}` strictly superior to `wf-asis` on {}/{} seeds",
            cell.label,
            superior_count(&cell.primaries(), &asis),
            asis.len()
        );
    }
    println!();

    print_clauses(
        &cells,
        &context,
        Outcome {
            gates_ok,
            ratio_ok,
            superior_ok,
        },
    );

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
