//! Population-based coalition-structure search atop AIPA (`SwarmAgentic` idea #4,
//! sustia-llc/koalisi#42).
//!
//! This is a deterministic, LLM-free search over **coalition structures** —
//! set-partitions of the `n` agents — that maximises total value
//! `Σ over blocks of ValueCalculator::calculate_value(block members)`.
//!
//! [AIPA](super::aipa) supplies the *scaffolding*: the integer-partition shapes
//! (block-size multisets) seed a diverse initial population. A `SwarmAgentic`-style
//! particle swarm then evolves full assignments with deterministic velocity
//! heuristics — personal-best / global-best pulls plus random mutation, driven by
//! a single `SplitMix64` stream (no `rand` dependency, fully reproducible from
//! [`PopulationConfig::seed`]).
//!
//! The [`search`] core is **pure and synchronous** (CPU-bound; offload it via
//! `tokio_rayon` from an async caller if you need to). Recording the improving
//! global-best lineage into a [`CoalitionManager`] is a **separate async** step,
//! [`record_trajectory`], so the search stays decoupled from the topology layer
//! and each improving epoch becomes a queryable point in the temporal hypergraph.
//!
//! # Which value models make this non-trivial
//!
//! The search is only interesting when the fitness has an *interior* optimum —
//! some grouping strictly beats both the grand coalition and all-singletons. The
//! built-in [`AdditiveCalculator`](super::AdditiveCalculator) is **constant**
//! across every set-partition (its size / capability / trust terms all sum to the
//! same total regardless of grouping), and
//! [`SynergisticCalculator`](super::SynergisticCalculator) /
//! [`MultiplicativeCalculator`](super::MultiplicativeCalculator) are effectively
//! split-favouring (all-singletons optimal), so with those the answer is
//! degenerate and the lineage is one epoch. Coverage-style calculators (reward
//! full coverage of a required capability set, penalise redundant members — see
//! `examples/population_search.rs`) and the decision-layer arms (magnitude / EFE /
//! the #41 feedback calculator) are where the swarm does real work.
//!
//! ```
//! use koalisi::algorithms::{CapabilityAgent, PopulationConfig, SynergisticCalculator, search};
//!
//! let agents = [
//!     CapabilityAgent::new(0, 0b001, 80),
//!     CapabilityAgent::new(1, 0b010, 70),
//!     CapabilityAgent::new(2, 0b100, 90),
//! ];
//! let cfg = PopulationConfig::default().with_seed(7);
//! let outcome = search(&agents, &SynergisticCalculator, &cfg);
//!
//! // The best structure partitions all agents; its fitness is the sum of block values.
//! assert_eq!(outcome.best.blocks().iter().map(Vec::len).sum::<usize>(), 3);
//! // The lineage always starts with the initial global best and only ever improves.
//! assert_eq!(outcome.best, *outcome.lineage.last().unwrap());
//! ```

use std::collections::HashMap;

use super::{AgentCapabilities, ValueCalculator};
use super::aipa::generate_integer_partitions;
use crate::topology::{CoalitionManager, HyperedgeTrait, TemporalResult, VertexIndex, VertexTrait};

/// `SplitMix64` — the reference constant-schedule PRNG shared across koalisi's
/// deterministic harnesses (see `examples/strategy_comparison.rs`). Seeded once
/// from [`PopulationConfig::seed`]; every draw advances by the golden-ratio gamma
/// and mixes, so the whole search is a pure function of the seed.
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// A uniform `f64` in `[0, 1)` from the top 53 bits of one draw (mirrors the
/// `next_unit` convention in `examples/strategy_comparison.rs`).
#[allow(clippy::cast_precision_loss)]
fn next_unit(rng: &mut SplitMix64) -> f64 {
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// A uniform integer in `0..bound` (`bound` ≥ 1) from one draw.
#[allow(clippy::cast_possible_truncation)]
fn gen_index(rng: &mut SplitMix64, bound: usize) -> usize {
    (rng.next_u64() % bound as u64) as usize
}

/// A candidate coalition structure: a set-partition of `n` agents.
///
/// `assignment[i]` is the block id of agent index `i`. The assignment is always
/// **canonicalised** — block ids are dense (`0..k`) and assigned in
/// first-occurrence order, so `[2, 2, 5, 2]` normalises to `[0, 0, 1, 0]`. Two
/// structures are equal exactly when they induce the same partition.
#[derive(Clone, Debug, PartialEq)]
pub struct CoalitionStructure {
    /// Block id per agent index (dense, first-occurrence canonical form).
    pub assignment: Vec<usize>,
    /// Total value: `Σ over blocks of ValueCalculator::calculate_value(block)`.
    pub fitness: f64,
}

impl CoalitionStructure {
    /// Group agent indices by block, returning blocks in ascending block-id order.
    ///
    /// Because the assignment is canonical, every returned block is non-empty and
    /// the members within each block are in ascending agent-index order.
    #[must_use]
    pub fn blocks(&self) -> Vec<Vec<usize>> {
        assignment_blocks(&self.assignment)
    }
}

/// Tuning parameters for [`search`].
///
/// `p_gbest` and `p_pbest` are the per-agent probabilities of copying the global
/// best / personal best block id during a velocity step; the remaining mass is
/// random reassignment. No invariant forces `p_gbest + p_pbest <= 1.0` — if their
/// sum exceeds 1 the random-mutation branch simply never fires.
#[derive(Clone, Copy, Debug)]
pub struct PopulationConfig {
    /// Number of particles (full candidate structures) in the swarm.
    pub population: usize,
    /// Number of velocity-update iterations to run.
    pub iterations: usize,
    /// Per-agent probability of pulling toward the global best.
    pub p_gbest: f64,
    /// Per-agent probability of pulling toward the particle's personal best.
    pub p_pbest: f64,
    /// Seed for the single `SplitMix64` stream — the search is a pure function of it.
    pub seed: u64,
}

impl PopulationConfig {
    /// Construct a config from explicit parameters.
    #[must_use]
    #[allow(clippy::similar_names)] // p_gbest / p_pbest are the documented field names
    pub fn new(population: usize, iterations: usize, p_gbest: f64, p_pbest: f64, seed: u64) -> Self {
        Self {
            population,
            iterations,
            p_gbest,
            p_pbest,
            seed,
        }
    }

    /// Copy of `self` with a different seed (ergonomic for seeded test sweeps).
    #[must_use]
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }
}

impl Default for PopulationConfig {
    fn default() -> Self {
        Self {
            population: 32,
            iterations: 100,
            p_gbest: 0.4,
            p_pbest: 0.3,
            seed: 0,
        }
    }
}

/// The result of a [`search`].
#[derive(Clone, Debug, PartialEq)]
pub struct SearchOutcome {
    /// The best coalition structure found.
    pub best: CoalitionStructure,
    /// The global-best **improvement lineage**: the first entry is the initial
    /// global best (best of the seeded population), and each subsequent entry is a
    /// structure that strictly improved the global best. `best == *lineage.last()`.
    pub lineage: Vec<CoalitionStructure>,
    /// Number of velocity-update iterations executed (equal to
    /// [`PopulationConfig::iterations`]).
    pub iterations_run: usize,
}

/// Relabel `assignment` in place to dense, first-occurrence canonical block ids.
fn canonicalize(assignment: &mut [usize]) {
    let mut remap: HashMap<usize, usize> = HashMap::new();
    let mut next = 0usize;
    for slot in assignment.iter_mut() {
        let id = *remap.entry(*slot).or_insert_with(|| {
            let v = next;
            next += 1;
            v
        });
        *slot = id;
    }
}

/// Group agent indices by block id, dropping any empty block so the result is
/// always a true set-partition (non-empty blocks in ascending block-id order).
/// Canonical assignments have no gaps, so the `retain` is a no-op for them; it
/// only guards the public [`CoalitionStructure::blocks`] path against a
/// hand-built non-canonical `assignment` (the field is `pub`).
fn assignment_blocks(assignment: &[usize]) -> Vec<Vec<usize>> {
    let Some(&max_id) = assignment.iter().max() else {
        return Vec::new();
    };
    let mut blocks = vec![Vec::new(); max_id + 1];
    for (agent, &block) in assignment.iter().enumerate() {
        blocks[block].push(agent);
    }
    blocks.retain(|b| !b.is_empty());
    blocks
}

/// Total fitness of an assignment: the sum of each block's calculator value.
fn fitness<A, C>(agents: &[A], calc: &C, assignment: &[usize]) -> f64
where
    A: AgentCapabilities,
    C: ValueCalculator,
{
    assignment_blocks(assignment)
        .iter()
        .map(|block| {
            let views: Vec<&dyn AgentCapabilities> =
                block.iter().map(|&i| &agents[i] as &dyn AgentCapabilities).collect();
            calc.calculate_value(&views)
        })
        .sum()
}

/// Seed `population` canonical assignments by cycling over the integer-partition
/// shapes of `n` (AIPA scaffolding): particle `p` realises `shapes[p % shapes.len()]`
/// by shuffling `0..n` and cutting the permutation into blocks of exactly those
/// sizes. Every seeded particle's block-size multiset is therefore an integer
/// partition of `n`.
fn seed_assignments(n: usize, population: usize, rng: &mut SplitMix64) -> Vec<Vec<usize>> {
    let shapes = generate_integer_partitions(n);
    (0..population)
        .map(|p| {
            let shape = &shapes[p % shapes.len()];
            // Fisher–Yates shuffle of 0..n.
            let mut perm: Vec<usize> = (0..n).collect();
            for i in (1..n).rev() {
                let j = gen_index(rng, i + 1);
                perm.swap(i, j);
            }
            // Cut the permutation into blocks of the shape's sizes.
            let mut assignment = vec![0usize; n];
            let mut pos = 0usize;
            for (block_id, &size) in shape.iter().enumerate() {
                for _ in 0..size {
                    assignment[perm[pos]] = block_id;
                    pos += 1;
                }
            }
            canonicalize(&mut assignment);
            assignment
        })
        .collect()
}

/// A swarm particle: its current position and its personal best.
struct Particle {
    current: Vec<usize>,
    pbest: CoalitionStructure,
}

/// Search for a high-value coalition structure over `agents`.
///
/// Total value being maximised is `Σ over blocks of calc.calculate_value(block)`.
/// The population is seeded from AIPA integer-partition shapes, then evolved with
/// a deterministic `SwarmAgentic`-style PSO velocity heuristic: for each agent, copy the global-best
/// block id with probability `p_gbest`, else the personal-best block id with
/// probability `p_pbest`, else pick a random block id in `0..=current_max + 1`
/// (the `+1` allowing a fresh singleton). Copying canonical block ids is a
/// heuristic pull toward the gbest / pbest *grouping* — an exact set-partition
/// velocity (a language-driven rewrite) is the LLM meta-layer's job (issue #20);
/// this is simply a sensible deterministic improver.
///
/// Returns the best structure plus the global-best improvement lineage (see
/// [`SearchOutcome`]). An empty agent slice yields a trivial outcome (empty
/// assignment, fitness `0.0`); `population` is clamped to at least 1.
#[must_use]
pub fn search<A, C>(agents: &[A], calc: &C, cfg: &PopulationConfig) -> SearchOutcome
where
    A: AgentCapabilities,
    C: ValueCalculator,
{
    let n = agents.len();
    if n == 0 {
        let best = CoalitionStructure {
            assignment: Vec::new(),
            fitness: 0.0,
        };
        return SearchOutcome {
            lineage: vec![best.clone()],
            best,
            iterations_run: 0,
        };
    }

    let population = cfg.population.max(1);
    let mut rng = SplitMix64::new(cfg.seed);

    // Seed the population from AIPA shapes and evaluate each particle.
    let mut particles: Vec<Particle> = seed_assignments(n, population, &mut rng)
        .into_iter()
        .map(|assignment| {
            let fit = fitness(agents, calc, &assignment);
            Particle {
                pbest: CoalitionStructure {
                    assignment: assignment.clone(),
                    fitness: fit,
                },
                current: assignment,
            }
        })
        .collect();

    // Initial global best = best personal best (first strict max wins ⇒ deterministic).
    let mut gbest = particles[0].pbest.clone();
    for particle in &particles[1..] {
        if particle.pbest.fitness > gbest.fitness {
            gbest = particle.pbest.clone();
        }
    }
    let mut lineage = vec![gbest.clone()];

    for _ in 0..cfg.iterations {
        for particle in &mut particles {
            let max_id = particle.current.iter().copied().max().unwrap_or(0);
            let mut next: Vec<usize> = (0..n)
                .map(|i| {
                    let r = next_unit(&mut rng);
                    if r < cfg.p_gbest {
                        gbest.assignment[i]
                    } else if r < cfg.p_gbest + cfg.p_pbest {
                        particle.pbest.assignment[i]
                    } else {
                        // 0..=max_id + 1 — the top id is a fresh block (new singleton).
                        gen_index(&mut rng, max_id + 2)
                    }
                })
                .collect();
            canonicalize(&mut next);
            let fit = fitness(agents, calc, &next);

            if fit > particle.pbest.fitness {
                particle.pbest = CoalitionStructure {
                    assignment: next.clone(),
                    fitness: fit,
                };
            }
            if fit > gbest.fitness {
                gbest = CoalitionStructure {
                    assignment: next.clone(),
                    fitness: fit,
                };
                lineage.push(gbest.clone());
            }
            particle.current = next;
        }
    }

    SearchOutcome {
        best: gbest,
        lineage,
        iterations_run: cfg.iterations,
    }
}

/// Record a global-best improvement `lineage` into `manager` as successive epochs
/// of the temporal hypergraph.
///
/// Each agent in `agents` is added as a vertex (agent index `i` ↦ the `i`-th
/// [`VertexIndex`] handed out). Then, for each lineage epoch in order, the
/// previous epoch's coalitions are dissolved and one hyperedge is formed per block
/// (every canonical block has size ≥ 1). Each coalition carries `HE::default()` as
/// its metadata weight. After recording, the final epoch's coalitions are live and
/// can be replayed with [`TemporalQueries`](crate::topology::TemporalQueries) /
/// [`CoalitionManager::coalition_members_at`].
///
/// Call this on a fresh (or agent-free) manager: it adds `agents` unconditionally,
/// so reusing a populated manager would duplicate vertices. Every structure in
/// `lineage` must be over the same `agents` (i.e. each `assignment.len() ==
/// agents.len()`) — pass the exact slice handed to [`search`]; a shorter `agents`
/// slice would index out of bounds.
///
/// # Errors
///
/// Propagates any [`TemporalError`](crate::topology::TemporalError) from the
/// underlying vertex / hyperedge mutations.
pub async fn record_trajectory<V, HE>(
    manager: &CoalitionManager<V, HE>,
    agents: &[V],
    lineage: &[CoalitionStructure],
) -> TemporalResult<()>
where
    V: VertexTrait + Clone + 'static,
    HE: HyperedgeTrait + Clone + Default + 'static,
{
    // 1. Materialise agents as vertices, keeping the index map.
    let mut vmap: Vec<VertexIndex> = Vec::with_capacity(agents.len());
    for &agent in agents {
        vmap.push(manager.add_agent(agent).await?);
    }

    // 2. Replay each improving epoch: dissolve the previous coalitions, form new ones.
    let mut prev: Vec<crate::topology::HyperedgeIndex> = Vec::new();
    for epoch in lineage {
        for &edge in &prev {
            manager.dissolve_coalition(edge).await?;
        }
        prev.clear();

        for block in epoch.blocks() {
            // Canonical blocks are never empty, but guard defensively.
            if block.is_empty() {
                continue;
            }
            let members: Vec<VertexIndex> = block.iter().map(|&i| vmap[i]).collect();
            let edge = manager.form_coalition(members, HE::default()).await?;
            prev.push(edge);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algorithms::{CapabilityAgent, SynergisticCalculator};
    use std::collections::HashSet;

    fn demo_agents() -> Vec<CapabilityAgent> {
        vec![
            CapabilityAgent::new(0, 0b0001, 80),
            CapabilityAgent::new(1, 0b0010, 70),
            CapabilityAgent::new(2, 0b0100, 90),
            CapabilityAgent::new(3, 0b1000, 60),
            CapabilityAgent::new(4, 0b0011, 75),
        ]
    }

    #[test]
    fn canonicalize_is_dense_first_occurrence_and_idempotent() {
        let mut a = vec![2, 2, 5, 2];
        canonicalize(&mut a);
        assert_eq!(a, vec![0, 0, 1, 0], "dense ids in first-occurrence order");

        // First occurrence of 7 precedes 3, so 7 -> 0 and 3 -> 1.
        let mut b = vec![7, 3, 7, 3, 9];
        canonicalize(&mut b);
        assert_eq!(b, vec![0, 1, 0, 1, 2]);

        // Idempotent: canonical input is unchanged.
        let mut c = b.clone();
        canonicalize(&mut c);
        assert_eq!(c, b);
    }

    #[test]
    fn blocks_group_by_id_in_order() {
        let cs = CoalitionStructure {
            assignment: vec![0, 1, 0, 2, 1],
            fitness: 0.0,
        };
        assert_eq!(cs.blocks(), vec![vec![0, 2], vec![1, 4], vec![3]]);

        let empty = CoalitionStructure {
            assignment: Vec::new(),
            fitness: 0.0,
        };
        assert!(empty.blocks().is_empty());
    }

    #[test]
    fn seeded_particles_realise_aipa_shapes() {
        let n = 5;
        let shapes: HashSet<Vec<usize>> =
            generate_integer_partitions(n).into_iter().collect();
        let mut rng = SplitMix64::new(123);
        let seeds = seed_assignments(n, 32, &mut rng);

        for assignment in &seeds {
            // Recover the block-size multiset, non-increasing (partition form).
            let mut sizes: Vec<usize> = assignment_blocks(assignment)
                .iter()
                .map(Vec::len)
                .collect();
            sizes.sort_unstable_by(|a, b| b.cmp(a));
            assert!(
                shapes.contains(&sizes),
                "seeded block sizes {sizes:?} must be an integer partition of {n}"
            );
            assert_eq!(assignment.len(), n);
        }
    }

    #[test]
    fn single_agent_is_one_block() {
        let agents = vec![CapabilityAgent::new(0, 0b1, 50)];
        let outcome = search(&agents, &SynergisticCalculator, &PopulationConfig::default());
        assert_eq!(outcome.best.assignment, vec![0]);
        assert_eq!(outcome.best.blocks(), vec![vec![0]]);
        assert_eq!(outcome.lineage.len(), 1);
    }

    #[test]
    fn lineage_is_strictly_increasing() {
        let agents = demo_agents();
        let outcome = search(&agents, &SynergisticCalculator, &PopulationConfig::default());
        for pair in outcome.lineage.windows(2) {
            assert!(
                pair[1].fitness > pair[0].fitness,
                "lineage fitness must strictly increase: {} !> {}",
                pair[1].fitness,
                pair[0].fitness
            );
        }
        assert_eq!(outcome.best, *outcome.lineage.last().unwrap());
    }
}
