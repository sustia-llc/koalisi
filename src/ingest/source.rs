//! [`DataSource`] — a domain-neutral producer of time-ordered [`Sample`]s —
//! and the generic pump that routes a source's samples into per-key
//! [`SampleMonitor`](super::monitor::SampleMonitor)s.
//!
//! This is the domain-neutral ingestion layer (issue #8): [`Pacing`] and the
//! generic [`pump_source`] loop route any [`DataSource`]'s samples into
//! monitors by key.

use std::collections::HashMap;
use std::future::Future;
use std::hash::BuildHasher;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use super::monitor::SampleMonitorHandle;
use super::sample::Sample;

// ---------------------------------------------------------------------------
// Pacing
// ---------------------------------------------------------------------------

/// Replay pacing strategy for a [`pump_source`] run.
///
/// Domain-neutral replay pacing (introduced with the ingestion layer, issue #8).
#[derive(Debug, Clone, Copy, Default)]
pub enum Pacing {
    /// Pump every sample as fast as the source can produce it.
    #[default]
    Asap,
    /// Pace by [`Sample::timestamp_ms`] deltas. `speed_factor == 1.0` matches
    /// wall-clock time to sample time; `2.0` plays back at 2x speed; `0.5` at
    /// half speed. Non-positive values are treated as [`Pacing::Asap`].
    Realtime { speed_factor: f64 },
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Outcome counters returned by [`pump_source`]: routing outcomes only.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PumpStats {
    /// Samples successfully forwarded into a monitor via acknowledged `feed`.
    pub fed: usize,
    /// Samples whose key had no monitor in the router (dropped).
    pub dropped: usize,
}

// ---------------------------------------------------------------------------
// DataSource
// ---------------------------------------------------------------------------

/// A producer of time-ordered typed [`Sample`]s.
///
/// `next_sample` returns `Ok(None)` when the stream is exhausted. Samples are
/// expected to be emitted in non-decreasing [`Sample::timestamp_ms`] order (the
/// pump paces on that assumption); a source merging several series must merge
/// them into a single time-ordered stream itself.
///
/// # Dyn-incompatibility
///
/// `next_sample` uses `async fn` in trait position (an RPITIT desugaring), which
/// makes `DataSource` **not** object-safe — you cannot form `dyn DataSource`.
/// This is acceptable here: sources are consumed at generic call sites
/// ([`pump_source`] / [`spawn_source_pump`] are generic over `D: DataSource`),
/// never as trait objects, so monomorphisation applies and no `dyn` is needed.
pub trait DataSource: Send {
    /// The sample type this source produces.
    type S: Sample;

    /// Yield the next time-ordered sample; `Ok(None)` = stream exhausted.
    fn next_sample(&mut self) -> impl Future<Output = Result<Option<Self::S>>> + Send;
}

// ---------------------------------------------------------------------------
// Pump
// ---------------------------------------------------------------------------

/// Pump every sample from `source` into the monitor whose key matches, honouring
/// `pacing`. Returns when the source is exhausted *or* `token` is cancelled
/// (whichever comes first).
///
/// Routing is by [`Sample::key`] through `router`; a sample whose key is absent
/// is dropped and counted in [`PumpStats::dropped`]. Delivery uses the
/// acknowledged [`SampleMonitorHandle::feed`], so a returned `Ok` means every
/// forwarded sample was ingested and its update published.
///
/// # Errors
///
/// Returns an error if the source's `next_sample` fails, or if a routed
/// monitor's task has stopped (an acknowledged `feed` to a gone monitor is a
/// genuine failure, propagated rather than counted as a drop).
// The one `i64`→`f64` cast below is on a millisecond delta; realistic replay
// spans stay far within f64's 2^52 exactly-representable range.
#[allow(clippy::cast_precision_loss)]
pub async fn pump_source<D: DataSource, H: BuildHasher>(
    mut source: D,
    router: &HashMap<<D::S as Sample>::Key, SampleMonitorHandle<D::S>, H>,
    pacing: Pacing,
    token: CancellationToken,
) -> Result<PumpStats> {
    let mut stats = PumpStats::default();
    let mut base_ts: Option<i64> = None;
    let wall_start = Instant::now();

    loop {
        let sample = tokio::select! {
            biased;
            () = token.cancelled() => {
                tracing::info!(fed = stats.fed, dropped = stats.dropped, "pump_source cancelled");
                return Ok(stats);
            }
            res = source.next_sample() => match res? {
                Some(s) => s,
                None => break,
            }
        };

        // Pacing: only the realtime branch sleeps, keyed on timestamp_ms deltas.
        if let Pacing::Realtime { speed_factor } = pacing
            && speed_factor > 0.0
        {
            let base = *base_ts.get_or_insert(sample.timestamp_ms());
            let delta_ms = sample.timestamp_ms().saturating_sub(base).max(0) as f64;
            let target_wall = Duration::from_secs_f64(delta_ms / 1_000.0 / speed_factor);
            let elapsed = wall_start.elapsed();
            if target_wall > elapsed {
                // `target_wall > elapsed` was just checked, so this cannot underflow.
                let to_sleep = target_wall.saturating_sub(elapsed);
                tokio::select! {
                    biased;
                    () = token.cancelled() => return Ok(stats),
                    () = tokio::time::sleep(to_sleep) => {}
                }
            }
        }

        // `key()` borrows, so routing does not clone the key.
        if let Some(handle) = router.get(sample.key()) {
            // Acknowledged feed: an error here means the monitor task is
            // gone, which is a genuine failure — propagate it.
            handle.feed(sample).await?;
            stats.fed += 1;
        } else {
            stats.dropped += 1;
            tracing::warn!(key = %sample.key(), "pump_source: no monitor for key; dropping sample");
        }
    }

    Ok(stats)
}

// ---------------------------------------------------------------------------
// Spawn helper — runs the pump on a TaskTracker under a child token
// ---------------------------------------------------------------------------

/// Spawn [`pump_source`] on `tracker` under `token` (pass a child token so the
/// pump participates in the owner's shutdown). The returned `JoinHandle`
/// resolves to the final [`PumpStats`] once the source is exhausted (or the
/// token is cancelled).
pub fn spawn_source_pump<D: DataSource + 'static, H: BuildHasher + Send + Sync + 'static>(
    tracker: &TaskTracker,
    token: CancellationToken,
    source: D,
    router: HashMap<<D::S as Sample>::Key, SampleMonitorHandle<D::S>, H>,
    pacing: Pacing,
) -> JoinHandle<Result<PumpStats>> {
    tracker.spawn(async move { pump_source(source, &router, pacing, token).await })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::monitor::{SampleUpdate, spawn_sample_monitor};
    use crate::ingest::synthetic::{MultiResolutionSource, NumericSample, SeriesSpec};
    use tokio::sync::broadcast;

    fn one_series(key: &str, count: usize) -> SeriesSpec {
        SeriesSpec {
            key: key.into(),
            start_ms: 0,
            step_ms: 1_000,
            count,
            base: 1.0,
            amplitude: 0.5,
            period: 8,
        }
    }

    #[tokio::test]
    async fn pump_routes_by_key_and_counts_drops() {
        let tracker = TaskTracker::new();
        let token = CancellationToken::new();
        let (bus, _rx) = broadcast::channel::<SampleUpdate<NumericSample>>(64);

        // A router with a monitor for "a" only; "b" samples have nowhere to go.
        let mut router = HashMap::new();
        router.insert(
            "a".to_string(),
            spawn_sample_monitor::<NumericSample>(
                &tracker,
                token.child_token(),
                "a".into(),
                16,
                bus.clone(),
            ),
        );

        let source = MultiResolutionSource::new(&[one_series("a", 4), one_series("b", 3)], 5);
        let stats = pump_source(source, &router, Pacing::Asap, token.child_token())
            .await
            .unwrap();

        assert_eq!(stats.fed, 4, "only the 4 'a' samples route to a monitor");
        assert_eq!(stats.dropped, 3, "the 3 'b' samples are dropped");

        token.cancel();
        tracker.close();
        tracker.wait().await;
    }

    #[tokio::test]
    async fn pump_stops_early_on_cancellation() {
        let tracker = TaskTracker::new();
        let token = CancellationToken::new();
        let (bus, _rx) = broadcast::channel::<SampleUpdate<NumericSample>>(64);
        let mut router = HashMap::new();
        router.insert(
            "a".to_string(),
            spawn_sample_monitor::<NumericSample>(
                &tracker,
                token.child_token(),
                "a".into(),
                16,
                bus,
            ),
        );

        // A pre-cancelled token: the biased select takes the cancel branch on the
        // first iteration, so nothing is fed.
        let pump_token = token.child_token();
        pump_token.cancel();
        let source = MultiResolutionSource::new(&[one_series("a", 100)], 5);
        let stats = pump_source(source, &router, Pacing::Asap, pump_token)
            .await
            .unwrap();
        assert_eq!(stats, PumpStats { fed: 0, dropped: 0 });

        token.cancel();
        tracker.close();
        tracker.wait().await;
    }
}
