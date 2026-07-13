# Generated retained-cash solver summary

Protocol: `solver-evaluation-v2-retained-cash`. Source revision: `0f0824ac892d1b9268fa45fded2004f7f9777ff7`.

Integrity: 545/545 records, 0 duplicates, 0 cross-solver scenario mismatches.

Every declared failure remains in the denominator. Runtime and quality summaries are conditional on successful verifier-valid runs; the success column always exposes that denominator. The budget-blind LP is an intentionally infeasible negative control.

## Robustness, certificate, and runtime

| Solver | Success | Invalid | At cap | Median s | Median retained gap % | Median cert. gap % | Max B use |
|---|---|---|---|---|---|---|---|
| Fisher | 10/10 | 0 | 0 | 0.0017 | 0.0000 | 0.000000 | 1.000 |
| Quasi | 114/135 | 0 | 0 | 0.0059 | 0.0000 | 0.000001 | 1.000 |
| LP | 125/125 | 0 | 19 | 0.0040 | 0.0080 | — | 1.000 |
| LP (no budget) | 52/125 | 73 | 0 | 0.0023 | 0.0000 | — | 7.448 |
| MILP | 15/15 | 0 | 0 | 0.0439 | 0.8078 | — | 1.000 |
| RC-FW | 135/135 | 0 | 25 | 0.0375 | 0.0000 | 0.000000 | 1.000 |

## Random-book quality

| Profile | Solver | Success | Retained gap % | Unconstrained welfare gap % | Bound ratio | Median s |
|---|---|---|---|---|---|---|
| concentrated | Quasi | 4/6 | 0.0129 | 0.663 | 0.035 | 0.0805 |
| concentrated | LP | 6/6 | 0.0083 | 0.882 | 0.046 | 0.0470 |
| concentrated | LP (no budget) | 0/6 | — | — | — | — |
| concentrated | RC-FW | 6/6 | 0.0026 | 0.936 | 0.049 | 2.0847 |
| neutral | Quasi | 17/18 | 0.0000 | 0.000 | 0.048 | 0.0734 |
| neutral | LP | 18/18 | 0.0000 | 0.000 | 0.044 | 0.0224 |
| neutral | LP (no budget) | 12/18 | 0.0000 | 0.000 | — | 0.0223 |
| neutral | RC-FW | 18/18 | 0.0000 | 0.000 | 0.049 | 0.0542 |

## Two-sided flash-liquidity budget sweep

| Budget | Solver | Success | Mean retained gap % | Bootstrap 95% CI | Median max B use | Bound ratio |
|---|---|---|---|---|---|---|
| 0.1× | Quasi | 9/10 | 0.0289 | [0.0065, 0.0567] | 0.975 | 0.320 |
| 0.1× | LP | 10/10 | 0.1284 | [0.0165, 0.3245] | 1.000 | 0.319 |
| 0.1× | LP (no budget) | 0/10 | — | — | 7.349 | — |
| 0.1× | RC-FW | 10/10 | 0.0091 | [0.0026, 0.0165] | 1.000 | 0.321 |
| 0.25× | Quasi | 7/10 | 0.0004 | [0.0002, 0.0007] | 0.945 | 0.331 |
| 0.25× | LP | 10/10 | 0.2783 | [0.2317, 0.3172] | 0.999 | 0.310 |
| 0.25× | LP (no budget) | 0/10 | — | — | 2.940 | — |
| 0.25× | RC-FW | 10/10 | 0.0000 | [0.0000, 0.0000] | 0.951 | 0.328 |
| 0.5× | Quasi | 10/10 | 0.0000 | [0.0000, 0.0001] | 0.870 | 0.312 |
| 0.5× | LP | 10/10 | 1.9862 | [1.9164, 2.0464] | 1.000 | 0.189 |
| 0.5× | LP (no budget) | 0/10 | — | — | 1.470 | — |
| 0.5× | RC-FW | 10/10 | 0.0003 | [0.0001, 0.0004] | 0.874 | 0.312 |
| 1× | Quasi | 7/10 | 0.0000 | [0.0000, 0.0000] | 0.727 | — |
| 1× | LP | 10/10 | 0.0000 | [0.0000, 0.0000] | 0.735 | — |
| 1× | LP (no budget) | 10/10 | 0.0000 | [0.0000, 0.0000] | 0.735 | — |
| 1× | RC-FW | 10/10 | 0.0000 | [0.0000, 0.0000] | 0.735 | — |
| 2× | Quasi | 10/10 | 0.0000 | [0.0000, 0.0000] | 0.367 | — |
| 2× | LP | 10/10 | 0.0000 | [0.0000, 0.0000] | 0.367 | — |
| 2× | LP (no budget) | 10/10 | 0.0000 | [0.0000, 0.0000] | 0.367 | — |
| 2× | RC-FW | 10/10 | 0.0000 | [0.0000, 0.0000] | 0.367 | — |
| 10× | Quasi | 8/10 | 0.0205 | [0.0121, 0.0302] | 0.073 | — |
| 10× | LP | 10/10 | 0.0000 | [0.0000, 0.0000] | 0.073 | — |
| 10× | LP (no budget) | 10/10 | 0.0000 | [0.0000, 0.0000] | 0.073 | — |
| 10× | RC-FW | 10/10 | 0.0000 | [0.0000, 0.0000] | 0.073 | — |

## Scaling

| Scale | Solver | Success | Median s | Retained gap % | P95 cert. gap % |
|---|---|---|---|---|---|
| flash-large | Quasi | 2/4 | 0.0342 | 0.0005 | 0.000059 |
| flash-large | LP | 4/4 | 0.0385 | 0.2907 | — |
| flash-large | LP (no budget) | 0/4 | — | — | — |
| flash-large | RC-FW | 4/4 | 0.2249 | 0.0000 | 0.000992 |
| flash-medium | Quasi | 4/6 | 0.0057 | 0.0002 | 0.000001 |
| flash-medium | LP | 6/6 | 0.0046 | 0.3611 | — |
| flash-medium | LP (no budget) | 0/6 | — | — | — |
| flash-medium | RC-FW | 6/6 | 0.0483 | 0.0000 | 0.000984 |
| flash-small | Quasi | 6/6 | 0.0015 | 0.0004 | 0.000001 |
| flash-small | LP | 6/6 | 0.0017 | 0.3701 | — |
| flash-small | LP (no budget) | 0/6 | — | — | — |
| flash-small | RC-FW | 6/6 | 0.0277 | 0.0001 | 0.022731 |

## Numerical-range stress

| Solver | Success | Median s | Retained gap % | LP welfare gap % | P95 cert. gap $ |
|---|---|---|---|---|---|
| Quasi | 5/10 | 0.0957 | 0.0039 | 0.0080 | 0.8572 |
| LP | 10/10 | 0.0486 | 0.0000 | 0.0000 | — |
| LP (no budget) | 5/10 | 0.0326 | 0.0000 | 0.0000 | — |
| RC-FW | 10/10 | 1.1593 | 0.0000 | 0.0182 | 361.3917 |

## Exact small-instance reference

| Solver | Success | Median s | Retained gap % | LP welfare gap % | P95 cert. gap $ |
|---|---|---|---|---|---|
| Quasi | 15/15 | 0.0011 | 0.0002 | 4.7202 | 0.0000 |
| LP | 15/15 | 0.0019 | 0.7543 | 0.0000 | — |
| LP (no budget) | 5/15 | 0.0012 | 0.0000 | 0.0000 | — |
| MILP | 15/15 | 0.0439 | 0.8078 | -0.2604 | — |
| RC-FW | 15/15 | 0.0029 | 0.0000 | 4.7205 | 0.0003 |

## Retained-cash ablation

| Solver | Success | Median s | Retained gap % | LP welfare gap % | P95 cert. gap $ |
|---|---|---|---|---|---|
| Fisher | 5/5 | 0.0017 | 0.0008 | — | 0.0000 |
| Quasi | 5/5 | 0.0016 | 0.0008 | — | 0.0000 |
| RC-FW | 5/5 | 0.0818 | 0.0000 | — | 0.0109 |
| Fisher | 5/5 | 0.0018 | 0.0000 | — | 0.0001 |
| Quasi | 5/5 | 0.0017 | 0.0000 | — | 0.0001 |
| RC-FW | 5/5 | 0.0033 | 0.0000 | — | 0.0000 |
