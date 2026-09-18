//! Trace instruments over a process-structured instance (feature `process`):
//! [`reconstruct`] replays a [`TraceEntry`] sequence over a
//! [`WorkflowInstance`] into per-task end states ([`TaskEnd`], [`Recon`]),
//! [`member_set_identity`] and [`roster_decomposition`] summarise
//! reconstructions as data, and [`RefPrune`] is the engine-free redundancy
//! prune policy.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use crate::algorithms::AgentCapabilities;
use crate::decision::{CoalitionDecisionPolicy, Decision, DecisionContext, TaskStart};
use crate::process::{Role, Step};

use super::trace::TraceEntry;
use super::workflow::WorkflowInstance;

/// One task's end state, reconstructed from the arrival order and the trace.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskEnd {
    /// Distinct roles in the task's demand.
    pub roster: usize,
    /// Distinct demanded `(bit, role)` steps.
    pub steps: usize,
    /// Of `steps`, those covered by a final member of the step's role.
    pub covered: usize,
    /// Final members, ascending agent id.
    pub members: Vec<usize>,
    /// Final members whose role has no step in the task's demand.
    pub off_demand: usize,
    /// Covered fraction divided by the final member count; `0` for an empty
    /// coalition or an empty demand.
    pub cov_eff: f64,
}

impl TaskEnd {
    /// `true` iff the demand is non-empty and every distinct step is covered.
    #[must_use]
    pub fn success(&self) -> bool {
        self.steps > 0 && self.covered == self.steps
    }

    /// `covered / steps`; `0` for an empty demand.
    #[must_use]
    pub fn covered_fraction(&self) -> f64 {
        if self.steps == 0 {
            0.0
        } else {
            self.covered as f64 / self.steps as f64
        }
    }
}

/// One instance's reconstructed task ends, with PRIMARY and churn recomputed
/// from them.
#[derive(Debug, Clone, PartialEq)]
pub struct Recon {
    /// One end state per task, in task order.
    pub tasks: Vec<TaskEnd>,
    /// Success rate × mean coverage efficiency over `tasks`; `0` for an
    /// instance without tasks.
    pub primary: f64,
    /// Leave entries that acted, summed over tasks.
    pub churn: usize,
}

/// Why a trace does not replay over an instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconError {
    /// The trace ran out while a task still expected an entry.
    TraceEndsEarly {
        /// The instance's seed.
        seed: u64,
        /// The task's index.
        task: usize,
    },
    /// The next entry's `leave` flag is not the one the replay expected.
    WrongKind {
        /// The instance's seed.
        seed: u64,
        /// The task's index.
        task: usize,
        /// The `leave` flag the replay expected; the entry carries its
        /// negation.
        expected_leave: bool,
    },
    /// Entries remain after the last task.
    Leftover {
        /// The instance's seed.
        seed: u64,
        /// How many entries remain.
        entries: usize,
    },
}

impl fmt::Display for ReconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TraceEndsEarly { seed, task } => {
                write!(
                    f,
                    "seed {seed}, task {task}: the trace ends inside the task"
                )
            }
            Self::WrongKind {
                seed,
                task,
                expected_leave,
            } => write!(
                f,
                "seed {seed}, task {task}: expected an entry with leave = {expected_leave}, \
                 found leave = {}",
                !expected_leave
            ),
            Self::Leftover { seed, entries } => write!(
                f,
                "seed {seed}: {entries} trace entries left after the last task"
            ),
        }
    }
}

impl std::error::Error for ReconError {}

/// Whether some agent index in `members` has role `step.role` in `inst` and
/// holds `step.bit`. An index outside `inst.roles`, and a bit at or above 32,
/// cover nothing.
#[must_use]
pub fn step_covered(inst: &WorkflowInstance, members: &[usize], step: Step) -> bool {
    let Some(mask) = step.capability_mask() else {
        return false;
    };
    members.iter().any(|&i| {
        inst.roles.get(i).is_some_and(|&r| r == step.role)
            && inst
                .agents
                .get(i)
                .is_some_and(|a| a.capabilities() & mask != 0)
    })
}

/// Replay `trace` over `inst` in the order `run_workflow_instance` calls a
/// policy: per task the first arrival joins, each later arrival consumes one
/// join entry and joins iff it acted, then each arrival that is a member
/// consumes one leave entry, in arrival order, and leaves iff it acted. Each
/// task's end state is scored as `run_workflow_instance` scores it.
///
/// # Errors
///
/// [`ReconError::TraceEndsEarly`] when the trace runs out inside a task,
/// [`ReconError::WrongKind`] when the next entry's `leave` flag is not the
/// expected one, [`ReconError::Leftover`] when entries remain after the last
/// task.
pub fn reconstruct(inst: &WorkflowInstance, trace: &[TraceEntry]) -> Result<Recon, ReconError> {
    let seed = inst.seed;
    let mut entries = trace.iter();
    let mut next = |task: usize, leave: bool| -> Result<bool, ReconError> {
        let entry = entries
            .next()
            .ok_or(ReconError::TraceEndsEarly { seed, task })?;
        if entry.leave != leave {
            return Err(ReconError::WrongKind {
                seed,
                task,
                expected_leave: leave,
            });
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
        return Err(ReconError::Leftover {
            seed,
            entries: left,
        });
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

/// `(identical, total)` over the tasks of two reconstructions compared by task
/// position: `identical` counts positions present in both whose final member
/// sets are equal, `total` is the longer task list's length.
#[must_use]
pub fn member_set_identity(a: &Recon, b: &Recon) -> (usize, usize) {
    let identical = a
        .tasks
        .iter()
        .zip(&b.tasks)
        .filter(|(x, y)| x.members == y.members)
        .count();
    (identical, a.tasks.len().max(b.tasks.len()))
}

/// Sums over the task ends of one realised roster size, or of every task.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RosterRow {
    /// The realised roster size the row holds; `None` for the row over every
    /// task.
    pub roster: Option<usize>,
    /// Tasks in the row.
    pub tasks: usize,
    /// Of `tasks`, those with [`TaskEnd::success`].
    pub successes: usize,
    /// Sum of [`TaskEnd::covered_fraction`].
    pub covered_fraction_sum: f64,
    /// Sum of [`TaskEnd::cov_eff`].
    pub cov_eff_sum: f64,
    /// Sum of final member counts.
    pub final_members: usize,
    /// Of `tasks`, those ending with no member.
    pub empty: usize,
    /// Sum of [`TaskEnd::off_demand`].
    pub off_demand: usize,
}

impl RosterRow {
    fn per_task(&self, sum: f64) -> Option<f64> {
        (self.tasks > 0).then(|| sum / self.tasks as f64)
    }

    /// `successes / tasks`; `None` for a row without tasks.
    #[must_use]
    pub fn success_rate(&self) -> Option<f64> {
        self.per_task(self.successes as f64)
    }

    /// Mean covered fraction per task; `None` for a row without tasks.
    #[must_use]
    pub fn mean_covered_fraction(&self) -> Option<f64> {
        self.per_task(self.covered_fraction_sum)
    }

    /// Mean coverage efficiency per task; `None` for a row without tasks.
    #[must_use]
    pub fn mean_cov_eff(&self) -> Option<f64> {
        self.per_task(self.cov_eff_sum)
    }

    /// Mean final member count per task; `None` for a row without tasks.
    #[must_use]
    pub fn mean_final_size(&self) -> Option<f64> {
        self.per_task(self.final_members as f64)
    }
}

/// The task ends of `recons` summed by realised roster size: `max_roster + 1`
/// rows, row `0` over every task and row `r` over the tasks of roster `r` for
/// `r` in `1..=max_roster`. A task whose roster is `0` or above `max_roster`
/// is counted in row `0` alone.
#[must_use]
pub fn roster_decomposition<'a>(
    recons: impl IntoIterator<Item = &'a Recon>,
    max_roster: usize,
) -> Vec<RosterRow> {
    let mut rows: Vec<RosterRow> = (0..=max_roster)
        .map(|r| RosterRow {
            roster: (r > 0).then_some(r),
            ..RosterRow::default()
        })
        .collect();
    for end in recons.into_iter().flat_map(|r| &r.tasks) {
        let own = (end.roster > 0).then_some(end.roster);
        for slot in std::iter::once(0).chain(own) {
            let Some(row) = rows.get_mut(slot) else {
                continue;
            };
            row.tasks += 1;
            row.successes += usize::from(end.success());
            row.covered_fraction_sum += end.covered_fraction();
            row.cov_eff_sum += end.cov_eff;
            row.final_members += end.members.len();
            row.empty += usize::from(end.members.is_empty());
            row.off_demand += end.off_demand;
        }
    }
    rows
}

/// The engine-free redundancy prune: `should_join` acts on every call;
/// `should_leave` acts iff every step of the last `begin_task` that the
/// coalition shown covers stays covered without the agent, a step
/// `(bit, role)` being covered by a member whose role in the map given to
/// [`RefPrune::new`] has index `role` and who holds `bit`. A member absent
/// from the map covers nothing, and before any `begin_task` every leave acts.
/// Every score is `0.0`.
pub struct RefPrune {
    roles: HashMap<usize, Role>,
    steps: Mutex<Vec<(u8, u8)>>,
}

impl RefPrune {
    /// A prune over the `agent_id → Role` map `roles`, with no task begun.
    #[must_use]
    pub fn new(roles: HashMap<usize, Role>) -> Self {
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

#[cfg(test)]
mod tests {
    use catgraph_applied::prop::colored::ColoredExpr;
    use catgraph_applied::prop::{Free, PropExpr};

    use super::*;
    use crate::algorithms::CapabilityAgent;
    use crate::harness::trace::TracedPolicy;
    use crate::harness::workflow::{OutcomeSignal, WorkflowTask, run_workflow_instance};
    use crate::process::{StaffingTable, WorkflowGen, chain, demand, step_expr};

    const SEED: u64 = 7;

    /// One role's steps on `bits` as a sequential chain.
    fn leg(role: Role, bits: &[u8]) -> PropExpr<WorkflowGen> {
        chain(
            bits.iter()
                .map(|&b| step_expr(Step::new(b, role)))
                .collect(),
        )
        .unwrap()
    }

    /// A task demanding the steps of `legs = [(role index, bits)]`, arriving
    /// in `arrival` order.
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
    /// bit 2 at role 1; agent 3 bit 2 at role 2.
    fn pool() -> (Vec<CapabilityAgent>, Vec<Role>) {
        (
            vec![
                CapabilityAgent::new(0, 0b001, 50),
                CapabilityAgent::new(1, 0b011, 50),
                CapabilityAgent::new(2, 0b100, 50),
                CapabilityAgent::new(3, 0b100, 50),
            ],
            vec![Role::new(0), Role::new(0), Role::new(1), Role::new(2)],
        )
    }

    fn role_map() -> HashMap<usize, Role> {
        pool().1.into_iter().enumerate().collect()
    }

    /// Task 0 demands `(0, r0)`, `(1, r0)`, `(2, r1)` with arrival 0, 1, 2, 3;
    /// task 1 demands `(1, r0)`, `(2, r2)` with arrival 3, 2, 0, 1.
    fn two_task_instance() -> WorkflowInstance {
        let (agents, roles) = pool();
        let table = StaffingTable::from_pool(
            agents
                .iter()
                .zip(&roles)
                .map(|(a, &r)| (a.capabilities(), r)),
        );
        let tasks = vec![
            task(&[(0, &[0, 1]), (1, &[2])], &[0, 1, 2, 3]),
            task(&[(0, &[1]), (2, &[2])], &[3, 2, 0, 1]),
        ];
        WorkflowInstance {
            seed: SEED,
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

    fn join(act: bool) -> TraceEntry {
        TraceEntry {
            leave: false,
            act,
            score_bits: 0,
        }
    }

    fn leave(act: bool) -> TraceEntry {
        TraceEntry {
            leave: true,
            act,
            score_bits: 0,
        }
    }

    /// Task 0: agents 1, 2, 3 join behind agent 0, then agents 0 and 2 leave.
    /// Task 1: agents 0 and 1 join behind agent 3 and agent 2 does not, then
    /// agent 0 leaves.
    fn two_task_trace() -> Vec<TraceEntry> {
        vec![
            join(true),
            join(true),
            join(true),
            leave(true),
            leave(false),
            leave(true),
            leave(false),
            join(false),
            join(true),
            join(true),
            leave(false),
            leave(true),
            leave(false),
        ]
    }

    /// Answers call `k` with `script[k].act` at score `0.0`.
    struct Scripted {
        script: Vec<TraceEntry>,
        calls: Mutex<usize>,
    }

    impl Scripted {
        fn answer(&self) -> Decision {
            let mut k = self.calls.lock().unwrap();
            let act = self.script[*k].act;
            *k += 1;
            Decision { act, score: 0.0 }
        }
    }

    impl CoalitionDecisionPolicy for Scripted {
        fn should_join(
            &self,
            _agent: &dyn AgentCapabilities,
            _coalition: &[&dyn AgentCapabilities],
            _ctx: &DecisionContext,
        ) -> Decision {
            self.answer()
        }

        fn should_leave(
            &self,
            _agent: &dyn AgentCapabilities,
            _coalition: &[&dyn AgentCapabilities],
            _ctx: &DecisionContext,
        ) -> Decision {
            self.answer()
        }
    }

    #[test]
    fn hand_derived_two_task_instance_reconstructs_to_the_hand_values() {
        let inst = two_task_instance();
        let recon = reconstruct(&inst, &two_task_trace()).unwrap();
        // Task 0 ends on {1, 3}: agent 1 covers (0, r0) and (1, r0); agent 3
        // holds bit 2 at role 2, so (2, r1) is uncovered and agent 3 is off
        // the demand; 2 of 3 steps over 2 members. Task 1 ends on {1, 3}:
        // agent 1 covers (1, r0), agent 3 covers (2, r2); 2 of 2 steps over 2
        // members.
        assert_eq!(
            recon.tasks,
            vec![
                TaskEnd {
                    roster: 2,
                    steps: 3,
                    covered: 2,
                    members: vec![1, 3],
                    off_demand: 1,
                    cov_eff: (2.0 / 3.0) / 2.0,
                },
                TaskEnd {
                    roster: 2,
                    steps: 2,
                    covered: 2,
                    members: vec![1, 3],
                    off_demand: 0,
                    cov_eff: 0.5,
                },
            ]
        );
        assert!(!recon.tasks[0].success());
        assert!(recon.tasks[1].success());
        assert_eq!(recon.tasks[0].covered_fraction(), 2.0 / 3.0);
        assert_eq!(recon.tasks[1].covered_fraction(), 1.0);
        // Success rate 1/2, mean cov_eff (1/3 + 1/2) / 2 = 5/12, PRIMARY 5/24.
        let expected: f64 = 0.5 * (((2.0 / 3.0) / 2.0 + 0.5) / 2.0);
        assert_eq!(
            recon.primary.to_bits(),
            expected.to_bits(),
            "PRIMARY {:?}, hand value {expected:?}",
            recon.primary
        );
        assert!((recon.primary - 5.0 / 24.0).abs() < 1e-15, "{recon:?}");
        assert_eq!(
            recon.churn, 3,
            "agents 0 and 2 leave task 0, agent 0 leaves task 1"
        );
    }

    #[test]
    fn reconstruction_agrees_with_the_harness_on_the_scripted_run() {
        let inst = two_task_instance();
        let script = two_task_trace();
        let inner = Scripted {
            script: script.clone(),
            calls: Mutex::new(0),
        };
        let traced = TracedPolicy::new(&inner);
        let mut lat = Vec::new();
        let result =
            run_workflow_instance(&traced, &inst, OutcomeSignal::RoleCoverage, &mut lat).unwrap();
        let trace = traced.entries();
        assert_eq!(trace, script, "the harness asks in the scripted order");
        let recon = reconstruct(&inst, &trace).unwrap();
        assert_eq!(
            recon.primary.to_bits(),
            result.primary.to_bits(),
            "PRIMARY recomputed {:?}, harness {:?}",
            recon.primary,
            result.primary
        );
        assert_eq!(
            recon.churn, result.churn,
            "churn recomputed {}, harness {}",
            recon.churn, result.churn
        );
        assert_eq!((result.success_rate, result.churn), (0.5, 3));
    }

    #[test]
    fn a_trace_that_ends_early_names_the_task_it_ended_in() {
        let inst = two_task_instance();
        let trace = two_task_trace();
        assert_eq!(
            reconstruct(&inst, &trace[..11]),
            Err(ReconError::TraceEndsEarly {
                seed: SEED,
                task: 1
            })
        );
        assert_eq!(
            reconstruct(&inst, &trace[..2]),
            Err(ReconError::TraceEndsEarly {
                seed: SEED,
                task: 0
            })
        );
        assert_eq!(
            reconstruct(&inst, &[]),
            Err(ReconError::TraceEndsEarly {
                seed: SEED,
                task: 0
            })
        );
    }

    #[test]
    fn an_entry_of_the_wrong_kind_names_the_expected_kind() {
        let inst = two_task_instance();
        let mut leave_for_join = two_task_trace();
        leave_for_join[0].leave = true;
        assert_eq!(
            reconstruct(&inst, &leave_for_join),
            Err(ReconError::WrongKind {
                seed: SEED,
                task: 0,
                expected_leave: false
            })
        );
        let mut join_for_leave = two_task_trace();
        join_for_leave[10].leave = false;
        let err = reconstruct(&inst, &join_for_leave).unwrap_err();
        assert_eq!(
            err,
            ReconError::WrongKind {
                seed: SEED,
                task: 1,
                expected_leave: true
            }
        );
        assert_eq!(
            err.to_string(),
            "seed 7, task 1: expected an entry with leave = true, found leave = false"
        );
    }

    #[test]
    fn entries_left_after_the_last_task_are_counted() {
        let inst = two_task_instance();
        let mut trace = two_task_trace();
        trace.extend([join(true), leave(false)]);
        assert_eq!(
            reconstruct(&inst, &trace),
            Err(ReconError::Leftover {
                seed: SEED,
                entries: 2
            })
        );
    }

    #[test]
    fn step_covered_needs_the_role_and_the_bit() {
        let inst = two_task_instance();
        let r1 = Role::new(1);
        assert!(step_covered(&inst, &[2], Step::new(2, r1)));
        assert!(
            !step_covered(&inst, &[3], Step::new(2, r1)),
            "agent 3 holds bit 2 at role 2"
        );
        assert!(
            !step_covered(&inst, &[0, 1], Step::new(2, Role::new(0))),
            "no role-0 agent holds bit 2"
        );
        assert!(!step_covered(&inst, &[], Step::new(2, r1)));
        assert!(
            !step_covered(&inst, &[9], Step::new(2, r1)),
            "index 9 is outside the pool"
        );
    }

    #[test]
    fn ref_prune_leave_acts_on_a_redundant_member_and_declines_on_a_needed_one() {
        let (agents, _) = pool();
        let policy = RefPrune::new(role_map());
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };
        let all: Vec<&dyn AgentCapabilities> =
            agents.iter().map(|a| a as &dyn AgentCapabilities).collect();
        policy.begin_task(&TaskStart {
            required: 0b111,
            steps: &[(0, 0), (1, 0), (2, 1)],
        });
        let acts: Vec<bool> = agents
            .iter()
            .map(|a| policy.should_leave(a, &all, &ctx).act)
            .collect();
        // Agent 0's only step (0, r0) is also agent 1's; agent 1 alone covers
        // (1, r0); agent 2 alone covers (2, r1) — agent 3 holds bit 2 at role
        // 2; agent 3 covers no demanded step.
        assert_eq!(acts, vec![true, false, false, true]);

        // A step the coalition does not cover constrains no leave.
        policy.begin_task(&TaskStart {
            required: 0b1001,
            steps: &[(0, 0), (3, 1)],
        });
        let pair: Vec<&dyn AgentCapabilities> = vec![&agents[0], &agents[1]];
        assert!(policy.should_leave(&agents[0], &pair, &ctx).act);
        let solo: Vec<&dyn AgentCapabilities> = vec![&agents[0]];
        assert!(
            !policy.should_leave(&agents[0], &solo, &ctx).act,
            "the sole coverer of (0, r0)"
        );
    }

    #[test]
    fn ref_prune_reads_roles_from_its_map_and_scores_zero() {
        let (agents, _) = pool();
        let ctx = DecisionContext::default();
        let all: Vec<&dyn AgentCapabilities> =
            agents.iter().map(|a| a as &dyn AgentCapabilities).collect();
        // Agent 1 is absent from the map, so agent 0 is the sole coverer of
        // (0, r0) and (1, r0) is covered by nobody.
        let mut partial = role_map();
        partial.remove(&1);
        let policy = RefPrune::new(partial);
        policy.begin_task(&TaskStart {
            required: 0b011,
            steps: &[(0, 0), (1, 0)],
        });
        let d = policy.should_leave(&agents[0], &all, &ctx);
        assert_eq!((d.act, d.score.to_bits()), (false, 0));
        let d = policy.should_leave(&agents[1], &all, &ctx);
        assert_eq!((d.act, d.score.to_bits()), (true, 0));
        // A bit at or above 32 is held by nobody.
        policy.begin_task(&TaskStart {
            required: 0,
            steps: &[(32, 0)],
        });
        assert!(policy.should_leave(&agents[0], &all, &ctx).act);
    }

    #[test]
    fn ref_prune_join_always_acts() {
        let (agents, _) = pool();
        let policy = RefPrune::new(role_map());
        let ctx = DecisionContext {
            required_capabilities: 0b001,
        };
        let none: Vec<&dyn AgentCapabilities> = Vec::new();
        let d = policy.should_join(&agents[3], &none, &ctx);
        assert_eq!((d.act, d.score.to_bits()), (true, 0), "no task begun");
        policy.begin_task(&TaskStart {
            required: 0b001,
            steps: &[(0, 0)],
        });
        let holders: Vec<&dyn AgentCapabilities> = vec![&agents[0], &agents[1]];
        for candidate in &agents {
            let d = policy.should_join(candidate, &holders, &ctx);
            assert_eq!(
                (d.act, d.score.to_bits()),
                (true, 0),
                "candidate {}",
                candidate.agent_id()
            );
        }
        // Before any begin_task there is no step to keep covered.
        let fresh = RefPrune::new(role_map());
        assert!(fresh.should_leave(&agents[1], &holders, &ctx).act);
    }

    #[test]
    fn member_set_identity_counts_equal_positions_over_the_longer_list() {
        let inst = two_task_instance();
        let a = reconstruct(&inst, &two_task_trace()).unwrap();
        assert_eq!(member_set_identity(&a, &a), (2, 2));
        // Task 1 without agent 0's leave ends on {0, 1, 3}.
        let mut kept = two_task_trace();
        kept[11].act = false;
        let b = reconstruct(&inst, &kept).unwrap();
        assert_eq!(b.tasks[1].members, vec![0, 1, 3]);
        assert_eq!(member_set_identity(&a, &b), (1, 2));
        let shorter = Recon {
            tasks: a.tasks[..1].to_vec(),
            primary: 0.0,
            churn: 0,
        };
        assert_eq!(member_set_identity(&a, &shorter), (1, 2));
        assert_eq!(member_set_identity(&shorter, &a), (1, 2));
    }

    fn end(roster: usize, steps: usize, covered: usize, members: &[usize], off: usize) -> TaskEnd {
        let cov_eff = if members.is_empty() || steps == 0 {
            0.0
        } else {
            (covered as f64 / steps as f64) / members.len() as f64
        };
        TaskEnd {
            roster,
            steps,
            covered,
            members: members.to_vec(),
            off_demand: off,
            cov_eff,
        }
    }

    #[test]
    fn roster_decomposition_sums_by_roster_and_over_all() {
        let first = Recon {
            tasks: vec![end(1, 2, 2, &[0], 0), end(2, 4, 2, &[1, 2], 1)],
            primary: 0.0,
            churn: 0,
        };
        let second = Recon {
            tasks: vec![
                end(3, 3, 0, &[], 0),
                end(3, 3, 3, &[0, 1, 2], 0),
                end(5, 5, 5, &[4], 0),
            ],
            primary: 0.0,
            churn: 0,
        };
        let rows = roster_decomposition([&first, &second], 4);
        assert_eq!(rows.len(), 5);
        assert_eq!(
            rows[0],
            RosterRow {
                roster: None,
                tasks: 5,
                successes: 3,
                covered_fraction_sum: 3.5,
                cov_eff_sum: 1.0 + 0.25 + 0.0 + 1.0 / 3.0 + 1.0,
                final_members: 7,
                empty: 1,
                off_demand: 1,
            }
        );
        assert_eq!(
            rows[1],
            RosterRow {
                roster: Some(1),
                tasks: 1,
                successes: 1,
                covered_fraction_sum: 1.0,
                cov_eff_sum: 1.0,
                final_members: 1,
                empty: 0,
                off_demand: 0,
            }
        );
        assert_eq!(
            rows[2],
            RosterRow {
                roster: Some(2),
                tasks: 1,
                successes: 0,
                covered_fraction_sum: 0.5,
                cov_eff_sum: 0.25,
                final_members: 2,
                empty: 0,
                off_demand: 1,
            }
        );
        assert_eq!(
            rows[3],
            RosterRow {
                roster: Some(3),
                tasks: 2,
                successes: 1,
                covered_fraction_sum: 1.0,
                cov_eff_sum: 1.0 / 3.0,
                final_members: 3,
                empty: 1,
                off_demand: 0,
            }
        );
        assert_eq!(rows[3].success_rate(), Some(0.5));
        assert_eq!(rows[3].mean_covered_fraction(), Some(0.5));
        assert_eq!(rows[3].mean_cov_eff(), Some(1.0 / 6.0));
        assert_eq!(rows[3].mean_final_size(), Some(1.5));
        assert_eq!(rows[0].success_rate(), Some(0.6));
        assert_eq!(rows[0].mean_final_size(), Some(1.4));
        // Roster 4 has no task; the roster-5 task is in row 0 alone.
        assert_eq!(
            rows[4],
            RosterRow {
                roster: Some(4),
                ..RosterRow::default()
            }
        );
        assert_eq!(rows[4].success_rate(), None);
        assert_eq!(rows[4].mean_covered_fraction(), None);
        assert_eq!(rows[4].mean_cov_eff(), None);
        assert_eq!(rows[4].mean_final_size(), None);
    }
}
