# koalisi `docs/` — the A/B showcase trail

Every pre-registered decision-strategy experiment koalisi has run, indexed by
path. The experiments are the public record behind the README's A/B table; the
run protocol is [`PROTOCOL.md`](PROTOCOL.md).

## Layout

| path | holds | mutability |
|---|---|---|
| `docs/*.md` (this level) | the **K4 lineage**: one pre-registration + one report per registered run, plus three memos | registered docs are **immutable** once their run is recorded — results are appended, sections are never edited |
| [`PROTOCOL.md`](PROTOCOL.md) | the run protocol every registration follows | living |
| [`runs/`](runs/README.md) | raw battery outputs, one committed log per registered run (`runs/K4-archive.log` is the frozen K4 battery at `v0.32.0` pins); the drift-check recipe for dependency re-pins | a log is written once per run and never edited |
| `k7/` | the **K7 lineage** (`prereg-K7-<n>-<slug>.md` + `ab-report-K7-<n>-<slug>.md`), created with the first K7 registration | as above |
| this file | index, verdict trail, seed ledger | living |

The K4 lineage ran inside one binary, `examples/strategy_comparison.rs`
(Parts 1–11). That binary is **frozen as the K4 archive**: it changes only
through `src/`, and its only remaining job is the X-battery gate at dependency
re-pins (`runs/README.md`). The K7 lineage runs on `examples/gauntlet.rs` plus
one `[[example]]` per registration under `examples/k7/`, all plumbing shared
through `src/harness/`.

## K4 lineage — verdict trail

Seventeen rows. Thirteen have a prereg + report pair here; `v1/v2` was
registered on issue #7 (report only), `K1` and `K6` are a backend-parity run
and an optimisation re-run (report only), and `#54` is a decision memo with its
design note.

| # | Arm / question | Issue | Seeds | Verdict | Pre-registration | Report |
|---|---|---|---|---|---|---|
| v1 / v2 | magnitude vs scalar AIF | #7 | 0..30 | `FALSIFIED (latency)` under v1 criteria / `VALIDATED (B)` under the v2 amendment | on the issue | [`ab-report-K4-yamafaktory.md`](ab-report-K4-yamafaktory.md) (pre-K1 backend) |
| K1 | catgraph backend parity | #4 | 0..30 | byte-identical re-run | — | [`ab-report-K4-catgraph.md`](ab-report-K4-catgraph.md) |
| K6 | evaluator hot path | #14 | 0..30 | Path A missed; dual verdict unchanged | — | [`ab-report-K4-catgraph-evaluator.md`](ab-report-K4-catgraph-evaluator.md) |
| v3 | multimodal AIF | #43 | 0..30 | `FALSIFIED (multimodality)` — decision-equivalent to scalar | [`prereg-K4v3-multimodal-aif.md`](prereg-K4v3-multimodal-aif.md) | [`ab-report-K4v3-multimodal-aif.md`](ab-report-K4v3-multimodal-aif.md) |
| v4 | persistent AIF | #44 | 0..30 | `FALSIFIED (persistence)` — escapes v3's theorem, loses on performance | [`prereg-K4-v4-persistent-aif.md`](prereg-K4-v4-persistent-aif.md); baseline [`baseline-aif-scalar-scope-b.md`](baseline-aif-scalar-scope-b.md) | [`ab-report-K4-v4-persistent-aif.md`](ab-report-K4-v4-persistent-aif.md) |
| v5 | E1-only persistent AIF | #53 | 30..60 | **`VALIDATED (gap closed)`** 1.62× | [`prereg-K4-v5-e1-persistent-aif.md`](prereg-K4-v5-e1-persistent-aif.md) | [`ab-report-K4-v5-e1-persistent-aif.md`](ab-report-K4-v5-e1-persistent-aif.md) |
| #46 | feedback-weighted arm | #46 | 0..30 | `FALSIFIED (feedback)` | [`prereg-feedback-arm-k4.md`](prereg-feedback-arm-k4.md) | [`ab-report-feedback-arm-k4.md`](ab-report-feedback-arm-k4.md) |
| #48 | selective-base feedback | #48 | 0..30 | `PARTIAL (mechanism only)` | [`prereg-feedback-arm-k4-v2.md`](prereg-feedback-arm-k4-v2.md) | [`ab-report-feedback-arm-k4-v2.md`](ab-report-feedback-arm-k4-v2.md) |
| #54 | arm-choice memo, Steps 1–4 | #54 | 30..60 | **DECIDED B+D — FINAL** (magnitude is the demonstrated default) | design note [`per-bit-outcome-plumbing-design.md`](per-bit-outcome-plumbing-design.md) | memo [`k4-arm-choice-memo.md`](k4-arm-choice-memo.md) |
| v6 | never-evict E1 | #56 | 60..90 | `FALSIFIED (never-evict)` — churn is the E1 mechanism | [`prereg-K4-v6-never-evict.md`](prereg-K4-v6-never-evict.md) | [`ab-report-K4-v6-never-evict.md`](ab-report-K4-v6-never-evict.md) |
| EQ1 | battery v2 de-saturation | #61 | 120..150 | `FALSIFIED (de-saturation)`; lever 1 `RUN-INVALID` → #63 | [`prereg-K4-battery-v2.md`](prereg-K4-battery-v2.md) | [`ab-report-K4-battery-v2.md`](ab-report-K4-battery-v2.md) |
| #63 | corrected block-level routing | #63 | 180..210 | `FALSIFIED (block-routing)` | [`prereg-K4-routing-corrected.md`](prereg-K4-routing-corrected.md) | [`ab-report-K4-routing-corrected.md`](ab-report-K4-routing-corrected.md) |
| EQ3 | latency re-match | #69 | 210..240 | `FALSIFIED (latency re-match)` | [`prereg-K4-eq3-latency-rematch.md`](prereg-K4-eq3-latency-rematch.md) | [`ab-report-K4-eq3-latency-rematch.md`](ab-report-K4-eq3-latency-rematch.md) |
| EQ4 | typed roles | #72 | 240..270 | **`VALIDATED (typed roles)`** 3.61×, 30/30 | [`prereg-K4-eq4-typed-roles.md`](prereg-K4-eq4-typed-roles.md) | [`ab-report-K4-eq4-typed-roles.md`](ab-report-K4-eq4-typed-roles.md) |
| EQ5a | process-structured tasks | #76 | 270..300 | `FALSIFIED (process structure)` | [`prereg-K4-eq5a-process-structured.md`](prereg-K4-eq5a-process-structured.md) | [`ab-report-K4-eq5a-process-structured.md`](ab-report-K4-eq5a-process-structured.md) |
| #80 | residual process-specificity | #80 | 300..330 | `FALSIFIED (coverage proxy)` | [`prereg-K4-residual-process-specificity.md`](prereg-K4-residual-process-specificity.md) | [`ab-report-K4-residual-process-specificity.md`](ab-report-K4-residual-process-specificity.md) |
| EQ5b | typed two-engine | #78 | 330..360 | **`VALIDATED (two-engine)`** 1.2567×, 22/30 | [`prereg-K4-eq5b-typed-two-engine.md`](prereg-K4-eq5b-typed-two-engine.md) | [`ab-report-K4-eq5b-typed-two-engine.md`](ab-report-K4-eq5b-typed-two-engine.md) |

Read the report's mechanism section before quoting a verdict: EQ5b passes its
bar by 0.7 % and the win rides members structurally blind to the candidate;
EQ4's lever is retained role-diverse redundancy, not coverage routing.

## K7 lineage

No registration yet. The first, `K7-1`, is locked on its koalisi issue before
its pre-registration is written; rows land here as each report is recorded.

## Seed ledger

Each registered run consumes a fresh block of 30 seeds; a block is never
reused across registrations.

| block | status | consumer |
|---|---|---|
| 0..30 | consumed | v1/v2, K1, K6, v3, v4, #46, #48 |
| 30..60 | consumed | v5, #54 |
| 60..90 | consumed | v6 |
| 90..120 | **reserved — K7-1** | released by the owner 2026-09-16 |
| 120..150 | consumed | EQ1 |
| 150..180 | **reserved — unconsumed** | released only by an owner decision on the registration's issue |
| 180..210 | consumed | #63 |
| 210..240 | consumed | EQ3 |
| 240..270 | consumed | EQ4 |
| 270..300 | consumed | EQ5a |
| 300..330 | consumed | #80 |
| 330..360 | consumed | EQ5b |

## Immutability rule

A pre-registration is committed before any implementation; amendments are
appended to it, posted on the issue, and are pre-verdict only. A report is
committed with the run of record and never rewritten: a later finding is a
new section or a new registration. A falsified arm stays falsified.
