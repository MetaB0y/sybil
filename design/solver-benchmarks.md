# Solver Benchmarks

Internal reference for experimental evaluation of the EG/Fisher market clearing solvers.
These results validate the theory in `lmsr-proof.typ` but are implementation-specific
and don't belong in the main paper.

## Setup

- 50 market groups of binary outcomes, ~11,000 single-market orders, 3 MMs with ladder orders
- All solvers implemented in Rust; LP via HiGHS, conic via Clarabel, EG via direct gradient descent
- Quantities and prices are integer-valued (nanos, 10^9 per dollar)
- All fills verified by an independent checker (`sybil-verifier`)

Reproduce with:
```bash
cargo run --release -p matching-sim --features lp,conic -- --markets 50 --orders 11000 --mms 3 --solver all -v
```

## Solver Comparison

Three objective modes:
- **Linear**: max Σ wⱼqⱼ (LP via HiGHS)
- **Quasi-Fisher**: max Σ [Bₖ ln(Uₖ + sₖ) - sₖ] + Σ wⱼqⱼ (conic via Clarabel; Theorem 4 of the paper)
- **Fisher**: max Σ Bₖ ln Uₖ + Σ wⱼqⱼ (EG via gradient descent; no cash variable)

| Solver | Welfare | Fills | Time | MM Util |
|--------|---------|-------|------|---------|
| Conic (quasi-Fisher) | $62.32K | 8331 | 0.29s | 100/100/100% |
| LP (linear) | $62.63K | 8316 | 0.17s | 100/100/100% |
| EG (Fisher) | $61.15K | 8331 | 2.28s | 92/75/100% |

Quasi-Fisher and LP produce near-identical welfare ($62.32K vs $62.63K, gap 0.5%),
confirming Proposition 4 (LP Recovery): when budgets are not severely binding, the programs agree.

The EG solver (Fisher objective, no cash variable) succeeds via direct gradient descent
but at 13x the solve time and lower welfare (-2.4%). Without the cash variable sₖ,
the Fisher objective forces MMs to deploy capital even on marginal orders — MM 2 uses
only 75% of its budget, reflecting suboptimal allocation rather than efficient throttling.

The conic Fisher mode (without cash variable) fails at this scale — Clarabel reports
`InsufficientProgress` when Vₖ = Σ Lᵢqᵢ approaches zero, making the exponential cone
ill-conditioned. The cash variable acts as a numerical buffer (Vₖ = Uₖ + sₖ ≥ sₖ > 0).

## Welfare Gap (Budget Scaling)

Proposition 5 bounds the welfare gap by Σ Δₖ²/(2Bₖ). We fix the order book and vary
only the MM budget by a scale factor.

| Budget scale | W^RA (quasi-Fisher) | W^LP (linear) | Gap |
|---|---|---|---|
| ×1.5 (slack) | $62.70K | $62.70K | 0.0% |
| ×1 (default) | $62.32K | $62.63K | 0.5% |
| ×0.5 (tight) | $61.77K | $62.17K | 0.6% |
| ×0.3 (severe) | $61.51K | $61.83K | 0.5% |
| ×0.1 (extreme) | $61.15K | $61.38K | 0.4% |

The gap is < 0.7% across all scales, much tighter than the quadratic bound predicts.
When budgets are non-binding (scale ≥ 1.5), the programs coincide exactly.

Total welfare drops from $62.70K to $61.15K (2.5%) as budgets shrink from 1.5x to 0.1x,
reflecting reduced MM participation. Both solvers track this decline in lockstep.

**Known issue**: At budget scales ≥ 2x, the conic solver becomes numerically ill-conditioned —
the cash variable sₖ grows large relative to Uₖ, degrading Clarabel's interior-point
convergence. The LP solver confirms welfare is flat across all slack budget scales.

## Decomposition

Decomposed solving partitions the problem by market group, solving each independently.
With single-market orders, no orders span groups and the minting cost separates exactly:
C_b = Σⱼ C_b^Gⱼ. The only coupling is MM budget allocation across components,
coordinated by **proportional response on deployed value** `Vₖᵐ = Uₖᵐ + sₖᵐ` (weighted
fill value plus retained cash). Fixed points are the *equal-scarcity* allocations
(`Bₖᵐ / Vₖᵐ` equal across a MM's active components), which are exactly the componentwise
restrictions of the monolithic optimum (decomposition companion note, Theorem 1).

> **Note (SYB-236).** Prior figures in this section used the *superseded* budget-update
> rule — multiplicative-weights ascent on per-component EG utility (equalizing `Uₖᵐ`), the
> February-2026 "surrogate". That surrogate converges to the wrong fixed point (EG optimal
> values are convex in budget, so its interior stationary point is a welfare *minimizer*).
> The rows below are re-measured with the corrected proportional-response rule and are
> **not comparable** to the earlier numbers.

Re-measured with the corrected rule (`matching-sim`, seed 42; monolithic vs decomposed,
same inner conic solver):

| Preset | Markets / MMs | Monolithic conic | Decomposed conic | Welfare ratio |
|--------|---------------|------------------|------------------|---------------|
| large  | 50 / 3        | $56.60 / 0.28s   | $56.47 / 3.6s    | 99.8%         |
| medium | 30 / 2        | $17.45 / 0.09s   | $17.01 / 1.1s    | 97.5%         |

On these **near-symmetric** synthetic books the corrected rule tracks the monolithic
optimum closely (≈98–100%). It also lands within noise of the old surrogate here
(large: surrogate $56.55 vs $56.47; medium: surrogate $17.40 vs $17.01) — expected, since
equal-scarcity ≈ equal-utility at symmetry, which is exactly why the surrogate "stuck"
near 93–99% on symmetric instances despite being wrong. The correctness win shows up on
**asymmetric** books, where equal-utility misallocates budget toward capacity-limited /
low-ROI components; see `decomposed::tests::test_asymmetric_equal_scarcity_coordination`
for a worked instance where the two targets diverge. The decomposed solver remains slower
here because coordination overhead dominates the savings from smaller per-component solves.

Convergence *rate* of proportional response in this setting (quasilinear utilities,
endogenous supply, retail orders alongside MMs) is an open problem — the companion note
proves fixed-point correctness only. The iteration is capped at 20 rounds and returns the
best-welfare round seen, so a not-fully-converged run is bounded, not catastrophic.

Cross-group orders (spreads, bundles) would additionally break minting-cost separability,
adding a structural welfare loss beyond the coordination gap.

**Known issues with decomposed LP/EG**: The decomposed LP and EG solvers produce
verification violations (QuantityExceedsMax, DuplicateFill) on MM orders — a pre-existing
bug in the decomposed solver's MM handling. Only the decomposed conic solver produces
valid results.

## Microbenchmarks

From `cargo bench --workspace` (release profile):

| Benchmark | Median |
|---|---|
| Scenario gen: small | 38µs |
| Scenario gen: medium | 287µs |
| Scenario gen: large | 909µs |
| LP: small (~300 orders) | 2.75ms |
| LP: medium (~3000 orders) | 26.9ms |
| LP: medium + hot markets | 26.9ms |
| Conic: small | 6.1ms |
| Conic: medium | 57.6ms |
| EG: small | 35.9ms |
| EG: medium | 354ms |
