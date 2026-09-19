//! K7-3 — the identity-keyed routed group on a performance-scored world, as
//! registered in `docs/k7/prereg-K7-3-performance-scored-world.md` (Amendment
//! 1 included): three `GroupAifPolicy` cells (`grp-id`, `grp-topo`,
//! `grp-topo-cov`) and the `ref-prune`, `ref-prune-id`, `ref-keep` and
//! `ref-first` reference cells over `WorkflowSpec::default()` with the
//! `PerformanceSpec` 0.7 / 0.05 / 0.40 draw, one instance per seed shared by
//! all seven cells, one fresh policy per seed, the header printed before any
//! cell runs, every gate computed before the report is rendered, and one
//! `VERDICT:` line last.
//!
//! The registered block, seeds `540..570`, runs with `K7_3_OFFICIAL=1`, without
//! `K7_3_SEEDS`, on a build whose package version is `0.43.0`. `K7_3_SEEDS`
//! accepts `8000..8003` and `8000..8030` and refuses every other value; under
//! it the binary runs every cell and gate, renders the report into a buffer it
//! does not print, and prints the header, the gate lines (pass counts) and
//! `VERDICT: SMOKE (no verdict)` — `VERDICT: RUN-INVALID` when a gate fails.
//! With neither variable, with both, with `K7_3_OFFICIAL` other
//! than `1`, or with any other environment variable whose name starts `K7_`,
//! the binary exits 2 before the header and before generating an instance.
//!
//! Run: `K7_3_OFFICIAL=1 cargo run --release --features
//! harness,decision,process --example k7_3`.

use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fmt::{self, Write as _};
use std::io::Write as _;
use std::process::ExitCode;

use koalisi::algorithms::AgentCapabilities;
use koalisi::decision::{
    AgreementSample, CoalitionDecisionPolicy, CoverageMasks, DecisionRead, GroupAifConfig,
    GroupAifCounters, GroupAifPolicy, GroupVote, ModelLabel, NonVacuity, PrecisionChannel,
    S_LEARN_VACUITY_TOL, VoteRouting, WorldModelTopology, models_moved, v5_e1_base,
};
use koalisi::harness::{
    OutcomeSignal, PerformanceScored, PerformanceSpec, Recon, ReconError, RefFirst, RefKeep,
    RefPrune, RefPruneId, SeedRange, TaskEnd, TraceEntry, TracedPolicy, WorkflowInstance,
    WorkflowResult, WorkflowSpec, WorkflowTask, median_iqr, member_set_identity, reconstruct,
    run_workflow_instance, step_covered, superior_count,
};
use koalisi::process::Role;

const PREREG: &str = "docs/k7/prereg-K7-3-performance-scored-world.md";
const REGISTERED_SEEDS: SeedRange = SeedRange {
    start: 540,
    end: 570,
};
const SEEDS_ENV: &str = "K7_3_SEEDS";
const OFFICIAL_ENV: &str = "K7_3_OFFICIAL";
/// Environment variable names with this prefix are read by [`seed_block`].
const ENV_PREFIX: &str = "K7_";
/// The package version the registered block runs on.
const OFFICIAL_VERSION: &str = "0.43.0";
/// The values `K7_3_SEEDS` accepts, compared as text.
const SMOKE_BLOCKS: [(&str, SeedRange); 2] = [
    (
        "8000..8003",
        SeedRange {
            start: 8000,
            end: 8003,
        },
    ),
    (
        "8000..8030",
        SeedRange {
            start: 8000,
            end: 8030,
        },
    ),
];
/// The registered performance draw.
const PERFORMANCE: PerformanceSpec = PerformanceSpec {
    reliable_prob: 0.7,
    rho_reliable: 0.05,
    rho_flaky: 0.40,
};
/// The signal the reference cells run under; none of them reads the per-bit
/// vector.
const REFERENCE_SIGNAL: OutcomeSignal = OutcomeSignal::Both;

/// XOR applied to the battery seed of the seed-invariance runs.
const INVARIANCE_XOR: u64 = 0x9E37_79B9_7F4A_7C15;

/// H-beats and H-below: ratio of median PRIMARY-P.
const BAR_RATIO: f64 = 1.25;
/// H-beats and H-below: seeds, as a fraction of the block (18 of 30).
const BAR_SEEDS: (usize, usize) = (3, 5);
/// H-equiv: tasks with an identical final member set, as a fraction of the
/// block's tasks (570 of 600).
const EQUIV_TASKS: (usize, usize) = (95, 100);
/// H-equiv: seeds with bit-identical PRIMARY-P, as a fraction of the block (24
/// of 30).
const EQUIV_SEEDS: (usize, usize) = (4, 5);

const LABEL_INVALID: &str = "RUN-INVALID";
const LABEL_SMOKE: &str = "SMOKE (no verdict)";
const LABEL_VALIDATED: &str = "VALIDATED (identity-keyed group beats the prune)";
const LABEL_EQUIV: &str = "FALSIFIED (arm ≡ prune)";
const LABEL_BELOW: &str = "FALSIFIED (arm below the prune)";
const LABEL_NO_EFFECT: &str = "FALSIFIED (no effect at the bar)";

const GRP_ID: usize = 0;
const GRP_TOPO: usize = 1;
const GRP_TOPO_COV: usize = 2;

/// Which model the centre internal of a cell queries, and what that model
/// observes — what the four-class table's window column stands in for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CentreModel {
    /// The role model of the candidate's role, observing that role's required
    /// bits against the per-bit vector of the signal.
    Role(OutcomeSignal),
    /// The candidate's agent model, observing `required_r(own role) &
    /// capabilities` with every bit read as the agent's `performed`, on the
    /// tasks that end with the agent a final member and that mask non-zero.
    Agent,
}

/// One registered group cell.
struct GroupCell {
    label: &'static str,
    role: &'static str,
    topology: WorldModelTopology,
    signal: OutcomeSignal,
    centre: CentreModel,
}

/// The three group cells, indexed by the `GRP_*` constants.
const GROUP_CELLS: [GroupCell; 3] = [
    GroupCell {
        label: "grp-id",
        role: "the arm",
        topology: WorldModelTopology::AgentKeyed,
        signal: OutcomeSignal::Both,
        centre: CentreModel::Agent,
    },
    GroupCell {
        label: "grp-topo",
        role: "reference — secondary reads 2, 3",
        topology: WorldModelTopology::RoleSpecialised,
        signal: OutcomeSignal::Both,
        centre: CentreModel::Role(OutcomeSignal::Both),
    },
    GroupCell {
        label: "grp-topo-cov",
        role: "reference — secondary read 3",
        topology: WorldModelTopology::RoleSpecialised,
        signal: OutcomeSignal::RoleCoverage,
        centre: CentreModel::Role(OutcomeSignal::RoleCoverage),
    },
];

/// The group cells S-determinism's seed-invariance leg rebuilds.
const INVARIANCE_CELLS: [usize; 2] = [GRP_ID, GRP_TOPO];

const REF_PRUNE: usize = 0;
const REF_PRUNE_ID: usize = 1;
const REF_KEEP: usize = 2;
const REF_FIRST: usize = 3;

/// `(label, table role)` of the reference cells, indexed by the `REF_*`
/// constants.
const REFERENCE_CELLS: [(&str, &str); 4] = [
    ("ref-prune", "the criterion's reference"),
    ("ref-prune-id", "reference — secondary read 1"),
    ("ref-keep", "reference"),
    ("ref-first", "reference"),
];

/// The registered configuration of group cell `cell` (prereg §3).
fn cell_config(cell: usize) -> GroupAifConfig {
    GroupAifConfig {
        topology: GROUP_CELLS[cell].topology,
        channel: PrecisionChannel::RoleRestricted,
        masks: CoverageMasks::RoleMatched,
        read: DecisionRead::Deterministic,
        vote: GroupVote::CertaintyWeighted,
        routing: VoteRouting::CandidateStar { lambda: 0.5 },
        n_roles: 3,
        base: v5_e1_base(),
    }
}

/// The registered world (prereg §2).
fn world() -> WorkflowSpec {
    WorkflowSpec {
        performance: Some(PERFORMANCE),
        ..WorkflowSpec::default()
    }
}

/// What the centre's query sees on one centre-present read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct CentreClass {
    /// `cfg0 & required_r == cfg1 & required_r` for the candidate's role `r`.
    identical: bool,
    /// Either of the last two observations of the model the centre queries
    /// holds a failure on a bit that `r` requires on this task.
    window: bool,
}

/// One decision of a replayed trace.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Read {
    task: usize,
    leave: bool,
    act: bool,
    /// `None` when the candidate's role has no step in the task's demand, and
    /// on every read of a replay made without a [`CentreModel`].
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
    let has_role = |i: usize| role_of(inst, i) == Some(role);
    let union = members
        .iter()
        .filter(|&&m| has_role(m) && !(leave && m == agent))
        .fold(0u32, |acc, &m| acc | caps_of(inst, m));
    let own = if has_role(agent) {
        caps_of(inst, agent)
    } else {
        0
    };
    if leave {
        (union | own, union)
    } else {
        (own, own | union)
    }
}

/// Agent `i`'s role index; `None` outside the pool.
fn role_of(inst: &WorkflowInstance, i: usize) -> Option<usize> {
    inst.roles.get(i).map(|r| usize::from(r.index()))
}

/// Agent `i`'s capability mask; `0` outside the pool.
fn caps_of(inst: &WorkflowInstance, i: usize) -> u32 {
    inst.agents.get(i).map_or(0, |a| a.capabilities())
}

/// Whether agent `i` performed on task `t`, as `run_workflow_instance` reports
/// it in `MemberOutcome`: `true` for every agent when the instance carries no
/// draw, else the entry of row `t`, `false` when there is none.
fn performed_on(inst: &WorkflowInstance, t: usize, i: usize) -> bool {
    inst.performance.as_ref().is_none_or(|rows| {
        rows.get(t)
            .and_then(|row| row.get(i))
            .copied()
            .unwrap_or(false)
    })
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

/// `required_r(own role) & capabilities` of agent `i` against `required`, the
/// task's [`required_by_role`]: the mask `grp-id`'s agent model and
/// `ref-prune-id`'s record observe under. `0` for an agent outside the pool or
/// of a role outside `required`.
fn evidence_mask(inst: &WorkflowInstance, required: &[u32], i: usize) -> u32 {
    role_of(inst, i)
        .and_then(|r| required.get(r))
        .map_or(0, |&req| req & caps_of(inst, i))
}

/// The bits `signal` reports `true` for task `t` of `inst` ending on
/// `members`, as `run_workflow_instance` fills the per-bit vector.
fn outcome_mask(
    inst: &WorkflowInstance,
    task: &WorkflowTask,
    t: usize,
    members: &[usize],
    signal: OutcomeSignal,
) -> u32 {
    let width = task.tags.len();
    match signal {
        OutcomeSignal::RoleCoverage => task
            .demand
            .distinct()
            .filter(|&s| usize::from(s.bit) < width && step_covered(inst, members, s))
            .fold(0u32, |acc, s| acc | s.capability_mask().unwrap_or(0)),
        OutcomeSignal::Both => task
            .demand
            .distinct()
            .filter(|&s| {
                usize::from(s.bit) < width
                    && members
                        .iter()
                        .any(|&m| performed_on(inst, t, m) && step_covered(inst, &[m], s))
            })
            .fold(0u32, |acc, s| acc | s.capability_mask().unwrap_or(0)),
        OutcomeSignal::Performance => {
            let low = if width >= 32 {
                u32::MAX
            } else {
                (1u32 << width) - 1
            };
            members
                .iter()
                .filter(|&&m| performed_on(inst, t, m))
                .fold(0u32, |acc, &m| acc | caps_of(inst, m))
                & low
        }
    }
}

/// Push `failed` onto `window` and keep its last two entries.
fn push_window(window: &mut Vec<u32>, failed: u32) {
    window.push(failed);
    if window.len() > 2 {
        window.remove(0);
    }
}

/// Replay `trace` over `inst` in `run_workflow_instance`'s call order, stopping
/// at the end of the trace. With a `centre_model`, each centre-present read is
/// classed against the failure masks of the last two observations of the model
/// that cell's centre queries, oldest first: per role, `required_r & !signal`
/// of the role's on-roster tasks; per agent, its [`evidence_mask`] on the tasks
/// it ends as a non-performing final member under a non-zero mask, and `0` on
/// those it ends performing.
fn replay(
    inst: &WorkflowInstance,
    trace: &[TraceEntry],
    n_roles: usize,
    n_bits: usize,
    centre_model: Option<CentreModel>,
) -> Replay {
    let mut out = Replay::default();
    let mut entries = trace.iter();
    let mut role_windows: Vec<Vec<u32>> = vec![Vec::new(); n_roles];
    let mut agent_windows: Vec<Vec<u32>> = vec![Vec::new(); inst.agents.len()];
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
            let centre = centre_model.and_then(|model| {
                let r = role_of(inst, agent).filter(|r| roster.contains(r))?;
                let (cfg0, cfg1) = role_masks(inst, r, agent, members, leave);
                let window = match model {
                    CentreModel::Role(_) => role_windows.get(r),
                    CentreModel::Agent => agent_windows.get(agent),
                };
                Some(CentreClass {
                    identical: cfg0 & required[r] == cfg1 & required[r],
                    window: window
                        .is_some_and(|w| w.iter().any(|&failed| failed & required[r] != 0)),
                })
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

        match centre_model {
            Some(CentreModel::Role(signal)) => {
                let ok = outcome_mask(inst, task, t, &members, signal);
                for &r in &roster {
                    push_window(&mut role_windows[r], required[r] & !ok);
                }
            }
            Some(CentreModel::Agent) => {
                for &m in &members {
                    let mask = evidence_mask(inst, &required, m);
                    if mask == 0 {
                        continue;
                    }
                    let failed = if performed_on(inst, t, m) { 0 } else { mask };
                    if let Some(window) = agent_windows.get_mut(m) {
                        push_window(window, failed);
                    }
                }
            }
            None => {}
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

    /// PRIMARY-P; `NaN` when the instance carries no performance draw.
    fn primary_p(&self) -> f64 {
        self.result
            .performance_scored
            .map_or(f64::NAN, |p| p.primary)
    }

    /// The trace entries of task 0: as many as the replay holds reads of it.
    fn task0(&self) -> &[TraceEntry] {
        let k = self.replay.reads.iter().take_while(|r| r.task == 0).count();
        &self.trace[..k.min(self.trace.len())]
    }
}

/// One group policy's ledger after its instance.
struct Ledger {
    counters: GroupAifCounters,
    moved: NonVacuity,
    /// `GroupAifPolicy::agent_model_updates`; empty off `grp-id`.
    agent_updates: Vec<(usize, u64)>,
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
    fn primaries_p(&self) -> Vec<f64> {
        self.runs.iter().map(SeedRun::primary_p).collect()
    }

    fn median_p(&self) -> f64 {
        median_iqr(&self.primaries_p()).0
    }

    fn median_of(&self, f: impl Fn(&SeedRun) -> f64) -> f64 {
        let values: Vec<f64> = self.runs.iter().map(f).collect();
        median_iqr(&values).0
    }

    fn samples(&self) -> Vec<AgreementSample> {
        self.ledgers
            .iter()
            .flat_map(|l| l.counters.agreement.iter().copied())
            .collect()
    }

    fn ends(&self) -> impl Iterator<Item = &TaskEnd> {
        self.runs
            .iter()
            .filter_map(SeedRun::recon)
            .flat_map(|r| &r.tasks)
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
/// package `version`. `K7_3_SEEDS` alone selects a block of [`SMOKE_BLOCKS`];
/// `K7_3_OFFICIAL=1` alone at `version == "0.43.0"` selects the registered
/// block.
///
/// # Errors
///
/// The refusal message: a name other than the two above, neither or both of
/// them, a `K7_3_SEEDS` value outside [`SMOKE_BLOCKS`], a `K7_3_OFFICIAL`
/// value other than `1`, or `K7_3_OFFICIAL=1` at another `version`.
fn seed_block(vars: &[(String, String)], version: &str) -> Result<(SeedRange, bool), String> {
    let foreign: Vec<String> = vars
        .iter()
        .filter(|(name, _)| name != SEEDS_ENV && name != OFFICIAL_ENV)
        .map(|(name, _)| format!("`{name}`"))
        .collect();
    if !foreign.is_empty() {
        return Err(format!(
            "refused: {} set; this binary runs with {SEEDS_ENV} or {OFFICIAL_ENV} and no other \
             environment variable whose name starts `{ENV_PREFIX}` (prereg §8)",
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
             {REGISTERED_SEEDS} runs only with {OFFICIAL_ENV}=1 (prereg §8)"
        )),
        (Some(_), Some(_)) => Err(format!(
            "refused: {SEEDS_ENV} and {OFFICIAL_ENV} are both set (prereg §8)"
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
             and this build is v{version} (prereg §8, §9)"
        )),
        (None, Some(other)) => Err(format!(
            "{OFFICIAL_ENV}={other} refused: the accepted value is `1` (prereg §8)"
        )),
    }
}

/// Run `policy` traced over `inst` under `signal`, reconstruct its trace and
/// replay it under the group cells' role count and universe width.
fn run_traced(
    policy: &dyn CoalitionDecisionPolicy,
    inst: &WorkflowInstance,
    signal: OutcomeSignal,
    centre_model: Option<CentreModel>,
    latencies: &mut Vec<f64>,
) -> Result<SeedRun, Box<dyn Error>> {
    let traced = TracedPolicy::new(policy);
    let result = run_workflow_instance(&traced, inst, signal, latencies)?;
    let trace = traced.entries();
    let recon = reconstruct(inst, &trace);
    let config = cell_config(GRP_TOPO);
    let replayed = replay(
        inst,
        &trace,
        config.n_roles,
        config.base.n_bits,
        centre_model,
    );
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

/// Group cell `cell` over `instances`: one fresh policy per instance, seeded
/// `instance.seed ^ seed_xor`, run under the cell's registered signal.
fn run_group(
    cell: usize,
    instances: &[WorkflowInstance],
    seed_xor: u64,
) -> Result<Cell, Box<dyn Error>> {
    let spec = &GROUP_CELLS[cell];
    let config = cell_config(cell);
    let mut out = Cell {
        label: spec.label,
        role: spec.role,
        runs: Vec::with_capacity(instances.len()),
        ledgers: Vec::with_capacity(instances.len()),
        latencies: Vec::new(),
    };
    for inst in instances {
        let policy = GroupAifPolicy::new(inst.seed ^ seed_xor, config, role_map(inst)?)?;
        let before = policy.model_snapshots();
        let run = run_traced(
            &policy,
            inst,
            spec.signal,
            Some(spec.centre),
            &mut out.latencies,
        )?;
        let counters = policy.counters();
        let moved = models_moved(&policy.model_snapshots(), &before, &counters.model_updates);
        out.runs.push(run);
        out.ledgers.push(Ledger {
            counters,
            moved,
            agent_updates: policy.agent_model_updates(),
        });
    }
    Ok(out)
}

/// Reference cell `which` of [`REFERENCE_CELLS`]: one fresh policy per
/// instance — [`RefPrune`] or [`RefPruneId`] over the instance's role map,
/// [`RefKeep`] or [`RefFirst`].
fn run_reference(which: usize, instances: &[WorkflowInstance]) -> Result<Cell, Box<dyn Error>> {
    let (label, role) = REFERENCE_CELLS[which];
    let mut cell = Cell {
        label,
        role,
        runs: Vec::with_capacity(instances.len()),
        ledgers: Vec::new(),
        latencies: Vec::new(),
    };
    for inst in instances {
        let policy: Box<dyn CoalitionDecisionPolicy> = match which {
            REF_PRUNE => Box::new(RefPrune::new(role_map(inst)?)),
            REF_PRUNE_ID => Box::new(RefPruneId::new(role_map(inst)?)),
            REF_KEEP => Box::new(RefKeep),
            _ => Box::new(RefFirst),
        };
        let run = run_traced(
            policy.as_ref(),
            inst,
            REFERENCE_SIGNAL,
            None,
            &mut cell.latencies,
        )?;
        cell.runs.push(run);
    }
    Ok(cell)
}

/// The three fields of a `PerformanceScored` as bits.
fn scored_bits(p: Option<PerformanceScored>) -> Option<[u64; 3]> {
    p.map(|p| {
        [
            p.success_rate.to_bits(),
            p.mean_cov_eff.to_bits(),
            p.primary.to_bits(),
        ]
    })
}

/// Trace entries (leave flag, act, raw score bits), PRIMARY-P bits, PRIMARY
/// bits and churn all equal.
fn identical(a: &SeedRun, b: &SeedRun) -> bool {
    a.trace == b.trace
        && a.primary_p().to_bits() == b.primary_p().to_bits()
        && a.result.primary.to_bits() == b.result.primary.to_bits()
        && a.result.churn == b.result.churn
}

/// The pass count of `first` against `second` under [`identical`], and
/// `"<label> <seed>"` for every seed on which they differ.
fn identity_part(first: &Cell, second: &Cell) -> (Part, Vec<String>) {
    let differing: Vec<String> = first
        .runs
        .iter()
        .zip(&second.runs)
        .filter(|(x, y)| !identical(x, y))
        .map(|(x, _)| format!("`{}` {}", first.label, x.result.seed))
        .collect();
    let part = Part {
        label: format!("`{}`", first.label),
        ok: first
            .runs
            .iter()
            .zip(&second.runs)
            .filter(|(x, y)| identical(x, y))
            .count(),
        n: first.runs.len().max(second.runs.len()),
    };
    (part, differing)
}

/// `pairs` is `(cell, its reference)`: `grp-id` against `grp-topo`, then
/// `ref-prune-id` against `ref-prune`.
fn gate_id0(pairs: &[(&Cell, &Cell)]) -> Gate {
    let mut failures = Vec::new();
    let mut parts = Vec::new();
    let mut compared = Vec::new();
    for (cell, reference) in pairs {
        let mut ok = 0usize;
        let mut entries = 0usize;
        for (run, other) in cell.runs.iter().zip(&reference.runs) {
            entries += run.task0().len();
            if run.task0() == other.task0() {
                ok += 1;
            } else {
                failures.push(format!(
                    "`{}` {} (task-0 entries differ from `{}`'s)",
                    cell.label, run.result.seed, reference.label
                ));
            }
        }
        parts.push(Part {
            label: format!("`{}`", cell.label),
            ok,
            n: cell.runs.len().max(reference.runs.len()),
        });
        compared.push(format!("`{}` {entries}", cell.label));
    }
    Gate {
        name: "X-id0",
        predicate: "per seed the task-0 trace entries (leave flag, act, raw score bits) of \
                    `grp-id` equal `grp-topo`'s and those of `ref-prune-id` equal `ref-prune`'s, \
                    seeds equal per cell"
            .to_owned(),
        parts,
        failures,
        detail: vec![format!(
            "X-id0 disclosure, not part of the predicate: task-0 entries compared over the \
             block: {}.",
            compared.join(" · ")
        )],
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
            match &run.recon {
                Ok(recon) => {
                    churn_equal += usize::from(recon.churn == run.result.churn);
                    let primary_ok = recon.primary.to_bits() == run.result.primary.to_bits();
                    let scored_ok = scored_bits(recon.performance_scored)
                        == scored_bits(run.result.performance_scored);
                    if primary_ok && scored_ok {
                        ok += 1;
                    } else {
                        failures.push(format!(
                            "`{}` {} (recomputed {:?} / {:?}, harness {:?} / {:?})",
                            cell.label,
                            run.result.seed,
                            recon.primary,
                            recon.performance_scored,
                            run.result.primary,
                            run.result.performance_scored
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
        predicate: "`primary` and `performance_scored` recomputed from the final member sets \
                    reconstructed from the arrival orders and the trace (every trace consumed \
                    exactly) against the `WorkflowResult`'s, bitwise, seeds passing per cell"
            .to_owned(),
        parts,
        failures,
        detail: vec![format!(
            "X-recon disclosure, not part of the predicate: recomputed churn equals \
             `WorkflowResult::churn` on {churn_equal}/{total} cell-seeds."
        )],
    }
}

/// `pairs` is `(first run, re-run)` per group cell, then `ref-prune-id`.
fn gate_determinism(pairs: &[(&Cell, &Cell)]) -> Gate {
    let mut failures = Vec::new();
    let mut parts = Vec::new();
    for (first, second) in pairs {
        let (part, differing) = identity_part(first, second);
        parts.push(part);
        failures.extend(differing);
    }
    Gate {
        name: "S-determinism",
        predicate: "every group cell and `ref-prune-id` re-run from scratch and compared per \
                    seed on trace entries (leave flag, act, raw score bits), PRIMARY-P bits, \
                    PRIMARY bits and churn, seeds identical per cell"
            .to_owned(),
        parts,
        failures,
        detail: Vec::new(),
    }
}

/// `reseeded` holds one cell per entry of [`INVARIANCE_CELLS`], in that order.
fn gate_invariance(groups: &[Cell], reseeded: &[Cell]) -> Gate {
    let mut failures = Vec::new();
    let mut parts = Vec::new();
    for (&i, second) in INVARIANCE_CELLS.iter().zip(reseeded) {
        let (part, differing) = identity_part(&groups[i], second);
        parts.push(part);
        failures.extend(differing);
    }
    Gate {
        name: "Seed invariance",
        predicate: format!(
            "`grp-id` and `grp-topo` rebuilt with `battery_seed = seed ^ {INVARIANCE_XOR:#018X}`, \
             same comparison, seeds identical per cell"
        ),
        parts,
        failures,
        detail: Vec::new(),
    }
}

fn gate_learn(groups: &[Cell]) -> Gate {
    let mut failures = Vec::new();
    let mut parts = Vec::new();
    let mut detail =
        vec!["S-learn (i) exempt models (`expected == 0`), as `(seed, model)`:".to_owned()];
    for cell in groups {
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
            "per seed and group cell, over the role models, `s_learn_exact()`, \
             `models_moved(..).ok` with its `expected == 0` exemption at `S_LEARN_VACUITY_TOL` = \
             {S_LEARN_VACUITY_TOL:e}, `begin_task_rejections == 0`, seeds passing per cell"
        ),
        parts,
        failures,
        detail,
    }
}

/// `(agent id, tasks)` per pool agent, ascending: the tasks of `recon` on which
/// the agent is a final member with a non-zero [`evidence_mask`].
fn expected_agent_updates(
    inst: &WorkflowInstance,
    recon: &Recon,
    n_roles: usize,
    n_bits: usize,
) -> Vec<(usize, u64)> {
    let mut counts = vec![0u64; inst.agents.len()];
    for (task, end) in inst.tasks.iter().zip(&recon.tasks) {
        let required = required_by_role(task, n_roles, n_bits);
        for &m in &end.members {
            if evidence_mask(inst, &required, m) != 0
                && let Some(slot) = counts.get_mut(m)
            {
                *slot += 1;
            }
        }
    }
    counts.into_iter().enumerate().collect()
}

/// `cell` is `grp-id`; `instances` the block's, in seed order.
fn gate_learn_id(cell: &Cell, instances: &[WorkflowInstance]) -> Gate {
    let config = cell_config(GRP_ID);
    let mut failures = Vec::new();
    let mut ok = 0usize;
    let mut total = 0u64;
    for ((run, ledger), inst) in cell.runs.iter().zip(&cell.ledgers).zip(instances) {
        total += ledger.agent_updates.iter().map(|&(_, n)| n).sum::<u64>();
        let Some(recon) = run.recon() else {
            failures.push(format!(
                "`{}` {} (the trace does not replay)",
                cell.label, run.result.seed
            ));
            continue;
        };
        let expected = expected_agent_updates(inst, recon, config.n_roles, config.base.n_bits);
        if ledger.agent_updates == expected {
            ok += 1;
        } else {
            failures.push(format!(
                "`{}` {} (applied {:?}, recomputed {:?})",
                cell.label, run.result.seed, ledger.agent_updates, expected
            ));
        }
    }
    if total == 0 {
        failures.push(format!("`{}` block total 0", cell.label));
    }
    Gate {
        name: "S-learn (id)",
        predicate: "per seed on `grp-id`, every agent model's applied-update count \
                    (`agent_model_updates`) equals the count recomputed from the reconstruction \
                    and the instance's capabilities (tasks on which the agent is a final member \
                    with `required_r(own role) & capabilities != 0`), block total > 0; seeds \
                    passing"
            .to_owned(),
        parts: vec![
            Part {
                label: format!("`{}`", cell.label),
                ok,
                n: cell.runs.len().max(instances.len()),
            },
            Part {
                label: "block total > 0".to_owned(),
                ok: usize::from(total > 0),
                n: 1,
            },
        ],
        failures,
        detail: vec![format!(
            "S-learn (id) block total of applied updates: {total}."
        )],
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

fn gate_route(groups: &[Cell]) -> Gate {
    let mut failures = Vec::new();
    let mut parts = Vec::new();
    let mut totals = Vec::new();
    let mut positive = 0usize;
    for cell in groups {
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
    parts.push(Part {
        label: "cells with block total > 0".to_owned(),
        ok: positive,
        n: groups.len(),
    });
    Gate {
        name: "S-route",
        predicate: "per seed on `grp-id`, `grp-topo` and `grp-topo-cov` `routed_reads` == ledger \
                    reads with `candidate_sensitive >= 1` and `roster >= 2`, block total > 0 per \
                    cell; seeds passing per cell"
            .to_owned(),
        parts,
        failures,
        detail: vec![format!("S-route block totals: {}.", totals.join(" · "))],
    }
}

/// Final member-slots of one cell over the block, and those of them that did
/// not perform; the same over the slots covering at least one demanded step.
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
struct Slots {
    all: usize,
    all_failed: usize,
    covering: usize,
    covering_failed: usize,
}

/// The [`Slots`] of `recon` over `inst`: a slot covers a demanded step iff the
/// member has the step's role and holds its bit.
fn slots(inst: &WorkflowInstance, recon: &Recon) -> Slots {
    let mut out = Slots::default();
    for (t, (task, end)) in inst.tasks.iter().zip(&recon.tasks).enumerate() {
        for &m in &end.members {
            let failed = usize::from(!performed_on(inst, t, m));
            let covering = task.demand.distinct().any(|s| step_covered(inst, &[m], s));
            out.all += 1;
            out.all_failed += failed;
            if covering {
                out.covering += 1;
                out.covering_failed += failed;
            }
        }
    }
    out
}

/// [`slots`] summed over `cell`'s replayed seeds.
fn cell_slots(cell: &Cell, instances: &[WorkflowInstance]) -> Slots {
    let mut out = Slots::default();
    for (run, inst) in cell.runs.iter().zip(instances) {
        if let Some(recon) = run.recon() {
            let s = slots(inst, recon);
            out.all += s.all;
            out.all_failed += s.all_failed;
            out.covering += s.covering;
            out.covering_failed += s.covering_failed;
        }
    }
    out
}

/// `keep` is `ref-keep`.
fn gate_draw(keep: &Cell, instances: &[WorkflowInstance]) -> Gate {
    let mut failures = Vec::new();
    let mut carrying = 0usize;
    for inst in instances {
        if inst.performance.is_some() {
            carrying += 1;
        } else {
            failures.push(format!("{} (no performance draw)", inst.seed));
        }
    }
    let found = cell_slots(keep, instances);
    if found.all_failed == 0 {
        failures.push(format!(
            "`{}` holds no non-performing final member-slot over the block",
            keep.label
        ));
    }
    Gate {
        name: "S-draw",
        predicate: "per seed the instance carries a performance draw; over the block \
                    `ref-keep`'s final member-slots include ≥ 1 that did not perform"
            .to_owned(),
        parts: vec![
            Part {
                label: "instances carrying a draw".to_owned(),
                ok: carrying,
                n: instances.len(),
            },
            Part {
                label: format!("`{}` block holds a non-performing slot", keep.label),
                ok: usize::from(found.all_failed > 0),
                n: 1,
            },
        ],
        failures,
        detail: vec![format!(
            "S-draw: `{}` final member-slots {}, of which {} did not perform.",
            keep.label, found.all, found.all_failed
        )],
    }
}

/// The first position at which two traces differ by act.
#[derive(Clone, Copy)]
struct First {
    position: usize,
    /// The task index of the read at `position` in the first cell's replay;
    /// `None` when the replay ended before `position`.
    task: Option<usize>,
    /// The entry's read kind.
    leave: bool,
    /// `true` when the first cell acted and the second declined.
    first_acted: bool,
}

/// Position-wise divergence of two cells' traces on one seed, compared up to
/// the shorter trace.
struct SeedDivergence {
    seed: u64,
    act: usize,
    score_bits: usize,
    length: usize,
    first: Option<First>,
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
                first: pairs().enumerate().find(|(_, (p, q))| p.act != q.act).map(
                    |(position, (p, _))| First {
                        position,
                        task: x.replay.reads.get(position).map(|r| r.task),
                        leave: p.leave,
                        first_acted: p.act,
                    },
                ),
            }
        })
        .collect()
}

/// Pooled first-divergence counts, indexed `[leave][first_acted]`, the seeds
/// without an act difference, and seeds per task index of the first act
/// difference.
struct FirstPool {
    counts: [[usize; 2]; 2],
    none: usize,
    by_task: BTreeMap<usize, usize>,
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
                pool.counts[usize::from(f.leave)][usize::from(f.first_acted)] += 1;
                if let Some(task) = f.task {
                    *pool.by_task.entry(task).or_insert(0) += 1;
                }
            }
            None => pool.none += 1,
        }
    }
    pool
}

/// One cell against another over the block, from the reconstructions and the
/// harness PRIMARY-P.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Against {
    same_tasks: usize,
    tasks: usize,
    same_seeds: usize,
    seeds: usize,
}

/// A seed without a reconstruction on either side adds nothing.
fn against(cell: &Cell, base: &Cell) -> Against {
    let mut out = Against {
        same_tasks: 0,
        tasks: 0,
        same_seeds: 0,
        seeds: cell.runs.len().max(base.runs.len()),
    };
    for (run, other) in cell.runs.iter().zip(&base.runs) {
        let (Some(recon), Some(other_recon)) = (run.recon(), other.recon()) else {
            continue;
        };
        let (same, total) = member_set_identity(recon, other_recon);
        out.same_tasks += same;
        out.tasks += total;
        out.same_seeds += usize::from(run.primary_p().to_bits() == other.primary_p().to_bits());
    }
    out
}

/// `ceil(n * num / den)`.
fn at_least(n: usize, (num, den): (usize, usize)) -> usize {
    (n * num).div_ceil(den)
}

/// H-beats or H-below read in one direction: `better` against `other`.
#[derive(Debug)]
struct Leg {
    better: &'static str,
    other: &'static str,
    better_median: f64,
    other_median: f64,
    superior: usize,
    seeds: usize,
    bar: usize,
}

impl Leg {
    /// `other`'s median PRIMARY-P is above `0`.
    fn divisor_ok(&self) -> bool {
        self.other_median > 0.0
    }

    fn ratio_ok(&self) -> bool {
        self.divisor_ok() && self.better_median / self.other_median >= BAR_RATIO
    }

    fn superior_ok(&self) -> bool {
        self.superior >= self.bar
    }

    fn pass(&self) -> bool {
        self.divisor_ok() && self.ratio_ok() && self.superior_ok()
    }
}

/// The three-way counted form of cell `a` against cell `b` on PRIMARY-P.
#[derive(Debug)]
struct ThreeWay {
    a: &'static str,
    b: &'static str,
    found: Against,
    tasks_bar: usize,
    seeds_bar: usize,
    /// `a` over `b`.
    beats: Leg,
    /// `b` over `a`.
    below: Leg,
}

impl ThreeWay {
    /// `a_values` and `b_values` are the per-seed PRIMARY-P of `a` and `b`.
    fn new(
        a: &'static str,
        b: &'static str,
        found: Against,
        a_values: &[f64],
        b_values: &[f64],
    ) -> Self {
        let seeds = found.seeds;
        let (a_median, b_median) = (median_iqr(a_values).0, median_iqr(b_values).0);
        let leg = |better, other, better_median, other_median, superior| Leg {
            better,
            other,
            better_median,
            other_median,
            superior,
            seeds,
            bar: at_least(seeds, BAR_SEEDS),
        };
        Self {
            a,
            b,
            tasks_bar: at_least(found.tasks, EQUIV_TASKS),
            seeds_bar: at_least(seeds, EQUIV_SEEDS),
            found,
            beats: leg(a, b, a_median, b_median, superior_count(a_values, b_values)),
            below: leg(b, a, b_median, a_median, superior_count(b_values, a_values)),
        }
    }

    fn of(a: &Cell, b: &Cell) -> Self {
        Self::new(
            a.label,
            b.label,
            against(a, b),
            &a.primaries_p(),
            &b.primaries_p(),
        )
    }

    fn tasks_ok(&self) -> bool {
        self.found.same_tasks >= self.tasks_bar
    }

    fn seeds_ok(&self) -> bool {
        self.found.same_seeds >= self.seeds_bar
    }

    fn equiv(&self) -> bool {
        self.tasks_ok() && self.seeds_ok()
    }

    /// §6's label for a run whose gates hold, in precedence order.
    fn label(&self) -> &'static str {
        if self.beats.pass() {
            LABEL_VALIDATED
        } else if self.equiv() {
            LABEL_EQUIV
        } else if self.below.pass() {
            LABEL_BELOW
        } else {
            LABEL_NO_EFFECT
        }
    }

    /// The legs that pass, by name, or `none of the three legs`.
    fn legs_passed(&self) -> String {
        let passed: Vec<&str> = [
            ("H-equiv", self.equiv()),
            ("H-beats", self.beats.pass()),
            ("H-below", self.below.pass()),
        ]
        .iter()
        .filter(|(_, pass)| *pass)
        .map(|(name, _)| *name)
        .collect();
        if passed.is_empty() {
            "none of the three legs".to_owned()
        } else {
            passed.join(", ")
        }
    }

    /// §6 clause 3's sentence for each ratio conjunct whose divisor's median is
    /// `0`.
    fn zero_median_sentences(&self) -> Vec<String> {
        [&self.beats, &self.below]
            .iter()
            .filter(|leg| !leg.divisor_ok())
            .map(|leg| {
                format!(
                    "*\"`{}`'s median PRIMARY-P is 0; the ratio is undefined; `{}` is strictly superior on {}/{} seeds.\"*",
                    leg.other, leg.better, leg.superior, leg.seeds
                )
            })
            .collect()
    }
}

/// The registered secondary reads.
struct Secondary {
    /// Read 1: `grp-id` against `ref-prune-id`.
    id_vs_prune_id: ThreeWay,
    /// Read 1, with it: `ref-prune-id` against `ref-prune`.
    prune_id_vs_prune: ThreeWay,
    /// Read 2: `grp-topo` against `ref-prune`.
    topo_vs_prune: ThreeWay,
    /// Read 3: `grp-topo` against `grp-topo-cov`.
    topo_vs_cov: ThreeWay,
}

impl Secondary {
    fn all(&self) -> [&ThreeWay; 4] {
        [
            &self.id_vs_prune_id,
            &self.prune_id_vs_prune,
            &self.topo_vs_prune,
            &self.topo_vs_cov,
        ]
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

/// `a / b` at 4 dp, `n/a` for a `b` not above zero.
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
    writeln!(
        out,
        "# K7-3 — the identity-keyed routed group on a performance-scored world"
    )?;
    writeln!(out)?;
    if smoke {
        writeln!(
            out,
            "**SMOKE — NOT THE REGISTERED BLOCK** (`{SEEDS_ENV}={seeds}`; the registered block is {REGISTERED_SEEDS}). The report is rendered and not printed."
        )?;
        writeln!(out)?;
    }
    writeln!(out, "- registration: K7-3 (koalisi #100)")?;
    writeln!(out, "- prereg: `{PREREG}` (Amendment 1 included)")?;
    writeln!(out, "- koalisi: v{}", env!("CARGO_PKG_VERSION"))?;
    writeln!(out, "- seeds: {seeds} ({} seeds)", seeds.len())?;
    let cells: Vec<String> = GROUP_CELLS
        .iter()
        .map(|c| {
            format!(
                "`{}` {:?}, reads {:?}{}",
                c.label,
                c.topology,
                c.signal,
                if c.centre == CentreModel::Agent {
                    " + members"
                } else {
                    ""
                }
            )
        })
        .chain(
            REFERENCE_CELLS
                .iter()
                .map(|(label, _)| format!("`{label}` engine-free")),
        )
        .collect();
    writeln!(
        out,
        "- cells, one instance per seed shared by all seven: {}",
        cells.join(" · ")
    )?;
    writeln!(
        out,
        "- scored outcome: PRIMARY-P = `WorkflowResult::performance_scored.primary`; the coverage-scored `WorkflowResult::primary` is reported beside it and enters no criterion"
    )?;
    writeln!(
        out,
        "- world: `WorkflowSpec::default()` with the performance draw — universe_bits {}, pool {:?}, caps_per_agent {:?}, trust {:?}, tasks {}, required_bits {:?}, roles {}, redraw_cap {}, fanout_denom {}, performance {:?}",
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

/// The gate lines: the gates checked outside the binary, then each in-binary
/// gate's [`Gate::line`].
fn render_gate_lines(out: &mut String, gates: &[Gate]) -> fmt::Result {
    writeln!(out, "## Gates")?;
    writeln!(out)?;
    writeln!(
        out,
        "- **X-battery** — checked outside this binary, conditional on the diff (prereg §5): `git diff --name-only v0.42.0..HEAD -- src Cargo.toml Cargo.lock`; a path under `src/` outside `src/harness/`, a dependency line in `Cargo.toml` or a foreign stanza in `Cargo.lock` puts on one serial run of `examples/strategy_comparison.rs` diffed against `docs/runs/K4-archive.log` with the latency column stripped (`docs/runs/README.md`); otherwise recorded not run."
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
    writeln!(
        out,
        "- **S-id** — checked outside this binary: `cargo test --features harness,decision,process --test k7_3_identity` (hand-built fixture; Amendment A1.1, A1.3)."
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

/// One per-seed table: a column per cell of `all`, each value from `value`.
fn render_per_seed(
    out: &mut String,
    title: &str,
    all: &[&Cell],
    value: impl Fn(&SeedRun) -> String,
) -> fmt::Result {
    writeln!(out, "### Per seed — {title}")?;
    writeln!(out)?;
    let labels: Vec<String> = all.iter().map(|c| format!("`{}`", c.label)).collect();
    writeln!(out, "| seed | n | {} |", labels.join(" | "))?;
    writeln!(out, "|---:|---:|{}", "---:|".repeat(labels.len()))?;
    let Some(first) = all.first() else {
        return writeln!(out);
    };
    for (i, run) in first.runs.iter().enumerate() {
        let values: Vec<String> = all
            .iter()
            .map(|c| c.runs.get(i).map_or_else(|| "—".to_owned(), &value))
            .collect();
        writeln!(
            out,
            "| {} | {} | {} |",
            run.result.seed,
            run.result.n,
            values.join(" | ")
        )?;
    }
    writeln!(out)
}

/// `all` is every cell in table order.
fn render_arms(out: &mut String, all: &[&Cell]) -> fmt::Result {
    writeln!(out, "## Arms (pooled; E-lat is record-only)")?;
    writeln!(out)?;
    writeln!(
        out,
        "| arm | role | median PRIMARY-P | median success rate (P) | median mean efficiency (P) | median coverage-scored PRIMARY | median churn | mean final size | tasks ending empty | E-lat: median µs/decision |"
    )?;
    writeln!(out, "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|")?;
    for cell in all {
        let scored = |f: fn(PerformanceScored) -> f64| {
            cell.median_of(|r| r.result.performance_scored.map_or(f64::NAN, f))
        };
        writeln!(
            out,
            "| `{}` | {} | {:.4} | {:.4} | {:.4} | {:.4} | {:.2} | {} | {} of {} | {:.3} |",
            cell.label,
            cell.role,
            cell.median_p(),
            scored(|p| p.success_rate),
            scored(|p| p.mean_cov_eff),
            cell.median_of(|r| r.result.primary),
            cell.median_of(|r| r.result.churn as f64),
            or_na(mean(cell.ends().map(|e| e.members.len() as f64)), 2),
            cell.ends().filter(|e| e.members.is_empty()).count(),
            cell.ends().count(),
            median_iqr(&cell.latencies).0
        )?;
    }
    writeln!(out)?;
    let scored = |run: &SeedRun, f: fn(PerformanceScored) -> f64| {
        run.result
            .performance_scored
            .map_or_else(|| "n/a".to_owned(), |p| format!("{:.4}", f(p)))
    };
    render_per_seed(out, "PRIMARY-P", all, |r| scored(r, |p| p.primary))?;
    render_per_seed(out, "success rate (P)", all, |r| {
        scored(r, |p| p.success_rate)
    })?;
    render_per_seed(out, "mean efficiency (P)", all, |r| {
        scored(r, |p| p.mean_cov_eff)
    })?;
    render_per_seed(out, "coverage-scored PRIMARY", all, |r| {
        format!("{:.4}", r.result.primary)
    })?;
    render_per_seed(out, "churn", all, |r| r.result.churn.to_string())
}

/// One three-way form, as a bullet list under `title`.
fn render_three_way(out: &mut String, title: &str, tw: &ThreeWay) -> fmt::Result {
    writeln!(out, "### {title} — `{}` against `{}`", tw.a, tw.b)?;
    writeln!(out)?;
    writeln!(
        out,
        "- H-equiv — final member set identical on ≥ {} of {} tasks: {} — **{}**; PRIMARY-P bit-identical on ≥ {}/{} seeds: {}/{} — **{}**; H-equiv: **{}**",
        tw.tasks_bar,
        tw.found.tasks,
        tw.found.same_tasks,
        word(tw.tasks_ok()),
        tw.seeds_bar,
        tw.found.seeds,
        tw.found.same_seeds,
        tw.found.seeds,
        word(tw.seeds_ok()),
        word(tw.equiv())
    )?;
    for (name, leg) in [("H-beats", &tw.beats), ("H-below", &tw.below)] {
        writeln!(
            out,
            "- {name} — `{}` over `{}`: `{}`'s median PRIMARY-P {:.4} > 0 — **{}**; ratio of medians ≥ {BAR_RATIO:.2}×: {:.4} / {:.4} = {} — **{}**; strictly superior on ≥ {}/{} seeds: {}/{} — **{}**; {name}: **{}**",
            leg.better,
            leg.other,
            leg.other,
            leg.other_median,
            word(leg.divisor_ok()),
            leg.better_median,
            leg.other_median,
            ratio_text(leg.better_median, leg.other_median),
            word(leg.ratio_ok()),
            leg.bar,
            leg.seeds,
            leg.superior,
            leg.seeds,
            word(leg.superior_ok()),
            word(leg.pass())
        )?;
    }
    writeln!(out, "- legs passed: {}", tw.legs_passed())?;
    for sentence in tw.zero_median_sentences() {
        writeln!(out, "- {sentence}")?;
    }
    writeln!(out)
}

fn render_criterion(out: &mut String, criterion: &ThreeWay, secondary: &Secondary) -> fmt::Result {
    writeln!(
        out,
        "## The counted criterion (against cells of this battery, on PRIMARY-P)"
    )?;
    writeln!(out)?;
    render_three_way(out, "Criterion", criterion)?;
    writeln!(
        out,
        "## Registered secondary reads (none gating; the same three-way form)"
    )?;
    writeln!(out)?;
    render_three_way(out, "Secondary read 1", &secondary.id_vs_prune_id)?;
    render_three_way(
        out,
        "Secondary read 1, with it",
        &secondary.prune_id_vs_prune,
    )?;
    render_three_way(out, "Secondary read 2", &secondary.topo_vs_prune)?;
    render_three_way(out, "Secondary read 3", &secondary.topo_vs_cov)
}

fn render_against(out: &mut String, groups: &[Cell], references: &[Cell]) -> fmt::Result {
    for reference in references {
        let name = reference.label;
        writeln!(out, "## Against `{name}` (disclosure)")?;
        writeln!(out)?;
        writeln!(
            out,
            "| cell | tasks with final member set identical to `{name}`'s | seeds with PRIMARY-P bit-identical to `{name}`'s |"
        )?;
        writeln!(out, "|---|---:|---:|")?;
        for cell in groups {
            let a = against(cell, reference);
            writeln!(
                out,
                "| `{}` | {} | {} of {} |",
                cell.label,
                share(a.same_tasks, a.tasks),
                a.same_seeds,
                a.seeds
            )?;
        }
        writeln!(out)?;
    }
    Ok(())
}

/// `all` is every cell in table order.
fn render_slots(out: &mut String, all: &[&Cell], instances: &[WorkflowInstance]) -> fmt::Result {
    writeln!(out, "## Non-performance among final members (disclosure)")?;
    writeln!(out)?;
    writeln!(
        out,
        "| cell | final member-slots | did not perform | slots covering ≥ 1 demanded step | of those, did not perform |"
    )?;
    writeln!(out, "|---|---:|---:|---:|---:|")?;
    for cell in all {
        let s = cell_slots(cell, instances);
        writeln!(
            out,
            "| `{}` | {} | {} | {} | {} |",
            cell.label,
            s.all,
            share(s.all_failed, s.all),
            s.covering,
            share(s.covering_failed, s.covering)
        )?;
    }
    writeln!(out)
}

/// The *after a failure* sums of one cell. An agent's first observed
/// non-performance splits the pool: `failed_*` are over the agents with one and
/// the tasks after it, `clean_*` over the agents with none and the tasks after
/// their first observation.
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
struct AfterFailure {
    /// Agents with at least one observed non-performance as a final member.
    failed_agents: usize,
    /// Of `failed_agents`, those with at least one later task.
    failed_with_later: usize,
    /// Tasks after the first observed non-performance, summed over
    /// `failed_agents`.
    failed_later: usize,
    /// Of `failed_later`, those the agent ends as a final member.
    failed_later_member: usize,
    /// Of `failed_with_later`, the agents never a final member afterwards.
    never_again: usize,
    /// Agents observed at least once and never as non-performing.
    clean_agents: usize,
    /// Tasks after the first observation, summed over `clean_agents`.
    clean_later: usize,
    /// Of `clean_later`, those the agent ends as a final member.
    clean_later_member: usize,
    /// Agents never observed.
    unobserved_agents: usize,
}

impl AfterFailure {
    fn add(&mut self, other: &Self) {
        self.failed_agents += other.failed_agents;
        self.failed_with_later += other.failed_with_later;
        self.failed_later += other.failed_later;
        self.failed_later_member += other.failed_later_member;
        self.never_again += other.never_again;
        self.clean_agents += other.clean_agents;
        self.clean_later += other.clean_later;
        self.clean_later_member += other.clean_later_member;
        self.unobserved_agents += other.unobserved_agents;
    }
}

/// The [`AfterFailure`] sums of `recon` over `inst`. An agent is observed on a
/// task it ends as a final member — with `evidence_only`, only under a non-zero
/// [`evidence_mask`], the set `grp-id`'s agent model and `ref-prune-id`'s record
/// observe.
fn after_failure(
    inst: &WorkflowInstance,
    recon: &Recon,
    n_roles: usize,
    n_bits: usize,
    evidence_only: bool,
) -> AfterFailure {
    let mut out = AfterFailure::default();
    let n_tasks = inst.tasks.len().min(recon.tasks.len());
    for agent in 0..inst.agents.len() {
        let member = |t: usize| recon.tasks[t].members.contains(&agent);
        let observed = |t: usize| {
            member(t)
                && (!evidence_only
                    || evidence_mask(
                        inst,
                        &required_by_role(&inst.tasks[t], n_roles, n_bits),
                        agent,
                    ) != 0)
        };
        let first_failure = (0..n_tasks).find(|&t| observed(t) && !performed_on(inst, t, agent));
        let first_observed = (0..n_tasks).find(|&t| observed(t));
        let later_member = |from: usize| (from + 1..n_tasks).filter(|&t| member(t)).count();
        match (first_failure, first_observed) {
            (Some(t0), _) => {
                let later = n_tasks - t0 - 1;
                let kept = later_member(t0);
                out.failed_agents += 1;
                out.failed_later += later;
                out.failed_later_member += kept;
                if later > 0 {
                    out.failed_with_later += 1;
                    out.never_again += usize::from(kept == 0);
                }
            }
            (None, Some(t1)) => {
                out.clean_agents += 1;
                out.clean_later += n_tasks - t1 - 1;
                out.clean_later_member += later_member(t1);
            }
            (None, None) => out.unobserved_agents += 1,
        }
    }
    out
}

/// [`after_failure`] summed over `cell`'s replayed seeds.
fn cell_after_failure(
    cell: &Cell,
    instances: &[WorkflowInstance],
    evidence_only: bool,
) -> AfterFailure {
    let config = cell_config(GRP_TOPO);
    let mut out = AfterFailure::default();
    for (run, inst) in cell.runs.iter().zip(instances) {
        if let Some(recon) = run.recon() {
            out.add(&after_failure(
                inst,
                recon,
                config.n_roles,
                config.base.n_bits,
                evidence_only,
            ));
        }
    }
    out
}

/// `all` is every cell in table order.
fn render_after_failure(
    out: &mut String,
    all: &[&Cell],
    instances: &[WorkflowInstance],
) -> fmt::Result {
    writeln!(out, "## After a failure (disclosure; the named risk)")?;
    writeln!(out)?;
    writeln!(
        out,
        "Per (seed, agent). *Observed*: the agent ends the task a final member with `required_r(own role) & capabilities != 0` — the set `grp-id`'s agent model and `ref-prune-id`'s record observe. *Later tasks*: the tasks after the agent's first observed non-performance; for an agent with none, the tasks after its first observation. The second table counts every task the agent ends as a final member as observed."
    )?;
    writeln!(out)?;
    for (evidence_only, title) in [
        (
            true,
            "observed = final member under a non-zero evidence mask",
        ),
        (false, "observed = final member"),
    ] {
        writeln!(out, "### {title}")?;
        writeln!(out)?;
        writeln!(
            out,
            "| cell | agents with ≥ 1 observed non-performance | later tasks ending as final member | agents with none | later tasks ending as final member | never a final member again (of agents with ≥ 1 later task) | agents never observed |"
        )?;
        writeln!(out, "|---|---:|---:|---:|---:|---:|---:|")?;
        for cell in all {
            let a = cell_after_failure(cell, instances, evidence_only);
            writeln!(
                out,
                "| `{}` | {} | {} | {} | {} | {} of {} | {} |",
                cell.label,
                a.failed_agents,
                share(a.failed_later_member, a.failed_later),
                a.clean_agents,
                share(a.clean_later_member, a.clean_later),
                a.never_again,
                a.failed_with_later,
                a.unobserved_agents
            )?;
        }
        writeln!(out)?;
    }
    Ok(())
}

fn render_live(out: &mut String, groups: &[Cell]) -> fmt::Result {
    let (id, topo) = (&groups[GRP_ID], &groups[GRP_TOPO]);
    let per_seed = seed_divergences(id, topo);
    writeln!(
        out,
        "## Live — `{}` against `{}` (disclosure; compared by position up to the shorter trace)",
        id.label, topo.label
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "The ACT and score-bit counts are positional; an upper bound after the first divergence. *Seeds with any difference* is uncontaminated."
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "- seeds with ≥ 1 raw-score-bit difference: {}/{n}; seeds with ≥ 1 act difference: {}/{n}",
        per_seed.iter().filter(|d| d.score_bits > 0).count(),
        per_seed.iter().filter(|d| d.act > 0).count(),
        n = per_seed.len()
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "| seed | decisions differing by ACT | decisions differing by raw score bits | length difference | position of the first act difference | task index | read kind | cell that acted |"
    )?;
    writeln!(out, "|---:|---:|---:|---:|---:|---:|---|---|")?;
    for d in &per_seed {
        match d.first {
            Some(f) => writeln!(
                out,
                "| {} | {} | {} | {} | {} | {} | {} | `{}` |",
                d.seed,
                d.act,
                d.score_bits,
                d.length,
                f.position,
                f.task.map_or_else(|| "—".to_owned(), |t| t.to_string()),
                if f.leave { "leave" } else { "join" },
                if f.first_acted { id.label } else { topo.label }
            )?,
            None => writeln!(
                out,
                "| {} | {} | {} | {} | — | — | — | — |",
                d.seed, d.act, d.score_bits, d.length
            )?,
        }
    }
    writeln!(out)?;
    let pool = first_pool(&per_seed);
    let mut buckets = Vec::new();
    for (leave, kind) in [(false, "join"), (true, "leave")] {
        for (first_acted, cell) in [(true, id.label), (false, topo.label)] {
            buckets.push(format!(
                "{kind} read, `{cell}` acted: {}",
                pool.counts[usize::from(leave)][usize::from(first_acted)]
            ));
        }
    }
    buckets.push(format!("no act difference: {}", pool.none));
    writeln!(out, "- pooled first divergence: {}", buckets.join(" · "))?;
    let by_task = if pool.by_task.is_empty() {
        "none".to_owned()
    } else {
        pool.by_task
            .iter()
            .map(|(task, seeds)| format!("task {task}: {seeds}"))
            .collect::<Vec<_>>()
            .join(" · ")
    };
    writeln!(
        out,
        "- pooled, seeds by the task index of the first act difference: {by_task}"
    )?;
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

fn render_follow(out: &mut String, groups: &[Cell]) -> fmt::Result {
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
    for cell in groups {
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

/// Sums over the task ends of one realised roster size, or of every task, on
/// the performance-scored fields.
#[derive(Default, Debug, PartialEq, Clone)]
struct ScoredRow {
    tasks: usize,
    successes: usize,
    /// Sum of `performed / steps`, `0` for an empty demand.
    fraction_sum: f64,
    /// Sum of `TaskEnd::performed_cov_eff`.
    eff_sum: f64,
    final_members: usize,
    empty: usize,
    off_demand: usize,
}

/// `max_roster + 1` rows: row `0` over every task end, row `r` over those of
/// realised roster `r`.
fn scored_rows<'a>(ends: impl Iterator<Item = &'a TaskEnd>, max_roster: usize) -> Vec<ScoredRow> {
    let mut rows = vec![ScoredRow::default(); max_roster + 1];
    for end in ends {
        let own = (end.roster > 0).then_some(end.roster);
        for slot in std::iter::once(0).chain(own) {
            let Some(row) = rows.get_mut(slot) else {
                continue;
            };
            row.tasks += 1;
            row.successes += usize::from(end.performed_success() == Some(true));
            row.fraction_sum += match end.performed {
                Some(performed) if end.steps > 0 => performed as f64 / end.steps as f64,
                _ => 0.0,
            };
            row.eff_sum += end.performed_cov_eff().unwrap_or(0.0);
            row.final_members += end.members.len();
            row.empty += usize::from(end.members.is_empty());
            row.off_demand += end.off_demand;
        }
    }
    rows
}

/// `all` is every cell in table order; `roles` bounds the realised roster.
fn render_decomposition(out: &mut String, all: &[&Cell], roles: u8) -> fmt::Result {
    writeln!(
        out,
        "## Outcome decomposition by realised roster (performance-scored; from the reconstruction)"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "| cell | roster | tasks | success rate (P) | mean performed fraction | mean efficiency (P) | mean final size | tasks ending empty | final members whose role has no demand |"
    )?;
    writeln!(out, "|---|---|---:|---:|---:|---:|---:|---:|---:|")?;
    for cell in all {
        for (r, row) in scored_rows(cell.ends(), usize::from(roles))
            .iter()
            .enumerate()
        {
            let per_task = |sum: f64| (row.tasks > 0).then(|| sum / row.tasks as f64);
            writeln!(
                out,
                "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} of {} |",
                cell.label,
                if r == 0 {
                    "all".to_owned()
                } else {
                    r.to_string()
                },
                row.tasks,
                pct(row.successes, row.tasks),
                or_na(per_task(row.fraction_sum), 4),
                or_na(per_task(row.eff_sum), 4),
                or_na(per_task(row.final_members as f64), 2),
                row.empty,
                row.off_demand,
                row.final_members
            )?;
        }
    }
    writeln!(out)
}

/// `all` is every cell in table order.
fn render_task_split(out: &mut String, all: &[&Cell]) -> fmt::Result {
    writeln!(
        out,
        "## Task 0 against tasks ≥ 1 (performance-scored; no task has been observed before task 0)"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "| cell | tasks | join-act rate | leave-act rate | success rate (P) | mean efficiency (P) | mean final size |"
    )?;
    writeln!(out, "|---|---|---:|---:|---:|---:|---:|")?;
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
                "| `{}` | {name} | {} | {} | {} | {} | {} |",
                cell.label,
                rate(false),
                rate(true),
                pct(
                    ends()
                        .filter(|e| e.performed_success() == Some(true))
                        .count(),
                    ends().count()
                ),
                or_na(
                    mean(ends().map(|e| e.performed_cov_eff().unwrap_or(0.0))),
                    4
                ),
                or_na(mean(ends().map(|e| e.members.len() as f64)), 2)
            )?;
        }
    }
    writeln!(out)
}

/// `(leave, roster, centre vote, blind act votes)` of one read.
type ReadKey = (bool, usize, Option<usize>, usize);

fn render_votes(out: &mut String, groups: &[Cell]) -> fmt::Result {
    writeln!(
        out,
        "## Votes (group cells, from the `AgreementSample` ledger)"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "| cell | join act: centre-present | join act: centre-absent | leave act: centre-present | leave act: centre-absent | churn: centre-present | churn: centre-absent | churn (harness) |"
    )?;
    writeln!(out, "|---|---:|---:|---:|---:|---:|---:|---:|")?;
    for cell in groups {
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
    for (i, cell) in groups.iter().enumerate() {
        for s in cell.samples() {
            let row = table
                .entry((s.leave, s.roster, s.centre_vote, s.votes_for_act_blind))
                .or_insert_with(|| vec![(0, 0); groups.len()]);
            row[i].0 += usize::from(s.group_act);
            row[i].1 += 1;
        }
    }
    let labels: Vec<String> = groups.iter().map(|c| format!("`{}`", c.label)).collect();
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

/// One group cell's row set of the four-class table: `(label, seeds checked,
/// counts, mismatching seeds)`.
type CentreRow<'a> = (&'a str, usize, CentreCounts, Vec<Mismatch>);

/// `"<class>: <share>"` for the two identical-mask classes of `counts`' reads
/// of kind `leave`.
fn identical_row_text(counts: &CentreCounts, leave: bool) -> String {
    let cell = |window: bool| {
        let (acts, reads) = counts[usize::from(leave)][1][usize::from(window)];
        share(acts, reads)
    };
    format!(
        "masks identical, window holds a failure: {}; masks identical, window holds none: {}",
        cell(true),
        cell(false)
    )
}

/// A cell with a mismatching seed gets no table row.
fn render_centre(out: &mut String, rows: &[CentreRow<'_>]) -> fmt::Result {
    writeln!(
        out,
        "## Centre-present reads by what the centre's query sees (disclosure)"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "Computed in this binary from the instance and the replayed trace. *Masks identical*: `cfg0 & required_r == cfg1 & required_r` for the candidate's role `r`, the masks as `coverage_masks` builds them under `RoleMatched`. *Window*: either of the last two observations of the model the centre queries holds a failure on a bit `r` requires now — on `grp-id` the candidate's agent model (its evidence mask read as its `performed`), on `grp-topo` and `grp-topo-cov` role `r`'s model (its required bits against the cell's per-bit signal) — a world-side stand-in for the engine's two-task replay window, not a read of it. Cells are group acts of reads."
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
        "| cell | read | masks identical, window holds a failure | masks identical, window holds none | masks differ, window holds a failure | masks differ, window holds none |"
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

fn render_reach(out: &mut String, groups: &[Cell]) -> fmt::Result {
    writeln!(out, "## Candidate reach")?;
    writeln!(out)?;
    writeln!(
        out,
        "| cell | zero candidate-sensitive | mean blind internals | blind CW weight share (by member) | blind CW weight share (by origin) | leave `cfg0 == cfg1` | act rate: sensitive / blind |"
    )?;
    writeln!(out, "|---|---:|---:|---:|---:|---:|---|")?;
    for cell in groups {
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
    for cell in groups {
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

/// What the reading and the scoped clauses are rendered from.
struct Findings<'a> {
    criterion: &'a ThreeWay,
    secondary: &'a Secondary,
    groups: &'a [Cell],
    /// `grp-id`'s four-class counts; `None` when its integrity check withholds
    /// them.
    id_centre: Option<CentreCounts>,
    /// `(cell label, its evidence-mask *after a failure* sums)` for `grp-id`
    /// and `ref-prune-id`.
    risk: Vec<(&'static str, AfterFailure)>,
}

/// §6's reading of `label` and its six scoped clauses.
fn render_one_reading(out: &mut String, label: &str, f: &Findings<'_>) -> fmt::Result {
    let c = f.criterion;
    let reading = match label {
        LABEL_INVALID => "A gate failed; the criterion is not read.".to_owned(),
        LABEL_VALIDATED => format!(
            "H-beats passes: `{}` over `{}` at {}, strictly superior on {}/{} seeds.",
            c.beats.better,
            c.beats.other,
            ratio_text(c.beats.better_median, c.beats.other_median),
            c.beats.superior,
            c.beats.seeds
        ),
        LABEL_EQUIV => format!(
            "H-equiv passes (final member set identical on {} of {} tasks, PRIMARY-P bit-identical on {}/{} seeds). *\"told who performed, the routed arm still ends where a learning-free arrival-order prune ends.\"*",
            c.found.same_tasks, c.found.tasks, c.found.same_seeds, c.found.seeds
        ),
        LABEL_BELOW => format!(
            "H-below passes: `{}` over `{}` at {}, strictly superior on {}/{} seeds.",
            c.below.better,
            c.below.other,
            ratio_text(c.below.better_median, c.below.other_median),
            c.below.superior,
            c.below.seeds
        ),
        _ => format!(
            "None of the three passes. Ratio of medians `{a}` / `{b}`: {} ({:.4} / {:.4}); `{a}` strictly superior on {}/{n} seeds, `{b}` strictly superior on {}/{n} seeds; final member set identical on {} of {} tasks.",
            ratio_text(c.beats.better_median, c.beats.other_median),
            c.beats.better_median,
            c.beats.other_median,
            c.beats.superior,
            c.below.superior,
            c.found.same_tasks,
            c.found.tasks,
            a = c.a,
            b = c.b,
            n = c.found.seeds
        ),
    };
    writeln!(out, "`{label}` — {reading}")?;
    writeln!(out)?;

    // Clause 1.
    let read1 = &f.secondary.id_vs_prune_id;
    let with_it = &f.secondary.prune_id_vs_prune;
    if label == LABEL_INVALID {
        writeln!(out, "1. **Whose gain.** Not read: a gate failed.")?;
    } else if label == LABEL_VALIDATED {
        write!(
            out,
            "1. **Whose gain.** Secondary read 1: `{}` against `{}` passes {}; `{}` against `{}` passes {}.",
            read1.a,
            read1.b,
            read1.legs_passed(),
            with_it.a,
            with_it.b,
            with_it.legs_passed()
        )?;
        if with_it.beats.pass() && !read1.beats.pass() {
            write!(
                out,
                " *\"an engine-free rule reading the same identity signal reaches the bar against the prune; the arm does not reach it against that rule.\"*"
            )?;
        }
        writeln!(out)?;
    } else {
        writeln!(
            out,
            "1. **Whose gain.** Does not apply: the label is not `{LABEL_VALIDATED}`."
        )?;
    }

    // Clause 2.
    if label == LABEL_INVALID {
        writeln!(
            out,
            "2. **Was the signal usable.** Not read: a gate failed."
        )?;
    } else if label == LABEL_VALIDATED {
        writeln!(
            out,
            "2. **Was the signal usable.** Does not apply: the label is `{LABEL_VALIDATED}`."
        )?;
    } else {
        writeln!(
            out,
            "2. **Was the signal usable.** `{}` against `{}` in the three-way form passes {}.",
            with_it.a,
            with_it.b,
            with_it.legs_passed()
        )?;
    }

    // Clause 3.
    let zero: Vec<String> = std::iter::once(c)
        .chain(f.secondary.all())
        .flat_map(ThreeWay::zero_median_sentences)
        .collect();
    if zero.is_empty() {
        writeln!(
            out,
            "3. **Zero medians.** No ratio conjunct of the criterion or of a secondary read has a divisor whose median is 0."
        )?;
    } else {
        writeln!(out, "3. **Zero medians.** {}", zero.join(" "))?;
    }

    // Clause 4.
    let mut premise = Vec::new();
    let mut departed = String::new();
    for (i, cell) in f.groups.iter().enumerate() {
        let (reads, differing) = centre_departures(cell);
        premise.push(format!(
            "`{}` {}",
            cell.label,
            share(reads - differing, reads)
        ));
        if i == GRP_ID && differing > 0 {
            departed = format!(
                " *\"on `grp-id` the group's act differed from the centre's argmax on {differing} of {reads} centre-present reads; on those reads the agent-keyed query did not decide.\"*"
            );
        }
    }
    writeln!(
        out,
        "4. **The premise.** E-follow over all centre-present reads: {}.{departed}",
        premise.join(" · ")
    )?;

    // Clause 5.
    let note = match label {
        LABEL_INVALID => "not read: a gate failed",
        LABEL_VALIDATED => "**contradicted**",
        _ => "**confirmed**",
    };
    let quoted = f.id_centre.map_or_else(
        || "withheld by the integrity check".to_owned(),
        |counts| identical_row_text(&counts, true),
    );
    writeln!(
        out,
        "5. **§4's note** is {note}. `grp-id`'s identical-mask leave row of the four-class table: {quoted}."
    )?;

    // Clause 6.
    let risk: Vec<String> = f
        .risk
        .iter()
        .map(|(cell, a)| {
            format!(
                "`{cell}` — agents with ≥ 1 observed non-performance end as final members on {} of their later tasks, agents with none on {}; {} of {} agents with a later task are never a final member after their first observed non-performance",
                share(a.failed_later_member, a.failed_later),
                share(a.clean_later_member, a.clean_later),
                a.never_again,
                a.failed_with_later
            )
        })
        .collect();
    writeln!(out, "6. **The named risk.** {}.", risk.join("; "))?;
    writeln!(out)
}

/// The registered block renders the reading of `label`. A smoke block renders
/// the reading of every label, whatever the criterion found.
fn render_reading(out: &mut String, label: &str, f: &Findings<'_>, smoke: bool) -> fmt::Result {
    writeln!(
        out,
        "## Reading and scoped clauses (prereg §6, the clauses reported under every verdict)"
    )?;
    writeln!(out)?;
    if smoke {
        writeln!(
            out,
            "Smoke block: every label's reading is rendered and none is a verdict."
        )?;
        writeln!(out)?;
        for label in [
            LABEL_INVALID,
            LABEL_VALIDATED,
            LABEL_EQUIV,
            LABEL_BELOW,
            LABEL_NO_EFFECT,
        ] {
            render_one_reading(out, label, f)?;
        }
    } else {
        render_one_reading(out, label, f)?;
    }
    writeln!(
        out,
        "Continuation: none is pre-committed; the next lock reads this report first."
    )?;
    writeln!(out)
}

/// What one execution writes to stdout after the header: `gate_lines`, then
/// `report` on the registered block and nothing under `K7_3_SEEDS`, then the
/// `VERDICT:` line.
struct Rendered {
    gate_lines: String,
    report: String,
    verdict: &'static str,
}

impl Rendered {
    fn stdout(&self, smoke: bool) -> String {
        let body = if smoke { "" } else { &self.report };
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
    let run_groups = |seed_xor: u64| -> Result<Vec<Cell>, Box<dyn Error>> {
        (0..GROUP_CELLS.len())
            .map(|i| run_group(i, instances, seed_xor))
            .collect()
    };
    let groups = run_groups(0)?;
    let reruns = run_groups(0)?;
    let reseeded = INVARIANCE_CELLS
        .iter()
        .map(|&i| run_group(i, instances, INVARIANCE_XOR))
        .collect::<Result<Vec<_>, _>>()?;
    let references = (0..REFERENCE_CELLS.len())
        .map(|which| run_reference(which, instances))
        .collect::<Result<Vec<_>, _>>()?;
    let prune_id_rerun = run_reference(REF_PRUNE_ID, instances)?;
    // The prereg's table order.
    let all: Vec<&Cell> = vec![
        &groups[GRP_ID],
        &references[REF_PRUNE],
        &references[REF_PRUNE_ID],
        &groups[GRP_TOPO],
        &groups[GRP_TOPO_COV],
        &references[REF_KEEP],
        &references[REF_FIRST],
    ];

    let determinism: Vec<(&Cell, &Cell)> = groups
        .iter()
        .zip(&reruns)
        .chain(std::iter::once((
            &references[REF_PRUNE_ID],
            &prune_id_rerun,
        )))
        .collect();
    let gates = [
        gate_id0(&[
            (&groups[GRP_ID], &groups[GRP_TOPO]),
            (&references[REF_PRUNE_ID], &references[REF_PRUNE]),
        ]),
        gate_recon(&all),
        gate_determinism(&determinism),
        gate_invariance(&groups, &reseeded),
        gate_learn(&groups),
        gate_learn_id(&groups[GRP_ID], instances),
        gate_route(&groups),
        gate_draw(&references[REF_KEEP], instances),
    ];
    let gates_ok = gates.iter().all(Gate::pass);
    let mut gate_lines = String::new();
    render_gate_lines(&mut gate_lines, &gates)?;

    let mut report = String::new();
    render_gate_detail(&mut report, &gates)?;
    render_arms(&mut report, &all)?;

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

    let criterion = ThreeWay::of(&groups[GRP_ID], &references[REF_PRUNE]);
    let secondary = Secondary {
        id_vs_prune_id: ThreeWay::of(&groups[GRP_ID], &references[REF_PRUNE_ID]),
        prune_id_vs_prune: ThreeWay::of(&references[REF_PRUNE_ID], &references[REF_PRUNE]),
        topo_vs_prune: ThreeWay::of(&groups[GRP_TOPO], &references[REF_PRUNE]),
        topo_vs_cov: ThreeWay::of(&groups[GRP_TOPO], &groups[GRP_TOPO_COV]),
    };
    let label = if gates_ok {
        criterion.label()
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

    render_criterion(&mut report, &criterion, &secondary)?;
    render_against(&mut report, &groups, &references)?;
    render_slots(&mut report, &all, instances)?;
    render_after_failure(&mut report, &all, instances)?;
    let centre: Vec<CentreRow<'_>> = groups
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
    render_live(&mut report, &groups)?;
    render_follow(&mut report, &groups)?;
    render_votes(&mut report, &groups)?;
    render_reach(&mut report, &groups)?;
    render_decomposition(&mut report, &all, spec.roles)?;
    render_task_split(&mut report, &all)?;
    let findings = Findings {
        criterion: &criterion,
        secondary: &secondary,
        groups: &groups,
        id_centre: centre
            .get(GRP_ID)
            .filter(|(_, _, _, bad)| bad.is_empty())
            .map(|&(_, _, counts, _)| counts),
        risk: vec![
            (
                groups[GRP_ID].label,
                cell_after_failure(&groups[GRP_ID], instances, true),
            ),
            (
                references[REF_PRUNE_ID].label,
                cell_after_failure(&references[REF_PRUNE_ID], instances, true),
            ),
        ],
    };
    render_reading(&mut report, label, &findings, smoke)?;

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
            eprintln!("k7_3: {refusal}");
            return ExitCode::from(2);
        }
    };
    let spec = world();
    let mut header = String::new();
    if render_header(&mut header, &spec, seeds, smoke).is_err() {
        eprintln!("k7_3: the header failed to render before any cell ran");
        return ExitCode::FAILURE;
    }
    print!("{header}");
    if std::io::stdout().flush().is_err() {
        eprintln!("k7_3: stdout failed to flush before any cell ran");
        return ExitCode::FAILURE;
    }
    let instances = match seeds
        .iter()
        .map(|seed| spec.generate(seed))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(instances) => instances,
        Err(e) => {
            if smoke {
                eprintln!(
                    "k7_3: instance generation failed before any cell ran; the error text is not printed under {SEEDS_ENV}"
                );
            } else {
                eprintln!("k7_3: instance generation failed before any cell ran: {e}");
            }
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
                    "k7_3: a cell failed to run; the error text is not printed under {SEEDS_ENV}"
                );
            } else {
                eprintln!("k7_3: a cell failed to run: {e}");
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
            for version in ["0.42.0", OFFICIAL_VERSION] {
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
        assert_eq!(
            (REGISTERED_SEEDS.start, REGISTERED_SEEDS.end),
            (540, 570),
            "the registered block"
        );
        assert_eq!(OFFICIAL_VERSION, "0.43.0");
    }

    #[test]
    fn seed_block_refuses_every_other_environment() {
        let refused: [&[(&str, &str)]; 9] = [
            &[],
            &[(OFFICIAL_ENV, "1"), (SEEDS_ENV, "8000..8003")],
            &[(SEEDS_ENV, "540..570")],
            &[(SEEDS_ENV, "8000..8004")],
            &[(SEEDS_ENV, "8001..8003")],
            &[(SEEDS_ENV, "6000..6003")],
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
        for version in ["0.42.0", "0.43.1", "0.40.0", ""] {
            let outcome = seed_block(&vars(&[(OFFICIAL_ENV, "1")]), version);
            assert!(outcome.is_err(), "official at {version:?}: {outcome:?}");
        }
        // A foreign `K7_` name is refused by name, beside an accepted variable
        // and alone.
        let foreign: [(&[(&str, &str)], &str); 5] = [
            (&[("K7_3_SEED", "8000..8003")], "`K7_3_SEED`"),
            (
                &[("K7_2_SEEDS", "6000..6003"), (SEEDS_ENV, "8000..8003")],
                "`K7_2_SEEDS`",
            ),
            (
                &[("K7_2_OFFICIAL", "1"), (OFFICIAL_ENV, "1")],
                "`K7_2_OFFICIAL`",
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

    #[test]
    fn the_registered_world_and_cells_are_the_prereg_s() {
        let spec = world();
        assert_eq!(
            spec.performance,
            Some(PerformanceSpec {
                reliable_prob: 0.7,
                rho_reliable: 0.05,
                rho_flaky: 0.40,
            })
        );
        assert_eq!(
            WorkflowSpec {
                performance: None,
                ..spec
            },
            WorkflowSpec::default()
        );
        let cells: Vec<(&str, WorldModelTopology, OutcomeSignal)> = GROUP_CELLS
            .iter()
            .map(|c| (c.label, c.topology, c.signal))
            .collect();
        assert_eq!(
            cells,
            vec![
                (
                    "grp-id",
                    WorldModelTopology::AgentKeyed,
                    OutcomeSignal::Both
                ),
                (
                    "grp-topo",
                    WorldModelTopology::RoleSpecialised,
                    OutcomeSignal::Both
                ),
                (
                    "grp-topo-cov",
                    WorldModelTopology::RoleSpecialised,
                    OutcomeSignal::RoleCoverage
                ),
            ]
        );
        for cell in 0..GROUP_CELLS.len() {
            let config = cell_config(cell);
            assert_eq!(config.channel, PrecisionChannel::RoleRestricted);
            assert_eq!(config.masks, CoverageMasks::RoleMatched);
            assert_eq!(config.read, DecisionRead::Deterministic);
            assert_eq!(config.vote, GroupVote::CertaintyWeighted);
            assert_eq!(config.routing, VoteRouting::CandidateStar { lambda: 0.5 });
            assert_eq!(config.n_roles, 3);
            assert!(config.base.query_novelty, "novelty on");
        }
    }

    #[test]
    fn the_thresholds_round_up_at_every_block_size() {
        assert_eq!(at_least(600, EQUIV_TASKS), 570);
        assert_eq!(at_least(30, EQUIV_SEEDS), 24);
        assert_eq!(at_least(30, BAR_SEEDS), 18);
        assert_eq!(at_least(60, EQUIV_TASKS), 57);
        assert_eq!(at_least(3, EQUIV_SEEDS), 3);
        assert_eq!(at_least(3, BAR_SEEDS), 2);
    }

    fn found(same_tasks: usize, same_seeds: usize) -> Against {
        Against {
            same_tasks,
            tasks: 100,
            same_seeds,
            seeds: 5,
        }
    }

    #[test]
    fn the_three_way_form_labels_each_leg_in_precedence_order() {
        let flat = [0.2; 5];
        // H-beats: medians 0.5 / 0.2, superior on 4 of 5 (bar 3).
        let beats = ThreeWay::new("a", "b", found(10, 0), &[0.5, 0.5, 0.5, 0.5, 0.1], &flat);
        assert!(beats.beats.pass() && !beats.equiv() && !beats.below.pass());
        assert_eq!(beats.label(), LABEL_VALIDATED);
        assert_eq!(beats.legs_passed(), "H-beats");
        // The ratio at the bar exactly: 0.625 / 0.5.
        let at_bar = ThreeWay::new("a", "b", found(0, 0), &[0.625; 5], &[0.5; 5]);
        assert!(at_bar.beats.pass(), "{at_bar:?}");
        // The ratio holds and the count does not: superior on 2 of 5.
        let two = ThreeWay::new(
            "a",
            "b",
            found(0, 0),
            &[0.9, 0.9, 0.3, 0.1, 0.1],
            &[0.2, 0.2, 0.31, 0.2, 0.2],
        );
        assert!(two.beats.ratio_ok() && !two.beats.superior_ok(), "{two:?}");
        assert_eq!(two.label(), LABEL_NO_EFFECT);
        // H-equiv at its two bars, 95 of 100 tasks and 4 of 5 seeds.
        let equiv = ThreeWay::new("a", "b", found(95, 4), &flat, &flat);
        assert_eq!(equiv.label(), LABEL_EQUIV);
        assert_eq!(equiv.legs_passed(), "H-equiv");
        for short in [found(94, 5), found(100, 3)] {
            let tw = ThreeWay::new("a", "b", short, &flat, &flat);
            assert_eq!(tw.label(), LABEL_NO_EFFECT, "{short:?}");
        }
        // H-beats outranks H-equiv.
        let both = ThreeWay::new("a", "b", found(100, 5), &[0.5, 0.5, 0.5, 0.5, 0.1], &flat);
        assert_eq!(both.label(), LABEL_VALIDATED);
        // H-below is H-beats read the other way.
        let below = ThreeWay::new("a", "b", found(10, 0), &flat, &[0.5, 0.5, 0.5, 0.5, 0.1]);
        assert!(below.below.pass() && !below.beats.pass());
        assert_eq!(below.label(), LABEL_BELOW);
        // Ratio 1.2 with every seed superior: no leg.
        let none = ThreeWay::new(
            "a",
            "b",
            found(10, 0),
            &[0.3, 0.3, 0.3, 0.3, 0.3],
            &[0.25, 0.25, 0.25, 0.25, 0.25],
        );
        assert_eq!(none.label(), LABEL_NO_EFFECT);
        assert_eq!(none.legs_passed(), "none of the three legs");
        assert!(none.zero_median_sentences().is_empty());
    }

    #[test]
    fn a_zero_divisor_median_fails_the_leg_and_is_reported_by_clause_three() {
        let tw = ThreeWay::new(
            "a",
            "b",
            found(10, 0),
            &[0.5, 0.5, 0.5, 0.0, 0.0],
            &[0.0, 0.0, 0.0, 0.1, 0.1],
        );
        assert!(!tw.beats.divisor_ok() && tw.beats.superior_ok());
        assert!(!tw.beats.pass());
        assert_eq!(tw.label(), LABEL_NO_EFFECT);
        assert_eq!(
            tw.zero_median_sentences(),
            vec![
                "*\"`b`'s median PRIMARY-P is 0; the ratio is undefined; `a` is strictly superior on 3/5 seeds.\"*"
                    .to_owned()
            ]
        );
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

    /// A task demanding the steps of `legs = [(role index, bits)]` over an
    /// eight-bit universe, arriving in `arrival` order.
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
        let mut tags = vec![Role::new(0); 8];
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
    /// `(0, r0)`, both with arrival 1, 0; task 4 `(0, r0)`, `(2, r1)` with
    /// arrival 2, 0, 3. `performance[t][agent]` is `rows`.
    fn five_task_instance(rows: Option<[[bool; 4]; 5]>) -> WorkflowInstance {
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
            task(&[(0, &[0]), (1, &[2])], &[2, 0, 3]),
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
            performance: rows.map(|rows| rows.iter().map(|row| row.to_vec()).collect()),
        }
    }

    /// Task 0: agents 1 and 3 did not perform. Task 1: agent 3. Task 2:
    /// nobody. Task 3: agent 0. Task 4: agent 2.
    const ROWS: [[bool; 4]; 5] = [
        [true, false, true, false],
        [true, true, true, false],
        [true, true, true, true],
        [false, true, true, true],
        [true, true, false, true],
    ];

    fn entry(leave: bool, act: bool) -> TraceEntry {
        TraceEntry {
            leave,
            act,
            score_bits: 0,
        }
    }

    /// Task 0: agents 1, 2, 3 join, then agents 0 and 2 leave — ends `{1, 3}`.
    /// Task 1: agent 2 does not join, agents 0 and 1 do, then agent 0 leaves —
    /// `{3, 1}`. Task 2: agent 0 joins and leaves — `{1}`. Task 3: agent 0
    /// joins, nobody leaves — `{1, 0}`. Task 4: agent 0 joins, agent 3 does
    /// not, nobody leaves — `{2, 0}`.
    fn five_task_trace() -> Vec<TraceEntry> {
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
            entry(join, true),
            entry(join, false),
            entry(leave, false),
            entry(leave, false),
        ]
    }

    /// `(task, leave, act, masks identical)` per read of [`five_task_trace`],
    /// the last `None` on a centre-absent read. Masks are `(cfg0, cfg1)` of
    /// the candidate's role; `req` is that role's required bits on the task.
    fn base_reads() -> Vec<(usize, bool, bool, Option<bool>)> {
        let (join, leave) = (false, true);
        vec![
            // Task 0, req r0 = 0b011, r1 = 0b100, r2 off the roster.
            (0, join, true, Some(true)),    // 0, agent 1: (0b011, 0b011)
            (0, join, true, Some(true)),    // 1, agent 2: (0b100, 0b100)
            (0, join, true, None),          // 2, agent 3: r2
            (0, leave, true, Some(true)),   // 3, agent 0: (0b011, 0b011)
            (0, leave, false, Some(false)), // 4, agent 1: (0b011, 0)
            (0, leave, true, Some(false)),  // 5, agent 2: (0b100, 0)
            (0, leave, false, None),        // 6, agent 3: r2
            // Task 1, req r0 = 0b010, r2 = 0b100, r1 off the roster.
            (1, join, false, None),         // 7, agent 2: r1
            (1, join, true, Some(true)),    // 8, agent 0: (0b001, 0b001)
            (1, join, true, Some(true)),    // 9, agent 1: (0b011, 0b011)
            (1, leave, false, Some(false)), // 10, agent 3: (0b100, 0)
            (1, leave, true, Some(true)),   // 11, agent 0: (0b011, 0b011)
            (1, leave, false, Some(false)), // 12, agent 1: (0b011, 0)
            // Task 2, req r0 = 0b010.
            (2, join, true, Some(false)), // 13, agent 0: (0b001, 0b011)
            (2, leave, false, Some(false)), // 14, agent 1: (0b011, 0b001)
            (2, leave, true, Some(true)), // 15, agent 0: (0b011, 0b011)
            // Task 3, req r0 = 0b001.
            (3, join, true, Some(true)),   // 16, agent 0: (0b001, 0b011)
            (3, leave, false, Some(true)), // 17, agent 1: (0b011, 0b001)
            (3, leave, false, Some(true)), // 18, agent 0: (0b011, 0b011)
            // Task 4, req r0 = 0b001, r1 = 0b100, r2 off the roster.
            (4, join, true, Some(true)),    // 19, agent 0: (0b001, 0b001)
            (4, join, false, None),         // 20, agent 3: r2
            (4, leave, false, Some(false)), // 21, agent 2: (0b100, 0)
            (4, leave, false, Some(false)), // 22, agent 0: (0b001, 0)
        ]
    }

    /// [`base_reads`] with `window` set on the read indices of `failing`.
    fn expected_reads(failing: &[usize]) -> Vec<Read> {
        base_reads()
            .into_iter()
            .enumerate()
            .map(|(i, (task, leave, act, identical))| Read {
                task,
                leave,
                act,
                centre: identical.map(|identical| CentreClass {
                    identical,
                    window: failing.contains(&i),
                }),
            })
            .collect()
    }

    #[test]
    fn replay_classes_each_read_against_the_model_its_centre_queries() {
        let inst = five_task_instance(Some(ROWS));
        let trace = five_task_trace();
        // Failure masks, oldest first, after each task.
        //
        // Role models under `RoleCoverage`: task 0 ends `{1, 3}` with `(2, r1)`
        // uncovered, so r1 holds [0b100] from then on; r0 and r2 never fail.
        // The one later read of an r1 candidate under an r1 demand is read 21.
        let coverage = replay(
            &inst,
            &trace,
            3,
            8,
            Some(CentreModel::Role(OutcomeSignal::RoleCoverage)),
        );
        assert_eq!(coverage.reads, expected_reads(&[21]));
        // Role models under `Both`: task 0, agent 1 did not perform — r0
        // [0b011], r1 [0b100]. Task 1, agent 1 performed and agent 3 did not —
        // r0 [0b011, 0], r2 [0b100]. Task 2 — r0 [0, 0]. Task 3, agent 1
        // performed on bit 0 — r0 [0, 0]. r0's window meets task 1's and task
        // 2's 0b010 and not task 3's or task 4's 0b001.
        let both = replay(
            &inst,
            &trace,
            3,
            8,
            Some(CentreModel::Role(OutcomeSignal::Both)),
        );
        assert_eq!(both.reads, expected_reads(&[8, 9, 11, 12, 13, 14, 15, 21]));
        // Agent models: agent 1 [0b011] after task 0, [0b011, 0] after task 1,
        // [0, 0] after task 2; agent 3 observes nothing on task 0 (r2 has no
        // demand) and [0b100] after task 1; agent 0 [0b001] after task 3;
        // agent 2 [0b100] after task 4.
        let agent = replay(&inst, &trace, 3, 8, Some(CentreModel::Agent));
        assert_eq!(agent.reads, expected_reads(&[9, 12, 14, 19, 22]));
        // Without a centre model no read is classed.
        let bare = replay(&inst, &trace, 3, 8, None);
        assert!(bare.reads.iter().all(|r| r.centre.is_none()));
        assert_eq!(bare.reads.len(), 23);
        // Leave queries: 4 reads × 2 roles, 3 × 2, 2 × 1, 2 × 1, 2 × 2.
        // Identical over the whole mask: task 0 — 2, 1, 1, 2; task 1 — 1, 2,
        // 1; task 2 — 0, 1; task 3 — 0, 1; task 4 — 1, 1.
        for replayed in [&coverage, &both, &agent, &bare] {
            assert_eq!(
                (replayed.leave_queries, replayed.leave_identical),
                (22, 14),
                "hand values 22 and 14"
            );
        }
    }

    #[test]
    fn replay_stops_at_the_end_of_a_short_trace() {
        let trace = five_task_trace();
        let replayed = replay(
            &five_task_instance(Some(ROWS)),
            &trace[..5],
            3,
            8,
            Some(CentreModel::Agent),
        );
        assert_eq!(replayed.reads.len(), 5);
        assert_eq!((replayed.leave_queries, replayed.leave_identical), (4, 3));
    }

    #[test]
    fn the_recomputed_agent_update_counts_are_the_hand_derived_ones() {
        let inst = five_task_instance(Some(ROWS));
        let recon = reconstruct(&inst, &five_task_trace()).unwrap();
        let members: Vec<Vec<usize>> = recon.tasks.iter().map(|e| e.members.clone()).collect();
        assert_eq!(
            members,
            vec![vec![1, 3], vec![1, 3], vec![1], vec![0, 1], vec![0, 2]]
        );
        // Agent 0: tasks 3, 4. Agent 1: tasks 0..=3. Agent 2: task 4. Agent 3:
        // task 1 — on task 0 its role has no demand.
        assert_eq!(
            expected_agent_updates(&inst, &recon, 3, 8),
            vec![(0, 2), (1, 4), (2, 1), (3, 1)],
            "hand values 2, 4, 1, 1"
        );
    }

    #[test]
    fn slots_and_the_scored_rows_are_the_hand_derived_ones() {
        let inst = five_task_instance(Some(ROWS));
        let recon = reconstruct(&inst, &five_task_trace()).unwrap();
        // Nine final slots; non-performing: agents 1 and 3 on task 0, agent 3
        // on task 1, agent 0 on task 3, agent 2 on task 4. Agent 3 on task 0
        // covers no demanded step.
        assert_eq!(
            slots(&inst, &recon),
            Slots {
                all: 9,
                all_failed: 5,
                covering: 8,
                covering_failed: 4,
            }
        );
        // Performed steps per task: 0 of 3, 1 of 2, 1 of 1, 1 of 1, 1 of 2;
        // final sizes 2, 2, 1, 2, 2; rosters 2, 2, 1, 1, 2.
        let row = |tasks, successes, fraction_sum, eff_sum, final_members, off_demand| ScoredRow {
            tasks,
            successes,
            fraction_sum,
            eff_sum,
            final_members,
            empty: 0,
            off_demand,
        };
        assert_eq!(
            scored_rows(recon.tasks.iter(), 3),
            vec![
                row(5, 2, 3.0, 2.0, 9, 1),
                row(2, 2, 2.0, 1.5, 3, 0),
                row(3, 0, 1.0, 0.5, 6, 1),
                ScoredRow::default(),
            ]
        );
        let scored = recon.performance_scored.unwrap();
        assert_eq!(
            (scored.success_rate, scored.mean_cov_eff),
            (0.4, 0.4),
            "2 of 5 tasks, efficiency sum 2.0 over 5"
        );
    }

    #[test]
    fn after_failure_splits_the_pool_at_the_first_observed_non_performance() {
        let inst = five_task_instance(Some(ROWS));
        let recon = reconstruct(&inst, &five_task_trace()).unwrap();
        // Evidence mask: agent 0 fails on task 3 (1 later task, a member of
        // it); agent 1 on task 0 (4 later, a member of 3); agent 2 on task 4
        // (none later); agent 3 on task 1 (3 later, a member of none).
        assert_eq!(
            after_failure(&inst, &recon, 3, 8, true),
            AfterFailure {
                failed_agents: 4,
                failed_with_later: 3,
                failed_later: 8,
                failed_later_member: 4,
                never_again: 1,
                ..AfterFailure::default()
            }
        );
        // Any final member: agent 3's first non-performance moves to task 0 (4
        // later, a member of 1).
        assert_eq!(
            after_failure(&inst, &recon, 3, 8, false),
            AfterFailure {
                failed_agents: 4,
                failed_with_later: 3,
                failed_later: 9,
                failed_later_member: 5,
                never_again: 0,
                ..AfterFailure::default()
            }
        );
        // No draw: everyone performed; first observations on tasks 3, 0, 4, 1
        // under the evidence mask, and agent 3's on task 0 without it.
        let clean = five_task_instance(None);
        let recon = reconstruct(&clean, &five_task_trace()).unwrap();
        assert_eq!(
            after_failure(&clean, &recon, 3, 8, true),
            AfterFailure {
                clean_agents: 4,
                clean_later: 8,
                clean_later_member: 4,
                ..AfterFailure::default()
            }
        );
        assert_eq!(
            after_failure(&clean, &recon, 3, 8, false),
            AfterFailure {
                clean_agents: 4,
                clean_later: 9,
                clean_later_member: 5,
                ..AfterFailure::default()
            }
        );
    }

    #[test]
    fn mismatch_is_none_on_equal_counts_and_carries_both_pairs_otherwise() {
        let replayed = Replay {
            reads: Vec::new(),
            leave_queries: 22,
            leave_identical: 14,
        };
        let mut counters = GroupAifCounters {
            leave_queries: 22,
            leave_queries_identical: 14,
            ..GroupAifCounters::default()
        };
        assert_eq!(mismatch(7, &replayed, &counters), None);
        counters.leave_queries_identical = 15;
        assert_eq!(
            mismatch(7, &replayed, &counters),
            Some(Mismatch {
                seed: 7,
                mirror: (22, 14),
                ledger: (22, 15),
            })
        );
        counters.leave_queries_identical = 14;
        counters.leave_queries = 21;
        assert!(mismatch(7, &replayed, &counters).is_some());
    }

    #[test]
    fn render_centre_withholds_a_cell_with_a_mismatching_seed() {
        let mut counts: CentreCounts = [[[(0, 0); 2]; 2]; 2];
        counts[1][1][0] = (3, 4);
        let bad = vec![Mismatch {
            seed: 8001,
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
                "- `dropped` withheld: seed 8001 — mirror 18 leave queries, 13 with `cfg0 == cfg1`; ledger 18 and 12."
            ),
            "{out}"
        );
        assert!(out.contains("`kept` 3/3 · `dropped` 2/3"), "{out}");
        assert_eq!(
            identical_row_text(&counts, true),
            "masks identical, window holds a failure: n/a (0 of 0); masks identical, window holds none: 75.0 % (3 of 4)"
        );
    }

    #[test]
    fn a_smoke_prints_the_gate_lines_and_the_verdict_alone() {
        let gate = Gate {
            name: "X-recon",
            predicate: "predicate text".to_owned(),
            parts: vec![Part {
                label: "`grp-id`".to_owned(),
                ok: 2,
                n: 3,
            }],
            failures: vec!["`grp-id` 8001 (recomputed 0.123456)".to_owned()],
            detail: vec!["detail 0.654321".to_owned()],
        };
        assert_eq!(
            gate.line(),
            "- **X-recon — FAIL.** predicate text: `grp-id` 2/3."
        );
        let rendered = Rendered {
            gate_lines: format!("{}\n", gate.line()),
            report: "REPORT 0.123456\n".to_owned(),
            verdict: LABEL_INVALID,
        };
        assert_eq!(
            rendered.stdout(true),
            "- **X-recon — FAIL.** predicate text: `grp-id` 2/3.\nVERDICT: RUN-INVALID\n"
        );
        assert!(rendered.stdout(false).contains("REPORT 0.123456"));
    }

    /// `grp-id`, `grp-topo`, `ref-prune-id` and `ref-prune` hosted over the
    /// hand-built instance: the in-binary identity gates hold on it, and the
    /// leave-query mirror equals the ledger under both stores.
    #[test]
    fn the_identity_gates_hold_on_the_hand_built_instance() {
        let instances = vec![five_task_instance(Some(ROWS))];
        let id = run_group(GRP_ID, &instances, 0).unwrap();
        let topo = run_group(GRP_TOPO, &instances, 0).unwrap();
        let prune = run_reference(REF_PRUNE, &instances).unwrap();
        let prune_id = run_reference(REF_PRUNE_ID, &instances).unwrap();

        let learn_id = gate_learn_id(&id, &instances);
        assert!(learn_id.pass(), "{:?}", learn_id.failures);
        assert!(topo.ledgers[0].agent_updates.is_empty());
        let id0 = gate_id0(&[(&id, &topo), (&prune_id, &prune)]);
        assert!(id0.pass(), "{:?}", id0.failures);
        assert!(!id.runs[0].task0().is_empty(), "task 0 holds reads");
        let recon = gate_recon(&[&id, &topo, &prune, &prune_id]);
        assert!(recon.pass(), "{:?}", recon.failures);
        for hosted in [&id, &topo] {
            assert_eq!(mismatches(hosted), Vec::new(), "`{}`", hosted.label);
        }
    }

    /// `grp-topo-cov`'s configuration — K7-1's `grp-topo` — hosted over the
    /// first three seeds of the consumed block 90..120, which carries no
    /// performance draw.
    #[test]
    fn replay_counts_equal_the_ledger_on_consumed_seeds() {
        let spec = WorkflowSpec::default();
        let instances: Vec<WorkflowInstance> =
            (90..93).map(|seed| spec.generate(seed).unwrap()).collect();
        let hosted = run_group(GRP_TOPO_COV, &instances, 0).unwrap();
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
