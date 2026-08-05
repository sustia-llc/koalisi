//! The optimize hook and the S-sound verification gate.

use catgraph_applied::prop::presentation::content::{content_eq, content_of_colored};
use catgraph_applied::prop::presentation::rewrite::{
    RewriteOutcome, RewriteRule, cost_of, optimize, replay,
};
use catgraph_magnitude::CatgraphError;
use tracing::debug;

use super::errors::ProcessError;
use super::signature::{Workflow, WorkflowGen};

/// Search for a cheaper writing of `start` under `rules`, spending at most
/// `fuel` rewrite applications.
///
/// # What the result means, and what it does not
///
/// Each recorded step is a convex-DPO step, so by BGKSZ **Thm 5.6** the returned
/// representative is related to `start` by `rules` in the free symmetric
/// monoidal category — the same job, differently written. That is the whole
/// claim.
///
/// The search is **bounded and best-found**: it is a fuel-limited best-first
/// walk over states deduplicated by canonical key. catgraph makes no
/// termination, confluence, or canonicality claim about it, and neither does
/// this wrapper. In particular:
///
/// - [`RewriteOutcome::best`] is the cheapest writing *found*, not the cheapest
///   writing that exists;
/// - [`RewriteOutcome::fuel_exhausted`] means the walk stopped at the budget, so
///   a larger budget may find something cheaper — how much of any margin is
///   fuel-bought is an empirical question, not a settled one;
/// - two runs at different fuel may return different representatives of the same
///   equivalence class.
///
/// `cost_of` is deliberately **not** invariant under the theory's equations —
/// that difference is the entire optimization signal.
///
/// # Errors
///
/// Propagates any [`CatgraphError`] from the upstream search: an ill-formed
/// `start` (reachable only across the serde trust boundary) or an
/// engine-invariant readback failure. Never panics; an upstream rejection is a
/// decline-and-count at the battery level, never a silent as-written fallback.
pub fn optimize_workflow(
    start: &Workflow,
    rules: &[RewriteRule<WorkflowGen>],
    fuel: usize,
    per_gen: impl Fn(&WorkflowGen) -> u64,
) -> Result<RewriteOutcome<WorkflowGen>, CatgraphError> {
    let outcome = optimize(start, rules, fuel, per_gen)?;
    debug!(
        initial_cost = outcome.initial_cost(),
        best_cost = outcome.best_cost(),
        steps = outcome.steps().len(),
        states_explored = outcome.states_explored(),
        fuel_exhausted = outcome.fuel_exhausted(),
        fuel,
        rules = rules.len(),
        "workflow rewrite search finished"
    );
    Ok(outcome)
}

/// The S-sound gate: check that an outcome's trace really derives the writing it
/// declares.
///
/// Two conditions, both required:
///
/// 1. [`replay`] of `outcome.steps()` against `rules` succeeds — every step is
///    re-validated as a convex match of the rule its index names before it is
///    applied, so a trace that does not describe a legal derivation is rejected
///    rather than trusted;
/// 2. the replayed content [`content_eq`]s the content of
///    [`RewriteOutcome::best`] — cospan iso under both feet, the only sense
///    available, since node indices are presentation-dependent.
///
/// Scoring runs against each arm's **declared writing**, so an arm that declares
/// an unverified writing would be grading its own homework. Call this per task,
/// on every declared writing that is not the as-written one, before scoring it.
///
/// Validation is relative to `rules` **as given** — a trace binds rule *indices*,
/// not rule identities. Verifying against a different slice than the one
/// `optimize` ran on is checking a different theory, and is expected to fail.
///
/// # Errors
///
/// - [`ProcessError::Catgraph`] when the replay itself is rejected;
/// - [`ProcessError::Unsound`] when the replay lands on a different process than
///   the outcome reported.
pub fn verify_optimization(
    start: &Workflow,
    rules: &[RewriteRule<WorkflowGen>],
    outcome: &RewriteOutcome<WorkflowGen>,
) -> Result<(), ProcessError> {
    let replayed = replay(start, rules, outcome.steps())?;
    if content_matches(&replayed, outcome.best()) {
        return Ok(());
    }
    Err(ProcessError::Unsound {
        message: format!(
            "replaying {} step(s) landed on a process that is not the reported representative",
            outcome.steps().len()
        ),
    })
}

/// Whether two workflows are the same process — content equality, not tree
/// equality.
///
/// Exposed because it is the comparison the S-sound gate makes, and a report
/// wanting to say "the declared writing IS the as-written one" asks exactly the
/// same question.
#[must_use]
pub fn content_matches(left: &Workflow, right: &Workflow) -> bool {
    content_eq(&content_of_colored(left), &content_of_colored(right))
}

/// The cost of a workflow's content under `per_gen`.
///
/// A thin convenience over [`cost_of`]: the cost is read off *content*, so it is
/// a function of the process rather than of the writing — but it is deliberately
/// **not** invariant under the theory's equations.
#[must_use]
pub fn workflow_cost(workflow: &Workflow, per_gen: impl Fn(&WorkflowGen) -> u64) -> u64 {
    cost_of(&content_of_colored(workflow), per_gen)
}

#[cfg(test)]
mod tests {
    use catgraph_applied::prop::Free;
    use catgraph_applied::prop::colored::ColoredExpr;
    use catgraph_syntax::frobenius::FrobeniusOr;

    use super::super::cost::uniform_cost;
    use super::super::demand::demand;
    use super::super::signature::{Role, Step, chain, spider_expr, step_expr};
    use super::super::theory::rule_theory;
    use super::*;

    const BITS: u8 = 8;
    const ROLES: u8 = 3;
    const FUEL: usize = 256;

    fn pin(role: Role, expr: catgraph_applied::prop::PropExpr<WorkflowGen>) -> Workflow {
        ColoredExpr::new(vec![role], expr).unwrap()
    }

    #[test]
    fn idempotence_reduces_cost_but_not_distinct_demand() {
        let role = Role::new(0);
        let step = Step::new(3, role);
        let rules = rule_theory(BITS, ROLES).unwrap();

        // s ; s ; s — three occurrences of one distinct step.
        let start = pin(
            role,
            chain(vec![step_expr(step), step_expr(step), step_expr(step)]).unwrap(),
        );
        let before = demand(&start);
        assert_eq!(before.total(), 3);
        assert_eq!(before.distinct_len(), 1);

        let outcome = optimize_workflow(&start, &rules, FUEL, uniform_cost()).unwrap();
        let after = demand(outcome.best());

        assert!(
            outcome.best_cost() < outcome.initial_cost(),
            "idempotence must reduce occurrence cost: {} -> {}",
            outcome.initial_cost(),
            outcome.best_cost()
        );
        assert_eq!(after.total(), 1);
        assert_eq!(
            after.distinct_len(),
            before.distinct_len(),
            "idempotence leaves distinct demand untouched — the inertness trap"
        );
        assert_eq!(after.distinct().collect::<Vec<_>>(), vec![step]);
        verify_optimization(&start, &rules, &outcome).expect("S-sound");
    }

    #[test]
    fn fusion_moves_distinct_demand_to_a_bit_outside_the_consumed_pair() {
        // Amendment A3.2: b'' = (b + b' + 4) mod bits, here (2 + 3 + 4) mod 8 = 1.
        // Read off the theory's own pair set rather than restated, so the test
        // cannot drift from the schema it is checking.
        let role = Role::new(1);
        let target = super::super::theory::fusion_pairs(BITS)
            .into_iter()
            .find(|&(first, second, _)| (first, second) == (2, 3))
            .expect("the (2, 3) ordered pair survives the two-sidedness skip")
            .2;
        assert_eq!(target, 1);
        let (b1, b2, b3) = (
            Step::new(2, role),
            Step::new(3, role),
            Step::new(target, role),
        );
        let rules = rule_theory(BITS, ROLES).unwrap();

        let start = pin(role, chain(vec![step_expr(b1), step_expr(b2)]).unwrap());
        let before = demand(&start);
        assert_eq!(before.distinct_len(), 2);

        let outcome = optimize_workflow(&start, &rules, FUEL, uniform_cost()).unwrap();
        let after = demand(outcome.best());

        assert_eq!(
            after.distinct_len(),
            1,
            "fusion DOES change distinct demand"
        );
        let fused: Vec<_> = after.distinct().collect();
        assert_eq!(fused, vec![b3]);
        assert!(
            !fused.contains(&b1) && !fused.contains(&b2),
            "the fused step demands a capability neither consumed step required"
        );
        verify_optimization(&start, &rules, &outcome).expect("S-sound");
    }

    #[test]
    fn spider_absorption_applies() {
        let role = Role::new(2);
        let step = Step::new(5, role);
        let rules = rule_theory(BITS, ROLES).unwrap();

        // δ_r ; (s ⊗ s) ; μ_r — 4 occurrences.
        let start = pin(
            role,
            chain(vec![
                spider_expr(FrobeniusOr::Delta(role)),
                Free::tensor(step_expr(step), step_expr(step)),
                spider_expr(FrobeniusOr::Mu(role)),
            ])
            .unwrap(),
        );
        assert_eq!(workflow_cost(&start, uniform_cost()), 4);

        let outcome = optimize_workflow(&start, &rules, FUEL, uniform_cost()).unwrap();
        assert_eq!(outcome.initial_cost(), 4);
        assert_eq!(outcome.best_cost(), 1, "the fan-out-and-rejoin collapses");
        assert_eq!(
            demand(outcome.best()).distinct().collect::<Vec<_>>(),
            vec![step]
        );
        assert!(!outcome.steps().is_empty());
        verify_optimization(&start, &rules, &outcome).expect("S-sound");
    }

    #[test]
    fn an_empty_rule_set_leaves_the_start_untouched() {
        // The X-reduce precondition: with no rules the rewriting cells must
        // reproduce the as-written control bit for bit.
        let role = Role::new(0);
        let start = pin(
            role,
            chain(vec![
                step_expr(Step::new(0, role)),
                step_expr(Step::new(1, role)),
                step_expr(Step::new(1, role)),
            ])
            .unwrap(),
        );

        let outcome = optimize_workflow(&start, &[], FUEL, uniform_cost()).unwrap();
        assert_eq!(outcome.steps().len(), 0);
        assert_eq!(outcome.best_cost(), outcome.initial_cost());
        assert!(!outcome.fuel_exhausted());
        assert!(content_matches(&start, outcome.best()));
        assert_eq!(demand(outcome.best()), demand(&start));
        verify_optimization(&start, &[], &outcome).expect("a zero-step trace verifies");
    }

    #[test]
    fn verification_accepts_a_real_run_and_rejects_a_tampered_trace() {
        let role = Role::new(0);
        let step = Step::new(1, role);
        let rules = rule_theory(BITS, ROLES).unwrap();
        let start = pin(
            role,
            chain(vec![step_expr(step), step_expr(step), step_expr(step)]).unwrap(),
        );

        let outcome = optimize_workflow(&start, &rules, FUEL, uniform_cost()).unwrap();
        assert!(!outcome.steps().is_empty(), "the run must actually rewrite");
        verify_optimization(&start, &rules, &outcome).expect("a real run verifies");

        // Tamper 1: the trace binds rule INDICES, so verifying against a theory
        // that has no such rule must fail.
        let err = verify_optimization(&start, &[], &outcome)
            .expect_err("a trace naming rules outside the slice must be rejected");
        assert!(matches!(err, ProcessError::Catgraph(_)), "got {err}");
        assert!(err.to_string().contains("upstream rejection"));

        // Tamper 2: the same trace against a different starting process — the
        // recorded hyperedges no longer describe a convex match.
        let other = pin(Role::new(2), step_expr(Step::new(4, Role::new(2))));
        assert!(verify_optimization(&other, &rules, &outcome).is_err());

        // Tamper 3: shifting the rule slice keeps the indices in range but
        // re-points them at rules whose labels do not match.
        let shifted: Vec<_> = rules.iter().rev().cloned().collect();
        assert!(verify_optimization(&start, &shifted, &outcome).is_err());

        // And the comparison the gate makes is content equality, not tree
        // equality: two genuinely different processes do not match.
        assert!(!content_matches(&start, &other));
        assert!(content_matches(&start, &start.clone()));
    }
}
