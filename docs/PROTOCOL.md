# koalisi run protocol

What every registered decision-strategy experiment follows, from the question
to the immutable report. The index of runs is [`README.md`](README.md); the raw
outputs and the re-pin drift check are [`runs/README.md`](runs/README.md).

## 1. Sequence of a registration

1. **Design-lock on the issue.** The owner posts the arm, the koalisi-side
   candidate it reads against, the seed block, and every upstream extension
   the arm needs — before any pre-registration text exists.
2. **Pin-first.** A dependency re-pin the registration needs lands in its own
   PR, the pin its own commit, so the registration is born on the final pins
   and a frozen-output drift is attributable to the pin alone.
3. **Pre-registration committed before implementation.** The document names
   the hypothesis, the arms and controls, the bar (ratio and paired-superiority
   count), the gates whose failure makes the run `RUN-INVALID`, the seed block,
   and the pre-committed scoped claims for each outcome. Falsification is a
   legitimate result; nothing is tuned to flip it.
4. **Implementation** through `src/` and one `[[example]]` per registration
   (`examples/k7/k7_<n>.rs`). A registration's binary changes only through
   `src/`; a later registration's PR never edits an earlier one's example.
5. **Three-lens review before the official run** — correctness,
   registration-conformance, modelling-semantics. Every finding is applied or
   owner-adjudicated on the issue; a finding that changes the registration is
   an amendment.
6. **Amendments are pre-verdict only**, appended to the pre-registration and
   posted on the issue before any affected code runs. After the official run
   the document is immutable.
7. **The official run** is one serial run on a quiet machine (§2). Its raw
   output is committed under `docs/runs/`.
8. **The report** is committed with the run: the verdict line exactly as the
   binary printed it, the bar it was scored against, every gate outcome, the
   mechanism measured, and a numbered ledger of what the run established. It
   is never rewritten.

## 2. Running anything whose output feeds a verdict

- **Serial, on a quiet machine.** `pgrep -c 'cargo|rustc'` prints 0 before the
  run starts; no suite, build or second battery runs alongside. Latency is
  excluded from comparisons as a metric, but a latency criterion inside the
  binary turns timing noise into a verdict flip, so load is not a neutral
  variable.
- **Release build.** Latency criteria are meaningful only on optimised
  builds.
- **Both sides of any comparison run under the same load**, never one side
  concurrent with a suite and the other alone.
- **Diff with the latency column stripped**, not by grepping for the word
  "latency". Table rows carry latency as an unlabelled trailing column; strip
  the trailing `| <float> |` from both sides and diff again. An empty diff is
  the X-battery PASS; a non-empty one is a recorded finding, never a
  re-baseline.

## 3. Gates every registration carries

| gate | what it checks |
|---|---|
| X-battery | the frozen archive output is byte-identical to its committed log after the latency column is stripped |
| X-identity | at the identity configuration the new arm reproduces its reference arm bit-for-bit on acts and raw score bits |
| S-determinism | same seed ⇒ same result, asserted per seed |
| registration-specific `S-*` gates | named in the pre-registration; `RUN-INVALID` conditions, not disclosures |

## 4. Review

Every PR gets the reviewer pass before tag or merge — re-pins and docs-only
slices included. Every finding is applied or owner-adjudicated on the issue,
regardless of severity. Reviewer output is evidence, not verdict: each claim
is checked against the diff. The loop has a cap: review → fix → one more
review scoped to the fix → at most one more fix, which ships unreviewed and is
labelled so in the PR body.

## 5. Seeds

The ledger is in [`README.md`](README.md#seed-ledger). A registration names
its block in the design-lock; a reserved block is released only by an owner
decision on the registration's issue; a consumed block is never reused.

## 6. Naming

Lineages are `K<n>`, registrations `K<n>-<m>`; upstream `aif` extensions keep
tira's `ext-<k>`; issue numbers carry their repo prefix (`koa#`, tira `#`,
`cg#`).
