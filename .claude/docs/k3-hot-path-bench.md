# K3 hot-path bench — kameo baseline vs tokio::sync (#6)

_2026-07-02 · `examples/hot_path_bench.rs`, release build, same hardware · pre-registered
acceptance: **hot-path latency not regressed vs the kameo baseline**_

Both runs use the identical measurement logic and output format; only the runtime
plumbing differs (commit A of the K3 branch ran the bench on the kameo runtime
before the swap — the parity-by-commits discipline from K1/#4).

## Method

- **Alert round-trip** (K = 500): aligned-tick triple + one dislocating tick per
  iteration (the `triangular_arbitrage` choreography, hysteresis re-armed between
  iterations), timed from just before the dislocating `feed_tick` until the alert-bus
  listener receives the alert.
- **Ask round-trip** (K = 10 000): `Ping`-style ask to the coordinator.
- **Throughput**: 100 000 aligned ticks (no alerts), ticks/sec.
- Warm-up pass before each timed section; deterministic tick values.

## Results

| metric | kameo baseline | tokio::sync after | change |
|---|---:|---:|---|
| alert RTT median | 22.52 µs | 9.02 µs | −60% |
| alert RTT p95 | 37.96 µs | 16.08 µs | −58% |
| alert RTT p99 | 56.05 µs | 26.00 µs | −54% |
| ask RTT median | 7.62 µs | 7.48 µs | −2% |
| ask RTT p99 | 17.32 µs | 13.32 µs | −23% |
| throughput | 77 200 ticks/s | 120 229 ticks/s | +56% |

**Verdict: NOT REGRESSED — every metric improved.** The µs numbers are
machine-varying; the relative comparison is the acceptance artifact.

## Reproduce

```sh
cargo run --release --manifest-path Cargo.toml --target-dir /tmp/koalisi-target \
  --example hot_path_bench
```

(The kameo baseline is only reproducible from the pre-swap commit — main carries
the tokio runtime.)
