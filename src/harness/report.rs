//! Report helpers: order statistics over `f64` samples, pairwise superiority
//! counts, the per-seed and summary markdown tables, and the verdict type.

use std::fmt;

use super::battery::BatteryResult;

/// The `p`-th percentile (`p` in `0..=100`, clamped) of an ascending-sorted
/// slice by linear interpolation between the two neighbouring ranks. An empty
/// slice gives `NaN`.
#[must_use]
pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let last = sorted.len() - 1;
    let rank = (p / 100.0).clamp(0.0, 1.0) * last as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    let frac = rank - lo as f64;
    sorted[lo] + (sorted[hi] - sorted[lo]) * frac
}

/// The median and interquartile range (`p75 - p25`) of `values`, sorted
/// internally by `total_cmp`. An empty slice gives `(NaN, NaN)`.
#[must_use]
pub fn median_iqr(values: &[f64]) -> (f64, f64) {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    (
        percentile(&sorted, 50.0),
        percentile(&sorted, 75.0) - percentile(&sorted, 25.0),
    )
}

/// The number of positions `i` with `a[i] > b[i]` (strict), over the first
/// `min(a.len(), b.len())` positions.
#[must_use]
pub fn superior_count(a: &[f64], b: &[f64]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x > y).count()
}

/// Print one battery's per-seed rows as a markdown table: seed, pool size,
/// completion rate, mean coverage efficiency, primary, churn. Latency is not a
/// column.
pub fn print_per_seed_table(result: &BatteryResult) {
    println!("### {} — per seed", result.label);
    println!();
    println!("| seed | n | completion | mean_cov_eff | primary | churn |");
    println!("|---:|---:|---:|---:|---:|---:|");
    for r in &result.per_seed {
        println!(
            "| {} | {} | {:.4} | {:.4} | {:.4} | {} |",
            r.seed, r.n, r.completion_rate, r.mean_cov_eff, r.primary, r.churn
        );
    }
    println!();
}

/// Print one summary row per battery as a markdown table: arm label, seed
/// count, median completion, median mean coverage efficiency, median and IQR
/// of primary, total churn, decision count, and — as the last column — the
/// p50 decision latency in microseconds.
pub fn print_summary(results: &[BatteryResult]) {
    println!("### summary");
    println!();
    println!(
        "| arm | seeds | completion (median) | mean_cov_eff (median) | primary (median) | primary (IQR) | churn (total) | decisions | latency p50 µs |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|");
    for r in results {
        let completion: Vec<f64> = r.per_seed.iter().map(|s| s.completion_rate).collect();
        let cov_eff: Vec<f64> = r.per_seed.iter().map(|s| s.mean_cov_eff).collect();
        let primary: Vec<f64> = r.per_seed.iter().map(|s| s.primary).collect();
        let churn: usize = r.per_seed.iter().map(|s| s.churn).sum();
        let (primary_median, primary_iqr) = median_iqr(&primary);
        let (latency_p50, _) = median_iqr(&r.latencies_us);
        println!(
            "| {} | {} | {:.4} | {:.4} | {:.4} | {:.4} | {} | {} | {:.3} |",
            r.label,
            r.per_seed.len(),
            median_iqr(&completion).0,
            median_iqr(&cov_eff).0,
            primary_median,
            primary_iqr,
            churn,
            r.latencies_us.len(),
            latency_p50
        );
    }
    println!();
}

/// A registration's verdict with its qualifier: validated, falsified, or an
/// invalid run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The pre-registered claim held; the qualifier names the claim.
    Validated(String),
    /// The pre-registered claim failed; the qualifier names the claim.
    Falsified(String),
    /// A gate failed before a verdict could be read; the qualifier names the
    /// gate.
    RunInvalid(String),
}

impl fmt::Display for Verdict {
    /// Renders as `VERDICT: \`VALIDATED (q)\``, `VERDICT: \`FALSIFIED (q)\`` or
    /// `VERDICT: \`RUN-INVALID (q)\`` for qualifier `q`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Verdict::Validated(q) => write!(f, "VERDICT: `VALIDATED ({q})`"),
            Verdict::Falsified(q) => write!(f, "VERDICT: `FALSIFIED ({q})`"),
            Verdict::RunInvalid(q) => write!(f, "VERDICT: `RUN-INVALID ({q})`"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_interpolates_linearly() {
        let sorted = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(percentile(&sorted, 0.0), 1.0, "p0");
        assert_eq!(percentile(&sorted, 50.0), 2.5, "p50");
        assert_eq!(percentile(&sorted, 100.0), 4.0, "p100");
        assert!(percentile(&[], 50.0).is_nan(), "empty slice must give NaN");
    }

    #[test]
    fn median_iqr_over_one_to_eight() {
        let values: Vec<f64> = (1..=8).map(f64::from).collect();
        let (median, iqr) = median_iqr(&values);
        assert_eq!(median, 4.5, "median of 1..=8");
        assert_eq!(iqr, 3.5, "p75 - p25 of 1..=8 (6.25 - 2.75)");
    }

    #[test]
    fn median_iqr_is_order_independent() {
        let shuffled = [8.0, 1.0, 5.0, 3.0, 7.0, 2.0, 6.0, 4.0];
        assert_eq!(median_iqr(&shuffled), (4.5, 3.5));
    }

    #[test]
    fn superior_count_is_strict_over_min_length() {
        let a = [1.0, 2.0, 3.0];
        let b = [1.0, 1.0, 4.0];
        assert_eq!(
            superior_count(&a, &b),
            1,
            "only 2 > 1 counts; 1 == 1 does not"
        );
        assert_eq!(
            superior_count(&a, &b[..2]),
            1,
            "length mismatch: min length"
        );
        assert_eq!(superior_count(&a[..1], &b), 0);
    }

    #[test]
    fn verdict_display_format() {
        assert_eq!(
            Verdict::Validated("x".into()).to_string(),
            "VERDICT: `VALIDATED (x)`"
        );
        assert_eq!(
            Verdict::Falsified("y".into()).to_string(),
            "VERDICT: `FALSIFIED (y)`"
        );
        assert_eq!(
            Verdict::RunInvalid("z".into()).to_string(),
            "VERDICT: `RUN-INVALID (z)`"
        );
    }
}
