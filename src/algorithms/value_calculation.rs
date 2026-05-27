//! Coalition value calculation strategies.
//!
//! Pluggable calculators for determining the value of a coalition
//! based on member capabilities, trust levels, and synergies.

use super::AgentCapabilities;

/// Per-member bonus for raw coalition size.
const SIZE_UNIT: f64 = 50.0;
/// Per-capability-bit bonus when counting individual capabilities.
const CAP_UNIT: f64 = 10.0;
/// Per-capability-bit bonus when scoring the unioned capability mask.
const SYNERGY_UNIT: f64 = 25.0;
/// Per-member team bonus applied when a coalition has 3+ members.
const TEAM_UNIT: f64 = 30.0;
/// Minimum coalition size that earns the team bonus.
const TEAM_THRESHOLD: usize = 3;

/// Trait for calculating coalition values.
pub trait ValueCalculator {
    fn calculate_value(&self, agents: &[&dyn AgentCapabilities]) -> f64;
}

fn size_bonus(agents: &[&dyn AgentCapabilities]) -> f64 {
    agents.len() as f64 * SIZE_UNIT
}

fn capability_bonus(agents: &[&dyn AgentCapabilities]) -> f64 {
    agents
        .iter()
        .map(|a| a.capabilities().count_ones() as f64 * CAP_UNIT)
        .sum()
}

fn trust_sum(agents: &[&dyn AgentCapabilities]) -> f64 {
    agents.iter().map(|a| a.trust_level() as f64).sum()
}

fn combined_capabilities(agents: &[&dyn AgentCapabilities]) -> u32 {
    agents
        .iter()
        .map(|a| a.capabilities())
        .fold(0, |acc, c| acc | c)
}

/// Additive: sum of size bonus + capability count + trust levels.
#[derive(Clone, Default)]
pub struct AdditiveCalculator;

impl ValueCalculator for AdditiveCalculator {
    fn calculate_value(&self, agents: &[&dyn AgentCapabilities]) -> f64 {
        if agents.is_empty() {
            return 0.0;
        }
        size_bonus(agents) + capability_bonus(agents) + trust_sum(agents)
    }
}

/// Synergistic: rewards complementary capabilities via bitwise OR union.
#[derive(Clone, Default)]
pub struct SynergisticCalculator;

impl ValueCalculator for SynergisticCalculator {
    fn calculate_value(&self, agents: &[&dyn AgentCapabilities]) -> f64 {
        if agents.is_empty() {
            return 0.0;
        }

        let avg_trust = trust_sum(agents) / agents.len() as f64;
        let synergy_bonus = combined_capabilities(agents).count_ones() as f64 * SYNERGY_UNIT;
        let team_bonus = if agents.len() >= TEAM_THRESHOLD {
            agents.len() as f64 * TEAM_UNIT
        } else {
            0.0
        };

        size_bonus(agents) + capability_bonus(agents) + avg_trust + synergy_bonus + team_bonus
    }
}

/// Multiplicative: agent contributions multiply each other.
#[derive(Clone)]
pub struct MultiplicativeCalculator {
    base_multiplier: f64,
}

impl MultiplicativeCalculator {
    pub fn new(base_multiplier: f64) -> Self {
        Self { base_multiplier }
    }
}

impl Default for MultiplicativeCalculator {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl ValueCalculator for MultiplicativeCalculator {
    fn calculate_value(&self, agents: &[&dyn AgentCapabilities]) -> f64 {
        if agents.is_empty() {
            return 0.0;
        }

        let product: f64 = agents
            .iter()
            .map(|a| {
                let capability_factor = a.capabilities().count_ones() as f64 + 1.0;
                let trust_factor = a.trust_level() as f64 / 100.0;
                capability_factor * trust_factor
            })
            .product();

        product * self.base_multiplier * agents.len() as f64
    }
}

/// Weighted: custom weights for size, capability, trust, and synergy factors.
///
/// Field weights are public for ergonomic construction but no invariants are
/// enforced — callers may set negative or zero weights and the calculator will
/// happily use them. Use `WeightedCalculator::balanced()` / `capability_focused()` /
/// `trust_focused()` for the common presets.
#[derive(Debug, Clone)]
pub struct WeightedCalculator {
    pub size_weight: f64,
    pub capability_weight: f64,
    pub trust_weight: f64,
    pub synergy_weight: f64,
}

impl WeightedCalculator {
    pub fn new(
        size_weight: f64,
        capability_weight: f64,
        trust_weight: f64,
        synergy_weight: f64,
    ) -> Self {
        Self {
            size_weight,
            capability_weight,
            trust_weight,
            synergy_weight,
        }
    }

    pub fn balanced() -> Self {
        Self::new(1.0, 1.0, 1.0, 1.0)
    }

    pub fn capability_focused() -> Self {
        Self::new(0.5, 2.0, 0.5, 1.5)
    }

    pub fn trust_focused() -> Self {
        Self::new(0.5, 0.5, 2.0, 0.5)
    }
}

impl Default for WeightedCalculator {
    fn default() -> Self {
        Self::balanced()
    }
}

impl ValueCalculator for WeightedCalculator {
    fn calculate_value(&self, agents: &[&dyn AgentCapabilities]) -> f64 {
        if agents.is_empty() {
            return 0.0;
        }

        let size_component = size_bonus(agents) * self.size_weight;
        let capability_component = capability_bonus(agents) * self.capability_weight;
        let trust_component = trust_sum(agents) * self.trust_weight;
        let synergy_component =
            combined_capabilities(agents).count_ones() as f64 * SYNERGY_UNIT * self.synergy_weight;

        size_component + capability_component + trust_component + synergy_component
    }
}
