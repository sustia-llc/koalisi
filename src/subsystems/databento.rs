//! Databento DBN-file adapter.
//!
//! Decodes a `.dbn` or `.dbn.zst` file emitted by Databento's ecosystem
//! and pushes the MBP-1 top-of-book records into the swarm's
//! `MarketMonitor`s via a clone-able [`SwarmFeeder`].
//!
//! Two pacing modes:
//!
//! - [`Pacing::Asap`] — pump as fast as the decoder can produce records.
//!   Used for the *historical bootstrap* path.
//! - [`Pacing::Realtime { speed_factor }`] — pace records by their
//!   `ts_recv` deltas, optionally accelerated. Used for *live replay*
//!   simulation of recorded data.
//!
//! [`spawn_dbn_pump`] launches the pump on the swarm's `TaskTracker`
//! with a child of its `CancellationToken`, so the adapter fully
//! participates in `Swarm::shutdown()` — no orphan tasks at teardown.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use dbn::decode::{AsyncDbnDecoder, DbnMetadata};
use dbn::{FIXED_PRICE_SCALE, Mbp1Msg, PitSymbolMap};
use tokio::io::AsyncReadExt;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::market::{Pair, Tick};
use crate::subsystems::swarm::{Swarm, SwarmFeeder};

// ---------------------------------------------------------------------------
// Pacing
// ---------------------------------------------------------------------------

/// Replay pacing strategy.
#[derive(Debug, Clone, Copy)]
pub enum Pacing {
    /// Pump every decoded record as fast as the decoder can produce it.
    /// Used by `swarm.replay_history(...)`-style bootstrap flows.
    Asap,
    /// Pace by `ts_recv` deltas. `speed_factor == 1.0` matches wall-clock
    /// time to recorded time; `2.0` plays back at 2x speed; `0.5` at half
    /// speed. Non-positive values are treated as [`Pacing::Asap`].
    Realtime { speed_factor: f64 },
}

impl Default for Pacing {
    fn default() -> Self {
        Pacing::Asap
    }
}

// ---------------------------------------------------------------------------
// Symbol mapper
// ---------------------------------------------------------------------------

/// User-supplied closure that maps a DBN record's identity (its numeric
/// `instrument_id` plus the optional resolved symbol string from the
/// file's symbol map) to a forex [`Pair`]. Returning `None` means
/// "skip this record".
pub type SymbolMapper = Arc<dyn Fn(u32, Option<&str>) -> Option<Pair> + Send + Sync>;

/// Build a [`SymbolMapper`] from a plain `Fn`.
pub fn mapper_from_fn<F>(f: F) -> SymbolMapper
where
    F: Fn(u32, Option<&str>) -> Option<Pair> + Send + Sync + 'static,
{
    Arc::new(f)
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Outcome counters returned by the pump.
#[derive(Debug, Default, Clone)]
pub struct PumpStats {
    /// Total records decoded (any rtype).
    pub records_decoded: usize,
    /// MBP-1 records seen (subset of `records_decoded`).
    pub mbp1_records: usize,
    /// Ticks successfully forwarded into the swarm.
    pub ticks_emitted: usize,
    /// Records for which the mapper returned `None` (no `Pair` mapping).
    pub unmapped: usize,
    /// `feed_tick` errors (typically: no monitor for that pair).
    pub feed_errors: usize,
}

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

/// Translate a DBN `Mbp1Msg` to a swarm [`Tick`]. Prices are converted
/// from DBN's fixed-point (1 unit = 1e-9) to floats; the timestamp comes
/// from `ts_recv` (nanos since UNIX epoch) and is downscaled to ms.
pub fn mbp1_to_tick(msg: &Mbp1Msg, pair: Pair) -> Tick {
    let level = &msg.levels[0];
    let bid = level.bid_px as f64 / FIXED_PRICE_SCALE as f64;
    let ask = level.ask_px as f64 / FIXED_PRICE_SCALE as f64;
    let ts_ms = (msg.ts_recv / 1_000_000) as i64;
    Tick::new(pair, bid, ask, ts_ms)
}

// ---------------------------------------------------------------------------
// Pump
// ---------------------------------------------------------------------------

/// Decode the DBN file at `path` and pump its MBP-1 records into the
/// swarm via `feeder`. Returns when the file is fully consumed *or* the
/// `token` is cancelled (whichever comes first).
///
/// `.dbn` and `.dbn.zst` are both supported (selected by extension).
pub async fn pump_dbn_file(
    feeder: SwarmFeeder,
    path: impl Into<PathBuf>,
    mapper: SymbolMapper,
    pacing: Pacing,
    token: CancellationToken,
) -> Result<PumpStats> {
    let path: PathBuf = path.into();
    let is_zstd = path.extension().and_then(|e| e.to_str()) == Some("zst");

    if is_zstd {
        let mut decoder = AsyncDbnDecoder::from_zstd_file(&path)
            .await
            .with_context(|| format!("opening zstd DBN file {}", path.display()))?;
        pump_decoder(&mut decoder, &feeder, &mapper, pacing, token).await
    } else {
        let mut decoder = AsyncDbnDecoder::from_file(&path)
            .await
            .with_context(|| format!("opening DBN file {}", path.display()))?;
        pump_decoder(&mut decoder, &feeder, &mapper, pacing, token).await
    }
}

/// Generic over the underlying reader. The dbn crate's `Decoder<R>` impls
/// require `R: AsyncReadExt + Unpin`, so we lift the same bound here and
/// reuse the loop body for both the plain-file and zstd-file branches.
async fn pump_decoder<R>(
    decoder: &mut AsyncDbnDecoder<R>,
    feeder: &SwarmFeeder,
    mapper: &SymbolMapper,
    pacing: Pacing,
    token: CancellationToken,
) -> Result<PumpStats>
where
    R: AsyncReadExt + Unpin,
{
    // Try to derive a date-based symbol map from the file's metadata. If
    // the metadata doesn't carry a usable date (or no mappings), we fall
    // back to a `None` map and the caller's mapper has to work from
    // `instrument_id` alone.
    let symbol_map: Option<PitSymbolMap> = {
        let metadata = decoder.metadata();
        let start_ns = metadata.start;
        time::OffsetDateTime::from_unix_timestamp_nanos(start_ns as i128)
            .ok()
            .and_then(|odt| metadata.symbol_map_for_date(odt.date()).ok())
    };

    let mut stats = PumpStats::default();
    let mut base_ts: Option<u64> = None;
    let wall_start = Instant::now();

    loop {
        // Decode the next Mbp1Msg, watching for cancellation. We clone
        // the message out so the decoder borrow ends before we sleep or
        // forward — the loop body needs to be reentrant.
        let msg: Mbp1Msg = tokio::select! {
            biased;
            _ = token.cancelled() => {
                tracing::info!(
                    records_decoded = stats.records_decoded,
                    "databento pump cancelled"
                );
                return Ok(stats);
            }
            res = decoder.decode_record::<Mbp1Msg>() => {
                match res? {
                    Some(msg) => msg.clone(),
                    None => break,
                }
            }
        };

        stats.records_decoded += 1;
        stats.mbp1_records += 1;

        let symbol_str = symbol_map
            .as_ref()
            .and_then(|m| m.get(msg.hd.instrument_id))
            .map(String::as_str);

        let Some(pair) = mapper(msg.hd.instrument_id, symbol_str) else {
            stats.unmapped += 1;
            continue;
        };

        // Pacing: only the realtime branch sleeps.
        if let Pacing::Realtime { speed_factor } = pacing
            && speed_factor > 0.0
        {
            let base = *base_ts.get_or_insert(msg.ts_recv);
            let delta_ns = msg.ts_recv.saturating_sub(base) as f64;
            let target_wall = Duration::from_nanos((delta_ns / speed_factor) as u64);
            let elapsed = wall_start.elapsed();
            if target_wall > elapsed {
                let to_sleep = target_wall - elapsed;
                tokio::select! {
                    biased;
                    _ = token.cancelled() => return Ok(stats),
                    _ = tokio::time::sleep(to_sleep) => {}
                }
            }
        }

        let tick = mbp1_to_tick(&msg, pair);
        match feeder.feed_tick(tick).await {
            Ok(()) => stats.ticks_emitted += 1,
            Err(err) => {
                tracing::warn!(error = %err, "databento feed_tick failed");
                stats.feed_errors += 1;
            }
        }
    }

    Ok(stats)
}

// ---------------------------------------------------------------------------
// Spawn helper — uses the swarm's TaskTracker + child cancellation token
// ---------------------------------------------------------------------------

/// Spawn the pump on the swarm's `TaskTracker` with a child of its root
/// `CancellationToken`. The returned `JoinHandle` resolves to the final
/// `PumpStats` once the file is exhausted (or the token is cancelled by
/// `Swarm::shutdown()`).
pub fn spawn_dbn_pump(
    swarm: &Swarm,
    path: impl Into<PathBuf>,
    mapper: SymbolMapper,
    pacing: Pacing,
) -> JoinHandle<Result<PumpStats>> {
    let token = swarm.cancellation_token().child_token();
    let feeder = swarm.feeder();
    let path = path.into();
    swarm
        .task_tracker()
        .spawn(async move { pump_dbn_file(feeder, path, mapper, pacing, token).await })
}
