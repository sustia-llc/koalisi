//! What a workflow asks of a coalition.

use std::collections::BTreeMap;

use catgraph_applied::prop::presentation::content::content_of_colored;
use catgraph_syntax::frobenius::FrobeniusOr;

use super::signature::{Step, Workflow};

/// The `(bit, role)` demand a workflow places on a coalition.
///
/// # Multiplicity prices the process; it does not add coverage demand
///
/// This asymmetry is **load-bearing** (prereg §2) and is the whole reason the
/// rewrite theory can matter:
///
/// - **Coverage** is asked per *distinct* `(bit, role)`: a step occurrence is
///   covered iff some coalition member of role `r` holds bit `b`. Two
///   occurrences of the same step are covered by the same one member, so
///   repeating a step adds **nothing** to what the coalition must be able to do.
/// - **Cost** is asked per *occurrence*:
///   [`cost_of`](catgraph_applied::prop::presentation::rewrite::cost_of) sums a
///   per-generator weight over hyperedges, so repeating a step is strictly more
///   expensive.
///
/// So [`distinct_len`](Demand::distinct_len) is the staffing question and
/// [`total`](Demand::total) is the pricing question, and a rewrite may move one
/// without moving the other. An **idempotence** rule (`s ; s ⇒ s`) reduces cost
/// and leaves distinct demand untouched; a **fusion** rule (`s_b1 ; s_b2 ⇒
/// s_b3`) moves both. A theory of the first kind alone would be structurally
/// inert for a staffing contest — which is why the pinned
/// [theory](super::theory::rule_theory) contains a fusion schema, widened by
/// prereg Amendment A3.2 to every same-role ordered pair of distinct bits so that
/// it is eligible on more than a sliver of drawn traffic.
///
/// # Spiders contribute no demand
///
/// `μ`/`η`/`δ`/`ε` occurrences are resource *wiring*, not work: they are
/// counted by `cost_of` (they are hyperedges like any other) but they name no
/// `(bit, role)`, so they never appear here. Only
/// [`FrobeniusOr::User`] occurrences do.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Demand {
    /// Every `User` occurrence, in the content's hyperedge order.
    occurrences: Vec<Step>,
    /// Multiplicity per distinct step. `BTreeMap` so every walk over the
    /// distinct set is deterministic — no iteration-order dependence anywhere.
    counts: BTreeMap<Step, usize>,
}

impl Demand {
    /// Every step occurrence, as a multiset flattened into a `Vec`.
    ///
    /// # Order is a property of the writing, not of the process
    ///
    /// The slice follows the content's hyperedge order, which is an artifact of
    /// the expression the content was read off. The *multiset* is the
    /// content-invariant part — use [`counts`](Demand::counts) whenever the
    /// answer must not depend on how the process was written.
    #[must_use]
    pub fn occurrences(&self) -> &[Step] {
        &self.occurrences
    }

    /// Multiplicity per distinct step, in `Step` order.
    #[must_use]
    pub fn counts(&self) -> &BTreeMap<Step, usize> {
        &self.counts
    }

    /// The distinct `(bit, role)` pairs, ascending — the **staffing** question.
    #[must_use]
    pub fn distinct(&self) -> impl ExactSizeIterator<Item = Step> + '_ {
        self.counts.keys().copied()
    }

    /// How many distinct `(bit, role)` pairs the workflow needs staffed.
    #[must_use]
    pub fn distinct_len(&self) -> usize {
        self.counts.len()
    }

    /// How many step occurrences there are in total — the **pricing** question
    /// at uniform weights (spiders excluded; they are priced but not demanded).
    #[must_use]
    pub fn total(&self) -> usize {
        self.occurrences.len()
    }

    /// How often `step` occurs.
    #[must_use]
    pub fn multiplicity(&self, step: Step) -> usize {
        self.counts.get(&step).copied().unwrap_or(0)
    }

    /// Whether the workflow demands nothing at all (a pure-wiring diagram).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.occurrences.is_empty()
    }
}

/// Read a workflow's `(bit, role)` demand off its content.
///
/// Content, not the expression tree: two writings of the same process have the
/// same demand, which is what makes the multiplicity-vs-coverage asymmetry
/// documented on [`Demand`] a statement about the *process*.
#[must_use]
pub fn demand(workflow: &Workflow) -> Demand {
    let content = content_of_colored(workflow);
    let mut occurrences = Vec::with_capacity(content.edges().len());
    let mut counts: BTreeMap<Step, usize> = BTreeMap::new();
    for edge in content.edges() {
        if let FrobeniusOr::User(step) = &edge.label {
            occurrences.push(*step);
            *counts.entry(*step).or_insert(0) += 1;
        }
    }
    Demand {
        occurrences,
        counts,
    }
}

#[cfg(test)]
mod tests {
    use catgraph_applied::prop::Free;
    use catgraph_applied::prop::colored::ColoredExpr;

    use super::super::signature::{Role, chain, spider_expr, step_expr};
    use super::*;

    #[test]
    fn multiplicity_prices_but_does_not_add_coverage_demand() {
        let role = Role::new(0);
        let a = Step::new(1, role);
        let b = Step::new(4, role);

        let once =
            ColoredExpr::new(vec![role], chain(vec![step_expr(a), step_expr(b)]).unwrap()).unwrap();
        let repeated = ColoredExpr::new(
            vec![role],
            chain(vec![step_expr(a), step_expr(a), step_expr(b), step_expr(a)]).unwrap(),
        )
        .unwrap();

        let d_once = demand(&once);
        let d_repeated = demand(&repeated);

        // The asymmetry, stated as two assertions.
        assert_eq!(d_once.distinct_len(), 2);
        assert_eq!(
            d_repeated.distinct_len(),
            2,
            "repetition adds no coverage demand"
        );
        assert_eq!(d_once.total(), 2);
        assert_eq!(d_repeated.total(), 4, "repetition does add occurrences");

        assert_eq!(d_repeated.multiplicity(a), 3);
        assert_eq!(d_repeated.multiplicity(b), 1);
        assert_eq!(d_repeated.multiplicity(Step::new(7, role)), 0);
        assert_eq!(
            d_repeated.distinct().collect::<Vec<_>>(),
            vec![a, b],
            "distinct walk is Step-ordered, not insertion-ordered"
        );
        assert_eq!(d_once.counts(), &BTreeMap::from([(a, 1), (b, 1)]));
    }

    #[test]
    fn spiders_contribute_no_demand() {
        let role = Role::new(2);
        let s = Step::new(3, role);

        // δ_r ; (s ⊗ s) ; μ_r — four hyperedges, two of them spiders.
        let fanned = ColoredExpr::new(
            vec![role],
            chain(vec![
                spider_expr(FrobeniusOr::Delta(role)),
                Free::tensor(step_expr(s), step_expr(s)),
                spider_expr(FrobeniusOr::Mu(role)),
            ])
            .unwrap(),
        )
        .unwrap();

        let d = demand(&fanned);
        assert_eq!(d.distinct_len(), 1);
        assert_eq!(d.total(), 2, "the two spiders are not demand");
        assert_eq!(d.occurrences(), &[s, s]);

        // A pure-wiring diagram demands nothing while still costing something.
        let wiring = ColoredExpr::new(
            vec![role],
            chain(vec![
                spider_expr(FrobeniusOr::Delta(role)),
                spider_expr(FrobeniusOr::Mu(role)),
            ])
            .unwrap(),
        )
        .unwrap();
        assert!(demand(&wiring).is_empty());
    }

    #[test]
    fn demand_is_a_property_of_the_process_not_the_writing() {
        let role = Role::new(1);
        let a = Step::new(0, role);
        let b = Step::new(1, role);

        let tensored =
            ColoredExpr::new(vec![role, role], Free::tensor(step_expr(a), step_expr(b))).unwrap();
        // (a ⊗ id) ; (id ⊗ b) — a different tree, the same process.
        let interchanged = ColoredExpr::new(
            vec![role, role],
            Free::compose(
                Free::tensor(step_expr(a), Free::identity(1)),
                Free::tensor(Free::identity(1), step_expr(b)),
            )
            .unwrap(),
        )
        .unwrap();

        assert!(tensored.eq_colored(&interchanged));
        assert_eq!(demand(&tensored).counts(), demand(&interchanged).counts());
    }
}
