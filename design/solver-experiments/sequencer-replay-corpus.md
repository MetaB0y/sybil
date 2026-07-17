# Sequencer solver replay corpus

Date: 2026-07-17

Status: development infrastructure; synthetic evidence only.

## Question

Can solver iteration use compact, lifecycle-shaped inputs without retaining a
full private block witness or pretending an independent random generator is
real order flow?

## Design

Protocol schema 3 can pin a MessagePack `SolverReplayCorpusV1` by BLAKE3 hash.
Each case retains only solver-relevant state:

- sorted market IDs and next ID, without market names;
- the exact accepted-order sequence;
- MM constraint sequence with canonical sorted side maps;
- market-group membership without names;
- a stable case ID and small shape traits.

Accounts, balances, signatures, authorizations, rejections, fills, prices, and
solver output are omitted. Reconstruction validates canonical ordering,
constraint membership, unique IDs, and the ordinary `Problem` invariants before
any solver runs.

The first corpus contains 20 consecutive blocks from `sequencer-sim` standard
scenario seed `27182`. It is 1.3 MiB and pinned as:

`9aa3fd354b88876addb206859babf88769e74791c1e07ba7d62afade86bc124f`

## Capture limitation discovered

The simulation's extra `debug_verify_full` path rejects the flash makers'
negative outcome inventory during settlement, so the normal debug simulation
aborts on the first block. Corpus generation explicitly uses production-level
block verification and projects the problem before settlement; the normal
simulation constructor keeps full debug verification enabled.

This makes the solver inputs useful but prevents calling the corpus
full-account-valid simulation evidence. The account/MM inventory mismatch is a
separate sequencer-simulation issue and is not hidden by the corpus.

## Experiment SRC-001 — initial captured budgets

Protocol: `solver-sequencer-replay-development-v1`, 20 cases, captured budget
multiplier `1.0` plus the initially tried `0.01`, four solvers, 160 rows.

All 160 rows completed and verified, but maximum MM utilization was only 1.7%
even at `0.01`. The corpus exercised growing resting books and latency tails but
did not meaningfully exercise pacing. The `0.01` point was rejected.

It nevertheless exposed a Clarabel landing tail on a small late book: maximum
landed retained-objective gap `0.8492%`, while LP, RC-FW structural, and the
structural bundle matched on the slack-budget cases.

## Experiment SRC-002 — tight budget counterfactual

The tight multiplier was changed to `0.0001`; the captured `1.0` control was
retained. This is an explicit counterfactual, not replayed production policy.

Result: 160/160 records, no panic, solver failure, timeout, fingerprint
mismatch, or verifier-invalid row.

| Solver | Tight max budget use | P50 / P95 latency | Tight retained P95 / max | Notable tail |
|---|---:|---:|---:|---|
| LP-SLP | 1.000 | 7.88 / 12.06 ms | 0.6176% / 0.7382% | 3 iteration caps; minting-duality P95 0.096442% |
| RC structural | 0.978 | 12.93 / 22.58 ms | 0.3821% / 1.2783% | landing-loss P95 0.1602% |
| Bundle structural | 0.967 | 12.82 / 23.15 ms | 2.2198% / 7.7973% | landing-loss P95 2.2741%, max 7.1449% |
| Clarabel quasi | 0.996 | 10.82 / 19.39 ms | 0.1214% / 0.7905% | best tight retained tail, independent numerical path |

At the captured slack budgets, LP, RC structural, and bundle structural had
zero retained gap on all 20 cases; Clarabel retained P95/max were
`0.1158%/0.8492%`.

## Experiment SRC-003 — bundle absolute-gap sweep

The worst bundle row in SRC-002 was block 10 at the tight budget. Its
continuous retained objective was about `$0.0887`; the first certified gap was
`858,804` nanodollars. The configured `$0.001` (`1,000,000` nanodollar)
absolute tolerance therefore declared convergence after one atom even though
the landed result lost `7.1449%` of the continuous objective. This was a
scale-sensitive stopping failure, not evidence that the bundle method needed a
different landing algorithm.

The same 160-row protocol was rerun at five absolute tolerances. The table
reports only the 20 tight-budget bundle rows; all runs were 20/20 successful
and verifier-valid.

| Absolute gap | Retained P95 / max | Landing P95 / max | P95 latency |
|---:|---:|---:|---:|
| `1,000,000` nanos | 2.2198% / 7.7973% | 2.2741% / 7.1449% | 23.15 ms |
| `500,000` nanos | 0.3597% / 1.4121% | 0.4089% / 1.2341% | 23.16 ms |
| `250,000` nanos | 0.1174% / 0.2851% | 0.2656% / 0.2851% | 22.94 ms |
| `100,000` nanos | 0.1174% / 0.2851% | 0.1366% / 0.2851% | 25.27 ms |
| `1,000` nanos | 0.1197% / 0.7556% | 0.1768% / 1.0888% | 23.10 ms |

The sweep is non-monotone after integer landing: a tighter continuous
certificate can select a different degenerate face and land worse. The widest
good threshold was `100,000` nanos (`$0.0001`), so it was selected over the
more expensive and no-better `1,000`-nano setting.

As a broad control, both `1,000,000` and `100,000` nanos were run on all 630
rows of `solver-pacing-bundle-development-v2`. Bundle availability remained
125/126 and the retained-objective maximum remained `0.4936%`; P50/P95 runtime
was `87.60/586.09 ms` versus `80.50/520.60 ms`, which is timing-noise-level
evidence rather than a speed claim. Five of 126 bundle rows changed:

- two large-book landings improved by `0.00074%` and `0.00307%` retained
  objective;
- one changed by less than `0.000004%`;
- one gained an iteration-limit status but changed retained objective by only
  `0.000000056%`; and
- one large neutral book regressed by `0.01370%` after the stricter continuous
  target landed on a worse integer face.

That last row prevents calling the change a strict Pareto improvement. The
trade is accepted for the experimental bundle because it removes a repeatable
`7.8%` lifecycle-replay tail, preserves hard success and the broad quality
tail, and requires only a clearer default tolerance rather than a new
heuristic. The remaining non-monotone integer-face behavior stays visible as a
future landing target.

## Experiment SRC-004 — multi-regime replay

The single standard trajectory was too narrow for an extended research loop.
Protocol v2 retains it and adds three compact, deterministic sequencer
trajectories:

| Regime | Capture command suffix | Cases | Size | BLAKE3 |
|---|---|---:|---:|---|
| Standard flow | `--scenario standard --seed 27182` | 20 | 1.3 MiB | `9aa3fd354b88876addb206859babf88769e74791c1e07ba7d62afade86bc124f` |
| Grouped news | `--scenario election --seed 31415` | 20 | 336 KiB | `6852eaec4c04e06139abf7dc13e2ae3ce243827db642ede0b74292ff6dcc131d` |
| Mid-resolution | `--scenario two_events_with_leak --seed 16180` | 20 | 420 KiB | `fa6105778d3fd67b379ece92e5c3da7377ed866dc7f298edf1f3ba4b5414e59c` |
| Dense stress | `--scenario stress --seed 27183 --batches 12` | 12 | 3.1 MiB | `90f6ca24eb335a3b06a79c6052358f11bb2865f64d508b2e3f849ccc5bb4fbd5` |

The common command prefix was:

```bash
target/release/sybil-sim \
  --solver-corpus-output /tmp/<corpus>.msgpack
```

Together the 72 cases cover the following sequencer-boundary shapes:

| Regime | Markets | Orders min / P50 / max | Retail / MM P50 | MMs | Groups |
|---|---:|---:|---:|---:|---:|
| Standard flow | 10 | 256 / 798 / 1,330 | 718 / 80 | 2 | 0 |
| Grouped news | 3 | 66 / 243 / 404 | 219 / 24 | 2 | 1 |
| Mid-resolution | 4 | 93 / 313 / 461 | 285 / 28 | 2 | 1 |
| Dense stress | 30 | 1,547 / 3,936 / 5,748 | 3,576 / 360 | 3 | 0 |

An initial calibration gave every regime the standard `0.0001` tight-budget
multiplier. It was rejected for grouped news and mid-resolution because their
maximum MM utilization remained below `0.56`, providing a weak pacing signal.
Their tight multiplier is now `0.00005`; standard and dense stress remain at
`0.0001`. The captured `1.0` control remains in every regime. These tight
points are explicitly labeled counterfactuals.

The final protocol produced 576/576 structurally valid records. All landed
results except two Clarabel rows passed the protocol verifier:

- Mid-resolution blocks `b006` and `b007` at `0.00005` were reported
  `Solved`/converged by Clarabel, but integer landing exceeded five order
  limits and was retained as `verifier_invalid`.
- Dense stress was materially discriminating: LP-SLP hit its iteration cap in
  10/12 tight cases and RC structural in 4/12. Tight retained-objective
  P95/max gaps were `0.6674%/0.7465%` for LP-SLP,
  `0.3913%/0.3913%` for RC structural, `0.9721%/1.7013%` for bundle
  structural, and `2.6774%/5.9498%` for Clarabel.
- Mid-resolution exposed an integer-landing tail under tight budgets:
  retained P95/max were `0.2262%/4.5225%` for RC structural and
  `0.1400%/2.7991%` for bundle structural. LP-SLP remained at zero retained
  gap but hit one iteration cap.
- Grouped news reached maximum utilization `0.915` for LP-SLP and `0.825` for
  retained-cash solvers while preserving zero retained gap, so it contributes
  group and price-support coverage without manufacturing a quality gap.
- Standard-flow bundle retained P95/max stayed at `0.1174%/0.2851%`, preserving
  the SRC-003 regression signal.

This validates the multi-metric approach: no single scalar captures verifier
availability, iteration caps, continuous quality, integer landing, budget
stress, and latency. The analyzer therefore reports coverage and quality per
regime as well as the aggregate, and keeps failures in each denominator.

## Interpretation

Replay is already a useful discriminator. It found a large
fully-corrective-bundle stopping/landing tail, an LP supporting-price residual,
and verifier-invalid Clarabel landings that aggregate synthetic headlines did
not make obvious. The added regimes now cover grouped outcomes, lifecycle
resolution, and dense books as distinct scorecard slices.

The corpus is still not a sufficient long-horizon optimization target. Blocks
are correlated within trajectories, each regime has one synthetic seed, maker
budgets need counterfactual tightening, and the account/MM inventory limitation
prevents calling the captures full-account-valid evidence. Next corpus work
should add held-out seeds per regime and privacy-reviewed redacted deployed
captures. Until then, use replay beside the numerical, flash, scale, and exact
reference suites, never instead of them.
