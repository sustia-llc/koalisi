//! Forex market value types shared across the swarm.
//!
//! Everything in this module is `Clone`able plain data — no actor refs, no
//! lifetimes. These are the messages flowing through the task command
//! channels and broadcast buses.

use std::fmt;
use std::str::FromStr;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::ingest::{Sample, SampleUpdate};

// ---------------------------------------------------------------------------
// Pair
// ---------------------------------------------------------------------------

/// A currency pair (base / quote), e.g. `EUR/USD` ⇒ base=`EUR`, quote=`USD`.
///
/// "EUR/USD = 1.10" means 1 EUR is worth 1.10 USD.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Pair {
    pub base: String,
    pub quote: String,
}

impl Pair {
    pub fn new(base: impl Into<String>, quote: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            quote: quote.into(),
        }
    }
}

impl fmt::Display for Pair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.base, self.quote)
    }
}

impl FromStr for Pair {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        let (base, quote) = s
            .split_once('/')
            .ok_or_else(|| anyhow!("pair must be in BASE/QUOTE form, got {s:?}"))?;
        if base.is_empty() || quote.is_empty() {
            return Err(anyhow!("pair components must be non-empty: {s:?}"));
        }
        Ok(Self::new(base, quote))
    }
}

// ---------------------------------------------------------------------------
// Tick + Quote
// ---------------------------------------------------------------------------

/// A raw market tick — what feeders push into a `MarketMonitor`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tick {
    pub pair: Pair,
    pub bid: f64,
    pub ask: f64,
    /// Unix epoch milliseconds.
    pub timestamp_ms: i64,
}

impl Tick {
    pub fn new(pair: Pair, bid: f64, ask: f64, timestamp_ms: i64) -> Self {
        Self {
            pair,
            bid,
            ask,
            timestamp_ms,
        }
    }

    pub fn mid(&self) -> f64 {
        (self.bid + self.ask) / 2.0
    }

    pub fn quote(&self) -> Quote {
        Quote {
            bid: self.bid,
            ask: self.ask,
            mid: self.mid(),
            timestamp_ms: self.timestamp_ms,
        }
    }
}

/// Forex instantiation of the domain-neutral ingestion [`Sample`] (issue #8):
/// a tick's routing key is its [`Pair`] and its distilled view is a [`Quote`].
/// This is what makes `MarketMonitor = SampleMonitor<Tick>` and
/// `TickUpdate = SampleUpdate<Tick>`.
impl Sample for Tick {
    type Key = Pair;
    type View = Quote;

    fn key(&self) -> &Pair {
        &self.pair
    }

    fn timestamp_ms(&self) -> i64 {
        self.timestamp_ms
    }

    fn view(&self) -> Quote {
        self.quote()
    }
}

/// A monitor's distilled view of a pair: the latest bid/ask/mid + when it
/// was observed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub bid: f64,
    pub ask: f64,
    pub mid: f64,
    pub timestamp_ms: i64,
}

// ---------------------------------------------------------------------------
// TickUpdate — pubsub broadcast from MarketMonitor → ArbitrageCoordinator
// ---------------------------------------------------------------------------

/// What a `MarketMonitor` publishes to the tick pubsub on every fresh tick.
///
/// This is the "collaboration signal": no individual monitor decides if
/// there's an arbitrage. They report observations; the coordinator combines
/// them.
///
/// Since issue #8 this is an alias for the generic
/// [`SampleUpdate`]`<`[`Tick`]`>`: the former `pair` / `quote` fields are now
/// the generic `key` / `view` fields (`key: Pair`, `view: Quote`).
pub type TickUpdate = SampleUpdate<Tick>;

// ---------------------------------------------------------------------------
// Triangle
// ---------------------------------------------------------------------------

/// A triangular-arbitrage triple sharing a common quote currency.
///
/// Given `leg_a = X/Z`, `leg_b = Y/Z`, `cross = X/Y`:
/// `synthetic(cross) = mid(leg_a) / mid(leg_b)`,
/// compared against `mid(cross)` for the edge.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Triangle {
    pub leg_a: Pair, // X/Z
    pub leg_b: Pair, // Y/Z
    pub cross: Pair, // X/Y
}

impl Triangle {
    /// Validates that `leg_a.quote == leg_b.quote`,
    /// `leg_a.base == cross.base`, `leg_b.base == cross.quote`.
    pub fn new(leg_a: Pair, leg_b: Pair, cross: Pair) -> Result<Self> {
        if leg_a.quote != leg_b.quote {
            return Err(anyhow!(
                "leg_a and leg_b must share a common quote currency \
                 (leg_a quote = {}, leg_b quote = {})",
                leg_a.quote,
                leg_b.quote,
            ));
        }
        if leg_a.base != cross.base {
            return Err(anyhow!(
                "cross.base must match leg_a.base ({} vs {})",
                cross.base,
                leg_a.base,
            ));
        }
        if leg_b.base != cross.quote {
            return Err(anyhow!(
                "cross.quote must match leg_b.base ({} vs {})",
                cross.quote,
                leg_b.base,
            ));
        }
        Ok(Self {
            leg_a,
            leg_b,
            cross,
        })
    }

    pub fn pairs(&self) -> [&Pair; 3] {
        [&self.leg_a, &self.leg_b, &self.cross]
    }

    /// Compute synthetic cross from leg mids.
    /// `synthetic = mid(leg_a) / mid(leg_b)`.
    pub fn synthetic_mid(&self, leg_a: &Quote, leg_b: &Quote) -> f64 {
        leg_a.mid / leg_b.mid
    }

    /// Edge in basis points, positive when the actual cross trades *above*
    /// the synthetic (so the synthetic route is cheaper to acquire `X`).
    ///
    /// `edge_bps = (actual - synthetic) / actual * 10_000`.
    pub fn edge_bps(&self, leg_a: &Quote, leg_b: &Quote, cross: &Quote) -> f64 {
        let synthetic = self.synthetic_mid(leg_a, leg_b);
        ((cross.mid - synthetic) / cross.mid) * 10_000.0
    }
}

// ---------------------------------------------------------------------------
// ArbitrageOpportunity — collective output of the swarm
// ---------------------------------------------------------------------------

/// Direction of the detected arbitrage relative to the synthetic vs the
/// actual cross.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    /// Actual > synthetic: buy `X` via the synthetic route (legs), sell on
    /// the cross.
    BuySyntheticSellActual,
    /// Actual < synthetic: buy on the cross, sell via the synthetic route.
    SellSyntheticBuyActual,
}

/// A triangular-arbitrage signal — the collective fire of the swarm.
///
/// `Serialize` + `Deserialize` so the type doubles as the wire payload
/// for the remote alert gateway (see `subsystems::distributed`, feature
/// `remote`). A production deployment would likely split this into a
/// separate stable wire schema; for the POC the local and remote types
/// are the same.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageOpportunity {
    pub triangle: Triangle,
    pub leg_a_quote: Quote,
    pub leg_b_quote: Quote,
    pub cross_quote: Quote,
    pub synthetic_mid: f64,
    pub actual_mid: f64,
    /// Signed edge in basis points (see [`Triangle::edge_bps`]).
    pub edge_bps: f64,
    pub direction: Direction,
    /// Max `timestamp_ms` across the three legs at the moment of detection.
    pub detected_at_ms: i64,
}

impl fmt::Display for ArbitrageOpportunity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "arb({} = {} / {}): synthetic={:.6} actual={:.6} edge={:+.2}bps {:?}",
            self.triangle.cross,
            self.triangle.leg_a,
            self.triangle.leg_b,
            self.synthetic_mid,
            self.actual_mid,
            self.edge_bps,
            self.direction,
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Pair {
        s.parse().unwrap()
    }

    #[test]
    fn pair_roundtrip() {
        let p = Pair::new("EUR", "USD");
        assert_eq!(p.to_string(), "EUR/USD");
        assert_eq!("EUR/USD".parse::<Pair>().unwrap(), p);
    }

    #[test]
    fn pair_rejects_bad_input() {
        assert!("EURUSD".parse::<Pair>().is_err());
        assert!("/USD".parse::<Pair>().is_err());
        assert!("EUR/".parse::<Pair>().is_err());
    }

    #[test]
    fn tick_mid_is_average_of_bid_and_ask() {
        let t = Tick::new(p("EUR/USD"), 1.0998, 1.1002, 0);
        assert!((t.mid() - 1.1000).abs() < 1e-9);
    }

    #[test]
    fn triangle_validates_relationships() {
        assert!(Triangle::new(p("EUR/USD"), p("GBP/USD"), p("EUR/GBP")).is_ok());
        // mismatched quote currencies on legs
        assert!(Triangle::new(p("EUR/USD"), p("GBP/JPY"), p("EUR/GBP")).is_err());
        // cross base doesn't match leg_a base
        assert!(Triangle::new(p("EUR/USD"), p("GBP/USD"), p("CHF/GBP")).is_err());
        // cross quote doesn't match leg_b base
        assert!(Triangle::new(p("EUR/USD"), p("GBP/USD"), p("EUR/CHF")).is_err());
    }

    #[test]
    fn triangle_edge_zero_when_synthetic_matches_actual() {
        let tri = Triangle::new(p("EUR/USD"), p("GBP/USD"), p("EUR/GBP")).unwrap();
        // EUR/USD = 1.10, GBP/USD = 1.30 → synthetic EUR/GBP = 1.10/1.30 ≈ 0.84615
        // set actual EUR/GBP = synthetic for zero edge
        let qa = Quote {
            bid: 1.0998,
            ask: 1.1002,
            mid: 1.1000,
            timestamp_ms: 0,
        };
        let qb = Quote {
            bid: 1.2998,
            ask: 1.3002,
            mid: 1.3000,
            timestamp_ms: 0,
        };
        let synthetic = tri.synthetic_mid(&qa, &qb);
        let qc = Quote {
            bid: synthetic - 0.0001,
            ask: synthetic + 0.0001,
            mid: synthetic,
            timestamp_ms: 0,
        };
        let edge = tri.edge_bps(&qa, &qb, &qc);
        assert!(edge.abs() < 1e-6, "edge should be ~0, got {edge}");
    }

    #[test]
    fn triangle_edge_positive_when_actual_above_synthetic() {
        let tri = Triangle::new(p("EUR/USD"), p("GBP/USD"), p("EUR/GBP")).unwrap();
        let qa = Quote {
            bid: 1.0998,
            ask: 1.1002,
            mid: 1.1000,
            timestamp_ms: 0,
        };
        let qb = Quote {
            bid: 1.2998,
            ask: 1.3002,
            mid: 1.3000,
            timestamp_ms: 0,
        };
        // synthetic ≈ 0.84615; pick actual 0.85 ⇒ edge ≈ (0.85 - 0.84615) / 0.85 * 10000 ≈ 45.25 bps
        let qc = Quote {
            bid: 0.8498,
            ask: 0.8502,
            mid: 0.8500,
            timestamp_ms: 0,
        };
        let edge = tri.edge_bps(&qa, &qb, &qc);
        assert!(
            edge > 40.0 && edge < 50.0,
            "edge {edge} not in [40, 50] bps"
        );
    }
}
