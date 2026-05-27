//! Fixtures for `tests/algorithms_test.rs`.
//!
//! `Agent` carries a `trust_level` so it can be fed to DCVC/value calculators
//! that consume the `AgentCapabilities` trait.

// Shared fixture: each integration test binary that imports this module pulls
// in everything via `mod common;`, but most consumers only use a subset.
#![allow(dead_code)]

use koalisi::algorithms::AgentCapabilities;

#[derive(Debug, Copy, Clone)]
pub struct Agent {
    pub id: usize,
    pub capabilities: u32,
    pub trust_level: u32,
}

impl Agent {
    pub fn new(id: usize, capabilities: u32, trust_level: u32) -> Self {
        Self {
            id,
            capabilities,
            trust_level,
        }
    }
}

impl AgentCapabilities for Agent {
    fn agent_id(&self) -> usize {
        self.id
    }
    fn capabilities(&self) -> u32 {
        self.capabilities
    }
    fn trust_level(&self) -> u32 {
        self.trust_level
    }
}

pub fn test_agents() -> Vec<Agent> {
    vec![
        Agent::new(1, 0b001, 80),
        Agent::new(2, 0b010, 90),
        Agent::new(3, 0b100, 70),
    ]
}

pub fn as_caps(agents: &[Agent]) -> Vec<&dyn AgentCapabilities> {
    agents.iter().map(|a| a as &dyn AgentCapabilities).collect()
}
