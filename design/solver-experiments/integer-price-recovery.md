# Integer supporting-price recovery

Date: 2026-07-17

Status: accepted candidate.

## Question

Are the new Clarabel replay failures defects in Clarabel's conic solve, or does
Sybil lose an otherwise feasible price when it converts floating LP duals to
integer protocol nanos?

## Hypothesis

Nearest-integer rounding of normalized HiGHS duals can cross a filled order's
limit by one nano. The rounded allocation itself implies an exact interval of
valid integer YES prices:

- YES buy: `p_yes <= limit`
- YES sell: `p_yes >= limit`
- NO buy: `p_yes >= $1 - limit`
- NO sell: `p_yes <= $1 - limit`

Clamping the floating-dual price to the intersection of these intervals should
preserve the allocation while making the integer price support explicit.
Grouped markets additionally require `sum(p_yes) <= $1`.

## Baseline evidence

The 576-row multi-regime replay found two consecutive Clarabel rows,
mid-resolution blocks `b006` and `b007` at budget multiplier `0.00005`, which
Clarabel reported as `Solved` with primal residuals near `6e-9`. Both landed at
YES price `317,674,413` nanos while five filled buys had limit
`317,674,412`. The verifier retained ten `PriceExceedsLimit` violations.

Two older wide-range bundle failures had a related supporting-price symptom:

- pacing protocol, numerical-range seed `16203` at `0.25`: best integer
  minting-price gap `$2,090.576580628`;
- structural-oracle protocol, numerical-range seed `21200` at `0.25`:
  HiGHS/structural bundle gaps `$8,565.067009324` and `$8,566.030353990`.

## Candidate

`integer_supporting_yes_prices` keeps the normalized LP dual as the target,
then:

1. derives lower and upper integer YES-price bounds from every fill that will
   survive quantity rounding;
2. clamps independent-market prices to their interval;
3. clamps grouped prices and deterministically removes any group-cap overflow
   without crossing a filled-order lower bound; and
4. leaves an infeasible interval unchanged so the verifier still exposes a
   genuine primal/dual inconsistency.

The same recovered price feeds MM-budget fixed-point checks, previews, and
final output. No fill, objective, verifier, or solver tolerance was changed.

## Results

| Protocol | Records | Baseline | Candidate | Outcome |
|---|---:|---|---|---|
| Multi-regime replay v2 | 576 | Clarabel 142/144; 2 verifier-invalid | Clarabel 144/144 | Both one-nano failures repaired |
| General retained-cash v2 | 545 | Known Clarabel core failures | Same 5 `InsufficientProgress`; no constrained-solver invalid rows | Distinguishes core convergence from landing |
| Pacing development v2 | 630 | Bundle 125/126 | Bundle 126/126 | `$2,090.58` price-gap failure repaired |
| Structural oracle v1 | 244 | Both bundles 60/61 | Both bundles 61/61 | Both `$8.5k` price-gap failures repaired |

In replay, allocations, welfare, retained objective, MM utilization, minting
cost, iteration counts, and landing diagnostics were unchanged in all 576
rows. Five cross-solver comparison fields moved only because the two repaired
rows re-entered the successful reference set or because a market price moved
one nano. No runtime claim is made from the sequential development runs.

Known unrelated failures remain visible:

- five Clarabel `InsufficientProgress` rows in the 545-row general protocol;
- two Clarabel `InsufficientProgress` rows and one retained-cash budget
  fixed-point failure in the pacing protocol; and
- the same paired retained-cash budget fixed-point failure in the structural
  protocol.

## Commands

```bash
cargo run --release -p matching-sim --all-features \
  --bin solver-experiments -- \
  --protocol benchmarks/solver/protocol-replay-development.json \
  --source-revision candidate-integer-price-support-v1 \
  --output-dir /tmp/solver-replay-integer-price-support-v1 --overwrite

cargo run --release -p matching-sim --all-features \
  --bin solver-experiments -- \
  --protocol benchmarks/solver/protocol-v2.json \
  --source-revision candidate-integer-price-support-v1 \
  --output-dir /tmp/solver-v2-integer-price-support-v1 --overwrite

cargo run --release -p matching-sim --all-features \
  --bin solver-experiments -- \
  --protocol benchmarks/solver/protocol-pacing-development.json \
  --source-revision candidate-integer-price-support-v1 \
  --output-dir /tmp/solver-pacing-integer-price-support-v1 --overwrite

cargo run --release -p matching-sim --all-features \
  --bin solver-experiments -- \
  --protocol benchmarks/solver/protocol-structural-oracle-development.json \
  --source-revision candidate-integer-price-support-v1 \
  --output-dir /tmp/solver-structural-integer-price-support-v1 --overwrite
```

Each output was validated with
`scripts/benchmarks/analyze_solver_experiments.py`.

## Decision

Keep the candidate. It adds a small domain-specific integer recovery step,
strictly improves availability on three distinct development slices, preserves
the scored allocations, and makes protocol support constraints explicit. It
also shows that the observed replay invalidity was not evidence for forking
Clarabel.

An empty integer support interval remains a future explicit failure path. The
next Clarabel work should target the reproducible `InsufficientProgress` rows
through formulation/scaling experiments before any upstream fork.
