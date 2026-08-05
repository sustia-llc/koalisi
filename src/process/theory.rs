//! The pinned rewrite theory (prereg Amendment 1 A1.1, widened by Amendment 3
//! A3.2).
//!
//! Three schemas, each closed over the `(bit, role)` index set. The theory is
//! their closure; [`rule_theory`] builds it, and every rule goes through
//! [`RewriteRule::new`] so nothing reaches a match site unvalidated.
//!
//! # Why fusion is a schema over *every* same-role pair
//!
//! Coverage is asked per *distinct* `(bit, role)` (see [`Demand`]), so
//! idempotence and spider absorption cannot change a staffing decision at all —
//! they move occurrence count only. **Fusion is the only staffing-relevant
//! schema**, and Amendment 1's *one designated pair per role* made it eligible on
//! 43 of 600 drawn tasks (7.2 %): a stream-level bar over a stream where ~93 % of
//! tasks are identical across arms was not reachable on merit. Amendment 3 A3.2
//! therefore instantiates fusion for every role and every ordered pair of
//! distinct bits, which is what makes the confirmatory leg powered rather than a
//! foregone negative.
//!
//! [`Demand`]: super::demand::Demand

use catgraph_applied::prop::Free;
use catgraph_applied::prop::colored::ColoredExpr;
use catgraph_applied::prop::presentation::rewrite::RewriteRule;
use catgraph_magnitude::CatgraphError;
use catgraph_syntax::frobenius::FrobeniusOr;

use super::signature::{Role, Step, Workflow, WorkflowGen, chain, spider_expr, step_expr};

/// The three schemas of the pinned theory, in the order [`rule_theory`] emits
/// them.
///
/// A trace binds rule **indices**, not rule identities, so this order is part of
/// the registered theory: replaying a trace against a differently-ordered slice
/// is replaying against a different theory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Schema {
    /// `s_{b,r} ; s_{b,r} ⇒ s_{b,r}` — doing the same step twice in a row is
    /// doing it once. Reduces occurrence count; leaves distinct demand
    /// unchanged.
    Idempotence,
    /// `s_{b,r} ; s_{b',r} ⇒ s_{b'',r}` for every role and every ordered pair of
    /// distinct bits, with `b'' = (b + b' + 4) mod bits` and the instance skipped
    /// when `b'' ∈ {b, b'}` (Amendment A3.2). The **only** schema that moves
    /// distinct demand, hence the only one that can move a staffing decision.
    Fusion,
    /// `δ_r ; (s_{b,r} ⊗ s_{b,r}) ; μ_r ⇒ s_{b,r}` — a same-step fan-out and
    /// rejoin collapses. The schema that justifies the spider vocabulary.
    SpiderAbsorption,
}

/// One rule of the theory, with the schema it came from.
///
/// Carried alongside the rule because [`RewriteStep`] records an index into the
/// rules slice: given the index and this slice, a report can say *which rule of
/// which schema* fired without re-deriving it. Since Amendment A3.2 the fusion
/// instances no longer follow from the role alone, so the consumed steps are
/// carried rather than recomputed — a printed theory that guessed them would be
/// printing a different theory than the one the trace indexes into.
///
/// [`RewriteStep`]: catgraph_applied::prop::presentation::rewrite::RewriteStep
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LabelledRule {
    /// Which schema this instance came from.
    pub schema: Schema,
    /// The two step occurrences the left-hand side consumes, in order: `[s, s]`
    /// for idempotence and absorption, `[s_b, s_b']` for fusion. Every schema
    /// consumes exactly two, which is why this is an array and not a `Vec`.
    pub sources: [Step; 2],
    /// The step the rule rewrites *to* — `s_{b,r}` for both idempotence and
    /// absorption, `s_{b'',r}` for fusion.
    pub target: Step,
}

/// Build the pinned theory over a `bits`-wide capability universe and `roles`
/// worker roles.
///
/// Returns the rules in a **fixed** order — all idempotence instances (role
/// major, then bit), then all fusion instances (role major, then `(b, b')`
/// ascending), then all absorption instances (role major, then bit).
/// [`rule_labels`] returns the matching per-index labels. At the registered
/// `(8, 3)` that is **174** instances: 24 idempotence + 126 fusion + 24
/// absorption.
///
/// # The fusion target is deliberately a *third* bit
///
/// `b'' = (b + b' + 4) mod bits ∉ {b, b'}` is the load-bearing choice, and it is
/// what keeps the confirmatory leg honest. Had the fusion target been one of the
/// two consumed bits, every application would strictly shrink distinct demand and
/// the rewriting arms would win **by construction** — the mirror image of the
/// inertness trap the schema set already guards against (an idempotence-only
/// theory changes occurrence count without changing distinct demand, so a
/// staffing contest could not see it at all). With a third bit the fused step
/// demands a capability neither consumed step required, so an application may
/// land on a `(bit, role)` that is scarce or absent in the drawn pool. The lever
/// is two-sided by design, and its losing side is what the battery's E-conc leg
/// measures.
///
/// Amendment A3.2 widened the schema from *one designated pair per role* to every
/// ordered pair of distinct bits; the two-sidedness guarantee is preserved by the
/// same rule it was before, now applied per instance — a pair whose target would
/// be one of the consumed bits is **not built**. At `bits = 8` that excludes
/// exactly the pairs involving bit 4 (`b'' = b` iff `b' ≡ −4 ≡ 4`, and `b'' = b'`
/// iff `b ≡ 4`), leaving 42 instances per role.
///
/// # Errors
///
/// [`CatgraphError::Presentation`] when
///
/// - `bits` or `roles` is zero;
/// - no ordered pair of distinct bits survives the two-sidedness skip, so the
///   theory would carry **no** fusion instance at all and could not move a
///   staffing decision (true for `bits ≤ 2`) — the inertness trap, refused rather
///   than merely unintended.
///
/// It also propagates anything [`RewriteRule::new`] reports. **That would be a
/// finding, not a runtime condition**: every instance of every schema is
/// parallel and mono-interfaced by construction (see
/// [`signature`](super::signature)), so a rejection means the construction and
/// the upstream contract have drifted apart.
pub fn rule_theory(bits: u8, roles: u8) -> Result<Vec<RewriteRule<WorkflowGen>>, CatgraphError> {
    check_shape(bits, roles)?;
    let pairs = fusion_pairs(bits);
    let mut theory = Vec::with_capacity(theory_len(bits, roles));

    for role in each_role(roles) {
        for bit in 0..bits {
            theory.push(idempotence(Step::new(bit, role))?);
        }
    }
    for role in each_role(roles) {
        for &(first, second, target) in &pairs {
            theory.push(fusion(
                Step::new(first, role),
                Step::new(second, role),
                Step::new(target, role),
            )?);
        }
    }
    for role in each_role(roles) {
        for bit in 0..bits {
            theory.push(spider_absorption(Step::new(bit, role))?);
        }
    }

    Ok(theory)
}

/// The per-index labels of [`rule_theory`]'s output, in the same order.
///
/// # Errors
///
/// The same shape conditions [`rule_theory`] checks.
pub fn rule_labels(bits: u8, roles: u8) -> Result<Vec<LabelledRule>, CatgraphError> {
    check_shape(bits, roles)?;
    let pairs = fusion_pairs(bits);
    let mut labels = Vec::with_capacity(theory_len(bits, roles));
    for role in each_role(roles) {
        for bit in 0..bits {
            let step = Step::new(bit, role);
            labels.push(LabelledRule {
                schema: Schema::Idempotence,
                sources: [step, step],
                target: step,
            });
        }
    }
    for role in each_role(roles) {
        for &(first, second, target) in &pairs {
            labels.push(LabelledRule {
                schema: Schema::Fusion,
                sources: [Step::new(first, role), Step::new(second, role)],
                target: Step::new(target, role),
            });
        }
    }
    for role in each_role(roles) {
        for bit in 0..bits {
            let step = Step::new(bit, role);
            labels.push(LabelledRule {
                schema: Schema::SpiderAbsorption,
                sources: [step, step],
                target: step,
            });
        }
    }
    Ok(labels)
}

/// The `(b, b', b'')` triples the fusion schema instantiates over a `bits`-wide
/// universe, in ascending `(b, b')` order and identical for every role.
///
/// Every **ordered** pair of distinct bits, with `b'' = (b + b' + 4) mod bits`,
/// minus the pairs where `b'' ∈ {b, b'}` — the two-sidedness skip documented on
/// [`rule_theory`]. Public because the pair set is also the exact structural
/// answer to "could fusion fire on this task at all", and a battery disclosure
/// that re-derived it by hand would be reporting a different schema than the one
/// the theory was built from.
///
/// Empty only for `bits ≤ 2`, which [`rule_theory`] refuses.
#[must_use]
pub fn fusion_pairs(bits: u8) -> Vec<(u8, u8, u8)> {
    let mut pairs = Vec::new();
    for first in 0..bits {
        for second in 0..bits {
            if second == first {
                continue;
            }
            let Some(target) = fusion_target(bits, first, second) else {
                continue;
            };
            if target == first || target == second {
                continue;
            }
            pairs.push((first, second, target));
        }
    }
    pairs
}

/// `b'' = (b + b' + 4) mod bits`, or `None` for an empty universe.
///
/// The sum runs in `u16` so `b + b' + 4` cannot wrap; the result is a remainder
/// modulo `bits`, so the narrowing back to `u8` is total rather than clamped.
fn fusion_target(bits: u8, first: u8, second: u8) -> Option<u8> {
    if bits == 0 {
        return None;
    }
    let width = u16::from(bits);
    u8::try_from((u16::from(first) + u16::from(second) + 4) % width).ok()
}

fn each_role(roles: u8) -> impl Iterator<Item = Role> {
    (0..roles).map(Role::new)
}

fn theory_len(bits: u8, roles: u8) -> usize {
    let width = usize::from(bits);
    let roles = usize::from(roles);
    2 * width * roles + roles * fusion_pairs(bits).len()
}

/// Reject any `(bits, roles)` the pinned schemas are not closed over.
///
/// One-sided *instances* are skipped by [`fusion_pairs`] rather than rejected
/// here, so what survives as a whole-theory condition is the inertness trap: a
/// universe in which **no** fusion instance exists leaves only the two
/// staffing-invisible schemas, and such a theory is unconstructible rather than
/// silently useless.
fn check_shape(bits: u8, roles: u8) -> Result<(), CatgraphError> {
    let reject = |message: String| CatgraphError::Presentation { message };
    if bits == 0 || roles == 0 {
        return Err(reject(format!(
            "rule theory: a universe needs at least one bit and one role (bits={bits}, \
             roles={roles})"
        )));
    }
    if fusion_pairs(bits).is_empty() {
        return Err(reject(format!(
            "rule theory: no ordered pair (b, b') of distinct bits in a {bits}-bit universe has \
             (b + b' + 4) mod {bits} outside {{b, b'}}, so the fusion schema would be empty — the \
             remaining schemas move occurrence count only and could not change a staffing decision"
        )));
    }
    Ok(())
}

/// Pin a single-wire term of `role`'s color.
fn pin(
    role: Role,
    expr: catgraph_applied::prop::PropExpr<WorkflowGen>,
) -> Result<Workflow, CatgraphError> {
    ColoredExpr::new(vec![role], expr)
}

/// `s_{b,r} ; s_{b,r} ⇒ s_{b,r}`.
fn idempotence(step: Step) -> Result<RewriteRule<WorkflowGen>, CatgraphError> {
    let lhs = pin(step.role, chain(vec![step_expr(step), step_expr(step)])?)?;
    let rhs = pin(step.role, step_expr(step))?;
    RewriteRule::new(lhs, rhs)
}

/// `s_{b1,r} ; s_{b2,r} ⇒ s_{b3,r}` — all three at the same role.
fn fusion(
    first: Step,
    second: Step,
    target: Step,
) -> Result<RewriteRule<WorkflowGen>, CatgraphError> {
    let role = first.role;
    let lhs = pin(role, chain(vec![step_expr(first), step_expr(second)])?)?;
    let rhs = pin(role, step_expr(target))?;
    RewriteRule::new(lhs, rhs)
}

/// `δ_r ; (s_{b,r} ⊗ s_{b,r}) ; μ_r ⇒ s_{b,r}`.
fn spider_absorption(step: Step) -> Result<RewriteRule<WorkflowGen>, CatgraphError> {
    let role = step.role;
    let lhs = pin(
        role,
        chain(vec![
            spider_expr(FrobeniusOr::Delta(role)),
            Free::tensor(step_expr(step), step_expr(step)),
            spider_expr(FrobeniusOr::Mu(role)),
        ])?,
    )?;
    let rhs = pin(role, step_expr(step))?;
    RewriteRule::new(lhs, rhs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The registered world: an 8-bit universe and R = 3 roles.
    const BITS: u8 = 8;
    const ROLES: u8 = 3;

    /// The registered theory size after Amendment A3.2's widening: 24
    /// idempotence + 126 fusion + 24 absorption.
    const REGISTERED_THEORY_LEN: usize = 174;

    #[test]
    fn every_registered_instance_constructs() {
        let rules = rule_theory(BITS, ROLES).expect("the pinned theory must construct");
        let fusion_count = fusion_pairs(BITS).len() * usize::from(ROLES);
        assert_eq!(fusion_count, 126, "42 surviving ordered pairs × 3 roles");
        assert_eq!(
            rules.len(),
            2 * 8 * 3 + fusion_count,
            "8×3 idempotence + 126 fusion + 8×3 absorption"
        );
        assert_eq!(
            rules.len(),
            REGISTERED_THEORY_LEN,
            "Amendment A3.2 pins the widened theory at 174 instances"
        );

        let labels = rule_labels(BITS, ROLES).unwrap();
        assert_eq!(labels.len(), rules.len());
        assert_eq!(labels[0].schema, Schema::Idempotence);
        assert_eq!(labels[24].schema, Schema::Fusion);
        assert_eq!(labels[24 + fusion_count].schema, Schema::SpiderAbsorption);
        assert_eq!(
            labels.iter().filter(|l| l.schema == Schema::Fusion).count(),
            fusion_count
        );

        // Every schema instance over a wider universe too — the closure is not
        // special-cased to (8, 3), and since A3.2 the fusion bits no longer
        // depend on the role, so `roles > bits` is a legal palette.
        for roles in 1..=4u8 {
            for bits in [3u8, 5, 8, 16] {
                rule_theory(bits, roles)
                    .unwrap_or_else(|e| panic!("theory ({bits}, {roles}) rejected: {e}"));
            }
        }
    }

    #[test]
    fn fusion_is_two_sided_at_every_instance() {
        // Amendment A3.2: b'' = (b + b' + 4) mod bits, never one of the consumed
        // bits, for every ordered pair of distinct bits.
        let pairs = fusion_pairs(BITS);
        assert_eq!(
            pairs.len(),
            42,
            "8×7 ordered pairs minus the 14 touching bit 4"
        );
        for &(first, second, target) in &pairs {
            assert_ne!(first, second);
            assert_ne!(target, first, "fusion target must not be a consumed bit");
            assert_ne!(target, second, "fusion target must not be a consumed bit");
            assert_eq!(target, (first + second + 4) % BITS, "the pinned formula");
            assert_ne!(first, 4, "at bits = 8 every surviving pair avoids bit 4");
            assert_ne!(second, 4);
        }
        // Two consequences of the widening worth pinning, because both differ
        // from Amendment 1 and a reader of A1.1 would expect otherwise.
        //
        // 1. A1.1's designated ORDERED PAIRS for roles 0 and 1 are still in the
        //    theory, but their TARGETS moved: A1.1 used b'' = (2r + 4) mod bits
        //    (a function of the first bit alone), A3.2 uses (b + b' + 4) mod bits.
        assert!(
            pairs.contains(&(0, 1, 5)),
            "A1.1's (0,1) now targets 5, not 4"
        );
        assert!(
            pairs.contains(&(2, 3, 1)),
            "A1.1's (2,3) now targets 1, not 6"
        );
        // 2. A1.1's designated pair for role 2, (4, 5), is now SKIPPED: its
        //    target (4 + 5 + 4) mod 8 = 5 is a consumed bit, so the instance
        //    would be one-sided. Excluded by the same guarantee, not by a
        //    special case.
        assert!(
            !pairs
                .iter()
                .any(|&(first, second, _)| (first, second) == (4, 5))
        );
    }

    #[test]
    fn shape_conditions_are_enforced() {
        assert!(rule_theory(0, 3).is_err());
        assert!(rule_theory(8, 0).is_err());
        // No two-sided pair exists below three bits, so the theory would carry
        // no staffing-relevant schema at all.
        assert!(fusion_pairs(1).is_empty());
        assert!(fusion_pairs(2).is_empty());
        assert!(rule_theory(1, 3).is_err());
        assert!(rule_theory(2, 3).is_err());
        assert!(!fusion_pairs(3).is_empty());
        assert!(rule_theory(3, 3).is_ok());
    }
}
