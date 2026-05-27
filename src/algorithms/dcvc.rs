//! Distributed Coalitional Value Calculation (DCVC).
//!
//! Distributes coalition value calculations across agents proportional
//! to their computational speed (trust level), achieving zero communication
//! overhead and optimal load balancing.
//!
//! Reference: Rahwan & Jennings (2007), "An algorithm for distributing
//! coalitional value calculations among cooperating agents."

use super::AgentCapabilities;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct WorkloadShare {
    pub agent_id: usize,
    pub coalition_indices: Vec<usize>,
    pub share_size: usize,
}

/// Aggregate statistics over all per-agent workload shares.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DistributionStats {
    pub min: usize,
    pub max: usize,
    pub avg: f64,
    pub total: usize,
}

pub struct DCVCDistributor {
    workload_distribution: HashMap<usize, WorkloadShare>,
}

impl DCVCDistributor {
    /// Distribute workload proportional to each agent's trust level (speed proxy).
    pub fn distribute_workload(agents: &[&dyn AgentCapabilities], total_coalitions: usize) -> Self {
        if agents.is_empty() || total_coalitions == 0 {
            return Self {
                workload_distribution: HashMap::new(),
            };
        }

        let mut workload_distribution = HashMap::new();

        let total_speed: f64 = agents.iter().map(|a| a.trust_level() as f64).sum();

        let mut start_idx = 0;

        for agent in agents {
            let speed = agent.trust_level() as f64;
            let share_fraction = speed / total_speed;
            let share_size = (total_coalitions as f64 * share_fraction).floor() as usize;

            let end_idx = (start_idx + share_size).min(total_coalitions);
            let coalition_indices: Vec<usize> = (start_idx..end_idx).collect();

            workload_distribution.insert(
                agent.agent_id(),
                WorkloadShare {
                    agent_id: agent.agent_id(),
                    coalition_indices,
                    share_size: end_idx - start_idx,
                },
            );

            start_idx = end_idx;
        }

        // Distribute remaining coalitions to fastest agents.
        if start_idx < total_coalitions {
            let remaining_indices: Vec<usize> = (start_idx..total_coalitions).collect();

            let mut sorted: Vec<&dyn AgentCapabilities> = agents.iter().copied().collect();
            sorted.sort_by(|a, b| b.trust_level().cmp(&a.trust_level()));

            for (i, &idx) in remaining_indices.iter().enumerate() {
                let agent = sorted[i % sorted.len()];
                if let Some(share) = workload_distribution.get_mut(&agent.agent_id()) {
                    share.coalition_indices.push(idx);
                    share.share_size += 1;
                }
            }
        }

        Self {
            workload_distribution,
        }
    }

    pub fn get_agent_share(&self, agent_id: usize) -> Option<&WorkloadShare> {
        self.workload_distribution.get(&agent_id)
    }

    pub fn get_all_shares(&self) -> &HashMap<usize, WorkloadShare> {
        &self.workload_distribution
    }

    pub fn calculate_statistics(&self) -> DistributionStats {
        if self.workload_distribution.is_empty() {
            return DistributionStats::default();
        }

        let (min, max, total, count) = self.workload_distribution.values().fold(
            (usize::MAX, 0usize, 0usize, 0usize),
            |(min, max, total, count), share| {
                (
                    min.min(share.share_size),
                    max.max(share.share_size),
                    total + share.share_size,
                    count + 1,
                )
            },
        );

        DistributionStats {
            min,
            max,
            avg: total as f64 / count as f64,
            total,
        }
    }

    pub fn verify_distribution(&self, total_coalitions: usize) -> bool {
        let mut covered = vec![false; total_coalitions];

        for share in self.workload_distribution.values() {
            for &idx in &share.coalition_indices {
                if idx >= total_coalitions || covered[idx] {
                    return false;
                }
                covered[idx] = true;
            }
        }

        covered.iter().all(|&c| c)
    }
}
