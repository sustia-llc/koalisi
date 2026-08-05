//! The two registered cost models (prereg §4 D5, Amendment 1 A1.3).
//!
//! [`cost_of`](catgraph_applied::prop::presentation::rewrite::cost_of) is a sum
//! of per-generator weights over hyperedge occurrences, so a cost model is
//! exactly a `Fn(&WorkflowGen) -> u64`. Two are registered:
//!
//! - [`uniform_cost`] — `|_| 1`, catgraph's default: generator count. The arm is
//!   **metric-blind**, so a margin under it means process cost genuinely
//!   converts to staffing quality.
//! - [`staffing_price`] — each step weighted by how scarce its `(bit, role)` is
//!   in the drawn pool. Higher pass probability, weaker claim; registered as its
//!   own cells rather than substituted for the uniform ones.
//!
//! # What a cost model cannot express
//!
//! `cost_of` sums over **occurrences**, so "minimise the number of *distinct*
//! `(bit, role)` pairs" — the staffing question — is not expressible as a
//! `per_gen` at all. Neither model is a proxy for distinct demand; both price
//! the process and let the staffing contest read the result.

use std::collections::BTreeMap;

use catgraph_syntax::frobenius::FrobeniusOr;

use super::signature::{Role, Step, WorkflowGen};

/// How many pool workers of each role hold each capability bit.
///
/// Built **once** from the drawn pool, before any decision, and constant for
/// the task — the registration requires the price to be a property of the world
/// rather than of the coalition being assembled.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StaffingTable {
    pool_size: u32,
    holders: BTreeMap<Step, u32>,
}

impl StaffingTable {
    /// Tabulate a pool of `(capability_mask, role)` workers.
    ///
    /// Every set bit of each mask counts as that worker holding that step's
    /// capability at that worker's role. A worker contributes to `pool_size`
    /// whatever its mask is, including an empty one — scarcity is measured
    /// against the whole pool, not against the pool that could plausibly help.
    pub fn from_pool<I>(workers: I) -> Self
    where
        I: IntoIterator<Item = (u32, Role)>,
    {
        const WIDTH: u8 = 32; // u32::BITS, kept as u8 so no cast is needed.
        let mut pool_size = 0u32;
        let mut holders: BTreeMap<Step, u32> = BTreeMap::new();
        for (mask, role) in workers {
            pool_size = pool_size.saturating_add(1);
            for bit in 0..WIDTH {
                if mask & (1u32 << bit) != 0 {
                    *holders.entry(Step::new(bit, role)).or_insert(0) += 1;
                }
            }
        }
        Self { pool_size, holders }
    }

    /// The number of workers drawn.
    #[must_use]
    pub fn pool_size(&self) -> u32 {
        self.pool_size
    }

    /// How many pool workers can perform `step`.
    #[must_use]
    pub fn holders(&self, step: Step) -> u32 {
        self.holders.get(&step).copied().unwrap_or(0)
    }

    /// Whether any pool worker can perform `step` — the role-matched
    /// feasibility question, asked one step at a time.
    #[must_use]
    pub fn is_staffable(&self, step: Step) -> bool {
        self.holders(step) > 0
    }

    /// `scarcity(b, r) = pool_size − |{workers of role r holding bit b}|`.
    ///
    /// Zero when every worker can do it; `pool_size` when none can.
    #[must_use]
    pub fn scarcity(&self, step: Step) -> u32 {
        // `holders` is built by counting pool members, so it can never exceed
        // `pool_size`; `saturating_sub` keeps that an invariant rather than a
        // panic if the table is ever built another way.
        self.pool_size.saturating_sub(self.holders(step))
    }

    /// `per_gen(s_{b,r}) = 1 + scarcity(b, r)`.
    ///
    /// An **unstaffable** step therefore costs `1 + pool_size` — expensive but
    /// finite, so the optimizer routes away from it without the objective going
    /// non-finite.
    #[must_use]
    pub fn price(&self, step: Step) -> u64 {
        1 + u64::from(self.scarcity(step))
    }
}

/// The uniform cost model: every generator occurrence weighs 1.
///
/// Spiders count too — they are hyperedges like any other, so a fan-out-and-
/// rejoin is not free.
pub fn uniform_cost() -> impl Fn(&WorkflowGen) -> u64 {
    |_| 1
}

/// The staffing-priced cost model: `1 + scarcity` per step, 1 per spider.
///
/// The table is borrowed rather than copied, so build it once per task (per the
/// registration) and hand the same closure to every
/// [`optimize`](catgraph_applied::prop::presentation::rewrite::optimize) call
/// for that task.
pub fn staffing_price(table: &StaffingTable) -> impl Fn(&WorkflowGen) -> u64 + '_ {
    move |generator| match generator {
        FrobeniusOr::User(step) => table.price(*step),
        FrobeniusOr::Mu(_)
        | FrobeniusOr::Eta(_)
        | FrobeniusOr::Delta(_)
        | FrobeniusOr::Epsilon(_) => 1,
    }
}

#[cfg(test)]
mod tests {
    use catgraph_applied::prop::colored::ColoredExpr;
    use catgraph_applied::prop::presentation::content::content_of_colored;
    use catgraph_applied::prop::presentation::rewrite::cost_of;

    use super::super::signature::{chain, spider_expr, step_expr};
    use super::*;

    fn table() -> StaffingTable {
        let r0 = Role::new(0);
        let r1 = Role::new(1);
        // 4 workers: three r0 (bits {0,1}, {0}, {0}) and one r1 (bits {1}).
        StaffingTable::from_pool([(0b011, r0), (0b001, r0), (0b001, r0), (0b010, r1)])
    }

    #[test]
    fn scarcity_and_price_follow_the_pinned_formula() {
        let t = table();
        let r0 = Role::new(0);
        let r1 = Role::new(1);

        assert_eq!(t.pool_size(), 4);
        assert_eq!(t.holders(Step::new(0, r0)), 3);
        assert_eq!(t.scarcity(Step::new(0, r0)), 1);
        assert_eq!(t.price(Step::new(0, r0)), 2);

        assert_eq!(t.holders(Step::new(1, r0)), 1);
        assert_eq!(t.price(Step::new(1, r0)), 4);

        // Unstaffable: no r1 worker holds bit 0, and nobody holds bit 7.
        assert!(!t.is_staffable(Step::new(0, r1)));
        assert_eq!(t.price(Step::new(0, r1)), 1 + u64::from(t.pool_size()));
        assert_eq!(t.price(Step::new(7, r0)), 5);
        // Finite, always — that is the point of the +1 and the clamp.
        assert!(t.price(Step::new(7, r0)) < u64::MAX);

        // Role-matched, not role-blind: r1 holds bit 1, r0 mostly does not.
        assert!(t.is_staffable(Step::new(1, r1)));
        assert_eq!(t.holders(Step::new(1, r1)), 1);
    }

    #[test]
    fn spiders_price_at_one_under_both_models() {
        let r0 = Role::new(0);
        let s = Step::new(1, r0);
        let workflow = ColoredExpr::new(
            vec![r0],
            chain(vec![
                spider_expr(FrobeniusOr::Delta(r0)),
                catgraph_applied::prop::Free::tensor(step_expr(s), step_expr(s)),
                spider_expr(FrobeniusOr::Mu(r0)),
            ])
            .unwrap(),
        )
        .unwrap();
        let content = content_of_colored(&workflow);

        // 4 occurrences: δ, s, s, μ.
        assert_eq!(cost_of(&content, uniform_cost()), 4);

        let t = table();
        // 2 spiders at 1 + 2 steps at price(bit 1, r0) = 4.
        assert_eq!(cost_of(&content, staffing_price(&t)), 2 + 2 * 4);
    }

    #[test]
    fn an_empty_pool_prices_everything_at_one() {
        let t = StaffingTable::from_pool(std::iter::empty());
        assert_eq!(t.pool_size(), 0);
        assert_eq!(t.scarcity(Step::new(0, Role::new(0))), 0);
        assert_eq!(t.price(Step::new(0, Role::new(0))), 1);
    }
}
