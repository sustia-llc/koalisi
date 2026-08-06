//! Process-structured coalition tasks (`EQ5a`, issue #76 — feature `process`).
//!
//! A coalition task is usually a flat capability mask: "these bits must be
//! covered". This module carries the other half of the picture — the
//! **process**: which step happens after which, which steps run in parallel, and
//! which worker role each step belongs to. A task becomes a colored string
//! diagram, and a coalition staffs it.
//!
//! The registration this surface exists for is
//! `docs/prereg-K4-eq5a-process-structured.md`: does optimizing the *process*
//! before staffing it beat staffing it as written? This module is the library
//! half (prereg §4 placement D8) — the workflow type, demand extraction, the
//! pinned rule theory, the two cost models, and the optimize hook a runtime
//! would use. Battery scaffolding — world draw, scorers, tables — stays
//! example-side.
//!
//! ## The four things worth knowing before using it
//!
//! **1. Multiplicity prices the process but adds no coverage demand.** Coverage
//! is asked per *distinct* `(bit, role)`; cost is summed per *occurrence*. So
//! [`Demand::distinct_len`] is the staffing question and [`Demand::total`] is
//! the pricing question, and a rewrite can move one without the other. See
//! [`Demand`] — the asymmetry is what makes a rewrite theory able to matter at
//! all.
//!
//! **2. The fusion target is a third bit, on purpose.** The pinned theory's
//! fusion schema rewrites `s_{b,r} ; s_{b',r} ⇒ s_{b'',r}` with `b'' ∉ {b, b'}`,
//! for every role and every ordered pair of distinct bits (Amendment A3.2 —
//! the narrow one-pair-per-role form was eligible on only 7.2 % of drawn tasks).
//! Had it targeted one of the consumed bits, every application would strictly
//! shrink distinct demand and the rewriting arms would win *by construction*; a
//! pair whose target would be consumed is therefore not built at all. A fused
//! step instead demands a capability neither consumed step required, so an
//! application may land on a `(bit, role)` no pool worker holds — the lever is
//! two-sided, which is exactly what makes the confirmatory leg falsifiable. See
//! [`rule_theory`].
//!
//! **3. The optimizer is bounded and best-found.** [`optimize_workflow`] is a
//! fuel-limited best-first search. catgraph claims no termination, no
//! confluence, and no canonicality for it, and neither does this module: a
//! cheaper writing may exist beyond the budget, and two budgets may return
//! different representatives of the same class. What *is* claimed is soundness
//! per step (BGKSZ Thm 5.6) — **theory-relative derivability**: the optimizer
//! did not step outside the rule set it was given. That is NOT a claim that the
//! result does the same job. Thm 5.6 says nothing about what the rules *mean*,
//! and a hand-authored rule set stipulates its equivalences rather than proving
//! them, so any "sameness" is theory-internal and belongs to whoever declared
//! the rules. See [`optimize_workflow`].
//!
//! **4. Upstream errors are outcomes, not panics.** Every fallible entry point
//! propagates [`CatgraphError`](catgraph_magnitude::CatgraphError) or
//! [`ProcessError`]. A task whose optimization or verification fails is a
//! decline-and-count, never a panic and never a silent as-written fallback.
//!
//! ## Staffing a process: the residual policy
//!
//! [`ResidualPolicy`] (added by the `#80` registration, prereg §7 / lock D6) is
//! the decision-side counterpart: a [`MagnitudePolicy`](crate::decision::MagnitudePolicy)
//! wrapper that charges `λ ·` the price of the demand a coalition **cannot
//! cover**, so occurrence multiplicity and step scarcity reach a decision for the
//! first time. Its four load-bearing properties — the penalty depends on the
//! coalition (so it does not cancel), spiders are excluded, an upstream decline is
//! detected independently rather than inferred from a zero score, and λ = 0
//! reproduces the inner policy bit for bit — are documented on [`residual`].
//!
//! ## Shape
//!
//! ```text
//!   (bit, role) tagged requirements
//!            │
//!            ▼   Free::compose / Free::tensor / FrobeniusOr spiders
//!        Workflow  ── demand() ─────────────▶  Demand   (staffing question)
//!            │                                    │
//!            │  optimize_workflow(rules, fuel, per_gen)
//!            ▼
//!      RewriteOutcome ── verify_optimization() ─▶ S-sound gate
//!            │
//!            └─ best() ── demand() ───────────▶  Demand of the declared writing
//! ```

pub mod cost;
pub mod demand;
pub mod errors;
pub mod residual;
pub mod rewrite;
pub mod signature;
pub mod theory;

pub use cost::{StaffingTable, staffing_price, uniform_cost};
pub use demand::{Demand, demand};
pub use errors::ProcessError;
pub use residual::{DeclineCounter, Residual, ResidualBasis, ResidualPolicy};
pub use rewrite::{content_matches, optimize_workflow, verify_optimization, workflow_cost};
pub use signature::{Role, Step, Workflow, WorkflowGen, chain, spider_expr, step_expr};
pub use theory::{LabelledRule, Schema, fusion_pairs, rule_labels, rule_theory};
