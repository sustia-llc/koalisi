//! Categorical-magnitude coalition-decision strategy (feature `magnitude`).
//!
//! This is the categorical A/B mirror of the Active Inference arm
//! ([`super::aif_policy`], feature `decision`). Where the AIF arm scores a
//! coalition by the (negated) expected free energy `−G` of a capability-coverage
//! POMDP, this arm scores it by its **coalition magnitude** at the pinned scale
//! `t = 1` — the "effective-member diversity" of the coalition read as a
//! cospan-weighted subgraph of an enriched category. The two arms are fully
//! independent: the `magnitude` feature pulls in `catgraph-magnitude` and never
//! references `aif`, and vice versa.
//!
//! The magnitude math lives upstream in
//! [`catgraph_magnitude::coalition`](https://github.com/sustia-llc/catgraph) —
//! see that module's docs for the full citation chain. In brief: Bradley–Vigneaux
//! 2025 (*Magnitude of Language Models*, arXiv:2501.06662) §3.5 Eq (7) defines
//! magnitude as the Möbius sum `Mag(tM) = Σ_{x,y} ζ_t⁻¹(x, y)` over the induced
//! Lawvere metric space; Bradley–Terilla–Vlassopoulos 2021 (arXiv:2106.07890)
//! supply the `[0, 1]` enrichment. koalisi only *maps* coalition membership onto
//! the couplings that upstream consumes — it does not re-derive the algebra.
//!
//! # The capability→coupling mapping (the semantic heart)
//!
//! [`catgraph_magnitude::coalition_value`] takes directed couplings
//! `(from, to, prob ∈ [0, 1])` — the hom-objects `A(i, j)` of the enriched
//! category (gemini-spec §IV.5 mapping). This arm derives them as a **directed
//! substitutability / containment** relation over the *task-relevant*
//! capabilities. Writing `rel_x = capabilities_x & required_capabilities`:
//!
//! ```text
//! A(i → j) = popcount(rel_i & rel_j) / popcount(rel_i)   if rel_i != 0
//! A(i → j) = 0.0                                         if rel_i == 0
//! ```
//!
//! Read `A(i → j)` as "the fraction of `i`'s task-relevant capabilities that `j`
//! also has" — the probability that `j` can stand in for `i` on this task.
//! Agents with `rel = 0` never actually reach the coupling: they are **excluded
//! from the member set** before the magnitude computation (see rationale
//! point 4). The resulting design choices (all hand-verified against the
//! upstream Möbius math):
//!
//! 1. **Clones merge (deliberate skeletalization).** Two agents with identical
//!    relevant masks are mutually coupled at exactly `1.0`, so upstream
//!    quotients them into ONE effective agent and the magnitude is unchanged.
//!    This deliberately mirrors the AIF arm's clone degeneracy (a redundant
//!    clone leaves `G` unchanged): task-interchangeable agents *are* one
//!    effective agent.
//! 2. **Subsumption is directional.** If `rel_j ⊃ rel_i` then `A(i → j) = 1.0`
//!    but `A(j → i) < 1.0`, so the pair is *not* merged; instead the subsumed
//!    agent receives Möbius weight `0` and contributes zero diversity (hand
//!    check: `ζ = [[1, 1], [0.5, 1]] ⇒ w = (0, 1) ⇒ Mag = 1.0`).
//! 3. **Complementary specialists count fully.** Disjoint relevant masks ⇒ zero
//!    couplings ⇒ `Mag = m` (the member count).
//! 4. **Irrelevant agents are excluded.** An agent with `rel = 0` is dropped
//!    from the member set before the magnitude computation, mirroring the AIF
//!    arm (where such an agent leaves capability coverage unchanged): as a
//!    candidate it produces join margin `0` ⇒ decline; as a member it produces
//!    leave delta `0` ⇒ leave. Exclusion — not a vacuous `1.0` coupling — is
//!    load-bearing: a one-way `1.0` coupling to every member would drive the
//!    irrelevant agent's Möbius weight *negative* and collapse coalition
//!    diversity (three disjoint specialists plus one irrelevant bystander would
//!    score `1.0` instead of `3.0`, and the bystander's presence would eject a
//!    unique specialist on leave).
//! 5. **`required == 0` ⇒ every agent is irrelevant** ⇒ every coalition scores
//!    `0` ⇒ the join margin is `0` ⇒ decline, and every member's leave delta is
//!    `0` ⇒ leave (mirrors the AIF arm, whose coverage is pinned at `1.0` both
//!    sides when nothing is required: join margin `0`, leave delta `0`).
//! 6. **No cross-arm calibration.** The score is *raw* magnitude; only the
//!    within-arm rank order is meaningful. The A/B metric that compares this arm
//!    to `−G` is pre-registered on koalisi #7 — this policy does not attempt to
//!    put magnitude and free energy on a common scale.
//!
//! # Execution
//!
//! Coalition magnitude is `O(m³)` (max-product transitive closure + Gaussian
//! elimination on the `m × m` zeta table). Unlike the AIF arm — where only the
//! final EFE eval is offloaded — here the *entire* magnitude computation is the
//! CPU-heavy part, so [`should_join_async`](MagnitudePolicy::should_join_async)
//! and [`should_leave_async`](MagnitudePolicy::should_leave_async) snapshot the
//! capability masks to owned `Vec<u32>` in a sync prologue (the `&dyn` borrows
//! are not `'static`) and run the whole closure on the rayon pool via
//! [`tokio_rayon::spawn`], leaving the tokio worker unblocked.
//!
//! Upstream errors ([`CatgraphError`]) are treated as **policy-level outcomes**,
//! never panics: a value calculation logs and returns [`f64::NEG_INFINITY`]; a
//! policy decision logs and declines with score `0.0`.

use std::future::Future;
use std::pin::Pin;

use catgraph_magnitude::CatgraphError;

use crate::algorithms::{AgentCapabilities, ValueCalculator};

use super::{CoalitionDecisionPolicy, Decision, DecisionContext};

/// The capability→coupling map. Public so the mapping can be tested directly
/// (mirror of `CapabilityModel` in the AIF arm).
#[derive(Debug, Clone, Copy, Default)]
pub struct CouplingModel;

impl CouplingModel {
    /// Directed substitutability coupling `A(from → to) ∈ [0, 1]`.
    ///
    /// With `rel_x = caps_x & required`, returns
    /// `popcount(rel_from & rel_to) / popcount(rel_from)` — the fraction of
    /// `from`'s task-relevant capabilities that `to` also has. Returns `0.0`
    /// when `rel_from == 0`: an agent with nothing task-relevant has no
    /// substitutability relation to express, and the mask pipeline excludes such
    /// agents from the member set anyway (see rationale point 4 in the module
    /// docs — a vacuous `1.0` here would collapse coalition diversity). Because
    /// `rel_x` masks with `required`, capability bits outside the task
    /// requirement are ignored, and `required == 0` yields `0.0` for every pair
    /// (rationale point 5).
    #[must_use]
    pub fn coupling(from_caps: u32, to_caps: u32, required: u32) -> f64 {
        let rel_from = from_caps & required;
        let rel_to = to_caps & required;
        if rel_from == 0 {
            return 0.0;
        }
        f64::from((rel_from & rel_to).count_ones()) / f64::from(rel_from.count_ones())
    }
}

/// Deduplicate `agents` by [`AgentCapabilities::agent_id`] (first occurrence
/// wins), drop agents with no task-relevant capabilities
/// (`capabilities & required == 0`), and return the survivors' capability masks.
///
/// Upstream [`catgraph_magnitude::coalition_value`] errors on duplicate members,
/// and a double-listed agent must not double-count toward diversity, so callers
/// dedup before building couplings. Irrelevant agents are excluded — not coupled
/// vacuously — because a one-way `1.0` coupling to every member drives their
/// Möbius weight negative and collapses the coalition's diversity (module docs,
/// rationale point 4).
fn relevant_masks(agents: &[&dyn AgentCapabilities], required: u32) -> Vec<u32> {
    let mut seen = std::collections::HashSet::new();
    let mut masks = Vec::with_capacity(agents.len());
    for a in agents {
        let caps = a.capabilities();
        if caps & required != 0 && seen.insert(a.agent_id()) {
            masks.push(caps);
        }
    }
    masks
}

/// Coalition magnitude of the capability `masks` under the substitutability
/// coupling, at the pinned scale `t = 1`.
///
/// Builds `agents = 0..masks.len()` (local `usize` indices), emits a directed
/// coupling for every ordered pair `i != j` whose [`CouplingModel::coupling`] is
/// strictly positive (a zero coupling equals an absent one upstream, and a
/// self-coupling `i == j` is rejected upstream), and calls
/// [`catgraph_magnitude::coalition_value`] with all indices as members.
///
/// `masks` must be non-empty: upstream errors on an empty member set, so callers
/// guard the empty case (via [`magnitude_or_zero`]) rather than reaching here.
///
/// # Errors
///
/// Propagates any [`CatgraphError`] from the upstream magnitude computation
/// (e.g. a singular `t`-scaled zeta from rare parametric coincidences).
fn magnitude_of_masks(masks: &[u32], required: u32) -> Result<f64, CatgraphError> {
    let agents: Vec<usize> = (0..masks.len()).collect();

    let mut couplings: Vec<(usize, usize, f64)> = Vec::new();
    for (i, &from) in masks.iter().enumerate() {
        for (j, &to) in masks.iter().enumerate() {
            if i == j {
                continue;
            }
            let p = CouplingModel::coupling(from, to, required);
            if p > 0.0 {
                couplings.push((i, j, p));
            }
        }
    }

    catgraph_magnitude::coalition_value(&agents, &couplings, &agents)
}

/// Magnitude of `masks`, or `Ok(0.0)` when empty.
///
/// `Mag(∅) = 0` (Leinster's convention, and consistent with every other koalisi
/// value calculator returning `0.0` for an empty coalition). This is the
/// call-site guard that keeps an empty member set from reaching upstream, which
/// would error. "Empty" includes a coalition whose members were *all* excluded
/// as task-irrelevant by [`relevant_masks`] (e.g. when `required == 0`).
fn magnitude_or_zero(masks: &[u32], required: u32) -> Result<f64, CatgraphError> {
    if masks.is_empty() {
        return Ok(0.0);
    }
    magnitude_of_masks(masks, required)
}

/// A [`ValueCalculator`] that scores a coalition by its magnitude (effective-
/// member diversity) under the capability-substitutability coupling.
///
/// Because [`ValueCalculator::calculate_value`] has no context argument, the
/// task's `required_capabilities` is stored as a field. Agents are deduplicated
/// by `agent_id` (first occurrence wins) and task-irrelevant agents
/// (`capabilities & required == 0`) are excluded before scoring, so a
/// double-listed agent does not double-count and a bystander with nothing
/// task-relevant does not distort diversity. A coalition that is empty — or
/// whose members are all task-irrelevant — scores `0.0`; on an upstream error
/// it logs and returns [`f64::NEG_INFINITY`] (exact mirror of the AIF arm's
/// `EfeValueCalculator`).
#[derive(Debug, Clone, Copy)]
pub struct MagnitudeValueCalculator {
    pub required_capabilities: u32,
}

impl ValueCalculator for MagnitudeValueCalculator {
    fn calculate_value(&self, agents: &[&dyn AgentCapabilities]) -> f64 {
        let masks = relevant_masks(agents, self.required_capabilities);
        match magnitude_or_zero(&masks, self.required_capabilities) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(error = %e, "magnitude value calculation failed");
                f64::NEG_INFINITY
            }
        }
    }
}

/// Categorical-magnitude join/leave policy driven by [`DecisionContext`].
///
/// The agent joins when adding it *raises* coalition magnitude (diversity) by
/// more than `join_margin`, and leaves when removing it does not *lower*
/// magnitude. This mirrors the AIF arm's join/leave rule with `−G` replaced by
/// magnitude. `join_margin` defaults to `0.0`.
#[derive(Debug, Clone, Copy, Default)]
pub struct MagnitudePolicy {
    pub join_margin: f64,
}

impl MagnitudePolicy {
    /// Snapshot the deduped, task-relevant `(with, without)` capability-mask
    /// lists for a join decision. `coalition` excludes `agent`; `with` is
    /// `coalition + agent`, `without` is `coalition`, both deduped by `agent_id`
    /// and with task-irrelevant agents excluded (see [`relevant_masks`]).
    /// Returns owned `Vec<u32>` so the async path can move them into the rayon
    /// closure.
    fn join_masks(
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        required: u32,
    ) -> (Vec<u32>, Vec<u32>) {
        let masks_without = relevant_masks(coalition, required);
        let mut with: Vec<&dyn AgentCapabilities> = coalition.to_vec();
        with.push(agent);
        let masks_with = relevant_masks(&with, required);
        (masks_with, masks_without)
    }

    /// Snapshot the deduped, task-relevant `(in, out)` capability-mask lists for
    /// a leave decision. `coalition` includes `agent`; `in` is the coalition,
    /// `out` is the coalition with `agent` removed by `agent_id`, both deduped
    /// and with task-irrelevant agents excluded (see [`relevant_masks`]).
    /// Returns owned `Vec<u32>` for the async path.
    fn leave_masks(
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        required: u32,
    ) -> (Vec<u32>, Vec<u32>) {
        let masks_in = relevant_masks(coalition, required);
        let agent_id = agent.agent_id();
        let without: Vec<&dyn AgentCapabilities> = coalition
            .iter()
            .filter(|a| a.agent_id() != agent_id)
            .copied()
            .collect();
        let masks_out = relevant_masks(&without, required);
        (masks_in, masks_out)
    }

    /// Pure join decision over owned magnitude results.
    ///
    /// `with` / `without` are the coalition magnitudes with and without the
    /// candidate (the empty "without" side is pre-mapped to `Ok(0.0)`). Shared
    /// by the sync [`CoalitionDecisionPolicy::should_join`] and the async
    /// [`CoalitionDecisionPolicy::should_join_async`] override. Any upstream
    /// [`CatgraphError`] is a policy-level decline (`act = false`, `score = 0.0`)
    /// — never an unwrap/panic. A non-finite margin is guarded the same way.
    fn join_decision_from_values(
        with: Result<f64, CatgraphError>,
        without: Result<f64, CatgraphError>,
        join_margin: f64,
    ) -> Decision {
        let (with, without) = match (with, without) {
            (Ok(w), Ok(wo)) => (w, wo),
            (Err(e), _) | (_, Err(e)) => {
                tracing::warn!(error = %e, "magnitude join computation failed");
                return Decision { act: false, score: 0.0 };
            }
        };
        let margin = with - without;
        if !margin.is_finite() {
            return Decision { act: false, score: 0.0 };
        }
        Decision {
            act: margin > join_margin,
            score: margin,
        }
    }

    /// Pure leave decision over owned magnitude results.
    ///
    /// `mag_in` is the coalition magnitude with the agent present, `mag_out` the
    /// magnitude after removing it (the empty remainder is pre-mapped to
    /// `Ok(0.0)`). If removing the agent does not lower magnitude
    /// (`mag_in - mag_out <= 0`) it contributes no diversity and should leave
    /// (exact mirror of the AIF arm's `g_out - g_in <= 0`). Shared by the sync
    /// and async leave paths; upstream error or non-finite delta ⇒ decline.
    fn leave_decision_from_values(
        mag_in: Result<f64, CatgraphError>,
        mag_out: Result<f64, CatgraphError>,
    ) -> Decision {
        let (mag_in, mag_out) = match (mag_in, mag_out) {
            (Ok(i), Ok(o)) => (i, o),
            (Err(e), _) | (_, Err(e)) => {
                tracing::warn!(error = %e, "magnitude leave computation failed");
                return Decision { act: false, score: 0.0 };
            }
        };
        let delta = mag_in - mag_out;
        if !delta.is_finite() {
            return Decision { act: false, score: 0.0 };
        }
        Decision {
            act: delta <= 0.0,
            score: delta,
        }
    }
}

impl CoalitionDecisionPolicy for MagnitudePolicy {
    /// Join iff adding `agent` raises coalition magnitude by more than
    /// `join_margin`.
    ///
    /// `coalition` excludes `agent` (see [module docs](super)).
    /// `margin = Mag(coalition + agent) - Mag(coalition)` is positive when the
    /// candidate adds effective-member diversity (new task-relevant coverage or
    /// non-substitutable specialization).
    fn should_join(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let required = ctx.required_capabilities;
        let (masks_with, masks_without) = Self::join_masks(agent, coalition, required);
        let mag_with = magnitude_or_zero(&masks_with, required);
        let mag_without = magnitude_or_zero(&masks_without, required);
        Self::join_decision_from_values(mag_with, mag_without, self.join_margin)
    }

    /// Leave iff removing `agent` does not lower coalition magnitude.
    ///
    /// `coalition` includes `agent` (see [module docs](super)).
    /// `delta = Mag(coalition) - Mag(coalition - agent)`; a redundant member
    /// (a clone, or one whose task-relevant capabilities are subsumed) leaves
    /// magnitude unchanged (`delta <= 0`) and should leave. A sole member has
    /// `delta = 1 - 0 > 0` and stays.
    fn should_leave(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let required = ctx.required_capabilities;
        let (masks_in, masks_out) = Self::leave_masks(agent, coalition, required);
        let mag_in = magnitude_or_zero(&masks_in, required);
        let mag_out = magnitude_or_zero(&masks_out, required);
        Self::leave_decision_from_values(mag_in, mag_out)
    }

    /// Async, runtime-friendly override of [`should_join`](Self::should_join).
    ///
    /// Snapshots the deduped capability masks to owned `Vec<u32>` in the sync
    /// prologue (the `&dyn` borrows are not `'static`), then offloads the entire
    /// `O(m³)` magnitude computation to the rayon pool via [`tokio_rayon::spawn`]
    /// so the tokio worker thread is not blocked. Produces the same [`Decision`]
    /// as the sync [`should_join`](Self::should_join).
    ///
    /// `coalition` excludes `agent` (same convention as the sync method).
    fn should_join_async<'a>(
        &'a self,
        agent: &'a dyn AgentCapabilities,
        coalition: &'a [&'a dyn AgentCapabilities],
        ctx: &'a DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Decision> + Send + 'a>> {
        let required = ctx.required_capabilities;
        let (masks_with, masks_without) = Self::join_masks(agent, coalition, required);
        let join_margin = self.join_margin;

        Box::pin(async move {
            tokio_rayon::spawn(move || {
                let mag_with = magnitude_or_zero(&masks_with, required);
                let mag_without = magnitude_or_zero(&masks_without, required);
                Self::join_decision_from_values(mag_with, mag_without, join_margin)
            })
            .await
        })
    }

    /// Async, runtime-friendly override of [`should_leave`](Self::should_leave).
    ///
    /// Snapshots the deduped capability masks to owned `Vec<u32>` in the sync
    /// prologue, then offloads the entire `O(m³)` magnitude computation to the
    /// rayon pool via [`tokio_rayon::spawn`]. Produces the same [`Decision`] as
    /// the sync [`should_leave`](Self::should_leave).
    ///
    /// `coalition` includes `agent` (same convention as the sync method).
    fn should_leave_async<'a>(
        &'a self,
        agent: &'a dyn AgentCapabilities,
        coalition: &'a [&'a dyn AgentCapabilities],
        ctx: &'a DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Decision> + Send + 'a>> {
        let required = ctx.required_capabilities;
        let (masks_in, masks_out) = Self::leave_masks(agent, coalition, required);

        Box::pin(async move {
            tokio_rayon::spawn(move || {
                let mag_in = magnitude_or_zero(&masks_in, required);
                let mag_out = magnitude_or_zero(&masks_out, required);
                Self::leave_decision_from_values(mag_in, mag_out)
            })
            .await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy)]
    struct TestAgent {
        id: usize,
        caps: u32,
        trust: u32,
    }

    impl AgentCapabilities for TestAgent {
        fn agent_id(&self) -> usize {
            self.id
        }
        fn capabilities(&self) -> u32 {
            self.caps
        }
        fn trust_level(&self) -> u32 {
            self.trust
        }
    }

    const EPS: f64 = 1e-9;

    // -----------------------------------------------------------------------
    // 1. Upstream seam pins — call `coalition_value` directly.
    // -----------------------------------------------------------------------

    #[test]
    fn upstream_chain_and_skeletalization_pins() {
        // Chain a→b→c with couplings 0.7, 0.5 over 3 members ⇒ Mag(1) = 1.80.
        let agents = [0usize, 1, 2];
        let chain = [(0usize, 1usize, 0.7f64), (1, 2, 0.5)];
        let mag = catgraph_magnitude::coalition_value(&agents, &chain, &agents).unwrap();
        assert!(
            (mag - 1.80).abs() < EPS,
            "chain Mag(1) = {mag}, expected 1.80"
        );

        // Mutual-1.0 pair ⇒ skeletalized to ONE effective agent ⇒ Mag = 1.0.
        let pair = [0usize, 1];
        let mutual = [(0usize, 1usize, 1.0f64), (1, 0, 1.0)];
        let mag = catgraph_magnitude::coalition_value(&pair, &mutual, &pair).unwrap();
        assert!(
            (mag - 1.0).abs() < EPS,
            "mutual-1.0 Mag = {mag}, expected 1.0"
        );
    }

    // -----------------------------------------------------------------------
    // 2. The capability→coupling mapping.
    // -----------------------------------------------------------------------

    #[test]
    fn coupling_mapping() {
        // Identical relevant masks ⇒ full substitutability.
        assert!((CouplingModel::coupling(0b011, 0b011, 0b111) - 1.0).abs() < EPS);
        // Subset (from ⊂ to): all of from's bits are in to ⇒ 1.0.
        assert!((CouplingModel::coupling(0b001, 0b011, 0b111) - 1.0).abs() < EPS);
        // Superset (from ⊃ to): only half of from's bits are in to ⇒ 0.5.
        assert!((CouplingModel::coupling(0b011, 0b001, 0b111) - 0.5).abs() < EPS);
        // Disjoint ⇒ 0.0.
        assert!(CouplingModel::coupling(0b001, 0b010, 0b111).abs() < EPS);
        // Required masking: the 0b001 bit lies outside required 0b100 and is
        // ignored, so both agents look identical on the task ⇒ 1.0.
        assert!((CouplingModel::coupling(0b101, 0b100, 0b100) - 1.0).abs() < EPS);
        // rel_from == 0 (nothing task-relevant) ⇒ no substitutability relation.
        assert!(CouplingModel::coupling(0b1000, 0b111, 0b111).abs() < EPS);
        // required == 0 ⇒ 0.0 for any pair (every agent is irrelevant).
        assert!(CouplingModel::coupling(0b001, 0b010, 0).abs() < EPS);
    }

    // -----------------------------------------------------------------------
    // 3. Calculator through the `ValueCalculator` surface (hand-computed).
    // -----------------------------------------------------------------------

    #[test]
    fn calculator_hand_computed_values_and_dedup() {
        let calc = MagnitudeValueCalculator {
            required_capabilities: 0b111,
        };

        let s0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let s1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let s2 = TestAgent {
            id: 2,
            caps: 0b100,
            trust: 50,
        };

        // Disjoint specialists ⇒ zero couplings ⇒ Mag = 3.0.
        let specialists: [&dyn AgentCapabilities; 3] = [&s0, &s1, &s2];
        assert!((calc.calculate_value(&specialists) - 3.0).abs() < EPS);

        // Clone pair (distinct ids, same caps) ⇒ mutual-1.0 ⇒ 1 effective ⇒ 1.0.
        let c0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let c1 = TestAgent {
            id: 1,
            caps: 0b001,
            trust: 50,
        };
        let clones: [&dyn AgentCapabilities; 2] = [&c0, &c1];
        assert!((calc.calculate_value(&clones) - 1.0).abs() < EPS);

        // Half-overlap pair {0b011, 0b110} ⇒ mutual-0.5 ⇒ Mag = 4/3.
        let h0 = TestAgent {
            id: 0,
            caps: 0b011,
            trust: 50,
        };
        let h1 = TestAgent {
            id: 1,
            caps: 0b110,
            trust: 50,
        };
        let half: [&dyn AgentCapabilities; 2] = [&h0, &h1];
        assert!((calc.calculate_value(&half) - 4.0 / 3.0).abs() < EPS);

        // Subsumption pair {0b001, 0b011}: A(0→1)=1.0, A(1→0)=0.5 ⇒ w=(0,1) ⇒ 1.0.
        let sub0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let sub1 = TestAgent {
            id: 1,
            caps: 0b011,
            trust: 50,
        };
        let subsume: [&dyn AgentCapabilities; 2] = [&sub0, &sub1];
        assert!((calc.calculate_value(&subsume) - 1.0).abs() < EPS);

        // Ordering: disjoint (2.0) > half-overlap (4/3) > clones (1.0).
        let d0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let d1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let disjoint_pair: [&dyn AgentCapabilities; 2] = [&d0, &d1];
        let v_disjoint = calc.calculate_value(&disjoint_pair);
        let v_half = calc.calculate_value(&half);
        let v_clones = calc.calculate_value(&clones);
        assert!((v_disjoint - 2.0).abs() < EPS);
        assert!(v_disjoint > v_half && v_half > v_clones);

        // Empty slice ⇒ 0.0.
        assert!(calc.calculate_value(&[]).abs() < EPS);

        // Dedup: the same agent_id listed twice scores like once.
        let twice: [&dyn AgentCapabilities; 2] = [&s0, &s0];
        let once: [&dyn AgentCapabilities; 1] = [&s0];
        assert!((calc.calculate_value(&twice) - calc.calculate_value(&once)).abs() < EPS);

        // Object safety alongside the existing calculators.
        let _: &dyn ValueCalculator = &calc;
    }

    // -----------------------------------------------------------------------
    // 4. Policy join.
    // -----------------------------------------------------------------------

    #[test]
    fn policy_join() {
        let policy = MagnitudePolicy::default();
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };

        let cand = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let m1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let m2 = TestAgent {
            id: 2,
            caps: 0b100,
            trust: 50,
        };

        // Joining raises Mag 2.0 → 3.0 ⇒ act, score ≈ 1.0.
        let coalition: [&dyn AgentCapabilities; 2] = [&m1, &m2];
        let d = policy.should_join(&cand, &coalition, &ctx);
        assert!(d.act, "specialist should join (score={})", d.score);
        assert!(
            (d.score - 1.0).abs() < EPS,
            "join margin = {}, expected 1.0",
            d.score
        );

        // Clone candidate joining a single clone: Mag 1.0 → 1.0 ⇒ decline, ≈ 0.0.
        let clone_partner = TestAgent {
            id: 1,
            caps: 0b001,
            trust: 50,
        };
        let clone_coalition: [&dyn AgentCapabilities; 1] = [&clone_partner];
        let d = policy.should_join(&cand, &clone_coalition, &ctx);
        assert!(!d.act, "redundant clone must not join (score={})", d.score);
        assert!(
            d.score.abs() < EPS,
            "clone join margin = {}, expected 0.0",
            d.score
        );

        // required == 0 ⇒ Mag = 1 both sides ⇒ decline, ≈ 0.0.
        let ctx0 = DecisionContext {
            required_capabilities: 0,
        };
        let d = policy.should_join(&cand, &coalition, &ctx0);
        assert!(!d.act, "no requirements ⇒ no reason to join");
        assert!(
            d.score.abs() < EPS,
            "no-req join margin = {}, expected 0.0",
            d.score
        );

        // Object safety behind a trait object.
        let _: Box<dyn CoalitionDecisionPolicy> = Box::new(MagnitudePolicy::default());
    }

    // -----------------------------------------------------------------------
    // 5. Policy leave.
    // -----------------------------------------------------------------------

    #[test]
    fn policy_leave_when_redundant_else_stay() {
        let policy = MagnitudePolicy::default();
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };

        let a0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let a1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let a2 = TestAgent {
            id: 2,
            caps: 0b100,
            trust: 50,
        };

        // Unique contributor: removing a0 drops Mag 3.0 → 2.0 ⇒ delta ≈ 1.0 ⇒ stay.
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
        let stay = policy.should_leave(&a0, &full, &ctx);
        assert!(
            !stay.act,
            "unique contributor should stay (score={})",
            stay.score
        );
        assert!(
            (stay.score - 1.0).abs() < EPS,
            "stay delta = {}, expected 1.0",
            stay.score
        );

        // Redundant clone (id 3, caps 0b001) in the 4-member set: removing it
        // leaves Mag 3.0 → 3.0 ⇒ delta ≈ 0.0 ⇒ leave.
        let redundant = TestAgent {
            id: 3,
            caps: 0b001,
            trust: 50,
        };
        let with_clone: [&dyn AgentCapabilities; 4] = [&a0, &a1, &a2, &redundant];
        let leave = policy.should_leave(&redundant, &with_clone, &ctx);
        assert!(
            leave.act,
            "redundant clone should leave (score={})",
            leave.score
        );
        assert!(
            leave.score.abs() < EPS,
            "leave delta = {}, expected 0.0",
            leave.score
        );

        // Sole member: Mag 1.0 → 0.0 ⇒ delta = 1.0 > 0 ⇒ stay.
        let solo_coalition: [&dyn AgentCapabilities; 1] = [&a0];
        let d = policy.should_leave(&a0, &solo_coalition, &ctx);
        assert!(!d.act, "sole member should stay (score={})", d.score);
        assert!(
            (d.score - 1.0).abs() < EPS,
            "sole-member delta = {}, expected 1.0",
            d.score
        );
    }

    // -----------------------------------------------------------------------
    // 5b. Irrelevant agents are excluded, not vacuously coupled (review
    // regression): a bystander with no task-relevant capabilities must neither
    // collapse coalition diversity nor eject a unique specialist.
    // -----------------------------------------------------------------------

    #[test]
    fn irrelevant_bystander_neither_collapses_value_nor_corrupts_decisions() {
        let calc = MagnitudeValueCalculator {
            required_capabilities: 0b111,
        };
        let policy = MagnitudePolicy::default();
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };

        let a0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let a1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let a2 = TestAgent {
            id: 2,
            caps: 0b100,
            trust: 50,
        };
        // All of x's capability bits lie outside required 0b111.
        let bystander = TestAgent {
            id: 3,
            caps: 0b1000,
            trust: 50,
        };

        // Value: the bystander is excluded, so diversity stays 3.0 (a vacuous
        // 1.0 coupling would collapse this to 1.0 via a negative Möbius weight).
        let with_bystander: [&dyn AgentCapabilities; 4] = [&a0, &a1, &a2, &bystander];
        assert!(
            (calc.calculate_value(&with_bystander) - 3.0).abs() < EPS,
            "bystander must not distort diversity"
        );

        // Join: the bystander adds no task-relevant diversity ⇒ decline, ≈ 0.0.
        let specialists: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
        let join = policy.should_join(&bystander, &specialists, &ctx);
        assert!(!join.act, "irrelevant candidate must not join");
        assert!(join.score.abs() < EPS);

        // Leave: a unique specialist stays despite the bystander's presence
        // (pre-fix, the collapsed magnitude made delta 0 and ejected it) ...
        let stay = policy.should_leave(&a0, &with_bystander, &ctx);
        assert!(
            !stay.act,
            "unique specialist must stay despite bystander (score={})",
            stay.score
        );
        assert!((stay.score - 1.0).abs() < EPS);

        // ... while the bystander itself is redundant and leaves.
        let leave = policy.should_leave(&bystander, &with_bystander, &ctx);
        assert!(leave.act, "irrelevant member should leave");
        assert!(leave.score.abs() < EPS);

        // All-irrelevant coalition (required == 0 makes everyone irrelevant):
        // value 0.0, and every member's leave delta is 0 ⇒ leave (AIF mirror).
        let calc0 = MagnitudeValueCalculator {
            required_capabilities: 0,
        };
        assert!(calc0.calculate_value(&specialists).abs() < EPS);
        let ctx0 = DecisionContext {
            required_capabilities: 0,
        };
        let leave0 = policy.should_leave(&a0, &specialists, &ctx0);
        assert!(leave0.act, "required == 0 ⇒ every member is redundant");
        assert!(leave0.score.abs() < EPS);
    }

    // -----------------------------------------------------------------------
    // 6. Err path — a real CatgraphError flows through the pure decision fns.
    // -----------------------------------------------------------------------

    #[test]
    fn upstream_error_declines_without_panic() {
        // Self-coupling (0, 0, _) is rejected upstream ⇒ a genuine CatgraphError.
        let err =
            catgraph_magnitude::coalition_value(&[0usize], &[(0usize, 0usize, 0.5f64)], &[0usize])
                .unwrap_err();

        let join = MagnitudePolicy::join_decision_from_values(Err(err.clone()), Ok(1.0), 0.0);
        assert!(
            !join.act && join.score.abs() < EPS,
            "join: error ⇒ decline, score 0.0"
        );

        let leave = MagnitudePolicy::leave_decision_from_values(Ok(1.0), Err(err));
        assert!(
            !leave.act && leave.score.abs() < EPS,
            "leave: error ⇒ decline, score 0.0"
        );
    }

    // -----------------------------------------------------------------------
    // 7. Async == sync, and async reachable through the trait object.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn async_matches_sync_join_and_leave() {
        let policy = MagnitudePolicy::default();
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };

        let a0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let a1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let a2 = TestAgent {
            id: 2,
            caps: 0b100,
            trust: 50,
        };

        // Join: candidate a0 into {a1, a2}.
        let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2];
        let sync_join = policy.should_join(&a0, &coalition, &ctx);
        let async_join = policy.should_join_async(&a0, &coalition, &ctx).await;
        assert_eq!(sync_join, async_join, "async join must equal sync");
        assert!(async_join.act && async_join.score > 0.0);

        // Leave: unique contributor stays; redundant clone leaves.
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
        let sync_stay = policy.should_leave(&a0, &full, &ctx);
        let async_stay = policy.should_leave_async(&a0, &full, &ctx).await;
        assert_eq!(sync_stay, async_stay, "async leave (stay) must equal sync");
        assert!(!async_stay.act);

        let redundant = TestAgent {
            id: 3,
            caps: 0b001,
            trust: 50,
        };
        let with_clone: [&dyn AgentCapabilities; 4] = [&a0, &a1, &a2, &redundant];
        let sync_leave = policy.should_leave(&redundant, &with_clone, &ctx);
        let async_leave = policy
            .should_leave_async(&redundant, &with_clone, &ctx)
            .await;
        assert_eq!(
            sync_leave, async_leave,
            "async leave (leave) must equal sync"
        );
        assert!(async_leave.act);
    }

    #[tokio::test]
    async fn async_path_reachable_through_trait_object() {
        let p: Box<dyn CoalitionDecisionPolicy> = Box::new(MagnitudePolicy::default());
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };

        let a0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let a1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let a2 = TestAgent {
            id: 2,
            caps: 0b100,
            trust: 50,
        };
        let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2];

        // The dynamic-dispatch async call must produce the non-degenerate join.
        let d = p.should_join_async(&a0, &coalition, &ctx).await;
        assert!(
            d.act,
            "trait-object async join should fire (score={})",
            d.score
        );
        assert!(d.score > 0.0, "join margin must be positive");

        // And the leave override is reachable too: a0 provides unique coverage.
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
        let stay = p.should_leave_async(&a0, &full, &ctx).await;
        assert!(
            !stay.act,
            "trait-object async leave: unique contributor stays"
        );
    }
}
