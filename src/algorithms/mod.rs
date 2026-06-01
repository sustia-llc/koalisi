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
/// `CoalitionActor`), where a `Vec<&dyn AgentCapabilities>` is held across the
/// policy's async offload. Concrete agent types are typically small `Copy`
/// data, so this bound is satisfied for free.
pub trait AgentCapabilities: Send + Sync {
    fn agent_id(&self) -> usize;
    fn capabilities(&self) -> u32;
    fn trust_level(&self) -> u32;
}
