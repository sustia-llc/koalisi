# `docs/runs/` — committed battery outputs

One log per registered run, written once and never edited. A log is the exact
stdout of the run of record; the report in `docs/` quotes its verdict lines.

| log | binary | features | produced |
|---|---|---|---|
| `K4-archive.log` | `examples/strategy_comparison.rs` (Parts 1–11, the frozen K4 archive) | `decision,magnitude,process` | 2026-09-16 at `v0.32.0` pins (catgraph `v0.23.0` ×3, `aif-v0.13.0`) |
| `K7-<n>.log` | `examples/k7/k7_<n>.rs` | per the registration | with each K7 report |

## Producing the archive log

Release build, then one serial run of the binary with nothing else on the
machine (`pgrep -c 'cargo|rustc'` prints 0 first):

```sh
cargo build --release --features decision,magnitude,process --example strategy_comparison
target/release/examples/strategy_comparison > docs/runs/K4-archive.log
```

The run takes about 20 minutes and prints about 2100 lines.

## Drift check at a dependency re-pin

One fresh serial run on the pinned tree, diffed against the committed log with
the trailing latency column stripped from both sides:

```sh
target/release/examples/strategy_comparison > /tmp/K4-repin.log
sed -E 's/\| *[0-9]+\.[0-9]+ *\|$/|/' docs/runs/K4-archive.log > /tmp/a.log
sed -E 's/\| *[0-9]+\.[0-9]+ *\|$/|/' /tmp/K4-repin.log        > /tmp/b.log
diff /tmp/a.log /tmp/b.log
```

An empty diff is the X-battery PASS. Lines that survive the strip and report
latency or wall-clock time in prose are the standing exclusion; any other
surviving line is drift, recorded as a finding on the re-pin PR and never
re-baselined. The verdict lines are checked separately:

```sh
rg 'VERDICT|FALSIFIED|VALIDATED' docs/runs/K4-archive.log > /tmp/a.v
rg 'VERDICT|FALSIFIED|VALIDATED' /tmp/K4-repin.log        > /tmp/b.v
diff /tmp/a.v /tmp/b.v
```
