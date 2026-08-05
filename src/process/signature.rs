//! The workflow signature: roles as colors, steps as generators.
//!
//! A coalition task is a **process**, not a flat capability mask: a string
//! diagram whose wires are typed by worker [`Role`] and whose boxes are typed
//! [`Step`]s. Sequencing is
//! [`Free::compose`](catgraph_applied::prop::Free::compose), parallel work is
//! [`Free::tensor`](catgraph_applied::prop::Free::tensor), and per-role resource
//! wiring is the [`FrobeniusOr`] spider family — a `Role(0)` wire cannot
//! silently merge with a `Role(1)` one.
//!
//! ## Why steps are role-PRESERVING
//!
//! `s_{b,r} : r → r` (prereg §2): a step consumes and produces a wire of its own
//! role color. Two consequences the rest of the module leans on:
//!
//! 1. every same-role composite is **parallel** to every other same-role
//!    composite (equal source *and* target words), which is exactly
//!    [`RewriteRule::new`](catgraph_applied::prop::presentation::rewrite::RewriteRule::new)'s
//!    first condition — so the [rule theory](super::theory) is well-formed by
//!    construction rather than by luck;
//! 2. a role's steps chain freely, so the world's shape draw can build a
//!    per-role sequential chain of arbitrary length.
//!
//! ## Colors are pinned by `ColoredExpr::new`
//!
//! [`Free::identity`](catgraph_applied::prop::Free::identity) and
//! [`Free::braid`](catgraph_applied::prop::Free::braid) are color-polymorphic by
//! design, so an expression alone does not determine its palette.
//! [`ColoredExpr::new`] pinning the source word is the only way colors get
//! fixed — that is why every constructor here hands back a bare
//! [`PropExpr`] and the caller pins it.
//!
//! [`PropExpr`]: catgraph_applied::prop::PropExpr

use std::borrow::Cow;

use catgraph_applied::prop::colored::ColoredExpr;
use catgraph_applied::prop::{Free, PropExpr, PropSignature};
use catgraph_magnitude::CatgraphError;
use catgraph_syntax::frobenius::FrobeniusOr;
use catgraph_syntax::text::{ColorSyntax, GeneratorSyntax};

/// A worker role — the color alphabet `Λ` of the workflow prop.
///
/// A transparent index rather than a fieldless enum, so the role count `R` is a
/// *world* parameter (the `EQ5a` battery draws `R = 3`) instead of a type-level
/// constant. The derived bound stack is everything the colored surface asks for:
/// `Clone + Copy + Eq + Hash + Debug` for
/// [`PropSignature::Color`], `+ Ord` for the spiders, `+ Copy` for the cospan
/// functor.
///
/// # Caller-side mapping
///
/// A coalition indexes its members however it likes; `Role` is *not* that index
/// and koalisi ships no converter. The role table stays caller-side by design —
/// the process signal rides **beside** a coalition's value signal, not through
/// it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Role(u8);

impl Role {
    /// The role with the given index.
    #[must_use]
    pub const fn new(index: u8) -> Self {
        Self(index)
    }

    /// This role's index.
    #[must_use]
    pub const fn index(self) -> u8 {
        self.0
    }
}

/// Roles print as `r0`, `r1`, … — one lexical atom over the
/// [`GeneratorSyntax`] clause-2 alphabet, so `mu@r1` lexes as one spider token.
///
/// [`implicit`](ColorSyntax::implicit) is `None`: the palette has no default
/// sort, so a bare `mu` is a **parse error** rather than a silently mistyped
/// spider.
impl ColorSyntax for Role {
    fn print_color(&self) -> Option<String> {
        Some(format!("r{}", self.0))
    }

    fn parse_color(token: &str) -> Option<Self> {
        let digits = token.strip_prefix('r')?;
        let index: u8 = digits.parse().ok()?;
        // Reject non-canonical spellings (`r007`) so parse ∘ print is the only
        // accepted round trip.
        (index.to_string() == digits).then_some(Self(index))
    }

    fn implicit() -> Option<Self> {
        None
    }
}

/// One step of a workflow: "capability `bit`, performed by a worker of role
/// `role`".
///
/// The generator `s_{b,r} : r → r` of prereg §2. A tagged required bit of the
/// v2t draw becomes exactly one of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Step {
    /// The capability bit this step needs (an index into the capability
    /// universe, *not* a mask).
    pub bit: u8,
    /// The worker role that must perform it.
    pub role: Role,
}

impl Step {
    /// A step demanding `bit` from a worker of `role`.
    #[must_use]
    pub const fn new(bit: u8, role: Role) -> Self {
        Self { bit, role }
    }

    /// This step's capability bit as a single-bit mask, or `None` when `bit` is
    /// outside a `u32` capability mask's range.
    ///
    /// Coverage is asked of a coalition member's `u32` capability mask, so the
    /// conversion is spelled once here rather than at every call site.
    #[must_use]
    pub fn capability_mask(self) -> Option<u32> {
        (u32::from(self.bit) < u32::BITS).then(|| 1u32 << self.bit)
    }
}

/// Role-preserving: `s_{b,r} : r → r`. See the module docs for why that shape is
/// load-bearing.
impl PropSignature for Step {
    type Color = Role;

    fn source_word(&self) -> Cow<'_, [Role]> {
        Cow::Borrowed(std::slice::from_ref(&self.role))
    }

    fn target_word(&self) -> Cow<'_, [Role]> {
        Cow::Borrowed(std::slice::from_ref(&self.role))
    }
}

/// Steps print as `s3_r1` — "bit 3, role 1". No `@`, which is reserved for the
/// spider color annotation, and no other clause-2 character.
impl GeneratorSyntax for Step {
    fn print_token(&self) -> String {
        format!("s{}_r{}", self.bit, self.role.index())
    }

    fn parse_token(token: &str) -> Option<Self> {
        let (bit_digits, role_digits) = token.strip_prefix('s')?.split_once("_r")?;
        let bit: u8 = bit_digits.parse().ok()?;
        let role: u8 = role_digits.parse().ok()?;
        (bit.to_string() == bit_digits && role.to_string() == role_digits)
            .then_some(Self::new(bit, Role::new(role)))
    }
}

/// The working signature: workflow steps plus one spider family per role.
///
/// Spiders and user steps are the two arms of **one** generator type, which is
/// what lets `optimize` / `cost_of` / `canonical_key` accept the whole workflow
/// vocabulary directly. No adapter is owed.
pub type WorkflowGen = FrobeniusOr<Step>;

/// A workflow: a color-pinned string diagram over [`WorkflowGen`].
pub type Workflow = ColoredExpr<WorkflowGen>;

/// One step, as a one-box term.
#[must_use]
pub fn step_expr(step: Step) -> PropExpr<WorkflowGen> {
    Free::generator(FrobeniusOr::User(step))
}

/// One spider, as a one-box term.
///
/// `μ_c : [c,c] → [c]`, `η_c : [] → [c]`, `δ_c : [c] → [c,c]`,
/// `ε_c : [c] → []` — see [`FrobeniusOr`].
#[must_use]
pub fn spider_expr(spider: WorkflowGen) -> PropExpr<WorkflowGen> {
    Free::generator(spider)
}

/// Compose `parts` left to right.
///
/// # Arity here, colors at the pin
///
/// [`Free::compose`](catgraph_applied::prop::Free::compose) checks interface
/// **widths**, not colors — the free prop is color-polymorphic until a source
/// word pins it. A same-width, wrong-color junction (`s_{b,r0} ; s_{b,r1}`)
/// therefore composes cleanly here and is rejected by [`ColoredExpr::new`]'s
/// `check` pass. Build with `chain`, then pin, and read color failures off the
/// pin.
///
/// # Errors
///
/// [`CatgraphError::Presentation`] when `parts` is empty (a pipeline needs at
/// least one stage), and whatever `Free::compose` reports for the first width
/// that does not meet.
pub fn chain(parts: Vec<PropExpr<WorkflowGen>>) -> Result<PropExpr<WorkflowGen>, CatgraphError> {
    let mut parts = parts.into_iter();
    let mut acc = parts.next().ok_or_else(|| CatgraphError::Presentation {
        message: "workflow chain: a pipeline needs at least one stage".to_string(),
    })?;
    for next in parts {
        acc = Free::compose(acc, next)?;
    }
    Ok(acc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_and_step_text_round_trip() {
        for index in [0u8, 1, 7, 255] {
            let role = Role::new(index);
            let token = role.print_color().unwrap();
            assert_eq!(Role::parse_color(&token), Some(role));
            // Clause 3: one lexical atom, no reserved characters.
            assert!(!token.is_empty());
            assert!(!token.contains([';', '*', '⊗', '=', ',', ':', '@', '(', ')', ' ']));
            assert!(token != "id" && token != "braid" && token != "->");
        }
        assert_eq!(
            Role::implicit(),
            None,
            "no implicit sort: bare `mu` is an error"
        );
        assert_eq!(
            Role::parse_color("r007"),
            None,
            "non-canonical spelling rejected"
        );
        assert_eq!(Role::parse_color("x0"), None);

        for step in [Step::new(0, Role::new(0)), Step::new(7, Role::new(2))] {
            let token = step.print_token();
            assert_eq!(Step::parse_token(&token), Some(step));
            assert!(!token.contains('@'));
        }
        assert_eq!(Step::parse_token("s3r1"), None);
        assert_eq!(Step::parse_token("s03_r1"), None);
    }

    #[test]
    fn steps_are_role_preserving_and_chain_freely() {
        let role = Role::new(1);
        let a = Step::new(2, role);
        let b = Step::new(5, role);
        assert_eq!(a.source_word().as_ref(), &[role]);
        assert_eq!(a.target_word().as_ref(), &[role]);

        // Same role ⇒ composable, and the composite is parallel to a lone step.
        let composite =
            ColoredExpr::new(vec![role], chain(vec![step_expr(a), step_expr(b)]).unwrap()).unwrap();
        let single = ColoredExpr::new(vec![role], step_expr(a)).unwrap();
        assert_eq!(composite.source_word(), single.source_word());
        assert_eq!(composite.target_word(), single.target_word());

        // Different roles ⇒ same width, so `chain` accepts it and the PIN
        // rejects it. Colors live at `ColoredExpr::new`, not at `Free::compose`.
        let other = Step::new(2, Role::new(0));
        let mismatched = chain(vec![step_expr(a), step_expr(other)])
            .expect("compose checks widths, and both steps are 1 -> 1");
        assert!(
            ColoredExpr::new(vec![role], mismatched).is_err(),
            "a wrong-color junction must be rejected when the palette is pinned"
        );
        assert!(chain(vec![]).is_err());
    }

    #[test]
    fn capability_mask_is_a_single_bit_or_none() {
        assert_eq!(Step::new(0, Role::new(0)).capability_mask(), Some(1));
        assert_eq!(Step::new(7, Role::new(0)).capability_mask(), Some(1 << 7));
        assert_eq!(Step::new(31, Role::new(0)).capability_mask(), Some(1 << 31));
        assert_eq!(Step::new(32, Role::new(0)).capability_mask(), None);
    }
}
