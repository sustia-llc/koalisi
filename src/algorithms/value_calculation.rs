//! Coalition value calculation strategies.
//!
//! Pluggable calculators for determining the value of a coalition
//! based on member capabilities, trust levels, and synergies.

use super::AgentCapabilities;

/// Trait for calculating coalition values.
pub trait ValueCalculator {
    fn calculate_value(&self, agents: &[&dyn AgentCapabilities]) -> f64;
}

/// Additive: sum of size bonus + capability count + trust levels.
#[derive(Clone, Default)]
pub struct AdditiveCalculator;

impl ValueCalculator for AdditiveCalculator {
    fn calculate_value(&self, agents: &[&dyn AgentCapabilities]) -> f64 {
        if agents.is_empty() {
            return 0.0;
        }

        let size_bonus = agents.len() as f64 * 50.0;

        let capability_bonus: f64 = agents
            .iter()
            .map(|a| a.capabilities().count_ones() as f64 * 10.0)
            .sum();

        let trust_bonus: f64 = agents.iter().map(|a| a.trust_level() as f64).sum();

        size_bonus + capability_bonus + trust_bonus
    }
}

/// Synergistic: rewards complementary capabilities via bitwise OR union.
#[derive(Clone, Default)]
pub struct SynergisticCalculator;

impl SynergisticCalculator {
    pub fn new() -> Self {
        Self
    }
}

impl ValueCalculator for SynergisticCalculator {
    fn calculate_value(&self, agents: &[&dyn AgentCapabilities]) -> f64 {
        if agents.is_empty() {
            return 0.0;
        }

        let size_bonus = agents.len() as f64 * 50.0;

        let capability_bonus: f64 = agents
            .iter()
            .map(|a| a.capabilities().count_ones() as f64 * 10.0)
            .sum();

        let avg_trust: f64 =
            agents.iter().map(|a| a.trust_level() as f64).sum::<f64>() / agents.len() as f64;

        let combined_caps: u32 = agents.iter().map(|a| a.capabilities()).fold(0, |acc, c| acc | c);
        let synergy_bonus = combined_caps.count_ones() as f64 * 25.0;

        let team_bonus = if agents.len() >= 3 {
            agents.len() as f64 * 30.0
        } else {
            0.0
        };

        size_bonus + capability_bonus + avg_trust + synergy_bonus + team_bonus
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

        let size_component = agents.len() as f64 * 50.0 * self.size_weight;

        let capability_component: f64 = agents
            .iter()
            .map(|a| a.capabilities().count_ones() as f64 * 10.0)
            .sum::<f64>()
            * self.capability_weight;

        let trust_component: f64 = agents
            .iter()
            .map(|a| a.trust_level() as f64)
            .sum::<f64>()
            * self.trust_weight;

        let combined_caps: u32 = agents.iter().map(|a| a.capabilities()).fold(0, |acc, c| acc | c);
        let synergy_component = combined_caps.count_ones() as f64 * 25.0 * self.synergy_weight;

        size_component + capability_component + trust_component + synergy_component
    }
}
