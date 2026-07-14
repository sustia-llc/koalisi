//! Coalition formation algorithms.
//!
//! - [`value_calculation`] — pluggable value calculators for coalitions
//! - [`dcvc`] — Distributed Coalitional Value Calculation (fair workload distribution)
//! - [`aipa`] — Anytime Integer Partition Algorithm (partition-based coalition search)

pub mod aipa;
pub mod dcvc;
pub mod value_calculation;

pub use aipa::{
    IntegerPartition, PartitionBounds, compute_all_partition_bounds, compute_partition_avg_bound,
    compute_partition_min_bound, compute_partition_upper_bound, find_best_partition,
    generate_integer_partitions, partition_count, verify_partition,
};
pub use dcvc::{DCVCDistributor, DistributionStats, WorkloadShare};
pub use value_calculation::{
    AdditiveCalculator, MultiplicativeCalculator, SynergisticCalculator, ValueCalculator,
    WeightedCalculator,
};

/// Trait for types that expose agent capabilities and trust level.
///
/// Implement this on your agent type so the coalition algorithms
/// (value calculators, DCVC workload distribution) can operate generically.
///
/// The `Send + Sync` supertrait bound lets `&dyn AgentCapabilities` capability
/// views cross `.await` points and thread boundaries — required by the async
/// decision seam (`CoalitionManager::try_join_coalition` /
/// `CoalitionService`), where a `Vec<&dyn AgentCapabilities>` is held across the
/// policy's async offload. Concrete agent types are typically small `Copy`
/// data, so this bound is satisfied for free.
pub trait AgentCapabilities: Send + Sync {
    fn agent_id(&self) -> usize;
    fn capabilities(&self) -> u32;
    fn trust_level(&self) -> u32;
}

/// A minimal, ready-made [`AgentCapabilities`] implementation: an agent id, a
/// capability bitmask, and a trust level.
///
/// This is the canonical concrete agent for demos, tests, and simple callers —
/// when coalition value should come from *capability coverage* (each agent
/// covers some `capabilities` bits), this is all you need. The derives
/// (`Copy + Eq + Debug + Send + Sync`) also satisfy the topology `VertexTrait`
/// blanket bound, so a `CapabilityAgent` can be added to a `CoalitionManager`
/// directly as a graph vertex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityAgent {
    id: usize,
    capabilities: u32,
    trust: u32,
}

impl CapabilityAgent {
    /// Build an agent covering `capabilities` (a bitmask) with the given
    /// `trust` level.
    #[must_use]
    pub fn new(id: usize, capabilities: u32, trust: u32) -> Self {
        Self {
            id,
            capabilities,
            trust,
        }
    }
}

impl AgentCapabilities for CapabilityAgent {
    fn agent_id(&self) -> usize {
        self.id
    }
    fn capabilities(&self) -> u32 {
        self.capabilities
    }
    fn trust_level(&self) -> u32 {
        self.trust
    }
}
