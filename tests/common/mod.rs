//! Shared scaffolding for integration tests.
//!
//! Each integration test binary that wants these fixtures should declare
//! `mod common;` at the top — Cargo treats `tests/common/mod.rs` as a
//! regular module, NOT as an integration test binary of its own.

pub mod algorithms;
pub mod topology;

#[allow(dead_code)]
#[path = "../../src/test_support.rs"]
mod test_support;
#[allow(unused_imports)]
pub(crate) use test_support::log_capture;

/// A two-thread [`koalisi::core::Executor`] behind an `Arc`.
#[allow(dead_code)]
pub fn executor() -> std::sync::Arc<koalisi::core::Executor> {
    std::sync::Arc::new(
        koalisi::core::Executor::with_num_threads(2).expect("two-thread pool builds"),
    )
}
