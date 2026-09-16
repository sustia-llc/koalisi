//! Deterministic draws for instance generation: a `SplitMix64` stream and the
//! two structured draws built on it (a permutation and a distinct-bit mask).

/// A `SplitMix64` stream: 64 bits of state advanced by the golden-ratio
/// increment and mixed by two multiply-xorshift rounds per output. Two streams
/// built from the same seed produce the same output sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// Build a stream whose first output is a function of `seed` alone.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Advance the state by the golden-ratio increment and return the mixed
    /// 64-bit output.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `[0, 1)`: the top 53 bits of the next output divided by
    /// `2^53`.
    pub fn next_unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A value in `[0, bound)` as the next output modulo `bound`, for
    /// `bound >= 1` (`bound == 0` is a division by zero and panics).
    pub fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }
}

/// A uniformly drawn permutation of `0..n` by Fisher–Yates: for `i` from
/// `n - 1` down to `1`, swap position `i` with position `below(i + 1)`. Draws
/// `n - 1` outputs for `n >= 2` and none otherwise.
#[must_use]
pub fn permutation(rng: &mut SplitMix64, n: usize) -> Vec<usize> {
    let mut out: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        let j = rng.below(i as u64 + 1) as usize;
        out.swap(i, j);
    }
    out
}

/// A mask of exactly `k` distinct bits, each below `universe`, by rejection
/// sampling: bit positions are drawn with `below(universe)` until `k` distinct
/// ones are held. Requires `k <= universe <= 32`; `k == 0` returns `0` without
/// drawing.
#[must_use]
pub fn distinct_bits(rng: &mut SplitMix64, k: u32, universe: u32) -> u32 {
    assert!(
        k <= universe && universe <= 32,
        "invariant: k <= universe <= 32 (k = {k}, universe = {universe})"
    );
    let mut held = 0u32;
    while held.count_ones() < k {
        held |= 1u32 << rng.below(u64::from(universe));
    }
    held
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_u64_matches_reference_vector_for_seed_zero() {
        let mut rng = SplitMix64::new(0);
        let observed = [rng.next_u64(), rng.next_u64(), rng.next_u64()];
        let expected = [
            0xE220_A839_7B1D_CDAF_u64,
            0x6E78_9E6A_A1B9_65F4,
            0x06C4_5D18_8009_454F,
        ];
        assert_eq!(
            observed, expected,
            "seed 0 must produce the SplitMix64 reference outputs"
        );
    }

    #[test]
    fn next_unit_and_below_stay_in_range() {
        let mut rng = SplitMix64::new(7);
        for _ in 0..10_000 {
            let u = rng.next_unit();
            assert!((0.0..1.0).contains(&u), "next_unit {u} outside [0, 1)");
            let b = rng.below(13);
            assert!(b < 13, "below(13) returned {b}");
        }
    }

    #[test]
    fn permutation_is_a_permutation_of_0_to_n_for_n_up_to_9() {
        for n in 0..=9usize {
            for seed in 0..50u64 {
                let mut rng = SplitMix64::new(seed);
                let mut p = permutation(&mut rng, n);
                assert_eq!(p.len(), n, "n = {n}, seed = {seed}: wrong length");
                p.sort_unstable();
                let identity: Vec<usize> = (0..n).collect();
                assert_eq!(p, identity, "n = {n}, seed = {seed}: not a permutation");
            }
        }
    }

    #[test]
    fn distinct_bits_has_exactly_k_bits_all_below_universe() {
        for universe in 1..=8u32 {
            for k in 1..=universe {
                for seed in 0..50u64 {
                    let mut rng = SplitMix64::new(seed);
                    let mask = distinct_bits(&mut rng, k, universe);
                    assert_eq!(
                        mask.count_ones(),
                        k,
                        "k = {k}, universe = {universe}, seed = {seed}: mask {mask:#b}"
                    );
                    assert!(
                        mask < (1u32 << universe),
                        "k = {k}, universe = {universe}, seed = {seed}: mask {mask:#b} has a bit >= {universe}"
                    );
                }
            }
        }
    }

    #[test]
    fn distinct_bits_zero_k_draws_nothing() {
        let mut rng = SplitMix64::new(3);
        let before = rng;
        assert_eq!(distinct_bits(&mut rng, 0, 8), 0);
        assert_eq!(rng, before, "k == 0 must not advance the stream");
    }
}
