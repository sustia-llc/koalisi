//! Shared scaffolding for integration tests.
//!
//! Each integration test binary that wants these fixtures should declare
//! `mod common;` at the top — Cargo treats `tests/common/mod.rs` as a
//! regular module, NOT as an integration test binary of its own.

pub mod algorithms;
pub mod topology;
