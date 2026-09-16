//! Process-structured instances (feature `process`): the v2w world draw —
//! the flat draw, then one role per agent, one role tag per required bit with
//! a role-feasibility re-draw, one as-written workflow per task, and an
//! optional per-agent performance draw — plus the role-matched scorer over
//! it and the battery loop that runs one [`WorkflowArm`] over a seed range.

use std::collections::HashMap;
use std::fmt;
use std::time::Instant;

use catgraph_applied::prop::colored::ColoredExpr;
use catgraph_applied::prop::{Free, PropExpr};
use catgraph_magnitude::CatgraphError;
use catgraph_syntax::frobenius::FrobeniusOr;
use catgraph_syntax::text::print as print_expr;

use crate::algorithms::{AgentCapabilities, CapabilityAgent};
use crate::decision::{CoalitionDecisionPolicy, DecisionContext, RoleId, TaskStart};
use crate::process::{
    Demand, Role, StaffingTable, Step, Workflow, WorkflowGen, chain, demand, spider_expr, step_expr,
};

use super::battery::{BatteryResult, InstanceResult, SeedRange};
use super::instance::InstanceSpec;
use super::rng::SplitMix64;

/// The per-agent performance draw appended after the shape draw: each agent
/// is reliable with probability `reliable_prob` (failure rate `rho_reliable`)
/// and flaky otherwise (failure rate `rho_flaky`); per task, per agent, one
/// unit draw below `1 - rho` is a performed task.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PerformanceSpec {
    /// Probability an agent is reliable.
    pub reliable_prob: f64,
    /// Failure rate of a reliable agent.
    pub rho_reliable: f64,
    /// Failure rate of a flaky agent.
    pub rho_flaky: f64,
}

/// The ranges one seed draws a [`WorkflowInstance`] from.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowSpec {
    /// The flat draw the workflow draw extends; `required_bits` must start at
    /// or above `1`.
    pub base: InstanceSpec,
    /// Number of roles, at least `1`; agent roles and step tags are drawn
    /// below it.
    pub roles: u8,
    /// Attempts a task gets to become role-feasible before generation errors.
    pub redraw_cap: usize,
    /// Fan-out denominator, at least `1`: a same-role adjacent pair draws
    /// `next_u64 % fanout_denom == 0` to write the later step as
    /// `δ ; (s ⊗ s) ; μ`.
    pub fanout_denom: u64,
    /// The performance draw, or `None` for no draw at all.
    pub performance: Option<PerformanceSpec>,
}

impl Default for WorkflowSpec {
    /// The registered v2w values: the default flat spec at `required_bits`
    /// `2..=8`, 3 roles, re-draw cap 1000, fan-out denominator 4, no
    /// performance draw.
    fn default() -> Self {
        Self {
            base: InstanceSpec {
                required_bits: 2..=8,
                ..InstanceSpec::default()
            },
            roles: 3,
            redraw_cap: 1000,
            fanout_denom: 4,
            performance: None,
        }
    }
}

/// One process-structured task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowTask {
    /// Required-capability bitmask.
    pub required: u32,
    /// Role tag per universe bit, indexed by bit; entries of bits outside
    /// `required` are `Role::new(0)` and carry no meaning.
    pub tags: Vec<Role>,
    /// Agent indices in arrival order.
    pub arrival: Vec<usize>,
    /// The as-written workflow.
    pub written: Workflow,
    /// The `(bit, role)` demand of `written`.
    pub demand: Demand,
}

/// One generated process-structured instance.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowInstance {
    /// The seed the instance was generated from.
    pub seed: u64,
    /// The agent pool; `agents[i].agent_id() == i`.
    pub agents: Vec<CapabilityAgent>,
    /// Role per agent, indexed by agent id.
    pub roles: Vec<Role>,
    /// The task list.
    pub tasks: Vec<WorkflowTask>,
    /// The flat draw's required mask per task, before any re-draw; equals
    /// `tasks[t].required` for a task that needed none.
    pub prefix_required: Vec<u32>,
    /// `(bit, role)` holder counts over the pool.
    pub table: StaffingTable,
    /// Role-feasibility re-draws summed over tasks.
    pub redraws: usize,
    /// The most attempts any single task needed to become role-feasible.
    pub max_attempts: usize,
    /// `performance[t][i]`: whether agent `i` performed on task `t`; `None`
    /// when the spec drew no performance.
    pub performance: Option<Vec<Vec<bool>>>,
}

impl WorkflowInstance {
    /// The pool's `agent_id → RoleId` map.
    #[must_use]
    pub fn role_map(&self) -> HashMap<usize, RoleId> {
        self.roles
            .iter()
            .enumerate()
            .map(|(id, role)| (id, RoleId::from(role.index())))
            .collect()
    }
}

impl fmt::Display for WorkflowInstance {
    /// One record: a `seed` line, a counts line, a `prefix_required` line,
    /// one `agent` line per agent (`id`, `caps`, `trust`, `role`), one
    /// `task` line per task (`required`, `tags` as `bit:role` pairs over the
    /// required bits, `arrival`, the pinned source word and the printed
    /// expression of `written`), then `performance none` or one `perf` line
    /// per task.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "seed {}", self.seed)?;
        writeln!(
            f,
            "n {} redraws {} max_attempts {}",
            self.agents.len(),
            self.redraws,
            self.max_attempts
        )?;
        let prefix: Vec<String> = self.prefix_required.iter().map(u32::to_string).collect();
        writeln!(f, "prefix_required {}", prefix.join(","))?;
        for (agent, role) in self.agents.iter().zip(&self.roles) {
            writeln!(
                f,
                "agent {} caps {} trust {} role {}",
                agent.agent_id(),
                agent.capabilities(),
                agent.trust_level(),
                role.index()
            )?;
        }
        for (t, task) in self.tasks.iter().enumerate() {
            let tags: Vec<String> = task
                .tags
                .iter()
                .enumerate()
                .filter(|(b, _)| task.required & (1u32 << b) != 0)
                .map(|(b, role)| format!("{b}:{}", role.index()))
                .collect();
            let arrival: Vec<String> = task.arrival.iter().map(usize::to_string).collect();
            let source: Vec<String> = task
                .written
                .source_word()
                .iter()
                .map(|r| format!("r{}", r.index()))
                .collect();
            writeln!(
                f,
                "task {t} required {} tags {} arrival {} written [{}] {}",
                task.required,
                tags.join(","),
                arrival.join(","),
                source.join(","),
                print_expr(task.written.expr())
            )?;
        }
        match &self.performance {
            None => writeln!(f, "performance none")?,
            Some(rows) => {
                for (t, row) in rows.iter().enumerate() {
                    let bits: String = row.iter().map(|&p| if p { '1' } else { '0' }).collect();
                    writeln!(f, "perf {t} {bits}")?;
                }
            }
        }
        Ok(())
    }
}

/// Which per-bit signal [`run_workflow_instance`] hands its task hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeSignal {
    /// Bit `b` is `true` iff some demanded step on `b` is covered by a final
    /// member of that step's role.
    RoleCoverage,
    /// Bit `b` is `true` iff some final member holding `b` performed on the
    /// task. Requires a performance draw.
    Performance,
    /// Bit `b` is `true` iff some demanded step on `b` is covered by a final
    /// member of that step's role who performed on the task. Requires a
    /// performance draw.
    Both,
}

impl OutcomeSignal {
    /// `true` for the two signals that read the performance draw.
    #[must_use]
    pub fn needs_performance(self) -> bool {
        !matches!(self, Self::RoleCoverage)
    }
}

/// What can go wrong generating or running a process-structured instance.
#[derive(Debug)]
pub enum WorkflowError {
    /// `WorkflowSpec::roles` is `0`.
    NoRoles,
    /// `WorkflowSpec::fanout_denom` is `0`.
    ZeroFanoutDenominator,
    /// The spec's `required_bits` range starts at `0`, so a task may carry no
    /// step at all.
    EmptyRequirement,
    /// A task stayed role-infeasible at the re-draw cap.
    Infeasible {
        /// The instance's seed.
        seed: u64,
        /// The task's index.
        task: usize,
        /// The cap that was reached.
        cap: usize,
    },
    /// The signal reads the performance draw and there is none.
    SignalNeedsPerformance {
        /// The signal asked for.
        signal: OutcomeSignal,
    },
    /// The performance table is not one row per task of one entry per agent.
    PerformanceShape {
        /// The instance's seed.
        seed: u64,
    },
    /// The upstream engine rejected a workflow's composition or pin.
    Catgraph(CatgraphError),
}

impl fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRoles => write!(f, "workflow spec: roles must be at least 1"),
            Self::ZeroFanoutDenominator => {
                write!(f, "workflow spec: fanout_denom must be at least 1")
            }
            Self::EmptyRequirement => {
                write!(f, "workflow spec: required_bits must start at or above 1")
            }
            Self::Infeasible { seed, task, cap } => write!(
                f,
                "seed {seed}, task {task}: still role-infeasible at the re-draw cap {cap}"
            ),
            Self::SignalNeedsPerformance { signal } => {
                write!(
                    f,
                    "signal {signal:?} needs a performance draw and there is none"
                )
            }
            Self::PerformanceShape { seed } => write!(
                f,
                "seed {seed}: performance table is not one row per task of one entry per agent"
            ),
            Self::Catgraph(inner) => write!(f, "workflow draw: upstream rejection: {inner}"),
        }
    }
}

impl std::error::Error for WorkflowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Catgraph(inner) => Some(inner),
            _ => None,
        }
    }
}

impl From<CatgraphError> for WorkflowError {
    fn from(inner: CatgraphError) -> Self {
        Self::Catgraph(inner)
    }
}

impl WorkflowSpec {
    /// Generate the instance for `seed`. Draw schedule on one `SplitMix64`
    /// stream seeded with `seed`: [`InstanceSpec::draw`]; one role below
    /// `roles` per agent; per task one tag below `roles` per required bit in
    /// ascending bit order, then while the task is role-infeasible (some
    /// required bit has no pool holder of its tag) a re-draw of the required
    /// mask (the base spec's task draw) and its tags, counting one re-draw
    /// and failing at `redraw_cap` attempts; per task its shape (per role in
    /// ascending order, the role's steps in ascending bit order as a chain,
    /// one draw per same-role adjacent pair selecting the fan-out); then the
    /// performance draw, if any.
    ///
    /// # Errors
    ///
    /// The spec-shape variants of [`WorkflowError`] before any draw,
    /// [`WorkflowError::Infeasible`] at the cap, and
    /// [`WorkflowError::Catgraph`] on a rejected composition or pin.
    pub fn generate(&self, seed: u64) -> Result<WorkflowInstance, WorkflowError> {
        if self.roles == 0 {
            return Err(WorkflowError::NoRoles);
        }
        if self.fanout_denom == 0 {
            return Err(WorkflowError::ZeroFanoutDenominator);
        }
        if *self.base.required_bits.start() == 0 {
            return Err(WorkflowError::EmptyRequirement);
        }

        let mut rng = SplitMix64::new(seed);
        let (agents, base_tasks) = self.base.draw(&mut rng);
        let roles: Vec<Role> = (0..agents.len())
            .map(|_| Role::new(rng.below(u64::from(self.roles)) as u8))
            .collect();
        let table = StaffingTable::from_pool(
            agents
                .iter()
                .zip(&roles)
                .map(|(agent, &role)| (agent.capabilities(), role)),
        );

        let universe = self.base.universe_bits as usize;
        let mut redraws = 0usize;
        let mut max_attempts = 0usize;
        let prefix_required: Vec<u32> = base_tasks.iter().map(|t| t.required).collect();
        let mut tagged = Vec::with_capacity(base_tasks.len());
        for (t, base) in base_tasks.into_iter().enumerate() {
            let mut required = base.required;
            let mut tags = draw_tags(&mut rng, required, universe, self.roles);
            let mut attempts = 1usize;
            while !feasible(&table, required, &tags) {
                if attempts >= self.redraw_cap {
                    return Err(WorkflowError::Infeasible {
                        seed,
                        task: t,
                        cap: self.redraw_cap,
                    });
                }
                required = self.base.draw_required(&mut rng);
                tags = draw_tags(&mut rng, required, universe, self.roles);
                attempts += 1;
                redraws += 1;
            }
            max_attempts = max_attempts.max(attempts);
            tagged.push((required, tags, base.arrival));
        }

        let mut tasks = Vec::with_capacity(tagged.len());
        for (required, tags, arrival) in tagged {
            let written = draw_shape(&mut rng, required, &tags, self.roles, self.fanout_denom)?;
            let demand = demand(&written);
            tasks.push(WorkflowTask {
                required,
                tags,
                arrival,
                written,
                demand,
            });
        }

        let performance = self.performance.map(|perf| {
            let rho: Vec<f64> = (0..agents.len())
                .map(|_| {
                    if rng.next_unit() < perf.reliable_prob {
                        perf.rho_reliable
                    } else {
                        perf.rho_flaky
                    }
                })
                .collect();
            (0..tasks.len())
                .map(|_| rho.iter().map(|&r| rng.next_unit() < 1.0 - r).collect())
                .collect()
        });

        Ok(WorkflowInstance {
            seed,
            agents,
            roles,
            tasks,
            prefix_required,
            table,
            redraws,
            max_attempts,
            performance,
        })
    }
}

/// One tag below `n_roles` per required bit in ascending bit order;
/// non-required bits draw nothing and read `Role::new(0)`.
fn draw_tags(rng: &mut SplitMix64, required: u32, universe: usize, n_roles: u8) -> Vec<Role> {
    (0..universe)
        .map(|b| {
            if required & (1u32 << b) != 0 {
                Role::new(rng.below(u64::from(n_roles)) as u8)
            } else {
                Role::new(0)
            }
        })
        .collect()
}

/// Every required bit has a pool holder of its tag.
fn feasible(table: &StaffingTable, required: u32, tags: &[Role]) -> bool {
    tags.iter()
        .enumerate()
        .filter(|(b, _)| required & (1u32 << b) != 0)
        .all(|(b, &role)| table.is_staffable(Step::new(b as u8, role)))
}

/// The task's as-written workflow: per role in ascending order the chain of
/// that role's steps in ascending bit order, with one draw per same-role
/// adjacent pair writing the later step as `δ ; (s ⊗ s) ; μ` on a
/// `next_u64 % fanout_denom == 0` hit; the legs tensored in role order and
/// pinned on the source word of their roles.
fn draw_shape(
    rng: &mut SplitMix64,
    required: u32,
    tags: &[Role],
    n_roles: u8,
    fanout_denom: u64,
) -> Result<Workflow, WorkflowError> {
    let steps: Vec<Step> = tags
        .iter()
        .enumerate()
        .filter(|(b, _)| required & (1u32 << b) != 0)
        .map(|(b, &role)| Step::new(b as u8, role))
        .collect();
    let mut source: Vec<Role> = Vec::new();
    let mut legs: Vec<PropExpr<WorkflowGen>> = Vec::new();
    for r in 0..n_roles {
        let role = Role::new(r);
        let group: Vec<Step> = steps.iter().copied().filter(|s| s.role == role).collect();
        if group.is_empty() {
            continue;
        }
        let mut parts: Vec<PropExpr<WorkflowGen>> = Vec::with_capacity(group.len());
        for (i, &s) in group.iter().enumerate() {
            if i > 0 && rng.next_u64().is_multiple_of(fanout_denom) {
                parts.push(spider_expr(FrobeniusOr::Delta(role)));
                parts.push(Free::tensor(step_expr(s), step_expr(s)));
                parts.push(spider_expr(FrobeniusOr::Mu(role)));
            } else {
                parts.push(step_expr(s));
            }
        }
        source.push(role);
        legs.push(chain(parts)?);
    }
    let expr = legs
        .into_iter()
        .reduce(Free::tensor)
        .ok_or(WorkflowError::EmptyRequirement)?;
    Ok(ColoredExpr::new(source, expr)?)
}

/// The metrics of one policy over one process-structured instance.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowResult {
    /// The instance's seed.
    pub seed: u64,
    /// The instance's pool size.
    pub n: usize,
    /// Fraction of tasks whose final coalition covers every distinct demanded
    /// `(bit, role)` step; `0` for an instance without tasks.
    pub success_rate: f64,
    /// Mean over tasks of coverage efficiency; `0` for an instance without
    /// tasks.
    pub mean_cov_eff: f64,
    /// `success_rate * mean_cov_eff`.
    pub primary: f64,
    /// Number of leave-sweep removals summed over tasks.
    pub churn: usize,
    /// Tasks declined before any coalition formed; always `0` here.
    pub declined: usize,
}

impl From<WorkflowResult> for InstanceResult {
    /// `completion_rate` is the success rate; `declined` is dropped.
    fn from(r: WorkflowResult) -> Self {
        Self {
            seed: r.seed,
            n: r.n,
            completion_rate: r.success_rate,
            mean_cov_eff: r.mean_cov_eff,
            primary: r.primary,
            churn: r.churn,
        }
    }
}

/// Run `policy` over every task of `instance`. Per task: the context mask is
/// the OR of the demand's distinct steps; `begin_task` with that mask and
/// the demand's distinct steps as `(bit, role index)` in `Step` order; the
/// first arrival joins unconditionally; each later arrival joins iff
/// `should_join` acts (the coalition shown excludes the candidate); then one
/// leave sweep in arrival order over the post-join membership, where
/// `should_leave` (the coalition shown is the current membership including
/// the agent) acting removes the agent and counts one churn; then
/// `observe_outcome` with the context mask and the per-bit `signal` over one
/// entry per tag of the task. A distinct demanded step is covered iff some
/// final member of its role holds its bit; the task succeeds iff the demand
/// is non-empty and every distinct step is covered; its coverage efficiency
/// is the covered fraction of distinct steps divided by the final member
/// count, `0` for an empty coalition or an empty demand. Every `should_join`
/// / `should_leave` call's wall time in microseconds is appended to
/// `latencies_us`.
///
/// # Errors
///
/// [`WorkflowError::SignalNeedsPerformance`] when `signal` reads the
/// performance draw and the instance has none;
/// [`WorkflowError::PerformanceShape`] when it has one that is not one row
/// per task of one entry per agent.
pub fn run_workflow_instance(
    policy: &dyn CoalitionDecisionPolicy,
    instance: &WorkflowInstance,
    signal: OutcomeSignal,
    latencies_us: &mut Vec<f64>,
) -> Result<WorkflowResult, WorkflowError> {
    let agents = &instance.agents;
    let performance = performance_rows(instance, signal)?;
    let mut success_count = 0usize;
    let mut cov_eff_sum = 0.0f64;
    let mut churn = 0usize;

    for (t, task) in instance.tasks.iter().enumerate() {
        let required = required_mask(&task.demand);
        let steps: Vec<(u8, u8)> = task
            .demand
            .distinct()
            .map(|s| (s.bit, s.role.index()))
            .collect();
        policy.begin_task(&TaskStart {
            required,
            steps: &steps,
        });
        let ctx = DecisionContext {
            required_capabilities: required,
        };
        let mut members: Vec<usize> = Vec::with_capacity(agents.len());
        let mut arrivals = task.arrival.iter().copied();
        if let Some(first) = arrivals.next() {
            members.push(first);
        }
        for candidate in arrivals {
            let coalition = views(agents, &members);
            let decision = timed(latencies_us, || {
                policy.should_join(&agents[candidate], &coalition, &ctx)
            });
            if decision.act {
                members.push(candidate);
            }
        }

        for &idx in &task.arrival {
            let Some(pos) = members.iter().position(|&m| m == idx) else {
                continue;
            };
            let coalition = views(agents, &members);
            let decision = timed(latencies_us, || {
                policy.should_leave(&agents[idx], &coalition, &ctx)
            });
            if decision.act {
                members.remove(pos);
                churn += 1;
            }
        }

        let d_len = task.demand.distinct_len();
        let covered = task
            .demand
            .distinct()
            .filter(|&s| step_covered(instance, &members, s))
            .count();
        let completed = d_len > 0 && covered == d_len;
        let cov_eff = if members.is_empty() || d_len == 0 {
            0.0
        } else {
            (covered as f64 / d_len as f64) / members.len() as f64
        };
        if completed {
            success_count += 1;
        }
        cov_eff_sum += cov_eff;

        let universe = task.tags.len();
        let mut per_bit = vec![false; universe];
        match signal {
            OutcomeSignal::RoleCoverage => {
                for step in task.demand.distinct() {
                    let b = usize::from(step.bit);
                    if b < universe && step_covered(instance, &members, step) {
                        per_bit[b] = true;
                    }
                }
            }
            OutcomeSignal::Performance => {
                let row = performance_row(performance, t);
                for (b, slot) in per_bit.iter_mut().enumerate() {
                    *slot = members
                        .iter()
                        .any(|&i| (agents[i].capabilities() >> b) & 1 == 1 && row[i]);
                }
            }
            OutcomeSignal::Both => {
                let row = performance_row(performance, t);
                for step in task.demand.distinct() {
                    let b = usize::from(step.bit);
                    if b < universe && step_covered_performed(instance, &members, step, row) {
                        per_bit[b] = true;
                    }
                }
            }
        }
        policy.observe_outcome(required, &per_bit);
    }

    let n_tasks = instance.tasks.len();
    let (success_rate, mean_cov_eff) = if n_tasks == 0 {
        (0.0, 0.0)
    } else {
        (
            success_count as f64 / n_tasks as f64,
            cov_eff_sum / n_tasks as f64,
        )
    };
    Ok(WorkflowResult {
        seed: instance.seed,
        n: agents.len(),
        success_rate,
        mean_cov_eff,
        primary: success_rate * mean_cov_eff,
        churn,
        declined: 0,
    })
}

/// The performance rows `signal` reads, validated to one row per task of one
/// entry per agent; `None` when the signal reads none.
fn performance_rows(
    instance: &WorkflowInstance,
    signal: OutcomeSignal,
) -> Result<Option<&[Vec<bool>]>, WorkflowError> {
    if !signal.needs_performance() {
        return Ok(None);
    }
    let rows = instance
        .performance
        .as_deref()
        .ok_or(WorkflowError::SignalNeedsPerformance { signal })?;
    let n = instance.agents.len();
    if rows.len() != instance.tasks.len() || rows.iter().any(|row| row.len() != n) {
        return Err(WorkflowError::PerformanceShape {
            seed: instance.seed,
        });
    }
    Ok(Some(rows))
}

/// Row `t` of the validated performance rows; empty when there are none.
fn performance_row(rows: Option<&[Vec<bool>]>, t: usize) -> &[bool] {
    rows.and_then(|rows| rows.get(t)).map_or(&[], Vec::as_slice)
}

/// The OR of the demand's distinct steps' bits.
fn required_mask(d: &Demand) -> u32 {
    d.distinct()
        .filter_map(Step::capability_mask)
        .fold(0u32, |acc, m| acc | m)
}

/// Some member of `step.role` holds `step.bit`.
fn step_covered(instance: &WorkflowInstance, members: &[usize], step: Step) -> bool {
    let Some(mask) = step.capability_mask() else {
        return false;
    };
    members.iter().any(|&i| {
        instance.roles.get(i).is_some_and(|&r| r == step.role)
            && instance.agents[i].capabilities() & mask != 0
    })
}

/// Some member of `step.role` holds `step.bit` and performed (`row[i]`).
fn step_covered_performed(
    instance: &WorkflowInstance,
    members: &[usize],
    step: Step,
    row: &[bool],
) -> bool {
    let Some(mask) = step.capability_mask() else {
        return false;
    };
    members.iter().any(|&i| {
        instance.roles.get(i).is_some_and(|&r| r == step.role)
            && instance.agents[i].capabilities() & mask != 0
            && row[i]
    })
}

/// Capability views of the agents at `members`, in `members` order.
fn views<'a>(agents: &'a [CapabilityAgent], members: &[usize]) -> Vec<&'a dyn AgentCapabilities> {
    members
        .iter()
        .map(|&m| &agents[m] as &dyn AgentCapabilities)
        .collect()
}

/// Run `f`, append its wall time in microseconds to `latencies_us`, and return
/// its output.
fn timed<T>(latencies_us: &mut Vec<f64>, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = f();
    latencies_us.push(start.elapsed().as_secs_f64() * 1e6);
    out
}

/// A per-instance policy factory.
pub type PolicyFactory = Box<dyn Fn(&WorkflowInstance) -> Box<dyn CoalitionDecisionPolicy>>;

/// One labelled per-instance policy factory under test.
pub struct WorkflowArm {
    /// Label printed in the tables.
    pub label: String,
    /// Builds the policy for one instance; called once per seed.
    pub make: PolicyFactory,
}

/// One arm's results over a seed range: the per-seed metrics in seed order
/// and every decision latency in call order.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowBatteryResult {
    /// The arm's label.
    pub label: String,
    /// One result per seed, in ascending seed order.
    pub per_seed: Vec<WorkflowResult>,
    /// Wall time of every `should_join` / `should_leave` call, microseconds,
    /// in call order across all seeds.
    pub latencies_us: Vec<f64>,
}

impl From<WorkflowBatteryResult> for BatteryResult {
    /// Each per-seed row through `From<WorkflowResult> for InstanceResult`.
    fn from(r: WorkflowBatteryResult) -> Self {
        Self {
            label: r.label,
            per_seed: r.per_seed.into_iter().map(InstanceResult::from).collect(),
            latencies_us: r.latencies_us,
        }
    }
}

/// Run `arm` over the instance `spec` generates for each seed in `seeds`, in
/// ascending seed order.
///
/// # Errors
///
/// [`WorkflowError::SignalNeedsPerformance`] before any seed when `signal`
/// reads the performance draw and `spec.performance` is `None`; otherwise
/// whatever [`WorkflowSpec::generate`] or [`run_workflow_instance`] returns
/// for the first failing seed.
pub fn run_workflow_battery(
    arm: &WorkflowArm,
    spec: &WorkflowSpec,
    seeds: SeedRange,
    signal: OutcomeSignal,
) -> Result<WorkflowBatteryResult, WorkflowError> {
    if signal.needs_performance() && spec.performance.is_none() {
        return Err(WorkflowError::SignalNeedsPerformance { signal });
    }
    let mut latencies_us = Vec::new();
    let mut per_seed = Vec::with_capacity(seeds.len());
    for seed in seeds.iter() {
        let instance = spec.generate(seed)?;
        let policy = (arm.make)(&instance);
        per_seed.push(run_workflow_instance(
            policy.as_ref(),
            &instance,
            signal,
            &mut latencies_us,
        )?);
    }
    Ok(WorkflowBatteryResult {
        label: arm.label.clone(),
        per_seed,
        latencies_us,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::Decision;
    use crate::harness::instance::Instance;
    use std::sync::Mutex;

    /// One policy call, in call order.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Event {
        BeginTask { required: u32, steps: Vec<(u8, u8)> },
        Join,
        Leave,
        ObserveOutcome { required: u32, per_bit: Vec<bool> },
    }

    /// Acts on join and on leave per the two flags; records every call as an
    /// [`Event`].
    struct Fixed {
        join: bool,
        leave: bool,
        events: Mutex<Vec<Event>>,
    }

    impl Fixed {
        fn new(join: bool, leave: bool) -> Self {
            Self {
                join,
                leave,
                events: Mutex::new(Vec::new()),
            }
        }

        /// The `(required, per_bit)` of every `ObserveOutcome` event.
        fn outcomes(&self) -> Vec<(u32, Vec<bool>)> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .filter_map(|e| match e {
                    Event::ObserveOutcome { required, per_bit } => {
                        Some((*required, per_bit.clone()))
                    }
                    _ => None,
                })
                .collect()
        }
    }

    impl CoalitionDecisionPolicy for Fixed {
        fn should_join(
            &self,
            _agent: &dyn AgentCapabilities,
            _coalition: &[&dyn AgentCapabilities],
            _ctx: &DecisionContext,
        ) -> Decision {
            self.events.lock().unwrap().push(Event::Join);
            Decision {
                act: self.join,
                score: 0.0,
            }
        }

        fn should_leave(
            &self,
            _agent: &dyn AgentCapabilities,
            _coalition: &[&dyn AgentCapabilities],
            _ctx: &DecisionContext,
        ) -> Decision {
            self.events.lock().unwrap().push(Event::Leave);
            Decision {
                act: self.leave,
                score: 0.0,
            }
        }

        fn begin_task(&self, task: &TaskStart<'_>) {
            self.events.lock().unwrap().push(Event::BeginTask {
                required: task.required,
                steps: task.steps.to_vec(),
            });
        }

        fn observe_outcome(&self, required: u32, per_bit_success: &[bool]) {
            self.events.lock().unwrap().push(Event::ObserveOutcome {
                required,
                per_bit: per_bit_success.to_vec(),
            });
        }
    }

    fn perf_spec() -> PerformanceSpec {
        PerformanceSpec {
            reliable_prob: 0.7,
            rho_reliable: 0.05,
            rho_flaky: 0.40,
        }
    }

    /// Two agents, one task: agent 0 holds bit 0 at `holder_role`, agent 1
    /// holds bit 1 at role 1; the task demands `s0_r1` alone.
    fn two_agent_instance(
        holder_role: u8,
        performance: Option<Vec<Vec<bool>>>,
    ) -> WorkflowInstance {
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
        let step = Step::new(0, r1);
        let written = ColoredExpr::new(vec![r1], step_expr(step)).unwrap();
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
            performance,
        }
    }

    fn run(
        instance: &WorkflowInstance,
        signal: OutcomeSignal,
    ) -> (WorkflowResult, Vec<(u32, Vec<bool>)>) {
        let policy = Fixed::new(true, false);
        let mut lat = Vec::new();
        let result = run_workflow_instance(&policy, instance, signal, &mut lat).unwrap();
        (result, policy.outcomes())
    }

    #[test]
    fn role_mismatched_holder_does_not_cover() {
        let inst = two_agent_instance(0, None);
        let (r, seen) = run(&inst, OutcomeSignal::RoleCoverage);
        assert_eq!(r.success_rate, 0.0, "s0_r1 has no r1 holder: {r:?}");
        assert_eq!(r.mean_cov_eff, 0.0, "{r:?}");
        assert_eq!(r.primary, 0.0, "{r:?}");
        assert_eq!(r.churn, 0);
        assert_eq!(r.n, 2);
        assert_eq!(seen, vec![(0b01, vec![false, false])]);
    }

    #[test]
    fn role_matched_holder_covers_at_half_efficiency() {
        let inst = two_agent_instance(1, None);
        let (r, seen) = run(&inst, OutcomeSignal::RoleCoverage);
        assert_eq!(r.success_rate, 1.0, "{r:?}");
        assert_eq!(r.mean_cov_eff, 0.5, "1 of 1 steps over 2 members: {r:?}");
        assert_eq!(r.primary, 0.5, "{r:?}");
        assert_eq!(seen, vec![(0b01, vec![true, false])]);
    }

    #[test]
    fn performance_and_both_signals_read_the_draw_and_leave_primary_alone() {
        // Agent 0 (the r1 holder of bit 0) did not perform; agent 1 did.
        let inst = two_agent_instance(1, Some(vec![vec![false, true]]));
        let (r_cov, seen_cov) = run(&inst, OutcomeSignal::RoleCoverage);
        let (r_perf, seen_perf) = run(&inst, OutcomeSignal::Performance);
        let (r_both, seen_both) = run(&inst, OutcomeSignal::Both);
        assert_eq!(r_cov, r_perf, "PRIMARY must not read the performance draw");
        assert_eq!(r_cov, r_both, "PRIMARY must not read the performance draw");
        assert_eq!(seen_cov, vec![(0b01, vec![true, false])]);
        // Performance: bit 1 is held by agent 1, who performed; bit 0's holder
        // did not.
        assert_eq!(seen_perf, vec![(0b01, vec![false, true])]);
        // Both: the only demanded step's covering member did not perform.
        assert_eq!(seen_both, vec![(0b01, vec![false, false])]);
    }

    #[test]
    fn performance_signals_without_a_draw_are_errors() {
        let inst = two_agent_instance(1, None);
        let mut lat = Vec::new();
        for signal in [OutcomeSignal::Performance, OutcomeSignal::Both] {
            let err = run_workflow_instance(&Fixed::new(true, false), &inst, signal, &mut lat)
                .unwrap_err();
            assert!(
                matches!(err, WorkflowError::SignalNeedsPerformance { signal: s } if s == signal),
                "{err}"
            );
        }
        let spec = WorkflowSpec::default();
        let arm = WorkflowArm {
            label: "fixed".into(),
            make: Box::new(|_| Box::new(Fixed::new(true, false))),
        };
        let err = run_workflow_battery(
            &arm,
            &spec,
            SeedRange { start: 0, end: 2 },
            OutcomeSignal::Both,
        )
        .unwrap_err();
        assert!(
            matches!(err, WorkflowError::SignalNeedsPerformance { .. }),
            "{err}"
        );

        let bad_shape = two_agent_instance(1, Some(vec![vec![true]]));
        let err = run_workflow_instance(
            &Fixed::new(true, false),
            &bad_shape,
            OutcomeSignal::Performance,
            &mut lat,
        )
        .unwrap_err();
        assert!(
            matches!(err, WorkflowError::PerformanceShape { seed: 0 }),
            "{err}"
        );
    }

    #[test]
    fn spec_shape_errors_precede_any_draw() {
        let no_roles = WorkflowSpec {
            roles: 0,
            ..WorkflowSpec::default()
        };
        assert!(matches!(no_roles.generate(1), Err(WorkflowError::NoRoles)));
        let zero_denom = WorkflowSpec {
            fanout_denom: 0,
            ..WorkflowSpec::default()
        };
        assert!(matches!(
            zero_denom.generate(1),
            Err(WorkflowError::ZeroFanoutDenominator)
        ));
        let empty = WorkflowSpec {
            base: InstanceSpec {
                required_bits: 0..=3,
                ..InstanceSpec::default()
            },
            ..WorkflowSpec::default()
        };
        assert!(matches!(
            empty.generate(1),
            Err(WorkflowError::EmptyRequirement)
        ));
    }

    #[test]
    fn redraw_cap_of_one_errors_on_the_first_infeasible_task() {
        // At cap 1 no re-draw is allowed, so the first infeasible task errors.
        // Over 0..200 at least one seed must hit it; the default cap must not.
        let capped = WorkflowSpec {
            redraw_cap: 1,
            ..WorkflowSpec::default()
        };
        let default = WorkflowSpec::default();
        let mut hit = 0usize;
        for seed in 0..200u64 {
            let inst = default.generate(seed).unwrap();
            match capped.generate(seed) {
                Ok(c) => {
                    assert_eq!(
                        inst.redraws, 0,
                        "seed {seed}: cap 1 succeeded with re-draws"
                    );
                    assert_eq!(c, inst);
                }
                Err(WorkflowError::Infeasible { seed: s, cap, .. }) => {
                    assert_eq!((s, cap), (seed, 1));
                    assert!(
                        inst.redraws > 0,
                        "seed {seed}: cap 1 failed without re-draws"
                    );
                    hit += 1;
                }
                Err(e) => panic!("seed {seed}: {e}"),
            }
        }
        assert!(hit > 0, "no seed in 0..200 needed a re-draw");
    }

    #[test]
    fn performance_draw_is_appended_after_the_identity_world() {
        let plain = WorkflowSpec::default();
        let with_perf = WorkflowSpec {
            performance: Some(perf_spec()),
            ..WorkflowSpec::default()
        };
        for seed in 0..20u64 {
            let a = plain.generate(seed).unwrap();
            let b = with_perf.generate(seed).unwrap();
            assert_eq!(a.performance, None);
            assert_eq!(a.agents, b.agents, "seed {seed}");
            assert_eq!(a.roles, b.roles, "seed {seed}");
            assert_eq!(a.tasks, b.tasks, "seed {seed}");
            assert_eq!(a.table, b.table, "seed {seed}");
            assert_eq!((a.redraws, a.max_attempts), (b.redraws, b.max_attempts));
            let rows = b.performance.as_ref().unwrap();
            assert_eq!(rows.len(), b.tasks.len(), "seed {seed}");
            assert!(
                rows.iter().all(|r| r.len() == b.agents.len()),
                "seed {seed}"
            );
        }
    }

    #[test]
    fn generated_instances_are_role_feasible_and_tags_cover_required_bits() {
        let spec = WorkflowSpec::default();
        for seed in 0..100u64 {
            let inst = spec.generate(seed).unwrap();
            assert_eq!(inst.seed, seed);
            assert_eq!(inst.roles.len(), inst.agents.len());
            assert_eq!(inst.tasks.len(), spec.base.tasks);
            assert_eq!(inst.role_map().len(), inst.agents.len());
            for (t, task) in inst.tasks.iter().enumerate() {
                assert_eq!(task.tags.len(), spec.base.universe_bits as usize);
                assert!(
                    spec.base
                        .required_bits
                        .contains(&task.required.count_ones()),
                    "seed {seed}, task {t}: {} required bits",
                    task.required.count_ones()
                );
                assert!(
                    feasible(&inst.table, task.required, &task.tags),
                    "seed {seed}, task {t}"
                );
                assert_eq!(
                    required_mask(&task.demand),
                    task.required,
                    "seed {seed}, task {t}: the demand's OR-mask is the required mask"
                );
                assert_eq!(
                    task.demand.distinct_len() as u32,
                    task.required.count_ones(),
                    "seed {seed}, task {t}: one distinct step per required bit"
                );
                let mut arrival = task.arrival.clone();
                arrival.sort_unstable();
                assert_eq!(arrival, (0..inst.agents.len()).collect::<Vec<_>>());
            }
        }
    }

    #[test]
    fn flat_prefix_is_the_workflow_pool_arrivals_and_prefix_required() {
        let spec = WorkflowSpec::default();
        for seed in 0..50u64 {
            let flat: Instance = spec.base.generate(seed);
            let wf = spec.generate(seed).unwrap();
            assert_eq!(flat.agents, wf.agents, "seed {seed}");
            assert_eq!(wf.prefix_required.len(), wf.tasks.len());
            for (t, (a, b)) in flat.tasks.iter().zip(&wf.tasks).enumerate() {
                assert_eq!(a.arrival, b.arrival, "seed {seed}, task {t}");
                assert_eq!(
                    a.required, wf.prefix_required[t],
                    "seed {seed}, task {t}: prefix_required is the flat draw"
                );
            }
        }
    }

    #[test]
    fn battery_runs_in_seed_order_and_converts_to_the_flat_shape() {
        let spec = WorkflowSpec::default();
        let arm = WorkflowArm {
            label: "fixed".into(),
            make: Box::new(|_| Box::new(Fixed::new(true, true))),
        };
        let seeds = SeedRange { start: 3, end: 7 };
        let result = run_workflow_battery(&arm, &spec, seeds, OutcomeSignal::RoleCoverage).unwrap();
        assert_eq!(result.label, "fixed");
        let seen: Vec<u64> = result.per_seed.iter().map(|r| r.seed).collect();
        assert_eq!(seen, vec![3, 4, 5, 6]);
        for r in &result.per_seed {
            assert_eq!(r.churn, r.n * spec.base.tasks, "every member leaves once");
            assert_eq!(r.mean_cov_eff, 0.0);
            assert_eq!(r.declined, 0);
        }
        let flat = BatteryResult::from(result.clone());
        assert_eq!(flat.label, result.label);
        assert_eq!(flat.latencies_us, result.latencies_us);
        for (a, b) in flat.per_seed.iter().zip(&result.per_seed) {
            assert_eq!(a.completion_rate, b.success_rate);
            assert_eq!(
                (a.seed, a.n, a.primary, a.churn),
                (b.seed, b.n, b.primary, b.churn)
            );
        }
    }

    #[test]
    fn hooks_bracket_each_task_with_the_distinct_steps_and_the_signal() {
        let spec = WorkflowSpec {
            performance: Some(perf_spec()),
            ..WorkflowSpec::default()
        };
        for seed in 0..10u64 {
            let inst = spec.generate(seed).unwrap();
            let n = inst.agents.len();
            for signal in [
                OutcomeSignal::RoleCoverage,
                OutcomeSignal::Performance,
                OutcomeSignal::Both,
            ] {
                // Under always-join / never-leave the final members are the
                // pool, so the expected signal is computed off the pool here.
                let policy = Fixed::new(true, false);
                let mut lat = Vec::new();
                run_workflow_instance(&policy, &inst, signal, &mut lat).unwrap();
                let events = policy.events.lock().unwrap();
                let pool: Vec<usize> = (0..n).collect();
                let mut expected = Vec::new();
                for (t, task) in inst.tasks.iter().enumerate() {
                    let required = required_mask(&task.demand);
                    let steps: Vec<(u8, u8)> = task
                        .demand
                        .distinct()
                        .map(|s| (s.bit, s.role.index()))
                        .collect();
                    assert!(
                        steps.windows(2).all(|w| w[0] < w[1]),
                        "seed {seed}, task {t}: steps {steps:?} not ascending"
                    );
                    expected.push(Event::BeginTask { required, steps });
                    expected.extend(std::iter::repeat_n(Event::Join, n - 1));
                    expected.extend(std::iter::repeat_n(Event::Leave, n));
                    // Stated off the tags and the pool, not the scorer: with
                    // one distinct step per required bit, bit `b` is
                    // role-covered iff some agent of role `tags[b]` holds it.
                    let row = inst.performance.as_ref().unwrap()[t].as_slice();
                    let per_bit: Vec<bool> = (0..task.tags.len())
                        .map(|b| {
                            let is_required = task.required & (1u32 << b) != 0;
                            let holders = pool
                                .iter()
                                .copied()
                                .filter(|&i| (inst.agents[i].capabilities() >> b) & 1 == 1);
                            match signal {
                                OutcomeSignal::RoleCoverage => {
                                    is_required
                                        && holders.clone().any(|i| inst.roles[i] == task.tags[b])
                                }
                                OutcomeSignal::Performance => holders.clone().any(|i| row[i]),
                                OutcomeSignal::Both => {
                                    is_required
                                        && holders
                                            .clone()
                                            .any(|i| inst.roles[i] == task.tags[b] && row[i])
                                }
                            }
                        })
                        .collect();
                    expected.push(Event::ObserveOutcome { required, per_bit });
                }
                assert_eq!(
                    events.len(),
                    expected.len(),
                    "seed {seed}, {signal:?}: {} events, expected {}",
                    events.len(),
                    expected.len()
                );
                let first_diff = events.iter().zip(&expected).position(|(a, b)| a != b);
                assert!(
                    first_diff.is_none(),
                    "seed {seed}, {signal:?}: event {} is {:?}, expected {:?}",
                    first_diff.unwrap_or(0),
                    events.get(first_diff.unwrap_or(0)),
                    expected.get(first_diff.unwrap_or(0))
                );
            }
        }
    }

    #[test]
    fn display_record_round_trips_the_counts() {
        let inst = WorkflowSpec::default().generate(5).unwrap();
        let text = inst.to_string();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "seed 5");
        assert!(lines[1].starts_with(&format!(
            "n {} redraws {} ",
            inst.agents.len(),
            inst.redraws
        )));
        assert!(lines[2].starts_with("prefix_required "));
        assert_eq!(lines[2].split(',').count(), inst.tasks.len());
        assert_eq!(
            lines.iter().filter(|l| l.starts_with("agent ")).count(),
            inst.agents.len()
        );
        assert_eq!(
            lines.iter().filter(|l| l.starts_with("task ")).count(),
            inst.tasks.len()
        );
        assert_eq!(lines.last(), Some(&"performance none"));
    }
}
