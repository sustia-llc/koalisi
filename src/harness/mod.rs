//! The K7 round's shared plumbing (feature `harness`): seeded instance
//! generation ([`InstanceSpec`] → [`Instance`]), the battery loop
//! ([`run_instance`], [`run_battery`]) that routes every membership decision
//! through one [`CoalitionDecisionPolicy`](crate::decision::CoalitionDecisionPolicy),
//! and the report helpers (order statistics, superiority counts, the per-seed
//! and summary tables, [`Verdict`]). Registrations are
//! `examples/k7/k7_<n>.rs`, one `[[example]]` each, built over this module;
//! the module itself holds none.
//!
//! - [`rng`] — the `SplitMix64` stream, `permutation`, `distinct_bits`.
//! - [`instance`] — `InstanceSpec`, `Task`, `Instance`.
//! - [`battery`] — `SeedRange`, `InstanceResult`, `Arm`, `BatteryResult`,
//!   `run_instance`, `run_battery`, `FLAT_SIGNAL_WIDTH`.
//! - [`report`] — `percentile`, `median_iqr`, `superior_count`,
//!   `print_per_seed_table`, `print_summary`, `Verdict`.
//! - [`trace`] — `TracedPolicy`, `TraceEntry`.
//! - `workflow` *(feature `process`)* — `WorkflowSpec`, `PerformanceSpec`,
//!   `WorkflowInstance`, `WorkflowTask`, `OutcomeSignal`, `WorkflowArm`,
//!   `PolicyFactory`, `WorkflowResult`, `WorkflowBatteryResult`,
//!   `WorkflowError`, `run_workflow_instance`, `run_workflow_battery`.

pub mod battery;
pub mod instance;
pub mod report;
pub mod rng;
pub mod trace;
#[cfg(feature = "process")]
pub mod workflow;

pub use battery::{
    Arm, BatteryResult, FLAT_SIGNAL_WIDTH, InstanceResult, SeedRange, run_battery, run_instance,
};
pub use instance::{Instance, InstanceSpec, Task};
pub use report::{
    Verdict, median_iqr, percentile, print_per_seed_table, print_summary, superior_count,
};
pub use rng::{SplitMix64, distinct_bits, permutation};
pub use trace::{TraceEntry, TracedPolicy};
#[cfg(feature = "process")]
pub use workflow::{
    OutcomeSignal, PerformanceSpec, PolicyFactory, WorkflowArm, WorkflowBatteryResult,
    WorkflowError, WorkflowInstance, WorkflowResult, WorkflowSpec, WorkflowTask,
    run_workflow_battery, run_workflow_instance,
};
