//! Seeded instance generation: a spec of ranges, and the agent pool plus task
//! list one seed draws from it.

use std::ops::RangeInclusive;

use crate::algorithms::CapabilityAgent;

use super::rng::{SplitMix64, distinct_bits, permutation};

/// The ranges one seed draws an [`Instance`] from. Capability and requirement
/// masks are over the low `universe_bits` bits (at most 32); every range is
/// non-empty and `caps_per_agent` / `required_bits` lie within
/// `0..=universe_bits`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceSpec {
    /// Width of the capability universe in bits, at most 32.
    pub universe_bits: u32,
    /// Range the agent-pool size is drawn from.
    pub pool: RangeInclusive<usize>,
    /// Range each agent's distinct-capability count is drawn from.
    pub caps_per_agent: RangeInclusive<u32>,
    /// Range each agent's trust level is drawn from.
    pub trust: RangeInclusive<u32>,
    /// Number of tasks per instance.
    pub tasks: usize,
    /// Range each task's distinct-required-bit count is drawn from.
    pub required_bits: RangeInclusive<u32>,
}

impl Default for InstanceSpec {
    /// 8 universe bits, pool `4..=16`, capabilities `1..=4` per agent, trust
    /// `20..=99`, 20 tasks, `1..=5` required bits per task.
    fn default() -> Self {
        Self {
            universe_bits: 8,
            pool: 4..=16,
            caps_per_agent: 1..=4,
            trust: 20..=99,
            tasks: 20,
            required_bits: 1..=5,
        }
    }
}

/// One task: the required-capability mask and the order in which the pool's
/// agents (by index) arrive at it. `arrival` is a permutation of the pool
/// indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// Required-capability bitmask.
    pub required: u32,
    /// Agent indices in arrival order.
    pub arrival: Vec<usize>,
}

/// One generated instance: the seed it came from, the agent pool (agent id
/// equals its index), and the tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    /// The seed the instance was generated from.
    pub seed: u64,
    /// The agent pool; `agents[i].agent_id() == i`.
    pub agents: Vec<CapabilityAgent>,
    /// The task list, `spec.tasks` long.
    pub tasks: Vec<Task>,
}

impl InstanceSpec {
    /// Generate the instance for `seed`. Draw schedule on one `SplitMix64`
    /// stream seeded with `seed`: pool size; per agent its capability count,
    /// mask (`distinct_bits`) and trust; per task its required-bit count, mask
    /// (`distinct_bits`) and arrival permutation.
    #[must_use]
    pub fn generate(&self, seed: u64) -> Instance {
        let mut rng = SplitMix64::new(seed);

        let n = draw_in(&mut rng, *self.pool.start() as u64, *self.pool.end() as u64) as usize;
        let agents = (0..n)
            .map(|id| {
                let k = draw_in(
                    &mut rng,
                    u64::from(*self.caps_per_agent.start()),
                    u64::from(*self.caps_per_agent.end()),
                ) as u32;
                let mask = distinct_bits(&mut rng, k, self.universe_bits);
                let trust = draw_in(
                    &mut rng,
                    u64::from(*self.trust.start()),
                    u64::from(*self.trust.end()),
                ) as u32;
                CapabilityAgent::new(id, mask, trust)
            })
            .collect();

        let tasks = (0..self.tasks)
            .map(|_| {
                let r = draw_in(
                    &mut rng,
                    u64::from(*self.required_bits.start()),
                    u64::from(*self.required_bits.end()),
                ) as u32;
                let required = distinct_bits(&mut rng, r, self.universe_bits);
                let arrival = permutation(&mut rng, n);
                Task { required, arrival }
            })
            .collect();

        Instance {
            seed,
            agents,
            tasks,
        }
    }
}

/// A value in `start..=end` drawn as `start + below(end - start + 1)`, for
/// `start <= end`.
fn draw_in(rng: &mut SplitMix64, start: u64, end: u64) -> u64 {
    assert!(
        start <= end,
        "invariant: spec range {start}..={end} is non-empty"
    );
    start + rng.below(end - start + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algorithms::AgentCapabilities;

    fn narrow_spec() -> InstanceSpec {
        InstanceSpec {
            universe_bits: 6,
            pool: 2..=5,
            caps_per_agent: 2..=3,
            trust: 0..=10,
            tasks: 7,
            required_bits: 3..=4,
        }
    }

    #[test]
    fn same_seed_generates_identical_instance() {
        for spec in [InstanceSpec::default(), narrow_spec()] {
            for seed in [0u64, 1, 42, u64::MAX] {
                let a = spec.generate(seed);
                let b = spec.generate(seed);
                assert_eq!(a.seed, b.seed, "seed field differs for seed {seed}");
                assert_eq!(a.agents, b.agents, "agents differ for seed {seed}");
                assert_eq!(a.tasks, b.tasks, "tasks differ for seed {seed}");
            }
        }
    }

    #[test]
    fn different_seeds_generate_different_instances() {
        let spec = InstanceSpec::default();
        let a = spec.generate(0);
        let b = spec.generate(1);
        assert!(
            a.agents != b.agents || a.tasks != b.tasks,
            "seeds 0 and 1 produced the same pool and tasks"
        );
    }

    #[test]
    fn generate_honours_every_spec_range_over_200_seeds() {
        for spec in [InstanceSpec::default(), narrow_spec()] {
            for seed in 0..200u64 {
                let inst = spec.generate(seed);
                assert_eq!(inst.seed, seed);
                let n = inst.agents.len();
                assert!(
                    spec.pool.contains(&n),
                    "seed {seed}: pool size {n} outside {:?}",
                    spec.pool
                );
                for (i, agent) in inst.agents.iter().enumerate() {
                    assert_eq!(agent.agent_id(), i, "seed {seed}: agent id != index");
                    let caps = agent.capabilities();
                    assert!(
                        spec.caps_per_agent.contains(&caps.count_ones()),
                        "seed {seed}, agent {i}: {} caps outside {:?}",
                        caps.count_ones(),
                        spec.caps_per_agent
                    );
                    assert!(
                        caps < (1u32 << spec.universe_bits),
                        "seed {seed}, agent {i}: caps {caps:#b} exceed universe"
                    );
                    assert!(
                        spec.trust.contains(&agent.trust_level()),
                        "seed {seed}, agent {i}: trust {} outside {:?}",
                        agent.trust_level(),
                        spec.trust
                    );
                }
                assert_eq!(inst.tasks.len(), spec.tasks, "seed {seed}: task count");
                for (t, task) in inst.tasks.iter().enumerate() {
                    assert!(
                        spec.required_bits.contains(&task.required.count_ones()),
                        "seed {seed}, task {t}: {} required bits outside {:?}",
                        task.required.count_ones(),
                        spec.required_bits
                    );
                    assert!(
                        task.required < (1u32 << spec.universe_bits),
                        "seed {seed}, task {t}: required {:#b} exceeds universe",
                        task.required
                    );
                    let mut arrival = task.arrival.clone();
                    arrival.sort_unstable();
                    let identity: Vec<usize> = (0..n).collect();
                    assert_eq!(
                        arrival, identity,
                        "seed {seed}, task {t}: arrival is not a permutation of 0..{n}"
                    );
                }
            }
        }
    }
}
