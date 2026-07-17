# Generated retained-cash solver summary

Protocol: `solver-adversarial-connectivity-evaluation-v1`. Source revision: `f82e2455c1c355e2d09bf25ab86323b10c5d7c66`.

Integrity: 60/60 records, 0 duplicates, 0 cross-solver scenario mismatches.

Every declared failure remains in the denominator. Runtime and quality summaries are conditional on successful verifier-valid runs; the success column always exposes that denominator.

## Robustness, certificate, and runtime

| Solver | Success | Invalid | At cap | Median s | Median retained gap % | Median cert. gap % | Max B use |
|---|---|---|---|---|---|---|---|
| Exact bundle | 20/20 | 0 | 0 | 3.5263 | 0.0000 | 0.000000 | 1.000 |
| Pacing bundle | 20/20 | 0 | 0 | 3.5105 | 0.0000 | 0.000000 | 1.000 |
| RC-FW | 19/20 | 0 | 2 | 3.6266 | 0.0006 | 0.000808 | 0.999 |

## Iterative work and latency tails

| Solver | P50 ms | P95 ms | P99 ms | Max ms | Oracle P50 | Oracle P95 | Oracle P50 ms | Master P50 | Atoms P50 / max |
|---|---|---|---|---|---|---|---|---|---|
| Exact bundle | 3526.26 | 85551.89 | 85585.45 | 85593.84 | 8.5 | 10.0 | 506.13 | 63.5 | 2.0 / 3 |
| Pacing bundle | 3510.45 | 85495.75 | 85873.35 | 85967.75 | 8.5 | 10.0 | 510.70 | 63.5 | 2.0 / 3 |
| RC-FW | 3626.62 | 56868.74 | 80292.22 | 86148.09 | 6 | 101.0 | 826.52 | — | — / — |

## Landed economic quality tails

| Solver | Welfare mean % | Welfare P50 % | Welfare P95 % | Welfare max % | Retained mean % | Retained P50 % | Retained P95 % | Retained max % |
|---|---|---|---|---|---|---|---|---|
| Exact bundle | 0.0001 | 0.0000 | 0.0012 | 0.0013 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| Pacing bundle | 0.0001 | 0.0000 | 0.0012 | 0.0013 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| RC-FW | 0.0022 | 0.0014 | 0.0058 | 0.0067 | 0.0006 | 0.0006 | 0.0010 | 0.0023 |

## Integer landing integrity

| Solver | Landing loss P50 $ | Landing loss P95 $ | Landing loss max $ | Relative loss P95 % | Relative loss max % | Allocation L1 P95 % | Allocation L1 max % | Budget repairs | Mint duality P50 $ | Mint duality P95 $ | Mint duality max $ | Mint duality P95 % | Mint duality max % |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Exact bundle | 0.000000 | 0.000000 | 0.000000 | 0.000000 | 0.000000 | 0.028425 | 0.034010 | 0 | 0.000000001 | 0.000000002 | 0.000000003 | 0.000000000 | 0.000000000 |
| Pacing bundle | 0.000000 | 0.000000 | 0.000000 | 0.000000 | 0.000000 | 0.028425 | 0.034010 | 0 | 0.000000001 | 0.000000002 | 0.000000003 | 0.000000000 | 0.000000000 |
| RC-FW | 0.000000 | 0.203472 | 0.330422 | 0.000114 | 0.000192 | 0.084285 | 0.100515 | 0 | 0.000000005 | 0.000000014 | 0.000000015 | 0.000000000 | 0.000000000 |

## Worst landed welfare gaps

| Experiment | Seed | Budget | Solver | Net welfare $ | Welfare gap % | Retained gap % | Landing loss % | Landing L1 % |
|---|---|---|---|---|---|---|---|---|
| tiny-mm-bridge-10000 | 72200 | 0.25× | RC-FW | 31109.6691 | 0.0067 | 0.0006 | 0.000000 | 0.000715 |
| tiny-mm-bridge-10000 | 72201 | 0.1× | RC-FW | 33108.3679 | 0.0057 | 0.0003 | 0.000000 | 0.000000 |
| tiny-mm-bridge-10000 | 72202 | 0.25× | RC-FW | 36378.7275 | 0.0050 | 0.0007 | 0.000000 | 0.001036 |
| tiny-mm-bridge-10000 | 72202 | 0.1× | RC-FW | 36370.8327 | 0.0040 | 0.0003 | 0.000000 | 0.002345 |
| tiny-mm-bridge-10000 | 72200 | 0.1× | RC-FW | 31102.5986 | 0.0032 | 0.0001 | 0.000000 | 0.001618 |
| tiny-mm-bridge-50000 | 72300 | 0.1× | RC-FW | 179160.5660 | 0.0026 | 0.0005 | 0.000106 | 0.001887 |
| global-mm-50000 | 72101 | 0.25× | RC-FW | 172111.9835 | 0.0024 | 0.0023 | 0.000000 | 0.033136 |
| tiny-mm-bridge-50000 | 72301 | 0.1× | RC-FW | 178295.4706 | 0.0023 | 0.0004 | 0.000000 | 0.000339 |
| global-mm-10000 | 72000 | 0.25× | RC-FW | 35108.2980 | 0.0015 | 0.0008 | 0.000000 | 0.014612 |
| tiny-mm-bridge-50000 | 72301 | 0.25× | RC-FW | 178304.1694 | 0.0014 | 0.0002 | 0.000000 | 0.000969 |
| global-mm-10000 | 72001 | 0.25× | RC-FW | 35959.8883 | 0.0014 | 0.0009 | 0.000000 | 0.082482 |
| global-mm-10000 | 72002 | 0.25× | RC-FW | 35645.1780 | 0.0014 | 0.0007 | 0.000000 | 0.009455 |

## Worst relative landing losses

| Experiment | Seed | Budget | Solver | Landing loss $ | Landing loss % | Allocation L1 % | Core gap $ |
|---|---|---|---|---|---|---|---|
| global-mm-50000 | 72101 | 0.1× | RC-FW | 0.330422 | 0.000192 | 0.006938 | 1.199619 |
| tiny-mm-bridge-50000 | 72300 | 0.1× | RC-FW | 0.189367 | 0.000106 | 0.001887 | 1.032574 |
| tiny-mm-bridge-10000 | 72200 | 0.1× | Exact bundle | 0.000000 | 0.000000 | 0.002379 | 0.000000 |
| tiny-mm-bridge-10000 | 72200 | 0.1× | Pacing bundle | 0.000000 | 0.000000 | 0.002379 | 0.000000 |
| global-mm-10000 | 72000 | 0.25× | Pacing bundle | 0.000000 | 0.000000 | 0.000452 | 0.000000 |
| global-mm-10000 | 72000 | 0.25× | Exact bundle | 0.000000 | 0.000000 | 0.000452 | 0.000000 |
| tiny-mm-bridge-10000 | 72201 | 0.1× | Pacing bundle | 0.000000 | 0.000000 | 0.000000 | 0.000000 |
| tiny-mm-bridge-10000 | 72201 | 0.1× | Exact bundle | 0.000000 | 0.000000 | 0.000000 | 0.000000 |
| global-mm-10000 | 72001 | 0.1× | Pacing bundle | 0.000000 | 0.000000 | 0.034010 | 0.000000 |
| global-mm-10000 | 72001 | 0.1× | Exact bundle | 0.000000 | 0.000000 | 0.034010 | 0.000000 |
| tiny-mm-bridge-50000 | 72300 | 0.1× | Pacing bundle | 0.000000 | 0.000000 | 0.000000 | 0.000000 |
| tiny-mm-bridge-50000 | 72300 | 0.1× | Exact bundle | 0.000000 | 0.000000 | 0.000000 | 0.000000 |

## Worst relative minting-duality residuals

| Experiment | Seed | Budget | Solver | Mint duality $ | Mint duality % | Landing L1 % |
|---|---|---|---|---|---|---|
| global-mm-10000 | 72000 | 0.25× | RC-FW | 0.000000014 | 0.000000000 | 0.014612 |
| global-mm-10000 | 72002 | 0.25× | RC-FW | 0.000000013 | 0.000000000 | 0.009455 |
| tiny-mm-bridge-10000 | 72202 | 0.25× | RC-FW | 0.000000008 | 0.000000000 | 0.001036 |
| global-mm-10000 | 72002 | 0.1× | RC-FW | 0.000000007 | 0.000000000 | 0.009153 |
| global-mm-10000 | 72001 | 0.25× | RC-FW | 0.000000007 | 0.000000000 | 0.082482 |
| tiny-mm-bridge-10000 | 72200 | 0.25× | RC-FW | 0.000000005 | 0.000000000 | 0.000715 |
| tiny-mm-bridge-50000 | 72300 | 0.1× | RC-FW | 0.000000015 | 0.000000000 | 0.001887 |
| global-mm-10000 | 72001 | 0.1× | RC-FW | 0.000000003 | 0.000000000 | 0.100515 |
| global-mm-10000 | 72000 | 0.1× | RC-FW | 0.000000002 | 0.000000000 | 0.009011 |
| tiny-mm-bridge-50000 | 72301 | 0.1× | RC-FW | 0.000000010 | 0.000000000 | 0.000339 |
| global-mm-50000 | 72101 | 0.1× | RC-FW | 0.000000009 | 0.000000000 | 0.006938 |
| tiny-mm-bridge-10000 | 72200 | 0.1× | Exact bundle | 0.000000002 | 0.000000000 | 0.002379 |

## Economic-connectivity coverage

| Experiment | Cases | Fragmented | Components P50/max | Largest cluster markets % | Largest cluster orders % | Largest cluster MMs % |
|---|---|---|---|---|---|---|
| global-mm-10000 | 3 | 0/3 | 1/1 | 100.0 | 100.0 | 100.0 |
| global-mm-50000 | 2 | 0/2 | 1/1 | 100.0 | 100.0 | 100.0 |
| tiny-mm-bridge-10000 | 3 | 0/3 | 1/1 | 100.0 | 100.0 | 100.0 |
| tiny-mm-bridge-50000 | 2 | 0/2 | 1/1 | 100.0 | 100.0 | 100.0 |

## Random-book quality

| Profile | Solver | Success | Retained gap % | Unconstrained welfare gap % | Bound ratio | Median s |
|---|---|---|---|---|---|---|

## Two-sided flash-liquidity budget sweep

| Budget | Solver | Success | Mean retained gap % | Bootstrap 95% CI | Median max B use | Bound ratio |
|---|---|---|---|---|---|---|
