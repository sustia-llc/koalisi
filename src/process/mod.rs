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
//! fusion schema rewrites `s_{b1,r} ; s_{b2,r} ⇒ s_{b3,r}` with `b3 ∉ {b1, b2}`.
//! Had it targeted one of the consumed bits, every application would strictly
//! shrink distinct demand and the rewriting arms would win *by construction*. A
//! fused step instead demands a capability neither consumed step required, so an
//! application may land on a `(bit, role)` no pool worker holds — the lever is
//! two-sided, which is exactly what makes the confirmatory leg falsifiable. See
//! [`rule_theory`].
//!
//! **3. The optimizer is bounded and best-found.** [`optimize_workflow`] is a
//! fuel-limited best-first search. catgraph claims no termination, no
//! confluence, and no canonicality for it, and neither does this module: a
//! cheaper writing may exist beyond the budget, and two budgets may return
//! different representatives of the same class. What *is* claimed is soundness
//! per step (BGKSZ Thm 5.6) — the result does the same job as the input.
//!
//! **4. Upstream errors are outcomes, not panics.** Every fallible entry point
//! propagates [`CatgraphError`](catgraph_magnitude::CatgraphError) or
//! [`ProcessError`]. A task whose optimization or verification fails is a
//! decline-and-count, never a panic and never a silent as-written fallback.
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
pub mod rewrite;
pub mod signature;
pub mod theory;

pub use cost::{StaffingTable, staffing_price, uniform_cost};
pub use demand::{Demand, demand};
pub use errors::ProcessError;
pub use rewrite::{content_matches, optimize_workflow, verify_optimization, workflow_cost};
pub use signature::{Role, Step, Workflow, WorkflowGen, chain, spider_expr, step_expr};
pub use theory::{LabelledRule, Schema, rule_labels, rule_theory};
