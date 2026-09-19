//! The battery loop: one policy over one instance's tasks, and one arm over a
//! seed range.

use std::fmt;
use std::time::Instant;

use crate::algorithms::{AgentCapabilities, CapabilityAgent};
use crate::decision::{CoalitionDecisionPolicy, DecisionContext, MemberOutcome, TaskStart};

use super::instance::{Instance, InstanceSpec};

/// Width of the flat path's per-bit outcome signal: every bit index of a
/// `u32` capability mask.
pub const FLAT_SIGNAL_WIDTH: usize = u32::BITS as usize;

/// A half-open seed range `start..end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeedRange {
    /// First seed, inclusive.
    pub start: u64,
    /// End seed, exclusive.
    pub end: u64,
}

impl SeedRange {
    /// Number of seeds in the range; `0` when `end <= start`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start) as usize
    }

    /// `true` iff the range holds no seed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    /// The seeds in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = u64> + use<> {
        self.start..self.end
    }
}

impl fmt::Display for SeedRange {
    /// Renders as `start..end`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

/// The metrics of one policy over one instance.
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceResult {
    /// The instance's seed.
    pub seed: u64,
    /// The instance's pool size.
    pub n: usize,
    /// Fraction of tasks whose final coalition covers every required bit;
    /// `0` for an instance without tasks.
    pub completion_rate: f64,
    /// Mean over tasks of coverage efficiency; `0` for an instance without
    /// tasks.
    pub mean_cov_eff: f64,
    /// `completion_rate * mean_cov_eff`.
    pub primary: f64,
    /// Number of leave-sweep removals summed over tasks.
    pub churn: usize,
}

/// Run `policy` over every task of `instance`. Per task: `begin_task` with
/// the required mask and no steps; the first arrival joins unconditionally;
/// each later arrival joins iff `should_join` acts (the coalition shown
/// excludes the candidate); then one leave sweep in arrival order over the
/// post-join membership, where `should_leave` (the coalition shown is the
/// current membership including the agent) acting removes the agent and
/// counts one churn; then `observe_outcome` with the required mask and
/// [`FLAT_SIGNAL_WIDTH`] entries, `per_bit[b]` true iff some final member
/// holds bit `b`, and one [`MemberOutcome`] per final member in membership
/// order, each with `performed` set. A task is completed iff the union of the final members'
/// capabilities covers `required`; its coverage efficiency is the fraction of
/// required bits covered (`1` for an empty requirement) divided by the final
/// member count, `0` for an empty coalition. Every `should_join` /
/// `should_leave` call's wall time in microseconds is appended to
/// `latencies_us`.
pub fn run_instance(
    policy: &dyn CoalitionDecisionPolicy,
    instance: &Instance,
    latencies_us: &mut Vec<f64>,
) -> InstanceResult {
    let agents = &instance.agents;
    let mut completed = 0usize;
    let mut cov_eff_sum = 0.0f64;
    let mut churn = 0usize;

    for task in &instance.tasks {
        policy.begin_task(&TaskStart {
            required: task.required,
            steps: &[],
        });
        let ctx = DecisionContext {
            required_capabilities: task.required,
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

        let sweep = members.clone();
        for agent in sweep {
            let coalition = views(agents, &members);
            let decision = timed(latencies_us, || {
                policy.should_leave(&agents[agent], &coalition, &ctx)
            });
            if decision.act {
                members.retain(|&m| m != agent);
                churn += 1;
            }
        }

        let union = members
            .iter()
            .fold(0u32, |acc, &m| acc | agents[m].capabilities());
        if union & task.required == task.required {
            completed += 1;
        }
        cov_eff_sum += cov_eff(union, task.required, members.len());
        let per_bit: Vec<bool> = (0..FLAT_SIGNAL_WIDTH)
            .map(|b| (union >> b) & 1 == 1)
            .collect();
        let outcomes = member_outcomes(agents, &members, |_| true);
        policy.observe_outcome(task.required, &per_bit, &outcomes);
    }

    let n_tasks = instance.tasks.len();
    let (completion_rate, mean_cov_eff) = if n_tasks == 0 {
        (0.0, 0.0)
    } else {
        (
            completed as f64 / n_tasks as f64,
            cov_eff_sum / n_tasks as f64,
        )
    };
    InstanceResult {
        seed: instance.seed,
        n: agents.len(),
        completion_rate,
        mean_cov_eff,
        primary: completion_rate * mean_cov_eff,
        churn,
    }
}

/// Capability views of the agents at `members`, in `members` order.
fn views<'a>(agents: &'a [CapabilityAgent], members: &[usize]) -> Vec<&'a dyn AgentCapabilities> {
    members
        .iter()
        .map(|&m| &agents[m] as &dyn AgentCapabilities)
        .collect()
}

/// One [`MemberOutcome`] per index `m` of `members`, in `members` order:
/// `agents[m]`'s agent id, and `performed(m)`.
pub(super) fn member_outcomes(
    agents: &[CapabilityAgent],
    members: &[usize],
    performed: impl Fn(usize) -> bool,
) -> Vec<MemberOutcome> {
    members
        .iter()
        .map(|&m| MemberOutcome {
            agent_id: agents[m].agent_id(),
            performed: performed(m),
        })
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

/// Coverage efficiency of a coalition whose capability union is `union`
/// against `required`: the covered fraction of required bits (`1` when
/// `required == 0`) divided by `member_count`; `0` when `member_count == 0`.
fn cov_eff(union: u32, required: u32, member_count: usize) -> f64 {
    if member_count == 0 {
        return 0.0;
    }
    let required_bits = required.count_ones();
    let fraction = if required_bits == 0 {
        1.0
    } else {
        f64::from((union & required).count_ones()) / f64::from(required_bits)
    };
    fraction / member_count as f64
}

/// One labelled policy under test.
pub struct Arm {
    /// Label printed in the tables.
    pub label: String,
    /// The policy every decision of the battery is routed through.
    pub policy: Box<dyn CoalitionDecisionPolicy>,
}

/// One arm's results over a seed range: the per-seed metrics in seed order
/// and every decision latency in call order.
#[derive(Debug, Clone, PartialEq)]
pub struct BatteryResult {
    /// The arm's label.
    pub label: String,
    /// One result per seed, in ascending seed order.
    pub per_seed: Vec<InstanceResult>,
    /// Wall time of every `should_join` / `should_leave` call, microseconds,
    /// in call order across all seeds.
    pub latencies_us: Vec<f64>,
}

/// Run `arm` over the instance `spec` generates for each seed in `seeds`, in
/// ascending seed order.
#[must_use]
pub fn run_battery(arm: &Arm, spec: &InstanceSpec, seeds: SeedRange) -> BatteryResult {
    let mut latencies_us = Vec::new();
    let per_seed = seeds
        .iter()
        .map(|seed| {
            let instance = spec.generate(seed);
            run_instance(arm.policy.as_ref(), &instance, &mut latencies_us)
        })
        .collect();
    BatteryResult {
        label: arm.label.clone(),
        per_seed,
        latencies_us,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algorithms::SynergisticCalculator;
    use crate::decision::{Decision, ThresholdPolicy};
    use crate::harness::instance::Task;
    use std::sync::Mutex;

    /// One policy call, in call order.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Event {
        BeginTask { required: u32, steps: Vec<(u8, u8)> },
        Join,
        Leave,
        ObserveOutcome { required: u32, per_bit: Vec<bool> },
    }

    /// Acts on join and on leave per the two flags; records every coalition
    /// (as agent ids) it is shown, per call kind, every call as an
    /// [`Event`], and the `members` of every `observe_outcome`.
    struct Fixed {
        join: bool,
        leave: bool,
        seen_join: Mutex<Vec<Vec<usize>>>,
        seen_leave: Mutex<Vec<(usize, Vec<usize>)>>,
        events: Mutex<Vec<Event>>,
        seen_members: Mutex<Vec<Vec<MemberOutcome>>>,
    }

    impl Fixed {
        fn new(join: bool, leave: bool) -> Self {
            Self {
                join,
                leave,
                seen_join: Mutex::new(Vec::new()),
                seen_leave: Mutex::new(Vec::new()),
                events: Mutex::new(Vec::new()),
                seen_members: Mutex::new(Vec::new()),
            }
        }
    }

    fn ids(coalition: &[&dyn AgentCapabilities]) -> Vec<usize> {
        coalition.iter().map(|a| a.agent_id()).collect()
    }

    impl CoalitionDecisionPolicy for Fixed {
        fn should_join(
            &self,
            _agent: &dyn AgentCapabilities,
            coalition: &[&dyn AgentCapabilities],
            _ctx: &DecisionContext,
        ) -> Decision {
            self.seen_join.lock().unwrap().push(ids(coalition));
            self.events.lock().unwrap().push(Event::Join);
            Decision {
                act: self.join,
                score: 0.0,
            }
        }

        fn should_leave(
            &self,
            agent: &dyn AgentCapabilities,
            coalition: &[&dyn AgentCapabilities],
            _ctx: &DecisionContext,
        ) -> Decision {
            self.seen_leave
                .lock()
                .unwrap()
                .push((agent.agent_id(), ids(coalition)));
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

        fn observe_outcome(
            &self,
            required: u32,
            per_bit_success: &[bool],
            members: &[MemberOutcome],
        ) {
            self.events.lock().unwrap().push(Event::ObserveOutcome {
                required,
                per_bit: per_bit_success.to_vec(),
            });
            self.seen_members.lock().unwrap().push(members.to_vec());
        }
    }

    /// The expected event sequence of one task: `BeginTask` with no steps,
    /// `n - 1` joins, `leaves` leaves, `ObserveOutcome` with `union`'s bits
    /// over [`FLAT_SIGNAL_WIDTH`] entries.
    fn expected_task_events(required: u32, n: usize, leaves: usize, union: u32) -> Vec<Event> {
        let mut out = vec![Event::BeginTask {
            required,
            steps: Vec::new(),
        }];
        out.extend(std::iter::repeat_n(Event::Join, n - 1));
        out.extend(std::iter::repeat_n(Event::Leave, leaves));
        out.push(Event::ObserveOutcome {
            required,
            per_bit: (0..FLAT_SIGNAL_WIDTH)
                .map(|b| (union >> b) & 1 == 1)
                .collect(),
        });
        out
    }

    fn pool_union(instance: &Instance) -> u32 {
        instance
            .agents
            .iter()
            .fold(0u32, |acc, a| acc | a.capabilities())
    }

    #[test]
    fn seed_range_len_iter_display() {
        let r = SeedRange { start: 3, end: 7 };
        assert_eq!(r.len(), 4);
        assert!(!r.is_empty());
        assert_eq!(r.iter().collect::<Vec<_>>(), vec![3, 4, 5, 6]);
        assert_eq!(r.to_string(), "3..7");
        let empty = SeedRange { start: 5, end: 5 };
        assert_eq!(empty.len(), 0);
        assert!(empty.is_empty());
        assert_eq!(empty.iter().count(), 0);
    }

    #[test]
    fn same_seed_gives_identical_result_sequence() {
        let spec = InstanceSpec::default();
        let seeds = SeedRange { start: 0, end: 6 };
        let arm = Arm {
            label: "syn".into(),
            policy: Box::new(ThresholdPolicy::new(SynergisticCalculator, 0.0, 0.0)),
        };
        let a = run_battery(&arm, &spec, seeds);
        let b = run_battery(&arm, &spec, seeds);
        assert_eq!(a.label, b.label);
        assert_eq!(a.per_seed.len(), seeds.len());
        assert_eq!(
            a.per_seed, b.per_seed,
            "per-seed results differ between runs"
        );
        assert_eq!(
            a.latencies_us.len(),
            b.latencies_us.len(),
            "decision counts differ between runs"
        );
        let seeds_seen: Vec<u64> = a.per_seed.iter().map(|r| r.seed).collect();
        assert_eq!(seeds_seen, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn always_join_never_leave_gives_zero_churn_and_pool_coverage_completion() {
        let spec = InstanceSpec::default();
        for seed in 0..20u64 {
            let instance = spec.generate(seed);
            let policy = Fixed::new(true, false);
            let mut latencies = Vec::new();
            let result = run_instance(&policy, &instance, &mut latencies);

            let n = instance.agents.len();
            let n_tasks = instance.tasks.len();
            let union = pool_union(&instance);
            let covered = instance
                .tasks
                .iter()
                .filter(|t| union & t.required == t.required)
                .count();
            let expected_completion = covered as f64 / n_tasks as f64;
            let expected_cov_eff = instance
                .tasks
                .iter()
                .map(|t| {
                    f64::from((union & t.required).count_ones())
                        / f64::from(t.required.count_ones())
                        / n as f64
                })
                .sum::<f64>()
                / n_tasks as f64;

            assert_eq!(result.seed, seed);
            assert_eq!(result.n, n);
            assert_eq!(
                result.churn, 0,
                "seed {seed}: never-leave must give churn 0"
            );
            assert_eq!(
                result.completion_rate, expected_completion,
                "seed {seed}: completion must equal the full-pool coverage fraction"
            );
            assert!(
                (result.mean_cov_eff - expected_cov_eff).abs() < 1e-12,
                "seed {seed}: mean_cov_eff {} != full-pool coverage / n = {expected_cov_eff}",
                result.mean_cov_eff
            );
            assert_eq!(result.primary, result.completion_rate * result.mean_cov_eff);
            assert_eq!(
                latencies.len(),
                n_tasks * (2 * n - 1),
                "seed {seed}: one latency per should_join (n-1) and should_leave (n) per task"
            );
            let leaves = policy.seen_leave.lock().unwrap();
            assert_eq!(leaves.len(), n_tasks * n);
            for (agent, coalition) in leaves.iter() {
                assert_eq!(coalition.len(), n, "leave sweep sees the full pool");
                assert!(
                    coalition.contains(agent),
                    "leave coalition includes the agent"
                );
            }
        }
    }

    #[test]
    fn never_join_sees_only_the_bootstrap_member() {
        let spec = InstanceSpec::default();
        for seed in 0..20u64 {
            let instance = spec.generate(seed);
            let policy = Fixed::new(false, false);
            let mut latencies = Vec::new();
            let result = run_instance(&policy, &instance, &mut latencies);

            let n = instance.agents.len();
            let joins = policy.seen_join.lock().unwrap();
            let leaves = policy.seen_leave.lock().unwrap();
            assert_eq!(joins.len(), instance.tasks.len() * (n - 1));
            assert_eq!(leaves.len(), instance.tasks.len());
            let mut join_iter = joins.iter();
            for (t, task) in instance.tasks.iter().enumerate() {
                let bootstrap = task.arrival[0];
                for _ in 1..n {
                    let seen = join_iter.next().expect("one join call per later arrival");
                    assert_eq!(
                        seen,
                        &vec![bootstrap],
                        "seed {seed}, task {t}: join coalition != [bootstrap]"
                    );
                }
                let (agent, coalition) = &leaves[t];
                assert_eq!(*agent, bootstrap, "seed {seed}, task {t}: leave agent");
                assert_eq!(
                    coalition,
                    &vec![bootstrap],
                    "seed {seed}, task {t}: leave coalition != [bootstrap]"
                );
            }

            let completed = instance
                .tasks
                .iter()
                .filter(|t| {
                    let caps = instance.agents[t.arrival[0]].capabilities();
                    caps & t.required == t.required
                })
                .count();
            assert_eq!(
                result.completion_rate,
                completed as f64 / instance.tasks.len() as f64,
                "seed {seed}: completion must be the bootstrap-only coverage fraction"
            );
            assert_eq!(result.churn, 0);
        }
    }

    #[test]
    fn always_leave_counts_one_churn_per_final_member() {
        let spec = InstanceSpec::default();
        for seed in 0..20u64 {
            let instance = spec.generate(seed);
            let policy = Fixed::new(true, true);
            let mut latencies = Vec::new();
            let result = run_instance(&policy, &instance, &mut latencies);
            let n = instance.agents.len();
            assert_eq!(
                result.churn,
                n * instance.tasks.len(),
                "seed {seed}: every joined member leaves once"
            );
            assert_eq!(
                result.mean_cov_eff, 0.0,
                "seed {seed}: an emptied coalition has cov_eff 0"
            );
            let leaves = policy.seen_leave.lock().unwrap();
            for (i, (_, coalition)) in leaves.iter().enumerate() {
                assert_eq!(
                    coalition.len(),
                    n - (i % n),
                    "seed {seed}: leave call {i} sees membership after earlier removals"
                );
            }
        }
    }

    #[test]
    fn hooks_bracket_each_task_and_flat_per_bit_is_the_final_union() {
        let spec = InstanceSpec::default();
        for seed in 0..20u64 {
            let instance = spec.generate(seed);
            let n = instance.agents.len();
            // (join, leave, final members after the sweep, leave calls)
            type FinalMembers = fn(&Instance, &Task) -> Vec<usize>;
            let cases: [(bool, bool, FinalMembers, usize); 3] = [
                (true, false, |inst, _| (0..inst.agents.len()).collect(), n),
                (true, true, |_, _| Vec::new(), n),
                (false, false, |_, task| vec![task.arrival[0]], 1),
            ];
            for (join, leave, final_members, leaves) in cases {
                let policy = Fixed::new(join, leave);
                let mut latencies = Vec::new();
                run_instance(&policy, &instance, &mut latencies);
                let events = policy.events.lock().unwrap();
                let mut expected = Vec::new();
                for task in &instance.tasks {
                    let union = final_members(&instance, task)
                        .iter()
                        .fold(0u32, |acc, &m| acc | instance.agents[m].capabilities());
                    expected.extend(expected_task_events(task.required, n, leaves, union));
                }
                assert_eq!(
                    events.len(),
                    expected.len(),
                    "seed {seed}, join {join}, leave {leave}: {} events, expected {}",
                    events.len(),
                    expected.len()
                );
                let first_diff = events.iter().zip(&expected).position(|(a, b)| a != b);
                assert!(
                    first_diff.is_none(),
                    "seed {seed}, join {join}, leave {leave}: event {} is {:?}, expected {:?}",
                    first_diff.unwrap_or(0),
                    events.get(first_diff.unwrap_or(0)),
                    expected.get(first_diff.unwrap_or(0))
                );
            }
        }
    }

    #[test]
    fn observe_outcome_receives_the_final_member_ids_in_order_all_performed() {
        // Agent ids differ from pool indices: index 0 is id 10, 1 is 20, 2 is 30.
        let instance = Instance {
            seed: 0,
            agents: vec![
                CapabilityAgent::new(10, 0b001, 50),
                CapabilityAgent::new(20, 0b010, 50),
                CapabilityAgent::new(30, 0b100, 50),
            ],
            tasks: vec![
                Task {
                    required: 0b111,
                    arrival: vec![2, 0, 1],
                },
                Task {
                    required: 0b011,
                    arrival: vec![1, 2, 0],
                },
            ],
        };
        let performed = |ids: &[usize]| -> Vec<MemberOutcome> {
            ids.iter()
                .map(|&agent_id| MemberOutcome {
                    agent_id,
                    performed: true,
                })
                .collect()
        };
        // (join, leave, the final member ids of task 0 and of task 1)
        let cases: [(bool, bool, [&[usize]; 2]); 3] = [
            (true, false, [&[30, 10, 20], &[20, 30, 10]]),
            (false, false, [&[30], &[20]]),
            (true, true, [&[], &[]]),
        ];
        for (join, leave, ids) in cases {
            let policy = Fixed::new(join, leave);
            let mut latencies = Vec::new();
            run_instance(&policy, &instance, &mut latencies);
            let seen = policy.seen_members.lock().unwrap().clone();
            let expected = vec![performed(ids[0]), performed(ids[1])];
            assert_eq!(
                seen, expected,
                "join {join}, leave {leave}: the hook received {seen:?}; the final members \
                 by agent id in membership order, all performed, are {expected:?}"
            );
        }
    }

    #[test]
    fn empty_task_list_gives_zero_metrics() {
        let spec = InstanceSpec {
            tasks: 0,
            ..InstanceSpec::default()
        };
        let instance = spec.generate(0);
        let policy = Fixed::new(true, false);
        let mut latencies = Vec::new();
        let result = run_instance(&policy, &instance, &mut latencies);
        assert_eq!(result.completion_rate, 0.0);
        assert_eq!(result.mean_cov_eff, 0.0);
        assert_eq!(result.primary, 0.0);
        assert!(latencies.is_empty());
        assert!(
            policy.events.lock().unwrap().is_empty(),
            "no task, so no hook call"
        );
    }
}
