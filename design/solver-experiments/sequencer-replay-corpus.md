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

## Interpretation

The replay is already a useful discriminator: it found a large
fully-corrective-bundle landing tail and an LP supporting-price residual that
the aggregate synthetic headline did not make obvious. It also shows why one
scalar is unsafe: Clarabel has the best tight-budget retained tail here despite
its known availability failures on separate exponential-cone stress cases.

The corpus is not yet a sufficient long-horizon optimization target. Blocks are
correlated, one synthetic policy produced them, quantities are small, and tight
budgets are counterfactual. Next corpus work should add independent seeds,
multi-outcome lifecycle traffic, and privacy-reviewed redacted deployed
captures. Until then, use replay beside the numerical, flash, scale, and exact
reference suites, never instead of them.
