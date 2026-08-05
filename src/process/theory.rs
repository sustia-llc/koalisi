//! The pinned rewrite theory (prereg Amendment 1, A1.1).
//!
//! Three schemas, each closed over the `(bit, role)` index set. The theory is
//! their closure; [`rule_theory`] builds it, and every rule goes through
//! [`RewriteRule::new`] so nothing reaches a match site unvalidated.

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
    /// `s_{b1,r} ; s_{b2,r} ⇒ s_{b3,r}` — one designated rule per role, with
    /// `b1 = 2r`, `b2 = 2r + 1`, `b3 = (2r + 4) mod bits`. Moves distinct
    /// demand.
    Fusion,
    /// `δ_r ; (s_{b,r} ⊗ s_{b,r}) ; μ_r ⇒ s_{b,r}` — a same-step fan-out and
    /// rejoin collapses. The schema that justifies the spider vocabulary.
    SpiderAbsorption,
}

/// One rule of the theory, with the schema it came from.
///
/// Carried alongside the rule because [`RewriteStep`] records an index into the
/// rules slice: given the index and this slice, a report can say *which schema*
/// fired without re-deriving it.
///
/// [`RewriteStep`]: catgraph_applied::prop::presentation::rewrite::RewriteStep
#[derive(Clone, Debug)]
pub struct LabelledRule {
    /// Which schema this instance came from.
    pub schema: Schema,
    /// The step the rule rewrites *to* — `s_{b,r}` for both idempotence and
    /// absorption, `s_{b3,r}` for fusion.
    pub target: Step,
}

/// Build the pinned theory over a `bits`-wide capability universe and `roles`
/// worker roles.
///
/// Returns the rules in a **fixed** order — all idempotence instances (role
/// major, then bit), then all fusion instances (one per role), then all
/// absorption instances (role major, then bit) — so `bits × roles + roles +
/// bits × roles` rules in total. At the registered `(8, 3)` that is 51.
/// [`rule_labels`] returns the matching per-index labels.
///
/// # The fusion target is deliberately a *third* bit
///
/// `b3(r) = (2r + 4) mod bits ∉ {b1(r), b2(r)}` is the load-bearing choice, and
/// it is what keeps the confirmatory leg honest. Had the fusion target been one
/// of the two consumed bits, every application would strictly shrink distinct
/// demand and the rewriting arms would win **by construction** — the mirror
/// image of the inertness trap the schema set already guards against (an
/// idempotence-only theory changes occurrence count without changing distinct
/// demand, so a staffing contest could not see it at all). With a third bit the
/// fused step demands a capability neither consumed step required, so an
/// application may land on a `(bit, role)` that is scarce or absent in the drawn
/// pool. The lever is two-sided by design.
///
/// At the registered `bits = 8` this is exactly Amendment 1's `(2r + 4) mod 8`;
/// the modulus follows `bits` so the schema stays closed over whatever universe
/// it is handed, and [`rule_theory`] refuses any `(bits, roles)` where the
/// two-sidedness would be lost.
///
/// # Errors
///
/// [`CatgraphError::Presentation`] when
///
/// - `bits` or `roles` is zero;
/// - `2·roles > bits`, so some role's `b1`/`b2` fall outside the universe;
/// - `b3(r) ∈ {b1(r), b2(r)}` for some role, which would make fusion
///   one-sided (see above);
/// - `bits` exceeds `u8::MAX`-representable step indices.
///
/// It also propagates anything [`RewriteRule::new`] reports. **That would be a
/// finding, not a runtime condition**: every instance of every schema is
/// parallel and mono-interfaced by construction (see
/// [`signature`](super::signature)), so a rejection means the construction and
/// the upstream contract have drifted apart.
pub fn rule_theory(bits: u8, roles: u8) -> Result<Vec<RewriteRule<WorkflowGen>>, CatgraphError> {
    check_shape(bits, roles)?;
    let mut theory = Vec::with_capacity(theory_len(bits, roles));

    for role in each_role(roles) {
        for bit in 0..bits {
            theory.push(idempotence(Step::new(bit, role))?);
        }
    }
    for role in each_role(roles) {
        let (b1, b2, b3) = fusion_bits_checked(bits, role)?;
        theory.push(fusion(
            Step::new(b1, role),
            Step::new(b2, role),
            Step::new(b3, role),
        )?);
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
    let mut labels = Vec::with_capacity(theory_len(bits, roles));
    for role in each_role(roles) {
        for bit in 0..bits {
            labels.push(LabelledRule {
                schema: Schema::Idempotence,
                target: Step::new(bit, role),
            });
        }
    }
    for role in each_role(roles) {
        let (_, _, b3) = fusion_bits_checked(bits, role)?;
        labels.push(LabelledRule {
            schema: Schema::Fusion,
            target: Step::new(b3, role),
        });
    }
    for role in each_role(roles) {
        for bit in 0..bits {
            labels.push(LabelledRule {
                schema: Schema::SpiderAbsorption,
                target: Step::new(bit, role),
            });
        }
    }
    Ok(labels)
}

/// The `(b1, b2, b3)` of the fusion schema at `role`: `b1 = 2r`, `b2 = 2r + 1`,
/// `b3 = (2r + 4) mod bits`.
///
/// `None` when `b1` or `b2` falls outside the `bits`-wide universe — i.e.
/// exactly when the `2·roles ≤ bits` condition [`check_shape`] enforces does not
/// hold. Arithmetic runs in `u16` so `2r + 4` cannot wrap; `b3` is a remainder
/// mod `bits`, so it is always in range when `bits > 0`. The conversion back to
/// `u8` is therefore total on the accepted domain rather than clamped.
fn fusion_bits(bits: u8, role: Role) -> Option<(u8, u8, u8)> {
    if bits == 0 {
        return None;
    }
    let r = u16::from(role.index());
    let width = u16::from(bits);
    let b1 = 2 * r;
    let b2 = b1 + 1;
    let b3 = (b1 + 4) % width;
    if b1 >= width || b2 >= width {
        return None;
    }
    Some((
        u8::try_from(b1).ok()?,
        u8::try_from(b2).ok()?,
        u8::try_from(b3).ok()?,
    ))
}

/// [`fusion_bits`] as a hard error, for the call sites that run after
/// [`check_shape`] has already accepted `(bits, roles)`.
fn fusion_bits_checked(bits: u8, role: Role) -> Result<(u8, u8, u8), CatgraphError> {
    fusion_bits(bits, role).ok_or_else(|| CatgraphError::Presentation {
        message: format!(
            "rule theory: fusion at role {} falls outside a {bits}-bit universe",
            role.index()
        ),
    })
}

fn each_role(roles: u8) -> impl Iterator<Item = Role> {
    (0..roles).map(Role::new)
}

fn theory_len(bits: u8, roles: u8) -> usize {
    let bits = usize::from(bits);
    let roles = usize::from(roles);
    2 * bits * roles + roles
}

/// Reject any `(bits, roles)` the pinned schemas are not closed over.
fn check_shape(bits: u8, roles: u8) -> Result<(), CatgraphError> {
    let reject = |message: String| CatgraphError::Presentation { message };
    if bits == 0 || roles == 0 {
        return Err(reject(format!(
            "rule theory: a universe needs at least one bit and one role (bits={bits}, \
             roles={roles})"
        )));
    }
    if 2 * u32::from(roles) > u32::from(bits) {
        return Err(reject(format!(
            "rule theory: the fusion schema consumes bits 2r and 2r+1, so it needs \
             2*roles <= bits (bits={bits}, roles={roles})"
        )));
    }
    for role in each_role(roles) {
        let (b1, b2, b3) = fusion_bits_checked(bits, role)?;
        if b3 == b1 || b3 == b2 {
            return Err(reject(format!(
                "rule theory: fusion at role {} would target a consumed bit (b1={b1}, b2={b2}, \
                 b3={b3}), which makes the lever one-sided",
                role.index()
            )));
        }
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

    #[test]
    fn every_registered_instance_constructs() {
        let rules = rule_theory(BITS, ROLES).expect("the pinned theory must construct");
        assert_eq!(
            rules.len(),
            2 * 8 * 3 + 3,
            "8×3 idempotence + 3 fusion + 8×3 absorption"
        );
        assert_eq!(rules.len(), 51);

        let labels = rule_labels(BITS, ROLES).unwrap();
        assert_eq!(labels.len(), rules.len());
        assert_eq!(labels[0].schema, Schema::Idempotence);
        assert_eq!(labels[24].schema, Schema::Fusion);
        assert_eq!(labels[27].schema, Schema::SpiderAbsorption);

        // Every schema instance over a wider universe too — the closure is not
        // special-cased to (8, 3).
        for roles in 1..=4u8 {
            let bits = 2 * roles + 3;
            rule_theory(bits, roles)
                .unwrap_or_else(|e| panic!("theory ({bits}, {roles}) rejected: {e}"));
        }
    }

    #[test]
    fn fusion_targets_a_third_bit_at_the_registered_shape() {
        // Amendment 1 A1.1: b1 = 2r, b2 = 2r+1, b3 = (2r+4) mod 8.
        let expected = [(0u8, 1u8, 4u8), (2, 3, 6), (4, 5, 0)];
        for (index, want) in expected.iter().enumerate() {
            let role = Role::new(u8::try_from(index).unwrap());
            let got = fusion_bits(BITS, role).expect("in range at the registered shape");
            assert_eq!(got, *want, "fusion bits at role {index}");
            assert_ne!(got.2, got.0);
            assert_ne!(got.2, got.1);
        }
    }

    #[test]
    fn shape_conditions_are_enforced() {
        assert!(rule_theory(0, 3).is_err());
        assert!(rule_theory(8, 0).is_err());
        // 2*roles > bits: role 3 would consume bits 6 and 7 of a 6-bit universe.
        assert!(rule_theory(6, 4).is_err());
        // b3 == b1: at bits = 4, role 0 targets (0+4) mod 4 = 0 = b1.
        assert!(rule_theory(4, 2).is_err());
    }
}
