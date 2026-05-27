//! Fixtures for `tests/topology_test.rs`.
//!
//! `Agent` and `Coalition` satisfy the `hypergraph` trait bounds used by
//! `TemporalHypergraph<V, HE>` (V: VertexTrait, HE: HyperedgeTrait).

// Shared fixture: each integration test binary that imports this module pulls
// in everything via `mod common;`, but most consumers only use a subset.
#![allow(dead_code)]

use std::fmt::{Display, Formatter};

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct Agent {
    pub id: &'static str,
    pub capabilities: u32,
}

impl Agent {
    pub const fn new(id: &'static str, capabilities: u32) -> Self {
        Self { id, capabilities }
    }
}

impl Display for Agent {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Agent({})", self.id)
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct Coalition {
    pub name: &'static str,
    pub formation_cost: usize,
    pub value: usize,
}

impl Coalition {
    pub const fn new(name: &'static str, formation_cost: usize, value: usize) -> Self {
        Self {
            name,
            formation_cost,
            value,
        }
    }
}

impl Display for Coalition {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Coalition({})", self.name)
    }
}

impl From<Coalition> for usize {
    fn from(c: Coalition) -> Self {
        c.formation_cost
    }
}
