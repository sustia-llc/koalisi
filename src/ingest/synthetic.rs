//! Seeded, credential-free synthetic [`DataSource`]s matching the two real
//! downstream driver shapes of the coalition semantic-layer roadmap (koalisi
//! issue #8). Both are fully deterministic (inline `SplitMix64`, no `rand`
//! dependency) so tests and examples reproduce byte-for-byte from a seed.
//!
//! # Driver shapes (from issue #8)
//!
//! - **NEST** (Nested Energy System Transitions) — time-ordered,
//!   *multi-resolution numeric* streams with a large temporal-resolution gap
//!   (5-year `MESSAGEix` planning periods ↔ hourly `URBANopt` load), driving
//!   downscaling/upscaling convergence loops. Modelled by
//!   [`MultiResolutionSource`]: N numeric series at independent step sizes,
//!   merged into one globally time-ordered stream.
//! - **tauhokohoko** (Indigenous data governance) — streaming *ecological
//!   sensor* data suited to SPRT-style hypothesis testing over an append-only
//!   event store. Modelled by [`SensorEventSource`]: M sensors at a fixed
//!   cadence, each with a baseline mean plus Gaussian noise and a configurable
//!   changepoint (the H0-baseline-vs-H1-shifted-mean shape SPRT consumes). No
//!   SPRT is implemented here — that belongs to the downstream driver; the
//!   fixture only produces the stream.

use std::collections::VecDeque;
use std::future::{Future, ready};

use anyhow::Result;

use super::sample::Sample;
use super::source::DataSource;

// ---------------------------------------------------------------------------
// SplitMix64 — inline deterministic PRNG (repo convention; no `rand` dep)
// ---------------------------------------------------------------------------

/// `SplitMix64` — the reference constant-schedule PRNG. Seeded per series so the
/// merged stream is reproducible from a single top-level seed.
#[derive(Debug, Clone)]
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)` (top 53 bits → f64 mantissa).
    // The two casts feed exactly 53 bits into an f64 — the standard construction;
    // "precision loss" is the intended low-bit truncation, not an error.
    #[allow(clippy::cast_precision_loss)]
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// One standard-normal draw via Box–Muller (chosen over sum-of-uniforms for
    /// exact unit variance). `u1` is shifted into `(0, 1]` so `ln` is finite.
    fn next_gaussian(&mut self) -> f64 {
        let u1 = 1.0 - self.next_f64();
        let u2 = self.next_f64();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

// ---------------------------------------------------------------------------
// NEST-shaped: multi-resolution numeric series
// ---------------------------------------------------------------------------

/// One numeric observation from a [`MultiResolutionSource`] series.
#[derive(Debug, Clone, PartialEq)]
pub struct NumericSample {
    /// Which series this value belongs to (the routing key).
    pub series: String,
    pub value: f64,
    /// Unix epoch milliseconds.
    pub timestamp_ms: i64,
}

/// A [`NumericSample`]'s distilled "latest" view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumericView {
    pub value: f64,
    pub timestamp_ms: i64,
}

impl Sample for NumericSample {
    type Key = String;
    type View = NumericView;

    fn key(&self) -> &String {
        &self.series
    }

    fn timestamp_ms(&self) -> i64 {
        self.timestamp_ms
    }

    fn view(&self) -> NumericView {
        NumericView {
            value: self.value,
            timestamp_ms: self.timestamp_ms,
        }
    }
}

/// Configuration for one numeric series.
///
/// The signal is `base + amplitude·sin(2π·i/period) + jitter`, where `jitter` is
/// a small seeded perturbation (±2.5 % of `amplitude`). `step_ms` sets this
/// series' temporal resolution — using very different `step_ms` across series is
/// exactly the NEST resolution-gap shape.
#[derive(Debug, Clone)]
pub struct SeriesSpec {
    /// Series key (routing key of the emitted samples).
    pub key: String,
    /// Timestamp of the first sample (ms).
    pub start_ms: i64,
    /// Spacing between successive samples (ms) — this series' resolution.
    pub step_ms: i64,
    /// Number of samples to emit.
    pub count: usize,
    /// Signal baseline.
    pub base: f64,
    /// Sinusoid amplitude.
    pub amplitude: f64,
    /// Number of samples spanning one full sine period. (Added beyond issue #8's
    /// listed field set so the smooth signal has a well-defined wavelength.)
    pub period: usize,
}

/// A NEST-shaped source: several numeric series at independent resolutions,
/// merged into a single globally time-ordered stream.
///
/// Ties on `timestamp_ms` are broken deterministically by series key, so the
/// merged order is a total order and reproducible from the seed.
pub struct MultiResolutionSource {
    queue: VecDeque<NumericSample>,
}

impl MultiResolutionSource {
    /// Build the merged stream. Each series gets its own PRNG substream derived
    /// from `seed` and the series index, so adding/reordering series does not
    /// perturb the others' values.
    // Casts are on bounded loop indices (`i` < `count`, `si` small): the `i64`
    // multiply cannot realistically wrap and the `f64` uses lose no meaningful
    // precision at fixture scale.
    #[must_use]
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_wrap)]
    pub fn new(specs: &[SeriesSpec], seed: u64) -> Self {
        let mut all: Vec<NumericSample> = Vec::new();
        for (si, spec) in specs.iter().enumerate() {
            let mut rng = SplitMix64::new(seed ^ (si as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let period = spec.period.max(1);
            for i in 0..spec.count {
                let ts = spec.start_ms + (i as i64) * spec.step_ms;
                let phase = std::f64::consts::TAU * (i as f64) / (period as f64);
                let jitter = (rng.next_f64() - 0.5) * spec.amplitude * 0.05;
                let value = spec.base + spec.amplitude * phase.sin() + jitter;
                all.push(NumericSample {
                    series: spec.key.clone(),
                    value,
                    timestamp_ms: ts,
                });
            }
        }
        // Global timestamp order; deterministic tie-break by series key.
        all.sort_by(|a, b| {
            a.timestamp_ms
                .cmp(&b.timestamp_ms)
                .then_with(|| a.series.cmp(&b.series))
        });
        Self { queue: all.into() }
    }

    /// Number of samples not yet yielded.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.queue.len()
    }
}

impl DataSource for MultiResolutionSource {
    type S = NumericSample;

    fn next_sample(&mut self) -> impl Future<Output = Result<Option<NumericSample>>> + Send {
        ready(Ok(self.queue.pop_front()))
    }
}

// ---------------------------------------------------------------------------
// tauhokohoko-shaped: sensor-event streams with a changepoint
// ---------------------------------------------------------------------------

/// One reading from a [`SensorEventSource`] sensor.
#[derive(Debug, Clone, PartialEq)]
pub struct SensorEvent {
    /// Which sensor produced the reading (the routing key).
    pub sensor: String,
    pub reading: f64,
    /// Unix epoch milliseconds.
    pub timestamp_ms: i64,
}

/// A [`SensorEvent`]'s distilled "latest" view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SensorReading {
    pub reading: f64,
    pub timestamp_ms: i64,
}

impl Sample for SensorEvent {
    type Key = String;
    type View = SensorReading;

    fn key(&self) -> &String {
        &self.sensor
    }

    fn timestamp_ms(&self) -> i64 {
        self.timestamp_ms
    }

    fn view(&self) -> SensorReading {
        SensorReading {
            reading: self.reading,
            timestamp_ms: self.timestamp_ms,
        }
    }
}

/// Configuration for one sensor.
///
/// Readings are `mean(i) + noise_sd·N(0,1)`, where `mean(i)` is `baseline_mean`
/// before the changepoint and `baseline_mean + shift` from sample index
/// `shift_at` onward — the H0-baseline / H1-shifted-mean shape SPRT tests.
#[derive(Debug, Clone)]
pub struct SensorSpec {
    /// Sensor key (routing key of the emitted samples).
    pub sensor: String,
    /// Mean before the changepoint.
    pub baseline_mean: f64,
    /// Standard deviation of the additive Gaussian noise.
    pub noise_sd: f64,
    /// Sample index at/after which the mean shifts. Use a value `>= count` (e.g.
    /// `usize::MAX`) for "no changepoint".
    pub shift_at: usize,
    /// Additive shift applied to the mean from `shift_at` onward.
    pub shift: f64,
}

/// A tauhokohoko-shaped source: M sensors emitting at a shared fixed cadence,
/// each with per-sensor seeded Gaussian noise and an optional mean changepoint.
///
/// All sensors share `start_ms`/`step_ms`/`count`; samples are merged in
/// timestamp order with ties broken deterministically by sensor key.
pub struct SensorEventSource {
    queue: VecDeque<SensorEvent>,
}

impl SensorEventSource {
    /// Build the merged sensor stream. `count` readings per sensor at
    /// `start_ms + i·step_ms`. Each sensor gets its own PRNG substream derived
    /// from `seed` and the sensor index.
    // The `i64` cast is on a bounded loop index (`i` < `count`) and cannot
    // realistically wrap at fixture scale.
    #[must_use]
    #[allow(clippy::cast_possible_wrap)]
    pub fn new(specs: &[SensorSpec], count: usize, start_ms: i64, step_ms: i64, seed: u64) -> Self {
        let mut all: Vec<SensorEvent> = Vec::new();
        for (si, spec) in specs.iter().enumerate() {
            let mut rng = SplitMix64::new(seed ^ (si as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            for i in 0..count {
                let ts = start_ms + (i as i64) * step_ms;
                let mean = spec.baseline_mean + if i >= spec.shift_at { spec.shift } else { 0.0 };
                let reading = mean + spec.noise_sd * rng.next_gaussian();
                all.push(SensorEvent {
                    sensor: spec.sensor.clone(),
                    reading,
                    timestamp_ms: ts,
                });
            }
        }
        all.sort_by(|a, b| {
            a.timestamp_ms
                .cmp(&b.timestamp_ms)
                .then_with(|| a.sensor.cmp(&b.sensor))
        });
        Self { queue: all.into() }
    }

    /// Number of readings not yet yielded.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.queue.len()
    }
}

impl DataSource for SensorEventSource {
    type S = SensorEvent;

    fn next_sample(&mut self) -> impl Future<Output = Result<Option<SensorEvent>>> + Send {
        ready(Ok(self.queue.pop_front()))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Drain a source into a Vec (both fixtures are in-memory, so this never
    /// blocks).
    async fn drain<D: DataSource>(mut source: D) -> Vec<D::S> {
        let mut out = Vec::new();
        while let Some(s) = source.next_sample().await.unwrap() {
            out.push(s);
        }
        out
    }

    #[tokio::test]
    async fn multi_resolution_merges_in_global_timestamp_order() {
        // An hourly series and a much coarser 5-hourly series over the same span.
        let hourly = SeriesSpec {
            key: "hourly".into(),
            start_ms: 0,
            step_ms: 3_600_000, // 1 hour
            count: 10,
            base: 100.0,
            amplitude: 10.0,
            period: 24,
        };
        let coarse = SeriesSpec {
            key: "planning".into(),
            start_ms: 0,
            step_ms: 18_000_000, // 5 hours — the resolution gap
            count: 3,
            base: 50.0,
            amplitude: 5.0,
            period: 4,
        };
        let src = MultiResolutionSource::new(&[hourly, coarse], 42);
        assert_eq!(src.remaining(), 13, "10 + 3 samples queued");

        let samples = drain(src).await;
        assert_eq!(
            samples.len(),
            13,
            "count exhaustion yields every sample once"
        );

        // Globally non-decreasing timestamps.
        for w in samples.windows(2) {
            assert!(
                w[0].timestamp_ms <= w[1].timestamp_ms,
                "merged stream must be time-ordered: {} then {}",
                w[0].timestamp_ms,
                w[1].timestamp_ms
            );
        }
        // Both resolutions actually interleave (both keys present).
        assert!(samples.iter().any(|s| s.series == "hourly"));
        assert!(samples.iter().any(|s| s.series == "planning"));
        // At t=0 both series emit; tie broken by key ("hourly" < "planning").
        assert_eq!(samples[0].series, "hourly");
        assert_eq!(samples[1].series, "planning");
    }

    #[tokio::test]
    async fn multi_resolution_is_deterministic_for_same_seed() {
        let spec = || SeriesSpec {
            key: "s".into(),
            start_ms: 0,
            step_ms: 1_000,
            count: 20,
            base: 1.0,
            amplitude: 0.5,
            period: 8,
        };
        let a = drain(MultiResolutionSource::new(&[spec()], 7)).await;
        let b = drain(MultiResolutionSource::new(&[spec()], 7)).await;
        assert_eq!(a, b, "same seed ⇒ identical stream");

        let c = drain(MultiResolutionSource::new(&[spec()], 8)).await;
        assert_ne!(a, c, "different seed ⇒ different jitter");
    }

    #[tokio::test]
    async fn sensor_source_cadence_and_repeatability() {
        let spec = SensorSpec {
            sensor: "temp".into(),
            baseline_mean: 20.0,
            noise_sd: 0.3,
            shift_at: usize::MAX, // no changepoint
            shift: 0.0,
        };
        let a = drain(SensorEventSource::new(
            std::slice::from_ref(&spec),
            8,
            1_000,
            500,
            99,
        ))
        .await;
        assert_eq!(a.len(), 8);
        // Fixed cadence: timestamps are start + i*step.
        for (i, ev) in a.iter().enumerate() {
            assert_eq!(ev.timestamp_ms, 1_000 + (i as i64) * 500);
        }
        let b = drain(SensorEventSource::new(&[spec], 8, 1_000, 500, 99)).await;
        assert_eq!(a, b, "same seed ⇒ identical readings");
    }

    #[tokio::test]
    async fn sensor_changepoint_shifts_the_mean() {
        let n = 400usize;
        let shift_at = 200usize;
        let shift = 5.0;
        let spec = SensorSpec {
            sensor: "salinity".into(),
            baseline_mean: 10.0,
            noise_sd: 0.5,
            shift_at,
            shift,
        };
        let evs = drain(SensorEventSource::new(&[spec], n, 0, 1_000, 2024)).await;
        assert_eq!(evs.len(), n);

        let mean_before: f64 =
            evs[..shift_at].iter().map(|e| e.reading).sum::<f64>() / shift_at as f64;
        let mean_after: f64 =
            evs[shift_at..].iter().map(|e| e.reading).sum::<f64>() / (n - shift_at) as f64;
        let observed = mean_after - mean_before;
        assert!(
            (observed - shift).abs() < 0.3,
            "changepoint should shift the mean by ~{shift}, observed {observed}"
        );
    }

    #[tokio::test]
    async fn sensor_streams_merge_across_sensors_in_time_order() {
        let a = SensorSpec {
            sensor: "a".into(),
            baseline_mean: 1.0,
            noise_sd: 0.1,
            shift_at: usize::MAX,
            shift: 0.0,
        };
        let b = SensorSpec {
            sensor: "b".into(),
            baseline_mean: 2.0,
            noise_sd: 0.1,
            shift_at: usize::MAX,
            shift: 0.0,
        };
        let evs = drain(SensorEventSource::new(&[a, b], 5, 0, 100, 1)).await;
        assert_eq!(evs.len(), 10);
        for w in evs.windows(2) {
            assert!(w[0].timestamp_ms <= w[1].timestamp_ms);
        }
    }
}
