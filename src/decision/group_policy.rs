//! Role-slotted Active Inference **group** coalition-decision policy (features
//! `decision` + `process`) — the `grp-*` arm of pre-registration K4-EQ5b
//! (`docs/prereg-K4-eq5b-typed-two-engine.md`, **including Amendments 1–5**,
//! which are the spec of record wherever they conflict with §3/§4).
//!
//! Where [`PersistentAifArm`] decides as *one* agent reading a *flat* capability
//! mask, this arm decides over a **workflow's** `(bit, role)` [`Demand`]: an
//! [`aif::GroupAgent`] with a [`CopyAgent`](aif::CopyAgent) sensory slot, one
//! internal agent per demanding role, and a [`VotingAgent`](aif::VotingAgent)
//! active slot in [`CertaintyWeighted`](aif::VotingMode::CertaintyWeighted) mode
//! over `n_actions = 2` (`0` = decline, `1` = act).
//!
//! # What this arm actually is (Amendment A5.1 — read this first)
//!
//! It is **not** "a group of role specialists deliberating about a candidate".
//! It is **a group of role-restricted coverage queries, of which only the
//! candidate's own role internal is candidate-sensitive; the remainder vote on
//! their role's coalition coverage.**
//!
//! That follows from A2.1's registered role-matched masks and is measured, not
//! inferred. [`coverage_masks`](GroupAifPolicy::coverage_masks) contributes the
//! candidate's capabilities only to the internal whose role the candidate holds,
//! so for every other internal:
//!
//! - on **leave**, `cfg0 == cfg1` exactly — that query carries *zero* information
//!   about the candidate;
//! - on **join**, the candidate appears nowhere in either mask.
//!
//! **And the candidate-blind internals do not abstain — they vote
//! maximum-confidence "act".** [`run_replay`] records control 0 before every
//! observation, so `update_a` accumulates only into the **membership-0** columns
//! of `pA`; the membership-1 half stays permanently less observed and A-novelty
//! rewards switching. Indifference therefore resolves as *act*, sharpened by
//! `α = 8` into a delta — and a zero-entropy delta carries
//! `confidence_weight = exp(0) = 1.0`, the **maximum** CW weight, so a blind
//! member outweighs a hesitant informed one.
//!
//! This is a **registered structural property, disclosed and measured** — not a
//! defect repaired mid-flight. Restricting the roster to candidate-sensitive
//! internals would force `R = 1` on every decision, i.e. under role-matched masks
//! a group deliberating about one candidate is structurally impossible, and that
//! is itself the finding. [`GroupAifCounters`] carries the four mandatory
//! disclosures: the zero-candidate-sensitive rate, the mean candidate-blind
//! count, **their CW weight share**, and the leave-path `cfg0 == cfg1` rate.
//!
//! # The five things worth knowing before using it
//!
//! **1. The internals are arm-E1 queries, built by arm-E1's own constructor.**
//! SP1 pins internal `r` as "arm-E1's existing query construction with
//! `required_r` substituted for `required`", where
//! `required_r = { b : (b, r) ∈ distinct demand }`. That is literally what happens
//! — [`PersistentAifArm::role_query`] is the same `build_query` the single-agent
//! arm calls, coverage-masked `initial_pa` injection and all. Nothing is
//! re-derived here, so the claim "this is arm-E1's mechanism in a group" is true
//! rather than approximated.
//!
//! **2. The specialisation lives in the world models, not in member continuity**
//! (Amendment A1.1). An arm-E1 query bakes the *this-candidate* coverage masks
//! into its matrices and the engine exposes no way to re-parameterize or replace a
//! built member, so members are **single-use on every leg**. What separates the
//! confirmatory cells from the reference cells is instead
//! [`WorldModelTopology`]: `R` role-specialised persistent world models (each
//! learning only from its own role's bits) versus **one shared** model read
//! through three role-restricted views.
//!
//! *Scope note (A5.10 L3-14):* "each learning only from its own role's bits" holds
//! because the as-written v2w world gives every required bit exactly **one** role
//! tag, so the role models observe disjoint bit sets. A rewriting arm that let one
//! bit carry two roles would break the disjointness, and this sentence must not be
//! transcribed into one.
//!
//! **3. There is no commit.** D4a is **struck** (Amendment A1.1): with members
//! discarded after each decision,
//! [`record_group_action`](aif::GroupAgent::record_group_action) would advance
//! nothing, so this arm never calls it and never calls
//! `group_distribution_recording` either. The MMP same-timestep double-update
//! hazard of koalisi **gotcha 32** is thereby *designed out* rather than gated —
//! a single-use member cannot be read twice. That hazard still applies to any
//! future arm that gives its members continuity.
//!
//! **4. A role with no demand does not vote** (Amendment A1.3). It leaves the
//! roster for that task, so the realised `R` varies 3 → 2 → 1. Realised roster
//! sizes and the empty-slot rate are a **mandatory disclosure**
//! ([`GroupAifCounters`]).
//!
//! A1.3's original *rationale* — "an abstaining member reads `[0.5, 0.5]` and
//! carries weight `exp(−ln 2) = 0.5`, dragging `p(act)` toward the threshold" —
//! is **WITHDRAWN as measurably false** (A5.1). A member with no
//! candidate-discriminating coverage reads ≈ 1.0 at weight 1.0, and its bias is
//! toward **act**, not toward the threshold. The *decision* to drop empty-demand
//! roles stands: the hazard was real and dropping is still right; only its stated
//! size and its **sign** were wrong.
//!
//! **5. Upstream errors are outcomes, not panics.** An [`aif::AifError`] anywhere
//! on the decision path is a decline **and a count**
//! ([`GroupAifCounters::declines_upstream`]) — never a panic, never a silent
//! fallback to some other arm's answer. Per the standing disclosure, a zero score
//! is **not** a decline: read the counters, never the score.
//!
//! # Coverage is role-matched (Amendment A2.1)
//!
//! SP1 substitutes `required_r` and was silent on the coverage masks; A2.1
//! registers them. A step `(b, r)` is covered iff some member **of role `r`** holds
//! bit `b` — the world's own `p9_step_covered` — so internal `r` is built with
//! `cfg0`/`cfg1` restricted to role-`r` participants. A role-blind union would let
//! a role-2 worker "cover" a role-0 step, scoring the query against a different
//! notion of coverage than the world it is evaluated in, and would leave the three
//! internals differing only in `required_r` — specialists in name only.
//!
//! Its cost is the A5.1 property above, and it is a cost the registration accepts
//! with its eyes open rather than one it failed to notice.
//!
//! [`CoverageMasks::RoleBlind`] exists as exactly that contrast, and is the
//! non-gating `grp-role-blind` reference leg: it isolates how much of a margin
//! comes from role-matched coverage versus role-restricted demand alone.
//!
//! # Determinism
//!
//! [`group_distribution`](aif::GroupAgent::group_distribution) draws no
//! randomness: members are [`POMDPAgent`](aif::POMDPAgent)s behind a deterministic
//! wrapper and the sensory slot is a `CopyAgent`, which are the conditions gotcha
//! 32 pins RNG-freedom on. Seeds are threaded for hygiene only; nothing draws from
//! them.
//!
//! *Correction (A5.10 L3-12):* it is **not** the only RNG-free path —
//! `group_distribution_recording` is equally RNG-free. The reason this arm avoids
//! that one is **D4a and gotcha 32**: it advances each member to its own argmax,
//! which is a per-member surrogate rather than the group's resolved action. Only
//! [`Agent::act`](aif::Agent::act) samples, and only the exploratory
//! [`DecisionRead::SeededSampling`] cell calls it.

use std::collections::HashMap;
use std::sync::Mutex;

use nalgebra::DVector;

use crate::algorithms::AgentCapabilities;
use crate::process::{Demand, Role, Step};

use super::aif_persistent_policy::{
    PersistentAifArm, PersistentAifConfig, PersistentAifState, low_mask, run_replay, set_bits,
    splitmix64,
};
use super::{CoalitionDecisionPolicy, Decision, DecisionContext, MemberOutcome, TaskStart};

// --- Pinned constants ------------------------------------------------------

/// The group's action space (SP3): `0` = decline, `1` = act.
pub const GROUP_N_ACTIONS: usize = 2;

/// Index of the "act" action within [`GROUP_N_ACTIONS`].
const ACTION_ACT: usize = 1;

/// The registered role count `R` (D2).
pub const DEFAULT_N_ROLES: usize = 3;

/// S-learn (i)'s **prereg-pinned** non-vacuity tolerance (Amendment A5.4),
/// compared on Dirichlet pA counts.
///
/// §5 twice described this as prereg-pinned while it existed only as a harness
/// local — a RUN-INVALID gate may not carry an unregistered parameter, so it lives
/// here, beside the state it measures, and [`models_moved`] is the only
/// implementation of the comparison.
pub const S_LEARN_VACUITY_TOL: f64 = 1e-9;

/// What S-learn (i)'s non-vacuity guard found (Amendment **A6.1**).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NonVacuity {
    /// Every model that was **asked to learn** moved by more than the tolerance.
    pub ok: bool,
    /// Models exempted because `expected == 0` — the world never gave their role
    /// anything to do, so they *cannot* move. **Disclosed, never silently
    /// dropped:** an exempt model says a role was unstaffable on that seed, which
    /// is information about the world.
    pub exempt: Vec<ModelLabel>,
}

/// S-learn (i)'s non-vacuity guard: every world model **with `expected > 0`** has
/// pA counts differing from the reference by more than [`S_LEARN_VACUITY_TOL`]
/// somewhere.
///
/// # Why `expected > 0` and not "all" (Amendment A6.1)
///
/// The first official run returned `RUN-INVALID` on this guard alone, with the
/// counted ledger 30/30 exact. Two seeds carried a role model at
/// `expected == 0, updates == 0, max |pA delta| == 0e0` — never asked to learn,
/// because `p8_task_feasible` requires a pool worker **of the tagged role**
/// holding each required bit, so a role absent from the pool can never be legally
/// tagged. Requiring such a model to move is requiring the impossible, and at
/// `P(role absent) = 3·(2/3)^n − 3·(1/3)^n` (0.553 at `n = 4`, ~12.5 % of seeds)
/// **no seed block passes the uncorrected guard**.
///
/// The predicate keys on `expected == 0` rather than on pool-absence because one
/// seed in 3000 reached zero demand with the role *present* — a lone worker
/// covering a single bit. Only the `expected` reading catches both.
///
/// **A5.4's `any → all` stands** for the models that remain in scope: it is right
/// for catching a frozen model hiding behind a learning sibling, and `expected > 0`
/// is exactly the separator between *"asked and frozen"* (a real defect) and
/// *"never asked"* (a world fact).
///
/// Necessary, and explicitly **not sufficient** — the counted ledger in
/// [`GroupAifCounters::model_updates`] is the gate. Learning-off configurations
/// carry no pA at all; an in-scope model without counts is reported as *not
/// moved*, which is the honest reading for a guard that exists to catch a frozen
/// model.
#[must_use]
pub fn models_moved(
    after: &[(ModelLabel, PersistentAifState)],
    before: &[(ModelLabel, PersistentAifState)],
    audits: &[ModelUpdateAudit],
) -> NonVacuity {
    let mut exempt = Vec::new();
    let mut ok = !after.is_empty() && after.len() == before.len();
    let mut in_scope = 0usize;
    for (i, ((label, a), (_, b))) in after.iter().zip(before.iter()).enumerate() {
        // A model with no audit row is treated as in scope: a missing row means
        // the ledger and the snapshots disagree about how many models exist, and
        // silently exempting it would hide that.
        if audits.get(i).is_some_and(|r| r.expected == 0) {
            exempt.push(*label);
            continue;
        }
        in_scope += 1;
        let moved = match (a.pa.as_ref(), b.pa.as_ref()) {
            (Some(x), Some(y)) => x
                .iter()
                .zip(y.iter())
                .any(|(p, q)| (p - q).iter().any(|d| d.abs() > S_LEARN_VACUITY_TOL)),
            _ => false,
        };
        ok &= moved;
    }
    // Every model exempt means nothing was asked to learn at all — vacuous in the
    // strongest sense, and not something the exemption is meant to permit.
    NonVacuity {
        ok: ok && in_scope > 0,
        exempt,
    }
}

/// The scalar handed to [`aif::GroupAgent::group_distribution`].
///
/// **Inert by construction.** It flows through the `CopyAgent` sensory slot into
/// each member's [`InternalAgent::action_probabilities`](aif::InternalAgent),
/// which this module implements as the arm-E1 *multi-modality* replay (Amendment
/// A1.2) and which therefore ignores it. The group surface offers no way to pass
/// one observation per modality; carrying the replay vector on the member is how
/// arm-E1's A1.4 two-task replay survives into the group.
const SENSORY_OBSERVATION: usize = 0;

// --- Configuration ---------------------------------------------------------

/// How many persistent world models the arm keeps, and what each learns from
/// (Amendment A1.1 — this is where D2a's contrast now lives).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldModelTopology {
    /// `R` models, one per role, each observing **only its own role's bits** —
    /// the **confirmatory** legs `grp-role` / `grp-mult`.
    RoleSpecialised,
    /// One model observing the whole task, read through `R` role-restricted
    /// views — the non-gating reference legs `grp-role-fresh` / `grp-mult-fresh`.
    Shared,
}

/// Which precision channel drives the internals (D3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrecisionChannel {
    /// SP1 alone: internal `r` sees only its role's `(b, r)` demand.
    RoleRestricted,
    /// SP1 **plus** SP2: each modality's injected Dirichlet counts are scaled by
    /// the occurrence multiplicity `m(b, r)` of that step in the declared writing.
    ///
    /// # The sign, corrected (Amendment A5.8)
    ///
    /// SP2's registered justification — "three occurrences carry three
    /// observations' worth of evidence **demand**" — has the sign **backwards**,
    /// and the word "demand" is **withdrawn**. Dirichlet concentration is evidence
    /// **possessed**, not evidence **wanted**, so a higher `m` means *less*
    /// parameter-information-gain left to collect and therefore **less** epistemic
    /// pull: measured, `p(act)` falls `0.999999991 → 0.501759150` as `m` goes
    /// 1 → 3. **A repeated step makes the arm markedly less willing to staff it.**
    /// The mechanism is unchanged; only its justification was wrong.
    ///
    /// Also disclosed: the scale multiplies the **whole** modality block, including
    /// the *uncovered* configuration's flat `[1, 1, 1]` prior — asserting
    /// pseudo-observations never made, in precisely the block whose novelty drives
    /// v5's mechanism.
    ///
    /// # Which channel it moves (Amendment A5.7)
    ///
    /// `A = column_normalize(pA)` is invariant under a positive uniform scale in
    /// ℝ, so SP2 moves the **concentration** — the novelty /
    /// parameter-information-gain term, v5's validated mechanism —
    /// **predominantly**, not *purely*. Two caveats, both measured: `update_a`
    /// re-derives `A` after adding an **unscaled** `η·joint`, so a scaled block is
    /// a damped learning rate and perturbs `A` at read time (novelty off, learning
    /// on: `0.500000000 → 0.500002287`, i.e. the novelty channel dominates by ~5
    /// orders of magnitude); and the invariance is only ulp-accurate in `f64`.
    ///
    /// Inert with **learning** off is confirmed bit-identical — no counts are
    /// injected then. The registered base has learning on.
    MultiplicityWeighted,
}

/// Whose capabilities count toward internal `r`'s coverage masks `cfg0`/`cfg1`
/// (Amendment **A2.1**).
///
/// SP1 pinned `required_r` and was silent on the masks; A2.1 registers
/// [`RoleMatched`](Self::RoleMatched) and adds [`RoleBlind`](Self::RoleBlind) as a
/// single non-gating reference leg (`grp-role-blind`) at the
/// [`RoleRestricted`](PrecisionChannel::RoleRestricted) channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageMasks {
    /// **Registered.** Only role-`r` participants can staff a role-`r` step, which
    /// is the world's own predicate (`p9_step_covered` requires the member's role
    /// to equal the step's role *and* the member to hold the bit). Anything else
    /// would score the query against a different notion of coverage than the world
    /// it is evaluated in.
    RoleMatched,
    /// **Reference leg only.** Every participant's capabilities count toward every
    /// internal's masks. Isolates how much of a margin comes from role-matched
    /// coverage versus role-restricted demand alone — under this setting the three
    /// internals differ **only** in `required_r`, specialists in name only.
    RoleBlind,
}

/// How the group's action is read (D4 + its registered exploratory contrast).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionRead {
    /// **Registered (D4).** [`group_distribution`](aif::GroupAgent::group_distribution) —
    /// no RNG drawn, no `last_action` advanced. `score` is the real margin
    /// `p(act) − 0.5`.
    Deterministic,
    /// **Exploratory `E-seed` only, non-gating.** Decide by the shipped
    /// [`Agent::act`](aif::Agent::act) draw, measuring what sampling costs against
    /// argmax. Seeded, so it stays reproducible.
    ///
    /// **This cell reports no margin.** `act` hands back a drawn action and no
    /// distribution, so `score` encodes the *draw* (`±0.5`) rather than a
    /// confidence — synthesising a margin from a sample would be inventing a
    /// number. Read `act` here, never `score`; PRIMARY, churn and superiority all
    /// depend on `act` alone, which is exactly what E-seed asks about.
    SeededSampling,
}

/// The active slot's voting mode (D2 + its registered `A3.1` contrast).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupVote {
    /// **Registered (D2).** The confidence-weighted mixture. `score` is a real
    /// margin `p(act) − 0.5`.
    CertaintyWeighted,
    /// **`grp-role-det` reference leg only, non-gating (Amendment A3.1).**
    ///
    /// D2 pinned CW as *forced* on the argument that only it carries a continuous
    /// margin at R ≤ 3 over 2 actions. Off-block smoke measured that premise
    /// **false on this world** — the CW mixture's margin median is 0.5, i.e. the
    /// mixture is itself a delta, because arm-E1's query posteriors saturate at
    /// ±0.5 (gotcha 25) and the internals inherit it. This leg measures what CW
    /// actually bought rather than conceding the point.
    ///
    /// **Its read is not a margin** — and it is not `k/R` either
    /// (**Amendment A5.6**). `VotingAgent::vote_distribution`'s *first* branch,
    /// which is the one a `Deterministic` slot takes, returns **uniform mass over
    /// the max-count winner set**; `counts/total` is the `Probabilistic` branch.
    /// So at `n_actions = 2` the read lives on **`{0, 0.5, 1}`** — never
    /// `{0, ⅓, ⅔, 1}` — with `0.5` reachable only on an exact vote tie.
    ///
    /// SP3 over that support therefore acts **iff `winners == {act}`**, i.e. a
    /// strict majority of the realised roster: majority at `R = 3`, unanimity at
    /// `R = 2`, and the single voter's own argmax at `R = 1`. The `score` is a
    /// winner-set indicator, **not commensurable** with a CW margin — compare this
    /// cell on **acts and PRIMARY**, never on score bits.
    Deterministic,
}

impl GroupVote {
    fn upstream(self) -> aif::VotingMode {
        match self {
            Self::CertaintyWeighted => aif::VotingMode::CertaintyWeighted,
            Self::Deterministic => aif::VotingMode::Deterministic,
        }
    }
}

/// How the role internals' outputs reach the active slot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VoteRouting {
    /// The active slot is the bare [`VotingAgent`](aif::VotingAgent): every
    /// internal's own output is aggregated.
    Off,
    /// The active slot is an [`aif::RoutedAggregator`] over an [`aif::Topology`]
    /// built per decision on the realised roster, members indexed by roster
    /// position, `hops = 1`, every member read out in roster order. The centre
    /// is the roster position of the candidate's role. With the centre on the
    /// roster its row is the identity row and every other member's row places
    /// `lambda` on the centre and `1 − lambda` on itself; with the centre off
    /// the roster every row is the identity row. `lambda` ranges over `[0, 1]`;
    /// `0` makes every row the identity row, `1` makes every row the centre's
    /// output.
    CandidateStar {
        /// Weight each non-centre row places on the centre's output.
        lambda: f64,
    },
}

/// Registered configuration of the group arm. The [`Default`] is the
/// **confirmatory `grp-role`** cell over arm-E1's registered **v5 E1** base.
#[derive(Debug, Clone, Copy)]
pub struct GroupAifConfig {
    /// World-model topology (Amendment A1.1).
    pub topology: WorldModelTopology,
    /// Precision channel (D3).
    pub channel: PrecisionChannel,
    /// Whose capabilities count toward a role internal's coverage masks
    /// (Amendment A2.1). Confirmatory cells run
    /// [`CoverageMasks::RoleMatched`]; only the `grp-role-blind` reference leg
    /// differs.
    pub masks: CoverageMasks,
    /// How the group's action is read (D4). Every gating cell runs
    /// [`DecisionRead::Deterministic`]; only the exploratory `E-seed` cell differs.
    pub read: DecisionRead,
    /// The active slot's voting mode (D2). Every gating cell runs
    /// [`GroupVote::CertaintyWeighted`]; only the `grp-role-det` reference leg
    /// differs (Amendment A3.1).
    pub vote: GroupVote,
    /// How the internals' outputs reach the active slot. A setting other than
    /// [`VoteRouting::Off`] is accepted only with [`DecisionRead::Deterministic`]
    /// and [`GroupVote::CertaintyWeighted`].
    pub routing: VoteRouting,
    /// The role count `R` the arm is configured for (D2: 3). Roles are indexed
    /// `0..n_roles`; a demand step naming a role outside that range is a
    /// [`GroupAifError::RoleOutOfRange`] at [`begin_task`](GroupAifPolicy::begin_task).
    pub n_roles: usize,
    /// The per-role world model's own configuration.
    ///
    /// **The default here is the v5 E1 arm, not [`PersistentAifConfig::default`]** —
    /// that default is the FALSIFIED v4 configuration (`query_dynamics: true`), and
    /// `EQ5b` contests the group *shape* over the validated model, not a re-tuned
    /// model (gotcha 24's relabelling discipline).
    pub base: PersistentAifConfig,
}

/// arm-E1's registered v5 configuration: learned per-bit precisions, `MeanField`
/// queries at the engine-default fixed γ = 16, novelty on, no precision dynamics.
#[must_use]
pub fn v5_e1_base() -> PersistentAifConfig {
    PersistentAifConfig {
        persistent_learning: true,
        query_dynamics: false,
        query_novelty: true,
        query_gamma: None,
        ..PersistentAifConfig::default()
    }
}

impl Default for GroupAifConfig {
    fn default() -> Self {
        Self {
            topology: WorldModelTopology::RoleSpecialised,
            channel: PrecisionChannel::RoleRestricted,
            masks: CoverageMasks::RoleMatched,
            read: DecisionRead::Deterministic,
            vote: GroupVote::CertaintyWeighted,
            routing: VoteRouting::Off,
            n_roles: DEFAULT_N_ROLES,
            base: v5_e1_base(),
        }
    }
}

// --- Errors ----------------------------------------------------------------

/// What can go wrong preparing a task for the group arm.
///
/// Hand-rolled `Display`/[`Error`](std::error::Error), in the same style as
/// `process::errors` and `topology::errors` — koalisi carries no `thiserror`
/// dependency.
#[derive(Debug)]
pub enum GroupAifError {
    /// A demand step names a role outside the configured `0..n_roles`.
    RoleOutOfRange {
        /// The offending role index.
        role: u8,
        /// The configured role count.
        n_roles: usize,
    },
    /// No `(bit, role)` of the demand lands inside the world model's `n_bits`
    /// universe, so no role could be staffed and no internal could be built.
    EmptyDemand {
        /// The world model's capability-bit width.
        n_bits: usize,
    },
    /// [`DecisionRead::SeededSampling`] combined with a discrete
    /// [`GroupVote`] — constructible, and silently all-declining, so it is
    /// refused (Amendment A5.10 L1-14).
    ///
    /// `Agent::act` routes a discrete mode through `VotingAgent::aggregate`, whose
    /// winner resolution does not agree with the sampled-mixture contract E-seed
    /// registers, and the pairing is not a registered cell in any case.
    UnsupportedReadVote,
    /// [`VoteRouting::CandidateStar`] with a `lambda` that is non-finite or
    /// outside `[0, 1]`.
    InvalidRoutingWeight {
        /// The offending weight.
        lambda: f64,
    },
    /// A routing other than [`VoteRouting::Off`] combined with
    /// [`DecisionRead::SeededSampling`] or with a [`GroupVote`] other than
    /// [`GroupVote::CertaintyWeighted`].
    UnsupportedRouting,
    /// The engine rejected a model.
    Aif(aif::AifError),
}

impl std::fmt::Display for GroupAifError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RoleOutOfRange { role, n_roles } => write!(
                f,
                "group arm: demand names role r{role}, outside the configured R = {n_roles}"
            ),
            Self::EmptyDemand { n_bits } => write!(
                f,
                "group arm: no demanded (bit, role) lies inside the {n_bits}-bit universe"
            ),
            Self::UnsupportedReadVote => write!(
                f,
                "group arm: seeded sampling is registered only against the \
                 CertaintyWeighted active slot"
            ),
            Self::InvalidRoutingWeight { lambda } => write!(
                f,
                "group arm: candidate-star routing weight {lambda} is not a finite value in [0, 1]"
            ),
            Self::UnsupportedRouting => write!(
                f,
                "group arm: vote routing is accepted only with the deterministic read \
                 and the CertaintyWeighted active slot"
            ),
            Self::Aif(inner) => write!(f, "group arm: engine rejection: {inner}"),
        }
    }
}

impl std::error::Error for GroupAifError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Aif(inner) => Some(inner),
            Self::RoleOutOfRange { .. }
            | Self::EmptyDemand { .. }
            | Self::UnsupportedReadVote
            | Self::InvalidRoutingWeight { .. }
            | Self::UnsupportedRouting => None,
        }
    }
}

impl From<aif::AifError> for GroupAifError {
    fn from(inner: aif::AifError) -> Self {
        Self::Aif(inner)
    }
}

// --- Instrumentation (S-learn, A1.3 / A1.4) --------------------------------

/// Which world model an audit row is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelLabel {
    /// A role-specialised model.
    Role(Role),
    /// The single shared model.
    Shared,
}

/// One world model's **counted** update ledger — the S-learn (i) gate.
///
/// The gate is *counted, not inferred from state movement*: under the persistent
/// model's MMP inference a missing action record makes an update apply **twice**
/// rather than not at all, so "did the state move?" passes a double-updating arm.
/// Both `updates < expected` (deficit) and `updates > expected` (surplus) are
/// RUN-INVALID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelUpdateAudit {
    /// The model this row describes.
    pub label: ModelLabel,
    /// Updates the engine actually applied.
    pub updates: u64,
    /// Updates it should have applied: tasks in which this model's role had
    /// demand (all observed tasks, for [`ModelLabel::Shared`]).
    pub expected: u64,
}

impl ModelUpdateAudit {
    /// Whether this model advanced exactly the right number of times.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.updates == self.expected
    }
}

/// One decision's **E-agree** sample: how the role internals voted, and how far
/// the group's mixture landed from the threshold.
///
/// The votes are each member's own argmax — exactly what
/// [`group_distribution`](aif::GroupAgent::group_distribution) tallies under the
/// discrete modes and what `CertaintyWeighted` mixes — read back off the roster
/// after the read. It is a *disclosure*, never a decision input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AgreementSample {
    /// Realised roster size for this decision (A1.3: `R` varies).
    pub roster: usize,
    /// How many internals' own argmax was "act".
    pub votes_for_act: usize,
    /// Internals whose coverage masks carry the candidate's capabilities — under
    /// the registered [`CoverageMasks::RoleMatched`] this is `1` when the
    /// candidate's role is on the roster and **`0` otherwise** (A5.1).
    pub candidate_sensitive: usize,
    /// Of [`candidate_sensitive`](Self::candidate_sensitive), how many voted act.
    pub votes_for_act_sensitive: usize,
    /// Of the candidate-**blind** internals (`roster − candidate_sensitive`), how
    /// many voted act. A5.1 predicts nearly all of them.
    pub votes_for_act_blind: usize,
    /// Share of the CW mixture's total weight `Σ exp(−H)` carried by the
    /// candidate-blind internals. A5.1's headline: a blind internal resolves its
    /// indifference to a zero-entropy delta, which is the **maximum** weight.
    pub blind_weight_share: f64,
    /// Readout rows whose effective weight on some candidate-sensitive internal
    /// is positive. With [`VoteRouting::Off`] the rows are the internals' own
    /// outputs and this equals [`candidate_sensitive`](Self::candidate_sensitive).
    pub sensitive_rows: usize,
    /// `Σᵢ wᵢ·(Σ_{j blind} Wᵢⱼ) / Σᵢ wᵢ` over the readout rows `i`, with `wᵢ =
    /// exp(−H)` of routed row `i` and `W` the effective routing matrix; `W` is
    /// the identity with [`VoteRouting::Off`]. `0` when `Σᵢ wᵢ` is `0`.
    pub blind_origin_share: f64,
    /// The own argmax of the internal at the roster position of the candidate's
    /// role; `None` when that role is off the roster.
    pub centre_vote: Option<usize>,
    /// `true` for a `should_leave` read, `false` for a `should_join` read.
    pub leave: bool,
    /// The SP3 act of this read (`p(act) > 0.5`) — the `act` of the
    /// [`Decision`] the read returned.
    pub group_act: bool,
    /// `|p(act) − 0.5|` — the read's distance from the SP3 threshold.
    ///
    /// A genuine CW mixture margin under [`GroupVote::CertaintyWeighted`]; under
    /// [`GroupVote::Deterministic`] it is a **vote-tally distance** and is not
    /// commensurable with one (Amendment A3.1). Nothing is recorded at all under
    /// [`DecisionRead::SeededSampling`], which exposes no distribution.
    pub margin: f64,
}

impl AgreementSample {
    /// Whether every internal voted the same way — the E-agree agreement event.
    #[must_use]
    pub fn unanimous(&self) -> bool {
        self.votes_for_act == 0 || self.votes_for_act == self.roster
    }

    /// Internals carrying no information about the candidate (A5.1).
    #[must_use]
    pub fn candidate_blind(&self) -> usize {
        self.roster.saturating_sub(self.candidate_sensitive)
    }
}

/// Everything the registered gates and disclosures need counted.
///
/// Struck by Amendment A1.4 and deliberately **absent**: member-advance counts and
/// committed-action counts. Members are single-use and there is no commit, so
/// neither has a referent — counting them would be ceremony that reads as a gate.
// `Eq` is deliberately absent: [`AgreementSample::margin`] is an `f64`. The
// counters are a report, not an equality key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GroupAifCounters {
    /// Tasks whose outcome was observed into the world models.
    pub tasks_observed: u64,
    /// Decisions the arm returned an act/decline for, error declines included.
    pub decisions: u64,
    /// Successful `group_distribution` reads (`decisions` minus every decline
    /// that never reached the engine, minus upstream errors).
    pub reads: u64,
    /// Declines caused by an [`aif::AifError`] on the decision path.
    pub declines_upstream: u64,
    /// Declines caused by a participant absent from the role map.
    pub declines_missing_role: u64,
    /// Declines caused by there being no task in force, or no demanding role.
    pub declines_no_demand: u64,
    /// Realised roster size per task, in task order (Amendment A1.3 disclosure).
    /// Also the **independent** expected-update source for S-learn (i): one entry
    /// per opened task, written by `begin_task`, never by `observe_outcome`.
    pub roster_sizes: Vec<usize>,
    /// Internal queries built on the **leave** path, and how many of them had
    /// `cfg0 == cfg1` — the A5.1 mandatory disclosure. A query with identical
    /// masks carries zero information about the candidate.
    pub leave_queries: u64,
    /// Of [`leave_queries`](Self::leave_queries), those with `cfg0 == cfg1`.
    pub leave_queries_identical: u64,
    /// `(task, role)` slots where the role had no demand and did not vote.
    pub empty_role_slots: u64,
    /// Per-model update ledger — the S-learn (i) gate.
    pub model_updates: Vec<ModelUpdateAudit>,
    /// One sample per successful deterministic read — the **E-agree** disclosure.
    /// Empty under [`DecisionRead::SeededSampling`], which exposes no mixture.
    pub agreement: Vec<AgreementSample>,
    /// Successful reads whose [`VoteRouting::CandidateStar`] topology has a
    /// non-identity row: the centre on the roster, a roster of at least two and
    /// `lambda > 0`. Written from the topology build, independently of
    /// [`agreement`](Self::agreement).
    pub routed_reads: u64,
    /// [`CoalitionDecisionPolicy::begin_task`] calls whose demand
    /// [`GroupAifPolicy::begin_task`] refused.
    pub begin_task_rejections: u64,
    /// [`CoalitionDecisionPolicy::observe_outcome`] calls for which
    /// [`GroupAifPolicy::observe_outcome`] advanced no world model.
    pub outcome_updates_unapplied: u64,
}

impl GroupAifCounters {
    /// Whether every world model advanced exactly its expected number of times,
    /// **and** the task stream itself is consistent (Amendment A5.4).
    ///
    /// The second conjunct — one observed task per opened task — is what keeps the
    /// ledger anchored: `expected` is written by `begin_task` and `updates` by
    /// `observe_outcome`, so a skipped or doubled observation shows up as a
    /// deficit or a surplus instead of advancing both counters together.
    #[must_use]
    pub fn s_learn_exact(&self) -> bool {
        self.model_updates.iter().all(ModelUpdateAudit::is_exact)
            && u64::try_from(self.roster_sizes.len()).is_ok_and(|n| n == self.tasks_observed)
    }
}

// --- The member wrapper (Amendment A1.2) -----------------------------------

/// One role's internal agent: an arm-E1 query POMDP plus the replay window it must
/// see, presented to [`aif::GroupAgent`] as an [`InternalAgent`](aif::InternalAgent).
///
/// # Why a wrapper exists at all
///
/// `GroupAgent` feeds every member one **scalar** observation, but an arm-E1 query
/// carries `|required_r|` modalities; a scalar would reach modality 0 alone and
/// silently discard arm-E1's A1.4 two-task replay — hollowing out the very
/// mechanism `EQ5b` exists to carry into the workflow world. `GroupAgent<S, I, X>` is
/// generic in `I`, so the member can carry its own observation vector instead. RNG
/// freedom is preserved: this is a leaf wrapper over a `POMDPAgent`, not a nested
/// group or a sampling sensory slot (gotcha 32).
struct RoleMember {
    query: aif::POMDPAgent,
    /// Replay sequences from the world model, restricted to this role's bits.
    replay: Vec<Vec<usize>>,
    /// `|required_r|` — the neutral read's width when the replay window is empty.
    modalities: usize,
    /// This member's own argmax from its last read — the **E-agree** vote. Read
    /// back through [`aif::GroupAgent::internal_agents`] after the group read; a
    /// disclosure only, never a decision input.
    last_vote: Option<usize>,
    /// `exp(−H)` of its last read — the weight the CW aggregator gave it. Recorded
    /// so A5.1's "the blind members carry the maximum weight" is a measurement.
    last_weight: f64,
    /// The distribution of its last read; `None` before any read and after a
    /// failed one.
    last_dist: Option<DVector<f64>>,
    /// Whether this internal's coverage masks carry the candidate's capabilities
    /// (A5.1). Set at construction from the mask computation, not inferred.
    candidate_sensitive: bool,
}

/// One coalition participant as the mask builder needs it: `(role, agent id,
/// capability mask)`.
type Participant = (Role, usize, u32);

/// One internal's coverage configuration, plus whether the candidate reached it
/// at all (Amendment A5.1).
struct MaskPair {
    cfg0: u32,
    cfg1: u32,
    /// `false` ⇒ this query is candidate-blind: on leave `cfg0 == cfg1` exactly,
    /// on join the candidate appears in neither mask.
    candidate_sensitive: bool,
}

/// What one deterministic read carries besides the group itself.
struct ReadContext<'a> {
    /// Realised roster size.
    roster: usize,
    /// Roster position of the candidate's role; `None` when off the roster.
    centre: Option<usize>,
    /// The active slot's routing; `None` for a bare `VotingAgent`.
    topology: Option<&'a aif::Topology>,
    /// Whether `topology` has a non-identity row, as [`candidate_star`] reports.
    routed: bool,
    /// `true` for a `should_leave` read.
    leave: bool,
    /// Leave-path queries built for this decision.
    leave_queries: u64,
    /// Of those, the ones with `cfg0 == cfg1`.
    leave_identical: u64,
}

/// The [`VoteRouting::CandidateStar`] topology over a roster of `n` members
/// (`hops = 1`, readout `0..n`), and whether any of its rows differs from the
/// identity row — `centre` is `Some`, `n >= 2` and `lambda > 0`.
///
/// With `centre = Some(c)`, row `c` is the identity row and every other row `i`
/// places `lambda` on `c` and `1 − lambda` on `i`; with `centre = None` every row
/// is the identity row.
///
/// # Errors
///
/// Whatever [`aif::Topology::from_adjacency`] returns — `n == 0` among them.
fn candidate_star(
    n: usize,
    centre: Option<usize>,
    lambda: f64,
) -> Result<(aif::Topology, bool), aif::AifError> {
    let rows: Vec<Vec<f64>> = (0..n)
        .map(|i| {
            (0..n)
                .map(|j| match centre {
                    Some(c) if i != c && j == c => lambda,
                    Some(c) if i != c && j == i => 1.0 - lambda,
                    _ if i == j => 1.0,
                    _ => 0.0,
                })
                .collect()
        })
        .collect();
    let topology = aif::Topology::from_adjacency(rows, (0..n).collect(), 1)?;
    Ok((topology, centre.is_some() && n >= 2 && lambda > 0.0))
}

/// Build one decision's [`AgreementSample`] by reading the members' own argmaxes
/// and confidence weights back off the roster, split by candidate-sensitivity
/// (Amendment A5.1). `read` supplies the roster size, the roster position of the
/// candidate's role, the routing the read went through (`None` for the
/// internals' own outputs) and the read kind; `group_act` is the SP3 act of the
/// read. No field of the sample feeds the decision's `act` or `score`. An `Err`
/// makes `GroupAifPolicy::deterministic_decision` return a decline, count it in
/// [`GroupAifCounters::declines_upstream`] and push no sample.
///
/// # Errors
///
/// Whatever [`origin_reach`] returns.
fn agreement_sample(
    members: &[RoleMember],
    read: &ReadContext<'_>,
    p_act: f64,
    group_act: bool,
) -> Result<AgreementSample, aif::AifError> {
    let (roster, centre) = (read.roster, read.centre);
    let (sensitive_rows, blind_origin_share) = origin_reach(members, read.topology)?;
    let mut votes_for_act = 0usize;
    let mut votes_sensitive = 0usize;
    let mut votes_blind = 0usize;
    let mut candidate_sensitive = 0usize;
    let mut blind_weight = 0.0f64;
    let mut total_weight = 0.0f64;
    for m in members {
        let acted = m.last_vote == Some(ACTION_ACT);
        total_weight += m.last_weight;
        if m.candidate_sensitive {
            candidate_sensitive += 1;
            votes_sensitive += usize::from(acted);
        } else {
            blind_weight += m.last_weight;
            votes_blind += usize::from(acted);
        }
        votes_for_act += usize::from(acted);
    }
    // A roster of all-zero weights is not reachable through a successful read
    // (every accepted distribution is normalized, so `exp(−H) ≥ 1/n_actions`), but
    // a zero denominator would be reported as a share rather than caught.
    let blind_weight_share = if total_weight > 0.0 {
        blind_weight / total_weight
    } else {
        0.0
    };
    Ok(AgreementSample {
        roster,
        votes_for_act,
        candidate_sensitive,
        votes_for_act_sensitive: votes_sensitive,
        votes_for_act_blind: votes_blind,
        blind_weight_share,
        sensitive_rows,
        blind_origin_share,
        centre_vote: centre
            .and_then(|c| members.get(c))
            .and_then(|m| m.last_vote),
        leave: read.leave,
        group_act,
        margin: (p_act - 0.5).abs(),
    })
}

/// `(sensitive_rows, blind_origin_share)` of [`AgreementSample`] for one read:
/// over the readout rows of `topology` applied to the members' last
/// distributions, or over the members' own last distributions with identity
/// weights when `topology` is `None`. A member without a last distribution
/// contributes weight `0` when `topology` is `None`.
///
/// # Errors
///
/// With a `topology`: [`aif::AifError::InvalidLength`] when a member has no last
/// distribution, and whatever [`aif::Topology::route`] returns.
fn origin_reach(
    members: &[RoleMember],
    topology: Option<&aif::Topology>,
) -> Result<(usize, f64), aif::AifError> {
    // `(member index, exp(−H) of the row read out for it)`, in readout order.
    let rows: Vec<(usize, f64)> = match topology {
        None => members
            .iter()
            .enumerate()
            .map(|(i, m)| (i, m.last_dist.as_ref().map_or(0.0, confidence_weight)))
            .collect(),
        Some(t) => {
            let mut outputs = Vec::with_capacity(members.len());
            for m in members {
                outputs.push(m.last_dist.clone().ok_or(aif::AifError::InvalidLength {
                    expected: GROUP_N_ACTIONS,
                    got: 0,
                })?);
            }
            t.readout()
                .iter()
                .copied()
                .zip(t.route(&outputs)?.iter().map(confidence_weight))
                .collect()
        }
    };
    let weight = |i: usize, j: usize| match topology {
        Some(t) => t.weight(i, j),
        None if i == j => 1.0,
        None => 0.0,
    };

    let mut sensitive_rows = 0usize;
    let mut blind_mass = 0.0f64;
    let mut total = 0.0f64;
    for &(i, w) in &rows {
        let mut row_blind = 0.0f64;
        let mut reaches_sensitive = false;
        for (j, m) in members.iter().enumerate() {
            let w_ij = weight(i, j);
            if m.candidate_sensitive {
                reaches_sensitive |= w_ij > 0.0;
            } else {
                row_blind += w_ij;
            }
        }
        sensitive_rows += usize::from(reaches_sensitive);
        blind_mass += w * row_blind;
        total += w;
    }
    let share = if total > 0.0 { blind_mass / total } else { 0.0 };
    Ok((sensitive_rows, share))
}

/// `exp(−H)` of a distribution — the confidence weight `VotingAgent` assigns it.
///
/// Mirrors upstream's `confidence_weight`: entropy over the entries above a small
/// floor, so a zero entry contributes nothing and a delta scores exactly `1.0`.
/// Recomputed here rather than read back because upstream exposes no accessor;
/// any drift would show up as a weight share outside `[0, 1]`.
fn confidence_weight(dist: &DVector<f64>) -> f64 {
    let h: f64 = dist
        .iter()
        .filter(|&&p| p > 1e-15)
        .map(|&p| -p * p.ln())
        .sum();
    (-h).exp()
}

impl aif::Agent for RoleMember {
    /// Unsupported on purpose: this member exists to be **read**, never to act.
    ///
    /// `GroupAgent::act` would call it; this arm calls only `group_distribution`,
    /// which polls
    /// [`InternalAgent::action_probabilities`](aif::InternalAgent::action_probabilities)
    /// instead. Reporting [`Unsupported`](aif::AifError::Unsupported) rather than
    /// sampling something is what keeps "this arm never draws" checkable.
    fn act(&mut self, _observation: usize) -> Result<usize, aif::AifError> {
        Err(aif::AifError::Unsupported(
            "koalisi group role member is a deterministic read-only slot; \
             use GroupAgent::group_distribution, not act"
                .to_string(),
        ))
    }
}

impl aif::InternalAgent for RoleMember {
    /// The arm-E1 replay + marginal action posterior, ignoring the group's scalar
    /// observation (see [`SENSORY_OBSERVATION`]).
    ///
    /// The trait has no error channel, so an engine rejection returns an **empty**
    /// distribution, which `group_distribution` turns into
    /// [`AifError::InvalidLength`](aif::AifError::InvalidLength) before anything is
    /// aggregated. The policy then declines and counts. Deterministic, non-panicking,
    /// and visible in the counters rather than smuggled into a score.
    fn action_probabilities(&mut self, _observation: usize) -> DVector<f64> {
        match run_replay(&mut self.query, &self.replay, self.modalities) {
            Ok(dist) => {
                // Ties to the lowest index. This matches the engine's
                // `argmax_index` on every distribution the group will accept —
                // `validate_sampleable` rejects a non-finite entry before the vote
                // is used — but the two differ on NaN, which upstream skips and
                // this fold would adopt. Unreachable, and stated rather than
                // claimed away (A5.10 L1-15).
                self.last_vote = dist
                    .iter()
                    .enumerate()
                    .fold(None::<(usize, f64)>, |best, (i, &p)| match best {
                        Some((_, b)) if b >= p => best,
                        _ => Some((i, p)),
                    })
                    .map(|(i, _)| i);
                self.last_weight = confidence_weight(&dist);
                self.last_dist = Some(dist.clone());
                dist
            }
            Err(e) => {
                tracing::warn!(error = %e, "group role member replay failed; declining the read");
                self.last_vote = None;
                self.last_weight = 0.0;
                self.last_dist = None;
                DVector::zeros(0)
            }
        }
    }

    fn record_action(&mut self, action: usize) {
        self.query.record_action(action);
    }

    fn reseed(&mut self, seed: u64) {
        self.query.reseed(seed);
    }
}

// --- Per-task state --------------------------------------------------------

/// The workflow demand of the task currently in force, pre-resolved per role.
#[derive(Debug, Clone)]
struct TaskState {
    /// `required_r` per role index, masked into the world model's universe.
    /// `0` ⇒ the role has no demand and does not vote (Amendment A1.3).
    required: Vec<u32>,
    /// `multiplicity[role][bit]` — SP2's `m(b, r)`, `0` outside the demand.
    multiplicity: Vec<Vec<u64>>,
    /// Demanding roles, ascending — the realised roster.
    roster: Vec<Role>,
    /// OR of every `required_r` — what a [`WorldModelTopology::Shared`] model
    /// observes.
    union_required: u32,
}

/// Mutable arm state behind one mutex.
struct Shared {
    task: Option<TaskState>,
    counters: GroupAifCounters,
    /// Per-model applied-update counts, index-aligned with [`Models`].
    updates: Vec<u64>,
    /// Per-model expected-update counts.
    expected: Vec<u64>,
    /// Monotonic decision counter — derives the per-decision seed (hygiene only).
    decision_counter: u64,
}

/// The arm's persistent world models (Amendment A1.1).
enum Models {
    /// One per role, index = role index.
    RoleSpecialised(Vec<PersistentAifArm>),
    /// One for the whole task.
    Shared(PersistentAifArm),
}

impl Models {
    /// The model internal `role` reads from.
    fn view(&self, role: Role) -> Option<&PersistentAifArm> {
        match self {
            Self::RoleSpecialised(models) => models.get(role.index() as usize),
            Self::Shared(model) => Some(model),
        }
    }

    fn labels(&self, n_roles: usize) -> Vec<ModelLabel> {
        match self {
            Self::RoleSpecialised(_) => (0..n_roles)
                .map(|r| {
                    ModelLabel::Role(Role::new(
                        u8::try_from(r).expect("invariant: n_roles is clamped below u8::MAX"),
                    ))
                })
                .collect(),
            Self::Shared(_) => vec![ModelLabel::Shared],
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::RoleSpecialised(models) => models.len(),
            Self::Shared(_) => 1,
        }
    }

    fn at(&self, index: usize) -> Option<&PersistentAifArm> {
        match self {
            Self::RoleSpecialised(models) => models.get(index),
            Self::Shared(model) => (index == 0).then_some(model),
        }
    }
}

// --- The arm ---------------------------------------------------------------

/// Role-slotted Active Inference group join/leave policy (`grp-*`, prereg K4-EQ5b).
///
/// One arm per seed. The harness calls [`begin_task`](Self::begin_task) with the
/// task's workflow [`Demand`] before its decisions, and
/// [`observe_outcome`](Self::observe_outcome) once after its leave sweep. See the
/// `decision::group_policy` module documentation for the five load-bearing
/// properties. (A plain code span, not an intra-doc link: the module is private
/// behind a re-export, like every other `decision` arm, so a `self` link would
/// point at an undocumented item.)
///
/// # Battery-scoped, NOT for a long-lived service
///
/// The registered disclosures are **unbounded histories**: `roster_sizes` grows
/// by one `usize` per [`begin_task`](Self::begin_task) and `agreement` by one
/// [`AgreementSample`] per successful deterministic read, neither ever trimmed,
/// and [`counters`](Self::counters) deep-clones both on every call. That is
/// deliberate — the per-seed distributions are what prereg A1.3 and the A5.1
/// table report, and a 30-seed battery is bounded by construction.
///
/// It is the wrong shape for a long-lived instance behind `CoalitionService`,
/// which would accumulate one sample per decision for the process lifetime.
/// A service use wants a bounded window instead, in the spirit of the remote
/// gateway's `EventBuffer` (gotcha 29) — deliberately not built here, because a
/// cap would silently truncate a **registered** disclosure and change what the
/// report measures. Construct one arm per seed and drop it, as the battery does.
pub struct GroupAifPolicy {
    models: Models,
    config: GroupAifConfig,
    /// `agent_id → Role`. Only ever *looked up*, never iterated — no `HashMap`
    /// ordering reaches a decision.
    agent_roles: HashMap<usize, Role>,
    battery_seed: u64,
    /// The world model width, read once at construction (post-clamp).
    n_bits: usize,
    shared: Mutex<Shared>,
}

impl GroupAifPolicy {
    /// Build a fresh group arm for `battery_seed`.
    ///
    /// `agent_roles` maps every pool worker's `agent_id` to its [`Role`]; a
    /// participant absent from it makes the decision a **decline-and-count**
    /// rather than an invented answer (the gotcha-28 typed-arm contract).
    ///
    /// # Errors
    ///
    /// [`GroupAifError::Aif`] if the engine rejects a world model, and
    /// [`GroupAifError::UnsupportedReadVote`] for the seeded-sampling /
    /// discrete-vote pairing, which is constructible upstream and silently
    /// all-declines (Amendment A5.10 L1-14).
    /// [`GroupAifError::InvalidRoutingWeight`] for a
    /// [`VoteRouting::CandidateStar`] `lambda` that is non-finite or outside
    /// `[0, 1]`, and [`GroupAifError::UnsupportedRouting`] for a routing other than
    /// [`VoteRouting::Off`] with [`DecisionRead::SeededSampling`] or a vote other
    /// than [`GroupVote::CertaintyWeighted`].
    ///
    /// # Panics
    ///
    /// Never in practice: the only `expect` asserts that a topology built at least
    /// one model, which both arms of the match below do unconditionally (`n_roles`
    /// is clamped to at least 1).
    pub fn new(
        battery_seed: u64,
        config: GroupAifConfig,
        agent_roles: HashMap<usize, Role>,
    ) -> Result<Self, GroupAifError> {
        if config.read == DecisionRead::SeededSampling
            && config.vote != GroupVote::CertaintyWeighted
        {
            return Err(GroupAifError::UnsupportedReadVote);
        }
        if let VoteRouting::CandidateStar { lambda } = config.routing {
            // A NaN is inside no range, so this refuses the non-finite values too.
            if !(0.0..=1.0).contains(&lambda) {
                return Err(GroupAifError::InvalidRoutingWeight { lambda });
            }
            if config.read != DecisionRead::Deterministic
                || config.vote != GroupVote::CertaintyWeighted
            {
                return Err(GroupAifError::UnsupportedRouting);
            }
        }
        let n_roles = config.n_roles.clamp(1, usize::from(u8::MAX));
        let config = GroupAifConfig { n_roles, ..config };

        let models = match config.topology {
            WorldModelTopology::RoleSpecialised => {
                let mut built = Vec::with_capacity(n_roles);
                for r in 0..n_roles {
                    // Distinct per-model seeds are hygiene: the world model never
                    // samples (single control, no precision dynamics), so this
                    // cannot perturb a decision — it only keeps the models from
                    // sharing an RNG stream by accident.
                    let seed = battery_seed ^ splitmix64(r as u64 + 1);
                    built.push(PersistentAifArm::new(seed, config.base)?);
                }
                Models::RoleSpecialised(built)
            }
            WorldModelTopology::Shared => {
                Models::Shared(PersistentAifArm::new(battery_seed, config.base)?)
            }
        };

        let n_bits = models
            .at(0)
            .expect("invariant: every topology builds at least one model")
            .n_bits();
        let model_count = models.len();

        Ok(Self {
            models,
            config,
            agent_roles,
            battery_seed,
            n_bits,
            shared: Mutex::new(Shared {
                task: None,
                counters: GroupAifCounters::default(),
                updates: vec![0; model_count],
                expected: vec![0; model_count],
                decision_counter: 0,
            }),
        })
    }

    /// Put a task's workflow demand in force and return the **realised roster
    /// size** — how many roles actually vote on it (Amendment A1.3).
    ///
    /// Call exactly once per task, before that task's decisions. Bits outside the
    /// world model's `n_bits` universe carry no factor and are dropped, exactly as
    /// arm-E1 drops them from `required`.
    ///
    /// # Errors
    ///
    /// [`GroupAifError::RoleOutOfRange`] if the demand names a role outside
    /// `0..n_roles`, and [`GroupAifError::EmptyDemand`] if nothing survives the
    /// universe mask. Either clears the task, so subsequent decisions decline and
    /// count rather than silently reusing the previous task's demand.
    ///
    /// # Panics
    ///
    /// Only on a poisoned arm mutex — i.e. only after another thread already
    /// panicked while holding it.
    pub fn begin_task(&self, demand: &Demand) -> Result<usize, GroupAifError> {
        let n_roles = self.config.n_roles;
        let mut required = vec![0u32; n_roles];
        let mut multiplicity = vec![vec![0u64; self.n_bits]; n_roles];

        let mut shared = self.shared.lock().expect("group arm mutex poisoned");
        shared.task = None;

        for step in demand.distinct() {
            let role = usize::from(step.role.index());
            if role >= n_roles {
                return Err(GroupAifError::RoleOutOfRange {
                    role: step.role.index(),
                    n_roles,
                });
            }
            let bit = usize::from(step.bit);
            if bit >= self.n_bits {
                // Outside the universe: no factor exists for it, so it cannot enter
                // a query. Dropped rather than errored — arm-E1 masks `required` the
                // same way, and a workflow wider than the world model is a
                // configuration choice, not a fault.
                continue;
            }
            required[role] |= 1u32 << bit;
            multiplicity[role][bit] = demand.multiplicity(step) as u64;
        }

        let roster: Vec<Role> = (0..n_roles)
            .filter(|&r| required[r] != 0)
            .map(|r| {
                Role::new(u8::try_from(r).expect("invariant: n_roles is clamped below u8::MAX"))
            })
            .collect();
        if roster.is_empty() {
            return Err(GroupAifError::EmptyDemand {
                n_bits: self.n_bits,
            });
        }

        let union_required = required.iter().fold(0u32, |acc, &m| acc | m);
        let size = roster.len();
        shared.counters.roster_sizes.push(size);
        shared.counters.empty_role_slots += (n_roles - size) as u64;
        // S-learn (i), Amendment A5.4: `expected` is derived HERE, from the task
        // stream, and never on the `observe_outcome` path. Incrementing it beside
        // the guarded `updates` made `updates > expected` unreachable, so the
        // RUN-INVALID surplus condition was near-tautological — a double
        // `observe_outcome` advanced both counters and read exact.
        match &self.models {
            Models::RoleSpecialised(_) => {
                for r in &roster {
                    if let Some(slot) = shared.expected.get_mut(usize::from(r.index())) {
                        *slot += 1;
                    }
                }
            }
            Models::Shared(_) => {
                if let Some(slot) = shared.expected.get_mut(0) {
                    *slot += 1;
                }
            }
        }
        shared.task = Some(TaskState {
            required,
            multiplicity,
            roster,
            union_required,
        });
        Ok(size)
    }

    /// Observe one task outcome into the world models — call once per task, after
    /// the leave sweep, exactly as arm-E1's
    /// [`observe_outcome`](PersistentAifArm::observe_outcome) is called.
    ///
    /// Under [`WorldModelTopology::RoleSpecialised`] each **demanding** role's model
    /// observes its own `required_r`; roles that left the roster observe nothing and
    /// their expected-update count does not advance. Under
    /// [`WorldModelTopology::Shared`] the one model observes the union — arm-E1's
    /// own call, verbatim.
    ///
    /// Returns how many models the engine actually advanced. A width mismatch is
    /// warned and the whole task is skipped (nothing is counted), so the S-learn
    /// ledger can never be inflated by an update that did not happen.
    ///
    /// # Panics
    ///
    /// Only on a poisoned arm mutex.
    pub fn observe_outcome(&self, per_bit_success: &[bool]) -> usize {
        if per_bit_success.len() != self.n_bits {
            tracing::warn!(
                got = per_bit_success.len(),
                expected = self.n_bits,
                "group arm observe_outcome width mismatch, skipping the task"
            );
            return 0;
        }
        let mut shared = self.shared.lock().expect("group arm mutex poisoned");
        let Some(task) = shared.task.clone() else {
            tracing::warn!("group arm observe_outcome with no task in force, skipping");
            return 0;
        };

        let mut applied = 0usize;
        match &self.models {
            Models::RoleSpecialised(models) => {
                for &role in &task.roster {
                    let index = usize::from(role.index());
                    let Some(model) = models.get(index) else {
                        continue;
                    };
                    // `expected` is NOT touched here — see `begin_task` (A5.4).
                    if model.observe_outcome_checked(task.required[index], per_bit_success) {
                        shared.updates[index] += 1;
                        applied += 1;
                    }
                }
            }
            Models::Shared(model) => {
                if model.observe_outcome_checked(task.union_required, per_bit_success) {
                    shared.updates[0] += 1;
                    applied = 1;
                }
            }
        }
        shared.counters.tasks_observed += 1;
        applied
    }

    /// The counted gate + disclosure ledger (S-learn (i), Amendment A1.3/A1.4).
    ///
    /// # Panics
    ///
    /// Only on a poisoned arm mutex.
    #[must_use]
    pub fn counters(&self) -> GroupAifCounters {
        let shared = self.shared.lock().expect("group arm mutex poisoned");
        let labels = self.models.labels(self.config.n_roles);
        let mut counters = shared.counters.clone();
        counters.model_updates = labels
            .into_iter()
            .enumerate()
            .map(|(i, label)| ModelUpdateAudit {
                label,
                updates: shared.updates.get(i).copied().unwrap_or(0),
                expected: shared.expected.get(i).copied().unwrap_or(0),
            })
            .collect();
        counters
    }

    /// Snapshot every world model — the S-learn (i) **non-vacuity guard** (end of
    /// stream must differ from initialization). Necessary, explicitly not
    /// sufficient; the counted ledger in [`counters`](Self::counters) is the gate.
    #[must_use]
    pub fn model_snapshots(&self) -> Vec<(ModelLabel, PersistentAifState)> {
        self.models
            .labels(self.config.n_roles)
            .into_iter()
            .enumerate()
            .filter_map(|(i, label)| Some((label, self.models.at(i)?.state_snapshot())))
            .collect()
    }

    /// The configuration in force (post-clamp).
    #[must_use]
    pub fn config(&self) -> GroupAifConfig {
        self.config
    }

    /// The role of one participant, or `None` when the map has no entry.
    fn role_of(&self, agent: &dyn AgentCapabilities) -> Option<Role> {
        self.agent_roles.get(&agent.agent_id()).copied()
    }

    /// A decline in the arm's own shape. A zero score is **not** a decline — the
    /// caller reads [`counters`](Self::counters) for that (standing disclosure).
    fn declined() -> Decision {
        Decision {
            act: false,
            score: 0.0,
        }
    }

    /// Shared decision core.
    ///
    /// `leave` selects the coverage configuration only; the SP3 rule is applied
    /// uniformly (`act` iff `p(act) > 0.5`, ties decline) because SP3 pins one
    /// criterion for the arm rather than arm-E1's join/leave tie split.
    fn decide(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        leave: bool,
    ) -> Decision {
        let mut shared = self.shared.lock().expect("group arm mutex poisoned");
        shared.counters.decisions += 1;

        let Some(task) = shared.task.clone() else {
            shared.counters.declines_no_demand += 1;
            return Self::declined();
        };

        // Role resolution first: an unmapped participant is a decline, never a
        // guess (gotcha 28). The candidate is resolved even on the leave path,
        // where it is also a coalition member, so a partial map cannot slip through.
        let Some((agent_role, members)) = self.resolve_participants(agent, coalition) else {
            shared.counters.declines_missing_role += 1;
            return Self::declined();
        };

        shared.decision_counter = shared.decision_counter.wrapping_add(1);
        let seed = self.battery_seed ^ splitmix64(shared.decision_counter);
        drop(shared);

        let agent_id = agent.agent_id();
        let agent_caps = agent.capabilities();

        let mut internals: Vec<RoleMember> = Vec::with_capacity(task.roster.len());
        // A5.1 disclosure, accumulated as the queries are built rather than
        // reconstructed afterwards.
        let mut leave_queries = 0u64;
        let mut leave_identical = 0u64;
        for &role in &task.roster {
            let required_r = task.required[usize::from(role.index())] & low_mask(self.n_bits);
            let masks =
                self.coverage_masks(role, (agent_role, agent_id, agent_caps), &members, leave);
            let (cfg0, cfg1) = (masks.cfg0, masks.cfg1);
            if leave {
                leave_queries += 1;
                if cfg0 == cfg1 {
                    leave_identical += 1;
                }
            }

            let scale = match self.config.channel {
                PrecisionChannel::RoleRestricted => None,
                PrecisionChannel::MultiplicityWeighted => Some(self.scale_for(&task, role)),
            };

            let Some(model) = self.models.view(role) else {
                self.count_upstream_decline();
                self.record_leave_masks(leave_queries, leave_identical);
                tracing::warn!(
                    role = role.index(),
                    "group arm has no world model for the role"
                );
                return Self::declined();
            };
            let member_seed = seed ^ splitmix64(u64::from(role.index()) + 1);
            match model.role_query(required_r, cfg0, cfg1, member_seed, scale.as_deref()) {
                Ok((query, replay)) => internals.push(RoleMember {
                    query,
                    replay,
                    modalities: required_r.count_ones() as usize,
                    last_vote: None,
                    last_weight: 0.0,
                    last_dist: None,
                    candidate_sensitive: masks.candidate_sensitive,
                }),
                Err(e) => {
                    tracing::warn!(error = %e, role = role.index(), "group role query construction failed");
                    self.count_upstream_decline();
                    self.record_leave_masks(leave_queries, leave_identical);
                    return Self::declined();
                }
            }
        }

        // D2 registered `CertaintyWeighted` on the argument that only it carries a
        // continuous margin at this shape. Amendment A3.1 records that argument as
        // measured FALSE on this world — the mixture is itself a delta, because
        // arm-E1's query posteriors saturate (gotcha 25) and the internals inherit
        // it — and adds `GroupVote::Deterministic` as a non-gating reference leg.
        // D2 itself stands. The seeds are hygiene: neither mode's read draws.
        let roster = internals.len();
        let centre = task.roster.iter().position(|&r| r == agent_role);
        match self.config.routing {
            VoteRouting::Off => {
                let mut group = aif::GroupAgent::with_slots_seeded(
                    aif::CopyAgent,
                    internals,
                    aif::VotingAgent::with_seed(GROUP_N_ACTIONS, self.config.vote.upstream(), seed),
                    GROUP_N_ACTIONS,
                    seed,
                );

                if self.config.read == DecisionRead::SeededSampling {
                    let d = self.sampled_decision(&mut group);
                    self.record_leave_masks(leave_queries, leave_identical);
                    return d;
                }

                self.deterministic_decision(
                    &mut group,
                    &ReadContext {
                        roster,
                        centre,
                        topology: None,
                        routed: false,
                        leave,
                        leave_queries,
                        leave_identical,
                    },
                )
            }
            VoteRouting::CandidateStar { lambda } => {
                let (topology, routed) = match candidate_star(roster, centre, lambda) {
                    Ok(built) => built,
                    Err(e) => {
                        tracing::warn!(error = %e, "group routing topology construction failed");
                        self.count_upstream_decline();
                        self.record_leave_masks(leave_queries, leave_identical);
                        return Self::declined();
                    }
                };
                let mut group = aif::GroupAgent::with_slots_seeded(
                    aif::CopyAgent,
                    internals,
                    aif::RoutedAggregator::with_seed(
                        aif::VotingAgent::with_seed(
                            GROUP_N_ACTIONS,
                            self.config.vote.upstream(),
                            seed,
                        ),
                        topology.clone(),
                        GROUP_N_ACTIONS,
                        seed,
                    ),
                    GROUP_N_ACTIONS,
                    seed,
                );
                self.deterministic_decision(
                    &mut group,
                    &ReadContext {
                        roster,
                        centre,
                        topology: Some(&topology),
                        routed,
                        leave,
                        leave_queries,
                        leave_identical,
                    },
                )
            }
        }
    }

    /// The [`DecisionRead::Deterministic`] read of `group` and the SP3 rule over
    /// it (`act` iff `p(act) > 0.5`), for any active slot. A failed read, a
    /// non-finite `p(act)` and a failed [`agreement_sample`] each decline and
    /// count in [`GroupAifCounters::declines_upstream`]; a successful read counts
    /// in `reads`, pushes its [`AgreementSample`], and counts in `routed_reads`
    /// when `read.routed`.
    fn deterministic_decision<X: aif::Aggregator>(
        &self,
        group: &mut aif::GroupAgent<aif::CopyAgent, RoleMember, X>,
        read: &ReadContext<'_>,
    ) -> Decision {
        let (leave_queries, leave_identical) = (read.leave_queries, read.leave_identical);

        // D4/D4a-as-amended: the pure read, and NOTHING else. No
        // `record_group_action`, no `group_distribution_recording`.
        let dist = match group.group_distribution(SENSORY_OBSERVATION) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(error = %e, "group distribution read failed");
                self.count_upstream_decline();
                self.record_leave_masks(leave_queries, leave_identical);
                return Self::declined();
            }
        };

        let p_act = dist.get(ACTION_ACT).copied().unwrap_or(f64::NAN);
        if !p_act.is_finite() {
            tracing::warn!(p_act, "group distribution returned a non-finite p(act)");
            self.count_upstream_decline();
            self.record_leave_masks(leave_queries, leave_identical);
            return Self::declined();
        }

        // SP3: act iff p(act) > 0.5; ties decline.
        let group_act = p_act > 0.5;
        let sample = match agreement_sample(group.internal_agents(), read, p_act, group_act) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "group agreement sample routing failed");
                self.count_upstream_decline();
                self.record_leave_masks(leave_queries, leave_identical);
                return Self::declined();
            }
        };
        let mut shared = self.shared.lock().expect("group arm mutex poisoned");
        shared.counters.reads += 1;
        shared.counters.routed_reads += u64::from(read.routed);
        shared.counters.leave_queries += leave_queries;
        shared.counters.leave_queries_identical += leave_identical;
        shared.counters.agreement.push(sample);
        drop(shared);

        Decision {
            act: group_act,
            score: p_act - 0.5,
        }
    }

    /// The candidate's role and the coalition as `(role, id, capabilities)`, or
    /// `None` if the role map is missing **any** participant.
    ///
    /// All-or-nothing on purpose: a partial map would let some internals see a
    /// candidate the others cannot, which is a silently different arm rather than a
    /// degraded one (gotcha 28).
    fn resolve_participants(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
    ) -> Option<(Role, Vec<Participant>)> {
        let agent_role = self.role_of(agent)?;
        let mut members = Vec::with_capacity(coalition.len());
        for m in coalition {
            members.push((self.role_of(*m)?, m.agent_id(), m.capabilities()));
        }
        Some((agent_role, members))
    }

    /// Internal `role`'s coverage masks `(cfg0, cfg1)` for one decision
    /// (Amendment **A2.1**).
    ///
    /// Under the registered [`CoverageMasks::RoleMatched`] only role-`role`
    /// participants count — the world's own predicate. [`CoverageMasks::RoleBlind`]
    /// is the `grp-role-blind` reference leg and counts everyone.
    ///
    /// `candidate` is `(role, id, capabilities)` of the agent being decided about;
    /// `members` is the coalition as `(role, id, capabilities)`, including the
    /// candidate on the leave path.
    fn coverage_masks(
        &self,
        role: Role,
        candidate: Participant,
        members: &[Participant],
        leave: bool,
    ) -> MaskPair {
        let (agent_role, agent_id, agent_caps) = candidate;
        let role_matched = self.config.masks == CoverageMasks::RoleMatched;
        let member_union = members
            .iter()
            .filter(|&&(r, id, _)| (!role_matched || r == role) && !(leave && id == agent_id))
            .fold(0u32, |acc, &(_, _, caps)| acc | caps);
        // A5.1: structural, not value-dependent. Under role-matched masks the
        // candidate reaches only its own role's internal — for every other one it
        // contributes nothing, whatever its capabilities are.
        let candidate_sensitive = !role_matched || agent_role == role;
        let own = if candidate_sensitive { agent_caps } else { 0 };
        let (cfg0, cfg1) = if leave {
            // cfg0 = the coalition WITH the member, cfg1 = without it.
            (member_union | own, member_union)
        } else {
            // cfg0 = the candidate alone, cfg1 = coalition ∪ {candidate}.
            (own, own | member_union)
        };
        MaskPair {
            cfg0,
            cfg1,
            candidate_sensitive,
        }
    }

    /// The `E-seed` branch: decide by the shipped draw (exploratory, non-gating).
    ///
    /// Under `CertaintyWeighted`, [`Agent::act`](aif::Agent::act) polls members
    /// through `action_probabilities` and samples with the group's own seeded RNG
    /// — it never calls a member's `act`, which is why the read-only wrapper can
    /// refuse that method and this cell still runs.
    fn sampled_decision<I: aif::InternalAgent>(
        &self,
        group: &mut aif::GroupAgent<aif::CopyAgent, I, aif::VotingAgent>,
    ) -> Decision {
        match aif::Agent::act(group, SENSORY_OBSERVATION) {
            Ok(action) => {
                let mut shared = self.shared.lock().expect("group arm mutex poisoned");
                shared.counters.reads += 1;
                drop(shared);
                let act = action == ACTION_ACT;
                Decision {
                    // NOT a margin — see `DecisionRead::SeededSampling`.
                    score: if act { 0.5 } else { -0.5 },
                    act,
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "group sampled act failed");
                self.count_upstream_decline();
                Self::declined()
            }
        }
    }

    /// SP2's per-modality count scale for `role`, index-aligned with the query's
    /// modalities (`set_bits(required_r)`).
    fn scale_for(&self, task: &TaskState, role: Role) -> Vec<f64> {
        let index = usize::from(role.index());
        let required_r = task.required[index] & low_mask(self.n_bits);
        set_bits(required_r, self.n_bits)
            .into_iter()
            .map(|bit| {
                let m = task
                    .multiplicity
                    .get(index)
                    .and_then(|row| row.get(bit))
                    .copied()
                    .unwrap_or(1)
                    .max(1);
                multiplicity_as_f64(m)
            })
            .collect()
    }

    fn count_upstream_decline(&self) {
        let mut shared = self.shared.lock().expect("group arm mutex poisoned");
        shared.counters.declines_upstream += 1;
    }

    /// Fold one decision's leave-path mask tally into the counters (A5.1). Split
    /// out because the sampling path returns before the E-agree block.
    fn record_leave_masks(&self, queries: u64, identical: u64) {
        if queries == 0 {
            return;
        }
        let mut shared = self.shared.lock().expect("group arm mutex poisoned");
        shared.counters.leave_queries += queries;
        shared.counters.leave_queries_identical += identical;
    }
}

/// An occurrence multiplicity as an `f64`, so the one lossy conversion in this
/// module has one site and one justification: a step's occurrence count in a
/// drawn workflow is bounded by the task's step count (single digits here, and
/// far below `2^53` for anything a caller could plausibly build).
fn multiplicity_as_f64(m: u64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    {
        m as f64
    }
}

impl CoalitionDecisionPolicy for GroupAifPolicy {
    /// Join iff the group's `p(act) > 0.5` — `coalition` excludes `agent`.
    fn should_join(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        _ctx: &DecisionContext,
    ) -> Decision {
        self.decide(agent, coalition, false)
    }

    /// Leave iff the group's `p(act) > 0.5` — `coalition` includes `agent`.
    ///
    /// Note the tie rule: SP3 pins `>` with **ties declining** for the whole arm,
    /// where arm-E1 leaves on a tie. One criterion, applied to both questions, is
    /// what makes "`EQ5b` changes the engine, not the criterion" checkable.
    fn should_leave(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        _ctx: &DecisionContext,
    ) -> Decision {
        self.decide(agent, coalition, true)
    }

    /// [`GroupAifPolicy::begin_task`] over the [`Demand`] with one occurrence per
    /// entry of `task.steps`. A refusal is warned and counted in
    /// [`GroupAifCounters::begin_task_rejections`].
    fn begin_task(&self, task: &TaskStart<'_>) {
        let demand = Demand::from_steps(
            task.steps
                .iter()
                .map(|&(bit, role)| Step::new(bit, Role::new(role))),
        );
        if let Err(e) = GroupAifPolicy::begin_task(self, &demand) {
            tracing::warn!(error = %e, "group arm refused the task's demand");
            let mut shared = self.shared.lock().expect("group arm mutex poisoned");
            shared.counters.begin_task_rejections += 1;
        }
    }

    /// [`GroupAifPolicy::observe_outcome`] over `per_bit_success`; a call that
    /// advances no world model is counted in
    /// [`GroupAifCounters::outcome_updates_unapplied`]. `_members` is not read.
    fn observe_outcome(
        &self,
        _required: u32,
        per_bit_success: &[bool],
        _members: &[MemberOutcome],
    ) {
        if GroupAifPolicy::observe_outcome(self, per_bit_success) == 0 {
            let mut shared = self.shared.lock().expect("group arm mutex poisoned");
            shared.counters.outcome_updates_unapplied += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use catgraph_applied::prop::colored::ColoredExpr;

    use super::super::aif_persistent_policy::NO_OBS;
    use super::*;
    use crate::process::{Step, chain, demand, step_expr};

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

    /// A single-role chain of `steps`, as a `Workflow`.
    fn leg(
        role: Role,
        bits: &[u8],
    ) -> catgraph_applied::prop::PropExpr<crate::process::WorkflowGen> {
        chain(
            bits.iter()
                .map(|&b| step_expr(Step::new(b, role)))
                .collect(),
        )
        .expect("a non-empty chain of same-role 1->1 steps composes")
    }

    /// A workflow over `legs = [(role, bits)]`, each role a sequential chain.
    fn workflow(legs: &[(Role, &[u8])]) -> crate::process::Workflow {
        let mut source = Vec::new();
        let mut exprs = Vec::new();
        for &(role, bits) in legs {
            source.push(role);
            exprs.push(leg(role, bits));
        }
        let expr = exprs
            .into_iter()
            .reduce(catgraph_applied::prop::Free::tensor)
            .expect("at least one leg");
        ColoredExpr::new(source, expr).expect("every leg is a single-role r -> r chain")
    }

    /// The three-role roster used by most tests: r0 needs bit 0, r1 bit 1, r2 bit 2.
    fn three_role_demand() -> Demand {
        demand(&workflow(&[
            (Role::new(0), &[0]),
            (Role::new(1), &[1]),
            (Role::new(2), &[2]),
        ]))
    }

    fn roles_map(pairs: &[(usize, u8)]) -> HashMap<usize, Role> {
        pairs.iter().map(|&(id, r)| (id, Role::new(r))).collect()
    }

    fn policy(config: GroupAifConfig, roles: &[(usize, u8)]) -> GroupAifPolicy {
        GroupAifPolicy::new(11, config, roles_map(roles)).unwrap()
    }

    /// The registered cell with **novelty off** — the S-learn (iii) ablation.
    ///
    /// Several tests need a mixture that is *not* a delta, and A5.1's measurement
    /// is exactly why: with novelty on, an internal with no candidate-discriminating
    /// coverage resolves its indifference to `p(act) = 1.0` at zero entropy, so
    /// unanimity and saturation are the norm. Novelty off collapses that read to
    /// `0.5`, which is the only way to reach a split vote, a non-degenerate
    /// mixture, or a live tie on this world.
    fn novelty_off() -> GroupAifConfig {
        GroupAifConfig {
            base: PersistentAifConfig {
                query_novelty: false,
                ..v5_e1_base()
            },
            ..GroupAifConfig::default()
        }
    }

    /// The default config is the **v5 E1** base, not the FALSIFIED v4 default
    /// (gotcha 24's relabelling discipline), and the registered `grp-role` cell.
    #[test]
    fn default_config_is_the_registered_cell() {
        let c = GroupAifConfig::default();
        assert_eq!(c.topology, WorldModelTopology::RoleSpecialised);
        assert_eq!(c.channel, PrecisionChannel::RoleRestricted);
        assert_eq!(c.n_roles, DEFAULT_N_ROLES);
        assert!(c.base.persistent_learning);
        assert!(c.base.query_novelty);
        assert!(
            !c.base.query_dynamics,
            "v5 E1 runs MeanField queries at fixed gamma, NOT the v4 precision dynamics"
        );
        assert!(c.base.query_gamma.is_none(), "engine-default gamma = 16");
    }

    /// The read consumes **no randomness** — shown by seed INVARIANCE, not by
    /// reproducibility (Amendment A5.10 L1-7 / L2-4).
    ///
    /// The previous version ran the same seed twice, which a seeded sampler would
    /// also pass. `battery_seed` reaches the engine only as `AgentParams { seed }`
    /// (the world models, the per-decision query seed, the `VotingAgent` and the
    /// group RNG), so if anything drew from it, two different seeds would diverge.
    /// They must not.
    #[test]
    fn read_is_rng_free_shown_by_seed_invariance() {
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
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };
        let map = roles_map(&[(0, 0), (1, 1), (2, 2)]);

        let run = |seed: u64| {
            let p = GroupAifPolicy::new(seed, GroupAifConfig::default(), map.clone()).unwrap();
            let mut out = Vec::new();
            for _ in 0..3 {
                p.begin_task(&three_role_demand()).unwrap();
                let j = p.should_join(&a0, &coalition, &ctx);
                out.push((j.act, j.score.to_bits()));
                let l = p.should_leave(&a0, &full, &ctx);
                out.push((l.act, l.score.to_bits()));
                p.observe_outcome(&[true, false, true, false, false, false, false, false]);
            }
            out
        };
        assert_eq!(
            run(11),
            run(4_242_424_242),
            "the deterministic read must be invariant to the seed; a divergence here \
             means something drew from it"
        );
        // Reproducibility too, since S-determinism is stated per seed.
        assert_eq!(run(11), run(11));

        // …and the seeded-sampling cell is NOT seed-invariant, which is what makes
        // the assertion above a real test rather than a property of the world.
        //
        // It has to run with **novelty off**: A5.1 measures that with novelty on
        // every indifferent internal reads a zero-entropy delta, and sampling from
        // a delta is deterministic — so the registered cell would show invariance
        // too, for a reason that has nothing to do with RNG. That is itself the
        // A3.1/A5.1 finding, and it is why the control needs the ablation.
        let sampled = GroupAifConfig {
            read: DecisionRead::SeededSampling,
            ..novelty_off()
        };
        let sample = |seed: u64| {
            let p = GroupAifPolicy::new(seed, sampled, map.clone()).unwrap();
            let mut out = Vec::new();
            for _ in 0..6 {
                p.begin_task(&three_role_demand()).unwrap();
                out.push(p.should_join(&a0, &coalition, &ctx).act);
                p.observe_outcome(&[true, false, true, false, false, false, false, false]);
            }
            out
        };
        assert_ne!(
            sample(1),
            sample(999),
            "the sampling cell DOES consume randomness, so the invariance above is \
             a property of the read and not of this world"
        );
    }

    /// SP3 declines an exact tie, exercised on a **live tie** rather than on a
    /// restatement of the predicate (Amendment A5.10 L1-5).
    ///
    /// A tie is reachable: under [`GroupVote::Deterministic`] the read is uniform
    /// over the winner set, so a split vote at `R = 2` reads exactly `0.5`. The
    /// test searches for one and **fails if it cannot find one**, rather than
    /// passing vacuously.
    #[test]
    fn sp3_declines_a_live_tie() {
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };
        // Novelty off, for the reason `novelty_off` documents: with it on, every
        // indifferent internal votes act at maximum confidence and a split never
        // occurs — the tie would be untestable, which is A5.1's finding and not a
        // reason to skip the tie rule.
        let cfg = GroupAifConfig {
            vote: GroupVote::Deterministic,
            ..novelty_off()
        };
        // A two-role task, so a split vote is an exact 1-1 tie.
        let two_role = demand(&workflow(&[(Role::new(0), &[0]), (Role::new(1), &[1])]));

        let mut found = false;
        'outer: for seed in 0..24u64 {
            let map = roles_map(&[(0, 0), (1, 1), (2, 1)]);
            let p = GroupAifPolicy::new(seed, cfg, map).unwrap();
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
            for t in 0..6 {
                p.begin_task(&two_role).unwrap();
                let d = p.should_join(&a0, &coalition, &ctx);
                let s = *p.counters().agreement.last().unwrap();
                if s.roster == 2 && s.votes_for_act == 1 {
                    assert_eq!(
                        d.score.to_bits(),
                        0.0f64.to_bits(),
                        "a 1-1 winner-set tie reads p(act) = 0.5 exactly"
                    );
                    assert!(!d.act, "SP3 must DECLINE the tie, not act on it");
                    found = true;
                    break 'outer;
                }
                let succ = [
                    t % 2 == 0,
                    t % 3 == 0,
                    true,
                    false,
                    false,
                    false,
                    false,
                    false,
                ];
                p.observe_outcome(&succ);
            }
        }
        assert!(
            found,
            "no split vote occurred in the search space — the tie rule went untested, \
             which is a vacuous pass and must fail instead"
        );
    }

    /// A participant absent from the role map declines and counts — it never
    /// invents a decision (gotcha 28's typed-arm contract).
    #[test]
    fn missing_role_map_entry_declines_and_counts() {
        let a0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let stranger = TestAgent {
            id: 9,
            caps: 0b010,
            trust: 50,
        };
        let ctx = DecisionContext::default();

        // The candidate is unmapped.
        let p = policy(GroupAifConfig::default(), &[(1, 1), (2, 2)]);
        p.begin_task(&three_role_demand()).unwrap();
        let d = p.should_join(&a0, &[], &ctx);
        assert!(!d.act && d.score == 0.0);
        assert_eq!(p.counters().declines_missing_role, 1);

        // A coalition MEMBER is unmapped.
        let q = policy(GroupAifConfig::default(), &[(0, 0)]);
        q.begin_task(&three_role_demand()).unwrap();
        let coalition: [&dyn AgentCapabilities; 1] = [&stranger];
        let d = q.should_join(&a0, &coalition, &ctx);
        assert!(!d.act && d.score == 0.0);
        assert_eq!(q.counters().declines_missing_role, 1);
        assert_eq!(q.counters().reads, 0, "no read happened");
    }

    /// Amendment A1.3: a role with no demand leaves the roster, `R` varies, and
    /// both the realised sizes and the empty-slot rate are disclosed.
    #[test]
    fn empty_demand_roles_leave_the_roster() {
        let p = policy(GroupAifConfig::default(), &[(0, 0), (1, 1), (2, 2)]);
        assert_eq!(p.begin_task(&three_role_demand()).unwrap(), 3);
        assert_eq!(
            p.begin_task(&demand(&workflow(&[(Role::new(1), &[1, 3])])))
                .unwrap(),
            1,
            "only r1 has demand"
        );
        assert_eq!(
            p.begin_task(&demand(&workflow(&[
                (Role::new(0), &[0]),
                (Role::new(2), &[2]),
            ])))
            .unwrap(),
            2
        );

        let c = p.counters();
        assert_eq!(c.roster_sizes, vec![3, 1, 2]);
        // (3 - 3) + (3 - 1) + (3 - 2) — the summed empty slots.
        assert_eq!(c.empty_role_slots, 3);

        // A role outside the configured R is an error, and it clears the task so
        // later decisions decline rather than reuse stale demand.
        let err = p.begin_task(&demand(&workflow(&[(Role::new(7), &[0])])));
        assert!(matches!(
            err,
            Err(GroupAifError::RoleOutOfRange {
                role: 7,
                n_roles: 3
            })
        ));
        let a0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let d = p.should_join(&a0, &[], &DecisionContext::default());
        assert!(!d.act);
        assert_eq!(p.counters().declines_no_demand, 1);
    }

    /// SP2 identity: at all-unit multiplicity the multiplicity-weighted channel
    /// reproduces the role-restricted one **bit for bit** — the library half of
    /// X-identity, and the pin that keeps the channel from being a free parameter.
    #[test]
    fn unit_multiplicity_is_bit_identical_to_the_role_restricted_channel() {
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
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };
        let map = [(0usize, 0u8), (1, 1), (2, 2)];

        // Every step occurs exactly once ⇒ m(b, r) == 1 everywhere.
        let flat = three_role_demand();
        for step in flat.distinct() {
            assert_eq!(flat.multiplicity(step), 1);
        }

        let run = |channel| {
            let p = policy(
                GroupAifConfig {
                    channel,
                    ..GroupAifConfig::default()
                },
                &map,
            );
            let mut out = Vec::new();
            for _ in 0..3 {
                p.begin_task(&three_role_demand()).unwrap();
                let j = p.should_join(&a0, &coalition, &ctx);
                out.push((j.act, j.score.to_bits()));
                p.observe_outcome(&[true, true, false, false, false, false, false, false]);
            }
            out
        };
        assert_eq!(
            run(PrecisionChannel::MultiplicityWeighted),
            run(PrecisionChannel::RoleRestricted),
            "m == 1 everywhere must be the identity configuration"
        );
    }

    /// SP2 is **live**: repeating a step changes the decision stream, even though
    /// repetition adds no coverage demand (the whole point of the channel).
    #[test]
    fn repeated_steps_move_the_multiplicity_channel() {
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
        let coalition: [&dyn AgentCapabilities; 1] = [&a1];
        let ctx = DecisionContext {
            required_capabilities: 0b011,
        };
        let map = [(0usize, 0u8), (1, 1)];

        // Same DISTINCT demand, different multiplicity.
        let once = demand(&workflow(&[(Role::new(0), &[0]), (Role::new(1), &[1])]));
        let thrice = demand(&workflow(&[
            (Role::new(0), &[0, 0, 0]),
            (Role::new(1), &[1]),
        ]));
        assert_eq!(once.distinct_len(), thrice.distinct_len());
        assert_eq!(thrice.multiplicity(Step::new(0, Role::new(0))), 3);

        let run = |channel, d: &Demand| {
            let p = policy(
                GroupAifConfig {
                    channel,
                    ..GroupAifConfig::default()
                },
                &map,
            );
            p.begin_task(d).unwrap();
            p.should_join(&a0, &coalition, &ctx).score
        };

        // The CONTROL the previous version lacked (Amendment A5.10 L1-12): the two
        // demands carry identical DISTINCT demand, so the base channel — which
        // never reads multiplicity — must not distinguish them. Without this, the
        // assertion below would also pass if the difference came from the demand.
        assert_eq!(
            run(PrecisionChannel::RoleRestricted, &once).to_bits(),
            run(PrecisionChannel::RoleRestricted, &thrice).to_bits(),
            "the role-restricted channel must be blind to occurrence multiplicity"
        );

        let m1 = run(PrecisionChannel::MultiplicityWeighted, &once);
        let m3 = run(PrecisionChannel::MultiplicityWeighted, &thrice);
        assert_ne!(
            m1.to_bits(),
            m3.to_bits(),
            "multiplicity must reach the score; an inert channel is the failure \
             class this lineage keeps shipping"
        );
        // Amendment A5.8: and the SIGN is LESS willing, not more — concentration is
        // evidence possessed, so a repeated step has less epistemic pull left.
        assert!(
            m3 < m1,
            "SP2's measured sign: a repeated step must make the arm LESS willing to \
             staff it (got {m3} at m = 3 vs {m1} at m = 1)"
        );
    }

    /// Amendment A1.1: the two topologies are genuinely different arms — the
    /// role-specialised models learn from disjoint bit sets and diverge from the
    /// one shared model over a task stream.
    #[test]
    fn topologies_diverge_over_a_stream() {
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
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };
        let map = [(0usize, 0u8), (1, 1), (2, 2)];

        let run = |topology| {
            let p = policy(
                GroupAifConfig {
                    topology,
                    ..GroupAifConfig::default()
                },
                &map,
            );
            let mut out = Vec::new();
            for t in 0..4 {
                p.begin_task(&three_role_demand()).unwrap();
                out.push(p.should_join(&a0, &coalition, &ctx).score.to_bits());
                let succ = [t % 2 == 0, true, false, false, false, false, false, false];
                p.observe_outcome(&succ);
            }
            out
        };
        assert_ne!(
            run(WorldModelTopology::RoleSpecialised),
            run(WorldModelTopology::Shared),
            "R role-specialised models must not decide identically to one shared model"
        );
    }

    /// S-learn (i): the per-model update ledger is EXACT, and both a deficit and a
    /// surplus are detectable. Roles that left the roster do not accrue an expected
    /// update, so the gate is not vacuously satisfiable by skipping work.
    #[test]
    fn s_learn_counts_are_exact_and_role_scoped() {
        let p = policy(GroupAifConfig::default(), &[(0, 0), (1, 1), (2, 2)]);
        let succ = [true, true, true, false, false, false, false, false];

        // Task 1: all three roles demand.
        p.begin_task(&three_role_demand()).unwrap();
        assert_eq!(p.observe_outcome(&succ), 3);
        // Task 2: only r1 demands.
        p.begin_task(&demand(&workflow(&[(Role::new(1), &[1])])))
            .unwrap();
        assert_eq!(p.observe_outcome(&succ), 1);

        let c = p.counters();
        assert_eq!(c.tasks_observed, 2);
        assert!(
            c.s_learn_exact(),
            "every model advanced exactly as expected"
        );
        let by_role: Vec<(ModelLabel, u64, u64)> = c
            .model_updates
            .iter()
            .map(|a| (a.label, a.updates, a.expected))
            .collect();
        assert_eq!(
            by_role,
            vec![
                (ModelLabel::Role(Role::new(0)), 1, 1),
                (ModelLabel::Role(Role::new(1)), 2, 2),
                (ModelLabel::Role(Role::new(2)), 1, 1),
            ],
            "r1 demanded twice, r0 and r2 once each"
        );

        // Non-vacuity guard: end-of-stream state differs from initialization, on
        // every model that was asked to learn (A5.4's `all`, A6.1's scope).
        let fresh = policy(GroupAifConfig::default(), &[(0, 0), (1, 1), (2, 2)]);
        let audits = &p.counters().model_updates;
        let nv = models_moved(&p.model_snapshots(), &fresh.model_snapshots(), audits);
        assert!(nv.ok, "every world model must actually have learned");
        assert!(nv.exempt.is_empty(), "all three roles had demand here");
        assert!(
            !models_moved(&fresh.model_snapshots(), &fresh.model_snapshots(), audits).ok,
            "…and an unmoved model must NOT pass the guard"
        );
    }

    /// **Amendment A6.1**: a model the world never asked to learn is **exempt**
    /// from non-vacuity and **disclosed**, while a model that WAS asked and stayed
    /// frozen still fails. Both halves, because exempting the first without still
    /// catching the second would gut the guard.
    #[test]
    fn non_vacuity_exempts_only_never_asked_models() {
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
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };
        let succ = [true, true, true, false, false, false, false, false];

        // R = 3 configured, but the world only ever demands roles 0 and 2 —
        // exactly seeds 354/355, where no pool worker carries role 1.
        let arm = policy(GroupAifConfig::default(), &[(0, 0), (1, 2), (2, 0)]);
        let reference = policy(GroupAifConfig::default(), &[(0, 0), (1, 2), (2, 0)]);
        let d = demand(&workflow(&[(Role::new(0), &[0, 1]), (Role::new(2), &[2])]));
        for _ in 0..4 {
            arm.begin_task(&d).unwrap();
            let _ = arm.should_join(&a0, &coalition, &ctx);
            arm.observe_outcome(&succ);
        }

        let c = arm.counters();
        // The counted ledger is untouched by A6.1 and still exact.
        assert!(
            c.s_learn_exact(),
            "the ledger stays exact: {:?}",
            c.model_updates
        );
        assert_eq!(
            c.model_updates[1].expected, 0,
            "role 1 was never asked to learn"
        );

        let nv = models_moved(
            &arm.model_snapshots(),
            &reference.model_snapshots(),
            &c.model_updates,
        );
        assert!(
            nv.ok,
            "a never-asked model must not fail the guard (A6.1) — this is the \
             RUN-INVALID the first official run returned"
        );
        assert_eq!(
            nv.exempt,
            vec![ModelLabel::Role(Role::new(1))],
            "…and it must be DISCLOSED, not silently dropped"
        );

        // The other half: a model that WAS asked and did not move still fails.
        // Constructed by handing the guard the arm's own audits against its own
        // post-run snapshots, so every in-scope model has zero delta.
        let frozen = models_moved(
            &arm.model_snapshots(),
            &arm.model_snapshots(),
            &c.model_updates,
        );
        assert!(
            !frozen.ok,
            "an asked-and-frozen model must STILL fail — A5.4's `all` stands \
             inside A6.1's scope"
        );
        assert_eq!(frozen.exempt, vec![ModelLabel::Role(Role::new(1))]);

        // And "everything exempt" is not a pass: nothing was asked to learn at all.
        let idle = policy(GroupAifConfig::default(), &[(0, 0)]);
        let idle_ref = policy(GroupAifConfig::default(), &[(0, 0)]);
        let idle_c = idle.counters();
        let all_exempt = models_moved(
            &idle.model_snapshots(),
            &idle_ref.model_snapshots(),
            &idle_c.model_updates,
        );
        assert_eq!(all_exempt.exempt.len(), 3, "no task was ever opened");
        assert!(
            !all_exempt.ok,
            "a run in which NOTHING was asked to learn is vacuous, not exempt"
        );
    }

    /// S-learn (i) catches a real **deficit** — a task opened and never observed
    /// (Amendment A5.4). The previous test claimed detectability and constructed
    /// neither failure.
    #[test]
    fn s_learn_detects_a_deficit() {
        let p = policy(GroupAifConfig::default(), &[(0, 0), (1, 1), (2, 2)]);
        let succ = [true, true, true, false, false, false, false, false];

        p.begin_task(&three_role_demand()).unwrap();
        assert_eq!(p.observe_outcome(&succ), 3);
        assert!(p.counters().s_learn_exact());

        // Open a second task and observe it with the WRONG width, so the engine
        // update is skipped whole while the task stream has already advanced.
        p.begin_task(&three_role_demand()).unwrap();
        assert_eq!(
            p.observe_outcome(&[true; 3]),
            0,
            "a width mismatch applies nothing"
        );

        let c = p.counters();
        assert!(
            !c.s_learn_exact(),
            "a task opened but not advanced is a DEFICIT and must be RUN-INVALID"
        );
        assert!(
            c.model_updates.iter().all(|a| a.updates < a.expected),
            "every model is short by exactly one: {:?}",
            c.model_updates
        );
        assert_eq!(c.tasks_observed, 1, "the skipped task is not observed");
        assert_eq!(c.roster_sizes.len(), 2, "…but it WAS opened");
    }

    /// S-learn (i) catches a real **surplus** — the MMP double-apply shape
    /// (Amendment A5.4).
    ///
    /// This is the case the pre-A5.4 ledger could not see: `expected` was
    /// incremented on the `observe_outcome` path immediately beside `updates`, so a
    /// doubled observation advanced both and read exact. `expected` now comes from
    /// `begin_task` alone.
    #[test]
    fn s_learn_detects_a_surplus() {
        let p = policy(GroupAifConfig::default(), &[(0, 0), (1, 1), (2, 2)]);
        let succ = [true, true, true, false, false, false, false, false];

        p.begin_task(&three_role_demand()).unwrap();
        assert_eq!(p.observe_outcome(&succ), 3);
        // One task, observed twice — exactly the double-apply the gate exists for.
        assert_eq!(p.observe_outcome(&succ), 3);

        let c = p.counters();
        assert!(
            !c.s_learn_exact(),
            "a doubled observation is a SURPLUS and must be RUN-INVALID"
        );
        for a in &c.model_updates {
            assert_eq!(
                (a.updates, a.expected),
                (2, 1),
                "{:?} advanced twice",
                a.label
            );
        }
        assert_eq!(c.tasks_observed, 2);
        assert_eq!(c.roster_sizes.len(), 1, "only one task was ever opened");
    }

    /// The shared topology's ledger is per-task, not per-role — stated as a test so
    /// the two topologies' expected counts can never drift apart silently.
    #[test]
    fn shared_topology_updates_once_per_task() {
        let p = policy(
            GroupAifConfig {
                topology: WorldModelTopology::Shared,
                ..GroupAifConfig::default()
            },
            &[(0, 0), (1, 1), (2, 2)],
        );
        let succ = [true, true, true, false, false, false, false, false];
        p.begin_task(&three_role_demand()).unwrap();
        assert_eq!(p.observe_outcome(&succ), 1);
        p.begin_task(&demand(&workflow(&[(Role::new(1), &[1])])))
            .unwrap();
        assert_eq!(p.observe_outcome(&succ), 1);

        let c = p.counters();
        assert_eq!(c.model_updates.len(), 1);
        assert_eq!(c.model_updates[0].label, ModelLabel::Shared);
        assert!(c.s_learn_exact());
        assert_eq!(c.model_updates[0].updates, 2);
    }

    /// A failing member yields an `AifError` at the group instead of a plausible
    /// answer, and the read-only slot refuses to act.
    ///
    /// **Renamed from `upstream_error_declines_and_counts` (Amendment A5.10
    /// L1-6).** That name promised something the test did not do: it never drove
    /// the *policy*, so `count_upstream_decline` was never reached and
    /// `declines_upstream` was never asserted. It is renamed rather than extended
    /// because the policy-level path is **unreachable under every registered
    /// configuration** — `begin_task` rejects out-of-range roles, `models.view`
    /// therefore always resolves, `role_query`'s model is well-formed by
    /// construction (normalized `A`/`B`/`D`, `C` in `(0, 1]`), and the replay
    /// width always equals the modality count. The counter guards a real defect
    /// class and stays; it simply cannot be provoked from outside, and saying so
    /// beats a test that pretends otherwise.
    #[test]
    fn a_failing_member_surfaces_as_an_engine_error_not_an_answer() {
        let mut member = {
            let arm = PersistentAifArm::new(3, v5_e1_base()).unwrap();
            let (query, _) = arm.role_query(0b011, 0b001, 0b011, 7, None).unwrap();
            RoleMember {
                query,
                // Deliberately the wrong width: the query has 2 modalities.
                replay: vec![vec![NO_OBS; 5]],
                modalities: 2,
                last_vote: None,
                last_weight: 0.0,
                last_dist: None,
                candidate_sensitive: true,
            }
        };
        let probs = <RoleMember as aif::InternalAgent>::action_probabilities(&mut member, 0);
        assert_eq!(
            probs.len(),
            0,
            "a failed replay reports an empty distribution"
        );

        // …and that empty distribution becomes a counted decline at the group.
        let mut group = aif::GroupAgent::with_slots_seeded(
            aif::CopyAgent,
            vec![member],
            aif::VotingAgent::with_seed(GROUP_N_ACTIONS, aif::VotingMode::CertaintyWeighted, 1),
            GROUP_N_ACTIONS,
            1,
        );
        assert!(matches!(
            group.group_distribution(SENSORY_OBSERVATION),
            Err(aif::AifError::InvalidLength {
                expected: 2,
                got: 0
            })
        ));

        // The read-only slot refuses to act rather than sampling something.
        let arm = PersistentAifArm::new(3, v5_e1_base()).unwrap();
        let (query, replay) = arm.role_query(0b011, 0b001, 0b011, 7, None).unwrap();
        let mut ok = RoleMember {
            query,
            replay,
            modalities: 2,
            last_vote: None,
            last_weight: 0.0,
            last_dist: None,
            candidate_sensitive: true,
        };
        assert!(matches!(
            <RoleMember as aif::Agent>::act(&mut ok, 0),
            Err(aif::AifError::Unsupported(_))
        ));

        // A5.10 L1-14: the one construction that IS rejected, so the arm cannot be
        // built into a silently all-declining shape.
        assert!(matches!(
            GroupAifPolicy::new(
                0,
                GroupAifConfig {
                    read: DecisionRead::SeededSampling,
                    vote: GroupVote::Deterministic,
                    ..GroupAifConfig::default()
                },
                roles_map(&[(0, 0)]),
            ),
            Err(GroupAifError::UnsupportedReadVote)
        ));
    }

    /// A decision before any `begin_task` declines and counts, rather than reading
    /// an empty roster into the engine.
    #[test]
    fn no_task_in_force_declines_and_counts() {
        let p = policy(GroupAifConfig::default(), &[(0, 0)]);
        let a0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let d = p.should_join(&a0, &[], &DecisionContext::default());
        assert!(!d.act && d.score == 0.0);
        let c = p.counters();
        assert_eq!(c.declines_no_demand, 1);
        assert_eq!(c.decisions, 1);
        assert_eq!(c.reads, 0);
    }

    /// Coverage is role-matched: a worker of the wrong role does not staff a step,
    /// so joining with a role-mismatched capability must not score like the
    /// role-matched one.
    #[test]
    fn coverage_is_role_matched() {
        let ctx = DecisionContext {
            required_capabilities: 0b011,
        };
        let d2 = demand(&workflow(&[(Role::new(0), &[0]), (Role::new(1), &[1])]));

        // `matched` holds bit 1 and IS role 1; `mismatched` holds bit 1 but is role 2.
        let candidate = TestAgent {
            id: 0,
            caps: 0b010,
            trust: 50,
        };
        let anchor = TestAgent {
            id: 1,
            caps: 0b001,
            trust: 50,
        };
        let coalition: [&dyn AgentCapabilities; 1] = [&anchor];

        let run = |candidate_role: u8| {
            let p = policy(GroupAifConfig::default(), &[(0, candidate_role), (1, 0)]);
            p.begin_task(&d2).unwrap();
            p.should_join(&candidate, &coalition, &ctx).score.to_bits()
        };
        assert_ne!(
            run(1),
            run(2),
            "the same capability under a different role must not read the same"
        );
    }

    /// Amendment A2.1: the `grp-role-blind` reference leg is a genuinely different
    /// arm from the registered role-matched one, and the difference is exactly the
    /// cross-role capability leaking into a role's coverage masks.
    #[test]
    fn role_blind_masks_diverge_from_role_matched() {
        let ctx = DecisionContext {
            required_capabilities: 0b011,
        };
        let d2 = demand(&workflow(&[(Role::new(0), &[0]), (Role::new(1), &[1])]));

        // The candidate is role 0 and holds bit 1 — which role 1 needs and role 0
        // does not. Role-matched: it contributes nothing to r1's masks. Role-blind:
        // it "covers" r1's step.
        let candidate = TestAgent {
            id: 0,
            caps: 0b010,
            trust: 50,
        };
        let anchor = TestAgent {
            id: 1,
            caps: 0b001,
            trust: 50,
        };
        let coalition: [&dyn AgentCapabilities; 1] = [&anchor];
        let map = [(0usize, 0u8), (1, 0)];

        let run = |masks| {
            let p = policy(
                GroupAifConfig {
                    masks,
                    ..GroupAifConfig::default()
                },
                &map,
            );
            let mut out = Vec::new();
            for _ in 0..2 {
                p.begin_task(&d2).unwrap();
                let j = p.should_join(&candidate, &coalition, &ctx);
                out.push((j.act, j.score.to_bits()));
                p.observe_outcome(&[true, false, false, false, false, false, false, false]);
            }
            out
        };
        assert_ne!(
            run(CoverageMasks::RoleBlind),
            run(CoverageMasks::RoleMatched),
            "role-blind masks must be a different arm, not a relabelling"
        );

        // …and the registered default is the role-matched one.
        assert_eq!(GroupAifConfig::default().masks, CoverageMasks::RoleMatched);
    }

    /// E-agree records one sample per read, and the sample carries the **A5.1**
    /// measurement: how many internals the candidate reached, and what weight the
    /// blind ones took.
    ///
    /// Rebuilt from `agreement_samples_track_every_read` (Amendment A5.10 L1-13),
    /// whose two headline assertions compared a function to its own definition
    /// (`unanimous()` against its body) or were trivially true (`votes ≤ roster`).
    /// Every assertion below can fail on a real regression.
    #[test]
    fn agreement_samples_measure_candidate_reach() {
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
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };

        // The candidate is role 0, and all three roles demand ⇒ exactly ONE
        // candidate-sensitive internal out of three (A5.1's central claim).
        let arm = policy(GroupAifConfig::default(), &[(0, 0), (1, 1), (2, 2)]);
        arm.begin_task(&three_role_demand()).unwrap();
        let decision = arm.should_join(&a0, &coalition, &ctx);

        let counters = arm.counters();
        assert_eq!(
            counters.agreement.len(),
            1,
            "one sample per successful read"
        );
        assert_eq!(
            u64::try_from(counters.agreement.len()).unwrap(),
            counters.reads
        );
        let s = counters.agreement[0];
        assert_eq!(s.roster, 3, "all three roles demanded");
        assert_eq!(
            s.candidate_sensitive, 1,
            "under role-matched masks only the candidate's own role internal sees it"
        );
        assert_eq!(s.candidate_blind(), 2);
        assert_eq!(
            s.votes_for_act_sensitive + s.votes_for_act_blind,
            s.votes_for_act,
            "the split must partition the votes"
        );
        assert!(
            (s.margin - decision.score.abs()).abs() < 1e-15,
            "the sample's margin is |p(act) − 0.5|"
        );
        // A5.1: the blind internals are not a rounding error in the mixture. Two of
        // three maximally-confident members carry the majority of the weight.
        assert!(
            s.blind_weight_share > 0.5,
            "the candidate-blind internals carry {:.3} of the CW weight — A5.1 \
             predicts a majority, and a share near 0 would mean the hazard is not real",
            s.blind_weight_share
        );
        assert!((0.0..=1.0).contains(&s.blind_weight_share));

        // Move the candidate to a role NOT on the roster ⇒ ZERO candidate-sensitive
        // internals, which is A5.1's ~41 % case.
        let off_roster = policy(GroupAifConfig::default(), &[(0, 2), (1, 1), (2, 1)]);
        off_roster
            .begin_task(&demand(&workflow(&[
                (Role::new(0), &[0]),
                (Role::new(1), &[1]),
            ])))
            .unwrap();
        let _ = off_roster.should_join(&a0, &coalition, &ctx);
        let s = off_roster.counters().agreement[0];
        assert_eq!(
            s.candidate_sensitive, 0,
            "a candidate of a non-rostered role reaches no internal at all"
        );
        assert!(
            (s.blind_weight_share - 1.0).abs() < 1e-12,
            "…and then the blind internals carry ALL of the weight"
        );

        // A single-role task shrinks the roster, and the sample says so.
        arm.begin_task(&demand(&workflow(&[(Role::new(1), &[1])])))
            .unwrap();
        let _ = arm.should_join(&a0, &coalition, &ctx);
        assert_eq!(arm.counters().agreement[1].roster, 1);
    }

    /// **A5.1, the measurement itself:** with the candidate's role off the roster,
    /// two candidates with *different capabilities* produce **bit-identical**
    /// scores — the query carries no information about the candidate at all.
    ///
    /// This is the finding, not a defect: it is what "a group deliberating about
    /// one candidate is structurally impossible under role-matched masks" means in
    /// code, and it must stay visible if the mask rule is ever touched.
    #[test]
    fn a_non_rostered_candidate_is_invisible_to_the_group() {
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };
        // Roster is roles 0 and 1; both candidates are role 2, so neither reaches
        // an internal under the registered masks. The coalition deliberately leaves
        // bit 1 UNCOVERED, so a candidate holding it is decision-relevant to
        // internal r1 the moment the masks stop filtering by role — without that
        // the role-blind contrast below would be swamped by saturation.
        let two_role = demand(&workflow(&[(Role::new(0), &[0]), (Role::new(1), &[1])]));
        let anchor_a = TestAgent {
            id: 1,
            caps: 0b001,
            trust: 50,
        };
        let anchor_b = TestAgent {
            id: 2,
            caps: 0b001,
            trust: 50,
        };
        let coalition: [&dyn AgentCapabilities; 2] = [&anchor_a, &anchor_b];
        let roles = [(0usize, 2u8), (1, 0), (2, 1)];

        let run = |masks, caps: u32| {
            let p = policy(
                GroupAifConfig {
                    masks,
                    ..GroupAifConfig::default()
                },
                &roles,
            );
            p.begin_task(&two_role).unwrap();
            let cand = TestAgent {
                id: 0,
                caps,
                trust: 50,
            };
            p.should_join(&cand, &coalition, &ctx).score.to_bits()
        };
        assert_eq!(
            run(CoverageMasks::RoleMatched, 0b010),
            run(CoverageMasks::RoleMatched, 0b000),
            "two candidates of a non-rostered role must read identically even when \
             one of them covers a bit nobody else does — that IS A5.1"
        );

        // The contrast that makes it a measurement: role-BLIND masks do see them.
        assert_ne!(
            run(CoverageMasks::RoleBlind, 0b010),
            run(CoverageMasks::RoleBlind, 0b000),
            "role-blind masks DO carry the candidate, so the identity above is a \
             property of the registered mask rule and not of this world"
        );
    }

    /// The leave path's `cfg0 == cfg1` disclosure counts what A5.1 says it counts.
    #[test]
    fn leave_path_identical_masks_are_counted() {
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
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];

        let p = policy(GroupAifConfig::default(), &[(0, 0), (1, 1), (2, 2)]);
        p.begin_task(&three_role_demand()).unwrap();
        let _ = p.should_leave(&a0, &full, &ctx);
        let c = p.counters();
        assert_eq!(c.leave_queries, 3, "one query per rostered role");
        assert_eq!(
            c.leave_queries_identical, 2,
            "the two internals whose role is not the candidate's see cfg0 == cfg1"
        );

        // The join path contributes nothing to this counter.
        p.begin_task(&three_role_demand()).unwrap();
        let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2];
        let _ = p.should_join(&a0, &coalition, &ctx);
        assert_eq!(p.counters().leave_queries, 3, "joins are not leave queries");
    }

    /// `E-seed`: the seeded-sampling cell runs (upstream never calls a member's
    /// `act` under `CertaintyWeighted`), stays deterministic under its seed, and
    /// reports the draw rather than a fabricated margin.
    #[test]
    fn seeded_sampling_cell_runs_and_reports_the_draw_not_a_margin() {
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
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };
        let map = [(0usize, 0u8), (1, 1), (2, 2)];
        let cfg = GroupAifConfig {
            read: DecisionRead::SeededSampling,
            ..GroupAifConfig::default()
        };

        let run = || {
            let p = policy(cfg, &map);
            let mut out = Vec::new();
            for _ in 0..3 {
                p.begin_task(&three_role_demand()).unwrap();
                let d = p.should_join(&a0, &coalition, &ctx);
                out.push((d.act, d.score.to_bits()));
                p.observe_outcome(&[true, false, true, false, false, false, false, false]);
            }
            (out, p.counters())
        };
        let (a, ca) = run();
        let (b, _) = run();
        assert_eq!(a, b, "the seeded draw must still be a function of the seed");
        for (act, bits) in &a {
            let expected = if *act { 0.5f64 } else { -0.5f64 };
            assert_eq!(
                *bits,
                expected.to_bits(),
                "score encodes the draw, not a margin"
            );
        }
        assert_eq!(ca.declines_upstream, 0, "the sampled path must not error");
        assert!(
            ca.agreement.is_empty(),
            "the sampling cell exposes no mixture, so it records no E-agree sample"
        );
        assert_eq!(ca.reads, 3);
    }

    /// Amendment A3.1 + **A5.6**: the `grp-role-det` reference leg runs, stays
    /// RNG-free, and reads **uniform-over-winners** — support `{0, 0.5, 1}`, not
    /// `k/R`.
    ///
    /// The previous assertion (`p_act == votes / roster`) was **vacuous**: it holds
    /// *iff* the read is unanimous, and ~23 % are not. This version asserts the
    /// support directly and **fails if it never sees a non-unanimous read**, so it
    /// cannot pass by only exercising the easy case.
    #[test]
    fn deterministic_vote_reads_uniform_over_winners() {
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
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };
        let map = [(0usize, 0u8), (1, 1), (2, 2)];
        // Novelty off so a non-unanimous read is reachable at all — see
        // `novelty_off`. The winner-set arithmetic under test is independent of it.
        let cfg = GroupAifConfig {
            vote: GroupVote::Deterministic,
            ..novelty_off()
        };

        let run = |seed: u64| {
            let p = GroupAifPolicy::new(seed, cfg, roles_map(&map)).unwrap();
            let mut out = Vec::new();
            for t in 0..8 {
                p.begin_task(&three_role_demand()).unwrap();
                let d = p.should_join(&a0, &coalition, &ctx);
                out.push((d.act, d.score.to_bits()));
                let succ = [
                    t % 2 == 0,
                    t % 3 == 0,
                    true,
                    false,
                    false,
                    false,
                    false,
                    false,
                ];
                p.observe_outcome(&succ);
            }
            (out, p.counters())
        };
        let (a, ca) = run(11);
        let (b, _) = run(4_242_424_242);
        assert_eq!(
            a, b,
            "the discrete read is RNG-free too — shown by seed invariance"
        );
        assert_eq!(ca.declines_upstream, 0, "the discrete read must not error");

        // A5.6: `vote_distribution`'s Deterministic branch is uniform mass over the
        // max-count winner set, so at n_actions = 2 the read is exactly one of
        // {0, 0.5, 1} — never k/R.
        let mut saw_split = false;
        for (sample, (act, bits)) in ca.agreement.iter().zip(a.iter()) {
            let p_act = f64::from_bits(*bits) + 0.5;
            assert!(
                [0.0f64, 0.5, 1.0].iter().any(|v| (p_act - v).abs() < 1e-12),
                "the discrete read must land on {{0, 0.5, 1}}, got {p_act}"
            );
            // …and SP3 over that support is "winners == {act}", i.e. strict majority.
            let majority = sample.votes_for_act * 2 > sample.roster;
            assert_eq!(
                *act, majority,
                "act ⇔ strict majority: {} of {} voted act, read {p_act}",
                sample.votes_for_act, sample.roster
            );
            if !sample.unanimous() {
                saw_split = true;
                // The case the old assertion silently skipped: k/R would be ⅓ or ⅔
                // here, and the read is not.
                assert!(
                    (p_act - 0.0).abs() < 1e-12
                        || (p_act - 0.5).abs() < 1e-12
                        || (p_act - 1.0).abs() < 1e-12,
                    "non-unanimous read must still be uniform-over-winners, got {p_act}"
                );
            }
        }
        assert!(
            saw_split,
            "every read was unanimous, so the k/R-versus-winner-set distinction went \
             untested — the exact vacuity A5.6 flagged"
        );

        // …and the registered default is still `CertaintyWeighted` (D2 stands).
        assert_eq!(GroupAifConfig::default().vote, GroupVote::CertaintyWeighted);
    }

    /// **X-identity's alignment conjunct (Amendment A5.10 L2-16).** SP2's scale
    /// vector must be indexed by the query's modality order — `set_bits(required_r)`
    /// — and the `m ≡ 1` identity structurally cannot see a permutation, because
    /// every entry is 1 there.
    ///
    /// Asked directly: one internal carrying two modalities whose multiplicities
    /// **differ**, with the assignment swapped between them. A scale indexed by
    /// anything else reads the two identically.
    #[test]
    fn sp2_scale_is_indexed_by_modality_order() {
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };
        let cfg = GroupAifConfig {
            channel: PrecisionChannel::MultiplicityWeighted,
            ..GroupAifConfig::default()
        };
        // Role 0 carries bits 0 and 2 with multiplicities (1, 3) then (3, 1);
        // role 1 carries bit 1 throughout.
        let a = demand(&workflow(&[
            (Role::new(0), &[0, 2, 2, 2]),
            (Role::new(1), &[1]),
        ]));
        let b = demand(&workflow(&[
            (Role::new(0), &[0, 0, 0, 2]),
            (Role::new(1), &[1]),
        ]));
        assert_eq!(a.multiplicity(Step::new(0, Role::new(0))), 1);
        assert_eq!(a.multiplicity(Step::new(2, Role::new(0))), 3);
        assert_eq!(b.multiplicity(Step::new(0, Role::new(0))), 3);
        assert_eq!(b.multiplicity(Step::new(2, Role::new(0))), 1);

        // The candidate covers only ONE of role 0's two bits. Full coverage would
        // saturate the read to `p(act) = 1.0` in both arrangements (A5.1) and the
        // probe would report a false FAIL — measured, not guessed: with
        // `caps = 0b101` both reads are exactly `0.5` score bits.
        let cand = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let other = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let coalition: [&dyn AgentCapabilities; 1] = [&other];
        let read = |d: &Demand| {
            let p = policy(cfg, &[(0, 0), (1, 1)]);
            p.begin_task(d).expect("the demand is inside the universe");
            p.should_join(&cand, &coalition, &ctx).score.to_bits()
        };
        assert_ne!(
            read(&a),
            read(&b),
            "permuting which bit carries which multiplicity must move the read; if it \
             does not, the scale is not aligned with `set_bits(required_r)` order"
        );
    }

    // --- Vote routing -------------------------------------------------------

    /// One decision of [`scripted_stream`]: `(act, score bits)`.
    type StreamDecisions = Vec<(bool, u64)>;

    /// Nine tasks — rosters 3, 2, 1, cycled three times — each with three joins
    /// and three leaves over a five-agent pool (roles 0, 1, 2, 0, 1), and one
    /// outcome observed after every task so the world models learn in between.
    /// Per cycle the candidate's role is on the roster with a roster of at least
    /// two in 6 + 4 + 0 decisions.
    fn scripted_stream(config: GroupAifConfig) -> (StreamDecisions, GroupAifCounters) {
        let pool = [
            TestAgent {
                id: 0,
                caps: 0b0001,
                trust: 50,
            },
            TestAgent {
                id: 1,
                caps: 0b0010,
                trust: 50,
            },
            TestAgent {
                id: 2,
                caps: 0b0100,
                trust: 50,
            },
            TestAgent {
                id: 3,
                caps: 0b1001,
                trust: 50,
            },
            TestAgent {
                id: 4,
                caps: 0b1010,
                trust: 50,
            },
        ];
        let p = policy(config, &[(0, 0), (1, 1), (2, 2), (3, 0), (4, 1)]);
        let tasks = [
            three_role_demand(),
            demand(&workflow(&[(Role::new(0), &[0, 3]), (Role::new(1), &[1])])),
            demand(&workflow(&[(Role::new(1), &[1, 3])])),
        ];
        let ctx = DecisionContext {
            required_capabilities: 0b1111,
        };
        let view = |ids: &[usize]| -> Vec<&dyn AgentCapabilities> {
            ids.iter()
                .map(|&i| &pool[i] as &dyn AgentCapabilities)
                .collect()
        };

        let mut out = Vec::new();
        for t in 0..9usize {
            p.begin_task(&tasks[t % 3]).unwrap();
            for (candidate, coalition) in [
                (0usize, vec![1usize, 2]),
                (2, vec![0, 1]),
                (4, vec![0, 1, 2]),
            ] {
                let d = p.should_join(&pool[candidate], &view(&coalition), &ctx);
                out.push((d.act, d.score.to_bits()));
            }
            for leaver in [0usize, 2, 1] {
                let d = p.should_leave(&pool[leaver], &view(&[0, 1, 2, 3]), &ctx);
                out.push((d.act, d.score.to_bits()));
            }
            let mut succ = [false; 8];
            for (b, slot) in succ.iter_mut().enumerate().take(4) {
                *slot = (t + b) % 2 == 0;
            }
            p.observe_outcome(&succ);
        }
        (out, p.counters())
    }

    fn star(lambda: f64, base: GroupAifConfig) -> GroupAifConfig {
        GroupAifConfig {
            routing: VoteRouting::CandidateStar { lambda },
            ..base
        }
    }

    /// `CandidateStar { lambda: 0.0 }` reproduces `Off` — acts, score bits and
    /// the whole counter ledger — over [`scripted_stream`], with novelty on and
    /// off; `lambda: 0.5` does not.
    #[test]
    fn candidate_star_at_lambda_zero_is_bit_identical_to_off() {
        for (name, base) in [
            ("default", GroupAifConfig::default()),
            ("novelty off", novelty_off()),
        ] {
            let (off, off_counters) = scripted_stream(base);
            let (id, id_counters) = scripted_stream(star(0.0, base));
            assert_eq!(off.len(), 54);
            assert_eq!(off_counters.declines_upstream, 0, "{name}");
            assert_eq!(off_counters.reads, 54, "{name}");
            assert_eq!(off_counters.roster_sizes, vec![3, 2, 1, 3, 2, 1, 3, 2, 1]);
            let first_diff = off.iter().zip(&id).position(|(a, b)| a != b);
            assert_eq!(
                first_diff,
                None,
                "{name}: lambda = 0 diverges from Off at decision {first_diff:?}: \
                 Off {:?}, lambda = 0 {:?}",
                first_diff.map(|i| off[i]),
                first_diff.map(|i| id[i])
            );
            assert_eq!(
                id_counters.routed_reads, 0,
                "{name}: lambda = 0 counted {} routed reads",
                id_counters.routed_reads
            );
            assert_eq!(id_counters, off_counters, "{name}: counter ledgers differ");

            let (half, _) = scripted_stream(star(0.5, base));
            let differing = off.iter().zip(&half).filter(|(a, b)| a != b).count();
            assert!(
                differing > 0,
                "{name}: lambda = 0.5 reproduced Off on all 54 decisions, so the \
                 identity above cannot fail on this stream"
            );
        }
        // Both answers occur under the default base, join and leave alike.
        let (off, _) = scripted_stream(GroupAifConfig::default());
        let acts = off.iter().filter(|(act, _)| *act).count();
        assert!(
            acts > 0 && acts < off.len(),
            "the stream must carry acts and declines, got {acts} acts of {}",
            off.len()
        );
    }

    /// A decision whose candidate's role is off the roster reads identically
    /// under `lambda = 0.5` and under `Off`, and counts no routed read.
    #[test]
    fn centre_absent_decisions_are_bit_identical_to_off() {
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
        let outsider = TestAgent {
            id: 2,
            caps: 0b011,
            trust: 50,
        };
        let coalition: [&dyn AgentCapabilities; 2] = [&a0, &a1];
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &outsider];
        let ctx = DecisionContext {
            required_capabilities: 0b011,
        };
        // Roster {r0, r1}; the candidate is role 2.
        let two_role = demand(&workflow(&[(Role::new(0), &[0]), (Role::new(1), &[1])]));

        let run = |config: GroupAifConfig| {
            let p = policy(config, &[(0, 0), (1, 1), (2, 2)]);
            let mut out = Vec::new();
            for t in 0..4usize {
                p.begin_task(&two_role).unwrap();
                let j = p.should_join(&outsider, &coalition, &ctx);
                out.push((j.act, j.score.to_bits()));
                let l = p.should_leave(&outsider, &full, &ctx);
                out.push((l.act, l.score.to_bits()));
                p.observe_outcome(&[t % 2 == 0, true, false, false, false, false, false, false]);
            }
            (out, p.counters())
        };
        for (name, base) in [
            ("default", GroupAifConfig::default()),
            ("novelty off", novelty_off()),
        ] {
            let (off, off_counters) = run(base);
            let (half, half_counters) = run(star(0.5, base));
            assert_eq!(
                half, off,
                "{name}: a centre-absent read moved under routing"
            );
            assert_eq!(
                (half_counters.reads, half_counters.routed_reads),
                (8, 0),
                "{name}: (reads, routed_reads)"
            );
            assert_eq!(half_counters, off_counters, "{name}");
            for s in &half_counters.agreement {
                assert_eq!(
                    (s.candidate_sensitive, s.sensitive_rows, s.centre_vote),
                    (0, 0, None),
                    "{name}"
                );
            }
        }
    }

    /// `p(act)` of a `CertaintyWeighted` `VotingAgent` behind the
    /// [`candidate_star`] topology over `outputs`.
    fn star_p_act(outputs: &[[f64; 2]], centre: Option<usize>, lambda: f64) -> f64 {
        use aif::Aggregator as _;
        let (topology, _) = candidate_star(outputs.len(), centre, lambda).unwrap();
        let mut routed = aif::RoutedAggregator::with_seed(
            aif::VotingAgent::with_seed(GROUP_N_ACTIONS, aif::VotingMode::CertaintyWeighted, 1),
            topology,
            GROUP_N_ACTIONS,
            1,
        );
        let dists: Vec<DVector<f64>> = outputs
            .iter()
            .map(|o| DVector::from_vec(o.to_vec()))
            .collect();
        routed
            .aggregate_weighted_distribution(&dists)
            .unwrap()
            .expect("a VotingAgent exposes its mixture")[ACTION_ACT]
    }

    /// The prereg §4 arithmetic on delta outputs: `k` blind members at `[0, 1]`,
    /// the centre at `[1, 0]`, `p(act) = k·w·(1 − λ) / (k·w + 1)` with
    /// `w = exp(−H(λ))`.
    #[test]
    fn candidate_star_arithmetic_on_delta_outputs() {
        const CENTRE: [f64; 2] = [1.0, 0.0];
        const BLIND: [f64; 2] = [0.0, 1.0];
        let close = |observed: f64, expected: f64, tol: f64, what: &str| {
            assert!(
                (observed - expected).abs() <= tol,
                "{what}: observed {observed}, expected {expected} ± {tol}"
            );
        };

        // k = 2, the centre in the MIDDLE roster position.
        let k2 = [BLIND, CENTRE, BLIND];
        close(star_p_act(&k2, Some(1), 0.0), 2.0 / 3.0, 1e-12, "k=2 λ=0");
        close(star_p_act(&k2, Some(1), 0.5), 0.25, 1e-12, "k=2 λ=0.5");
        close(star_p_act(&k2, Some(1), 0.25), 0.39949, 1e-4, "k=2 λ=0.25");
        close(star_p_act(&k2, Some(1), 1.0), 0.0, 1e-12, "k=2 λ=1");
        // The SP3 flip at k = 2 sits between λ = 0.130 and λ = 0.135.
        let closed_form = |lambda: f64| {
            let h = -(lambda * lambda.ln() + (1.0 - lambda) * (1.0 - lambda).ln());
            let w = (-h).exp();
            2.0 * w * (1.0 - lambda) / (2.0 * w + 1.0)
        };
        let at_130 = star_p_act(&k2, Some(1), 0.130);
        let at_135 = star_p_act(&k2, Some(1), 0.135);
        close(at_130, closed_form(0.130), 1e-12, "k=2 λ=0.130 closed form");
        close(at_135, closed_form(0.135), 1e-12, "k=2 λ=0.135 closed form");
        close(at_130, 0.5012, 1e-4, "k=2 λ=0.130");
        close(at_135, 0.4963, 1e-4, "k=2 λ=0.135");
        assert!(
            at_130 > 0.5 && at_135 < 0.5,
            "the flip must sit between λ = 0.130 (observed {at_130}) and λ = 0.135 \
             (observed {at_135})"
        );

        // k = 1, the centre LAST.
        close(
            star_p_act(&[BLIND, CENTRE], Some(1), 0.5),
            1.0 / 6.0,
            1e-12,
            "k=1 λ=0.5",
        );
        // The mirrored case: the centre votes act, the two others decline.
        close(
            star_p_act(&[[0.0, 1.0], [1.0, 0.0], [1.0, 0.0]], Some(0), 0.5),
            0.75,
            1e-12,
            "centre act, 2 decliners, λ=0.5",
        );
        // No centre: identity rows whatever λ is.
        close(star_p_act(&k2, None, 0.5), 2.0 / 3.0, 1e-12, "no centre");
    }

    /// `routed_reads` counts exactly the successful reads with the candidate's
    /// role on a roster of at least two under `lambda > 0`, and nothing under
    /// `Off` or `lambda = 0`.
    #[test]
    fn routed_reads_counts_the_non_identity_topologies() {
        let ledger = |c: &GroupAifCounters| {
            c.agreement
                .iter()
                .filter(|s| s.candidate_sensitive >= 1 && s.roster >= 2)
                .count() as u64
        };
        for lambda in [0.5, 1.0] {
            let (_, c) = scripted_stream(star(lambda, GroupAifConfig::default()));
            assert_eq!(c.declines_upstream, 0);
            assert_eq!(
                c.routed_reads, 30,
                "λ={lambda}: 3 cycles × (6 + 4 + 0), observed {}",
                c.routed_reads
            );
            assert_eq!(
                c.routed_reads,
                ledger(&c),
                "λ={lambda}: counter {} vs ledger {}",
                c.routed_reads,
                ledger(&c)
            );
        }
        let (_, off) = scripted_stream(GroupAifConfig::default());
        assert_eq!(ledger(&off), 30, "the Off ledger sees the same 30 reads");
        assert_eq!(off.routed_reads, 0, "Off routes nothing");
        let (_, id) = scripted_stream(star(0.0, GroupAifConfig::default()));
        assert_eq!(id.routed_reads, 0, "λ = 0 is an identity topology");
    }

    /// With routing `Off`, `blind_origin_share` is `blind_weight_share` bit for
    /// bit and `sensitive_rows` is `candidate_sensitive`, on every sample; with
    /// `lambda = 1` a centre-present read has every row candidate-sensitive, no
    /// blind-origin mass, and the group's act equal to the centre's own vote.
    #[test]
    fn origin_share_is_the_member_share_at_the_identity() {
        let blind_masks = GroupAifConfig {
            masks: CoverageMasks::RoleBlind,
            ..GroupAifConfig::default()
        };
        let mut blind_members_seen = 0usize;
        for (name, config) in [
            ("default", GroupAifConfig::default()),
            ("novelty off", novelty_off()),
            ("role-blind masks", blind_masks),
        ] {
            let (_, c) = scripted_stream(config);
            assert_eq!(c.agreement.len(), 54, "{name}");
            for (i, s) in c.agreement.iter().enumerate() {
                assert_eq!(
                    s.blind_origin_share.to_bits(),
                    s.blind_weight_share.to_bits(),
                    "{name}, sample {i}: origin {} vs member {}",
                    s.blind_origin_share,
                    s.blind_weight_share
                );
                assert_eq!(
                    s.sensitive_rows, s.candidate_sensitive,
                    "{name}, sample {i}: {} sensitive rows vs {} sensitive internals",
                    s.sensitive_rows, s.candidate_sensitive
                );
                blind_members_seen += s.candidate_blind();
            }
        }
        assert!(blind_members_seen > 0, "no blind member was ever weighed");

        let (decisions, c) = scripted_stream(star(1.0, GroupAifConfig::default()));
        assert_eq!(c.agreement.len(), decisions.len());
        let mut centre_present = 0usize;
        for (i, (s, (act, _))) in c.agreement.iter().zip(&decisions).enumerate() {
            if s.candidate_sensitive == 0 {
                assert_eq!((s.sensitive_rows, s.centre_vote), (0, None), "sample {i}");
                assert_eq!(
                    s.blind_origin_share.to_bits(),
                    1.0f64.to_bits(),
                    "sample {i}"
                );
                continue;
            }
            centre_present += 1;
            assert_eq!(
                s.sensitive_rows, s.roster,
                "sample {i}: {} of {} rows reach the centre at λ = 1",
                s.sensitive_rows, s.roster
            );
            assert_eq!(
                s.blind_origin_share.to_bits(),
                0.0f64.to_bits(),
                "sample {i}: every row is the centre's output at λ = 1"
            );
            assert_eq!(
                s.centre_vote,
                Some(usize::from(*act)),
                "sample {i}: the group follows the centre at λ = 1"
            );
        }
        // 3 cycles × (6 + 4 + 2): the roster-1 task has two role-1 candidates.
        assert_eq!(centre_present, 36);
    }

    /// Over [`scripted_stream`] under `Off` and under `lambda = 0.5`: sample `k`
    /// carries `group_act` equal to decision `k`'s `act` and `leave` equal to the
    /// kind of call `k` (three joins then three leaves per task), and the stream
    /// carries at least one act and at least one decline.
    #[test]
    fn agreement_samples_carry_the_read_kind_and_the_group_act() {
        for (name, config) in [
            ("Off", GroupAifConfig::default()),
            ("lambda 0.5", star(0.5, GroupAifConfig::default())),
        ] {
            let (decisions, c) = scripted_stream(config);
            assert_eq!(decisions.len(), 54, "{name}");
            assert_eq!(c.agreement.len(), 54, "{name}: one sample per decision");
            // (join acts, join declines, leave acts, leave declines)
            let mut seen = [0usize; 4];
            for (k, (s, (act, _))) in c.agreement.iter().zip(&decisions).enumerate() {
                let leave_call = k % 6 >= 3;
                assert_eq!(
                    s.leave, leave_call,
                    "{name}, sample {k}: leave {} on a call with leave {leave_call}",
                    s.leave
                );
                assert_eq!(
                    s.group_act, *act,
                    "{name}, sample {k}: group_act {} vs Decision::act {act}",
                    s.group_act
                );
                seen[2 * usize::from(leave_call) + usize::from(!*act)] += 1;
            }
            assert!(
                seen[0] + seen[2] > 0 && seen[1] + seen[3] > 0,
                "{name}: (join acts, join declines, leave acts, leave declines) = {seen:?}"
            );
        }
    }

    /// Beliefs and Dirichlet counts of every model, for equality.
    type SnapshotKey = Vec<(
        ModelLabel,
        Vec<DVector<f64>>,
        Option<Vec<nalgebra::DMatrix<f64>>>,
    )>;

    fn snapshot_key(p: &GroupAifPolicy) -> SnapshotKey {
        p.model_snapshots()
            .into_iter()
            .map(|(label, s)| (label, s.beliefs, s.pa))
            .collect()
    }

    /// The two trait hooks, driven through `&dyn CoalitionDecisionPolicy`, leave
    /// the arm where the inherent `begin_task` / `observe_outcome` leave a twin;
    /// a refused demand is counted.
    #[test]
    fn trait_hooks_forward_to_the_inherent_lifecycle() {
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
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };
        let map = [(0usize, 0u8), (1, 1), (2, 2)];
        let tasks = [
            three_role_demand(),
            demand(&workflow(&[(Role::new(0), &[0]), (Role::new(2), &[2])])),
        ];

        let hooked = policy(GroupAifConfig::default(), &map);
        let inherent = policy(GroupAifConfig::default(), &map);
        let fresh = snapshot_key(&inherent);
        let mut hooked_out = Vec::new();
        let mut inherent_out = Vec::new();
        for t in 0..4usize {
            let d = &tasks[t % 2];
            let succ = [
                t % 2 == 0,
                true,
                t % 3 == 0,
                false,
                false,
                false,
                false,
                false,
            ];

            let steps: Vec<(u8, u8)> = d.distinct().map(|s| (s.bit, s.role.index())).collect();
            let dynp: &dyn CoalitionDecisionPolicy = &hooked;
            dynp.begin_task(&TaskStart {
                required: 0b111,
                steps: &steps,
            });
            hooked_out.push(dynp.should_join(&a0, &coalition, &ctx));
            hooked_out.push(dynp.should_leave(&a0, &full, &ctx));
            dynp.observe_outcome(0b111, &succ, &[]);

            inherent.begin_task(d).unwrap();
            inherent_out.push(inherent.should_join(&a0, &coalition, &ctx));
            inherent_out.push(inherent.should_leave(&a0, &full, &ctx));
            inherent.observe_outcome(&succ);
        }
        assert_eq!(hooked_out, inherent_out);
        let c = hooked.counters();
        assert_eq!(c, inherent.counters());
        assert_eq!(
            (
                c.reads,
                c.tasks_observed,
                c.begin_task_rejections,
                c.outcome_updates_unapplied
            ),
            (8, 4, 0, 0)
        );
        assert_eq!(c.roster_sizes, vec![3, 2, 3, 2]);
        assert!(c.s_learn_exact());
        assert_eq!(snapshot_key(&hooked), snapshot_key(&inherent));
        assert_ne!(snapshot_key(&hooked), fresh, "the hooked arm learned");

        // A role outside `0..n_roles` is refused, counted, and clears the task.
        let dynp: &dyn CoalitionDecisionPolicy = &hooked;
        dynp.begin_task(&TaskStart {
            required: 0b001,
            steps: &[(0, 7)],
        });
        let c = hooked.counters();
        assert_eq!(c.begin_task_rejections, 1);
        assert_eq!(c.roster_sizes.len(), 4, "a refused task opens nothing");
        assert!(!dynp.should_join(&a0, &coalition, &ctx).act);
        assert_eq!(hooked.counters().declines_no_demand, 1);

        // An outcome with no task in force advances no model and is counted.
        dynp.observe_outcome(0b001, &[true; 8], &[]);
        let c = hooked.counters();
        assert_eq!(
            (c.outcome_updates_unapplied, c.tasks_observed),
            (1, 4),
            "one unapplied outcome, no task observed"
        );
    }

    /// `GroupAifPolicy::new` refuses a `lambda` outside `[0, 1]` or non-finite,
    /// and any routing under seeded sampling or a non-CW vote.
    #[test]
    fn routing_construction_refusals() {
        let build = |config: GroupAifConfig| GroupAifPolicy::new(0, config, roles_map(&[(0, 0)]));
        for lambda in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.1, 1.0 + 1e-9] {
            assert!(
                matches!(
                    build(star(lambda, GroupAifConfig::default())),
                    Err(GroupAifError::InvalidRoutingWeight { .. })
                ),
                "λ = {lambda} must be refused"
            );
        }
        for lambda in [0.0, 0.25, 0.5, 1.0] {
            assert!(
                build(star(lambda, GroupAifConfig::default())).is_ok(),
                "λ = {lambda} must be accepted"
            );
        }
        assert!(matches!(
            build(star(
                0.5,
                GroupAifConfig {
                    read: DecisionRead::SeededSampling,
                    ..GroupAifConfig::default()
                }
            )),
            Err(GroupAifError::UnsupportedRouting)
        ));
        assert!(matches!(
            build(star(
                0.5,
                GroupAifConfig {
                    vote: GroupVote::Deterministic,
                    ..GroupAifConfig::default()
                }
            )),
            Err(GroupAifError::UnsupportedRouting)
        ));
        assert_eq!(GroupAifConfig::default().routing, VoteRouting::Off);
    }

    /// Object-safety: the arm is usable behind the decision trait object, like
    /// every other koalisi policy.
    #[test]
    fn object_safe() {
        let p = policy(GroupAifConfig::default(), &[(0, 0)]);
        let _: Box<dyn CoalitionDecisionPolicy> = Box::new(p);
    }
}
