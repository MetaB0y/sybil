# Generated retained-cash solver summary

Protocol: `solver-bundle-promotion-evaluation-v1`. Source revision: `c9fb939d521f92fd23e099208f15c931ddb51352`.

Integrity: 136/136 records, 0 duplicates, 0 cross-solver scenario mismatches.

Every declared failure remains in the denominator. Runtime and quality summaries are conditional on successful verifier-valid runs; the success column always exposes that denominator.

## Robustness, certificate, and runtime

| Solver | Success | Invalid | At cap | Median s | Median retained gap % | Median cert. gap % | Max B use |
|---|---|---|---|---|---|---|---|
| Exact bundle | 68/68 | 0 | 2 | 0.1756 | 0.0000 | 0.000000 | 1.000 |
| RC-FW | 68/68 | 0 | 22 | 0.1906 | 0.0004 | 0.000610 | 1.000 |

## Iterative work and latency tails

| Solver | P50 ms | P95 ms | P99 ms | Max ms | Oracle P50 | Oracle P95 | Oracle P50 ms | Master P50 | Atoms P50 / max |
|---|---|---|---|---|---|---|---|---|---|
| Exact bundle | 175.60 | 424.66 | 476.95 | 477.70 | 14.0 | 101.0 | 35.06 | 75.5 | 3.0 / 30 |
| RC-FW | 190.57 | 461.70 | 2317.21 | 2333.64 | 22.0 | 101.0 | 43.51 | — | — / — |

## Landed economic quality tails

| Solver | Welfare mean % | Welfare P50 % | Welfare P95 % | Welfare max % | Retained mean % | Retained P50 % | Retained P95 % | Retained max % |
|---|---|---|---|---|---|---|---|---|
| Exact bundle | 0.0024 | 0.0000 | 0.0125 | 0.0630 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| RC-FW | 1.0417 | 0.0023 | 0.6224 | 34.5325 | 0.5143 | 0.0004 | 0.0818 | 17.5966 |

## Integer landing integrity

| Solver | Landing loss P50 $ | Landing loss P95 $ | Landing loss max $ | Relative loss P95 % | Relative loss max % | Allocation L1 P95 % | Allocation L1 max % | Budget repairs | Mint duality P50 $ | Mint duality P95 $ | Mint duality max $ | Mint duality P95 % | Mint duality max % |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Exact bundle | 0.000000 | 0.000000 | 0.646449 | 0.000000 | 0.003696 | 0.011804 | 0.281044 | 0 | 0.000000000 | 0.000004157 | 0.000107842 | 0.000000007 | 0.000000024 |
| RC-FW | 0.000000 | 0.619600 | 82.992958 | 0.011981 | 17.595863 | 0.422229 | 38.542478 | 0 | 0.000000002 | 0.000000043 | 0.000000056 | 0.000000000 | 0.000000001 |

## Worst landed welfare gaps

| Experiment | Seed | Budget | Solver | Net welfare $ | Welfare gap % | Retained gap % | Landing loss % | Landing L1 % |
|---|---|---|---|---|---|---|---|---|
| promotion-orders-00080 | 70501 | 0.25× | RC-FW | 388.5364 | 34.5325 | 17.5966 | 17.595863 | 36.558950 |
| promotion-orders-00080 | 70502 | 0.25× | RC-FW | 421.5662 | 32.4223 | 16.5440 | 16.537185 | 38.542478 |
| promotion-flash-budget | 70402 | 0.1× | RC-FW | 1146.2902 | 0.9177 | 0.0530 | 0.050617 | 0.518377 |
| promotion-flash-budget | 70401 | 0.1× | RC-FW | 1157.4291 | 0.7275 | 0.0129 | 0.012219 | 0.237973 |
| promotion-mms-04 | 70902 | 0.25× | RC-FW | 20854.9662 | 0.4271 | 0.0124 | 0.011538 | 0.282392 |
| promotion-orders-02000 | 70600 | 0.25× | RC-FW | 20288.3925 | 0.3761 | 0.0041 | 0.003722 | 0.184787 |
| promotion-orders-10000 | 70700 | 0.25× | RC-FW | 103252.9255 | 0.1952 | 0.0009 | 0.000122 | 0.090329 |
| promotion-concentrated | 70101 | 0.25× | RC-FW | 17995.7171 | 0.1710 | 0.1288 | 0.000000 | 0.196641 |
| promotion-mms-16 | 71001 | 0.25× | RC-FW | 20503.8698 | 0.0938 | 0.0624 | 0.000044 | 0.000187 |
| promotion-concentrated | 70104 | 0.25× | RC-FW | 16129.8121 | 0.0860 | 0.0824 | 0.000000 | 0.216391 |
| promotion-mms-16 | 71002 | 0.25× | RC-FW | 20922.1260 | 0.0856 | 0.0729 | 0.000000 | 0.001953 |
| promotion-mms-01 | 70801 | 0.25× | RC-FW | 20621.5158 | 0.0855 | 0.0002 | 0.000000 | 0.014597 |

## Worst relative landing losses

| Experiment | Seed | Budget | Solver | Landing loss $ | Landing loss % | Allocation L1 % | Core gap $ |
|---|---|---|---|---|---|---|---|
| promotion-orders-00080 | 70501 | 0.25× | RC-FW | 81.959614 | 17.595863 | 36.558950 | 0.006259 |
| promotion-orders-00080 | 70502 | 0.25× | RC-FW | 82.992958 | 16.537185 | 38.542478 | 0.112468 |
| promotion-flash-budget | 70402 | 0.1× | RC-FW | 0.518155 | 0.050617 | 0.518377 | 0.051455 |
| promotion-flash-budget | 70401 | 0.1× | RC-FW | 0.128199 | 0.012219 | 0.237973 | 0.010263 |
| promotion-mms-04 | 70902 | 0.25× | RC-FW | 2.152725 | 0.011538 | 0.282392 | 0.172292 |
| promotion-market-like | 70203 | 0.25× | RC-FW | 0.354221 | 0.004032 | 0.203532 | 0.028300 |
| promotion-orders-02000 | 70600 | 0.25× | RC-FW | 0.674224 | 0.003722 | 0.184787 | 0.092268 |
| promotion-neutral | 70003 | 0.25× | Exact bundle | 0.646449 | 0.003696 | 0.281044 | 0.000014 |
| promotion-concentrated | 70100 | 0.25× | Exact bundle | 0.313911 | 0.002099 | 0.066175 | 0.000386 |
| promotion-market-like | 70200 | 1× | RC-FW | 0.068139 | 0.000744 | 0.054460 | 0.038772 |
| promotion-neutral | 70002 | 0.25× | RC-FW | 0.063090 | 0.000348 | 0.077760 | 0.179199 |
| promotion-orders-10000 | 70700 | 0.25× | RC-FW | 0.112613 | 0.000122 | 0.090329 | 0.709649 |

## Worst relative minting-duality residuals

| Experiment | Seed | Budget | Solver | Mint duality $ | Mint duality % | Landing L1 % |
|---|---|---|---|---|---|---|
| promotion-neutral | 70002 | 0.25× | Exact bundle | 0.000004906 | 0.000000024 | 0.000004 |
| promotion-numerical | 70304 | 0.25× | Exact bundle | 0.000107842 | 0.000000013 | 0.000000 |
| promotion-neutral | 70001 | 0.25× | Exact bundle | 0.000002765 | 0.000000010 | 0.012886 |
| promotion-concentrated | 70104 | 0.25× | Exact bundle | 0.000005189 | 0.000000009 | 0.000005 |
| promotion-concentrated | 70100 | 0.25× | Exact bundle | 0.000001401 | 0.000000003 | 0.066175 |
| promotion-numerical | 70303 | 0.25× | Exact bundle | 0.000013410 | 0.000000002 | 0.000000 |
| promotion-orders-00080 | 70502 | 0.25× | RC-FW | 0.000000006 | 0.000000001 | 38.542478 |
| promotion-neutral | 70003 | 0.25× | Exact bundle | 0.000000250 | 0.000000001 | 0.281044 |
| promotion-neutral | 70001 | 0.25× | RC-FW | 0.000000052 | 0.000000000 | 0.160899 |
| promotion-flash-budget | 70403 | 0.1× | RC-FW | 0.000000008 | 0.000000000 | 0.000097 |
| promotion-orders-00080 | 70501 | 0.25× | RC-FW | 0.000000001 | 0.000000000 | 36.558950 |
| promotion-orders-00080 | 70502 | 0.25× | Exact bundle | 0.000000001 | 0.000000000 | 0.000031 |

## Economic-connectivity coverage

| Experiment | Cases | Fragmented | Components P50/max | Largest cluster markets % | Largest cluster orders % | Largest cluster MMs % |
|---|---|---|---|---|---|---|
| promotion-concentrated | 5 | 1/5 | 1/2 | 100.0 | 100.0 | 100.0 |
| promotion-flash-budget | 5 | 5/5 | 2/2 | 50.0 | 50.0 | 50.0 |
| promotion-market-like | 5 | 0/5 | 1/1 | 100.0 | 100.0 | 100.0 |
| promotion-mms-01 | 3 | 0/3 | 1/1 | 100.0 | 100.0 | 100.0 |
| promotion-mms-04 | 3 | 3/3 | 4/4 | 25.0 | 25.0 | 25.0 |
| promotion-mms-16 | 3 | 3/3 | 16/16 | 7.5 | 7.5 | 6.2 |
| promotion-neutral | 5 | 0/5 | 1/1 | 100.0 | 100.0 | 100.0 |
| promotion-numerical | 5 | 0/5 | 1/1 | 100.0 | 100.0 | 100.0 |
| promotion-orders-00080 | 3 | 3/3 | 2/2 | 50.0 | 50.0 | 50.0 |
| promotion-orders-02000 | 3 | 3/3 | 2/2 | 50.0 | 50.0 | 50.0 |
| promotion-orders-10000 | 3 | 3/3 | 2/2 | 50.0 | 50.0 | 50.0 |

## Random-book quality

| Profile | Solver | Success | Retained gap % | Unconstrained welfare gap % | Bound ratio | Median s |
|---|---|---|---|---|---|---|
| concentrated | Exact bundle | 5/5 | 0.0000 | — | — | 0.3206 |
| concentrated | RC-FW | 5/5 | 0.0269 | — | — | 0.3240 |
| neutral | Exact bundle | 10/10 | 0.0000 | — | — | 0.2206 |
| neutral | RC-FW | 10/10 | 0.0002 | — | — | 0.2369 |

## Two-sided flash-liquidity budget sweep

| Budget | Solver | Success | Mean retained gap % | Bootstrap 95% CI | Median max B use | Bound ratio |
|---|---|---|---|---|---|---|
| 0.1× | Exact bundle | 5/5 | 0.0000 | [0.0000, 0.0000] | 0.983 | — |
| 0.1× | RC-FW | 5/5 | 0.0137 | [0.0008, 0.0344] | 0.972 | — |
| 0.25× | Exact bundle | 5/5 | 0.0000 | [0.0000, 0.0000] | 0.931 | — |
| 0.25× | RC-FW | 5/5 | 0.0004 | [0.0002, 0.0006] | 0.931 | — |
| 1× | Exact bundle | 5/5 | 0.0000 | [0.0000, 0.0000] | 0.738 | — |
| 1× | RC-FW | 5/5 | 0.0000 | [0.0000, 0.0000] | 0.738 | — |

## Fixed-MM order-count scaling

| Scale | Solver | Success | P50 ms | P95 ms | P99 ms | Oracle P95 | Max atoms | Retained gap % |
|---|---|---|---|---|---|---|---|---|
| orders-00080 | Exact bundle | 3/3 | 2.80 | 3.11 | 3.14 | 13.0 | 4 | 0.0000 |
| orders-00080 | RC-FW | 3/3 | 11.07 | 11.89 | 11.96 | 101.0 | — | 16.5440 |
| orders-02000 | Exact bundle | 3/3 | 35.25 | 37.46 | 37.66 | 22.9 | 4 | 0.0000 |
| orders-02000 | RC-FW | 3/3 | 87.66 | 92.69 | 93.13 | 23.6 | — | 0.0007 |
| orders-10000 | Exact bundle | 3/3 | 411.85 | 429.59 | 431.16 | 28.0 | 4 | 0.0000 |
| orders-10000 | RC-FW | 3/3 | 2309.12 | 2331.18 | 2333.15 | 19.9 | — | 0.0003 |

## Fixed-book market-maker scaling

| Scale | Solver | Success | P50 ms | P95 ms | P99 ms | Oracle P95 | Max atoms | Retained gap % |
|---|---|---|---|---|---|---|---|---|
| mms-01 | Exact bundle | 3/3 | 77.51 | 78.41 | 78.49 | 12.9 | 2 | 0.0000 |
| mms-01 | RC-FW | 3/3 | 123.66 | 129.04 | 129.51 | 18.4 | — | 0.0001 |
| mms-04 | Exact bundle | 3/3 | 22.42 | 24.58 | 24.78 | 36.6 | 8 | 0.0000 |
| mms-04 | RC-FW | 3/3 | 150.80 | 173.50 | 175.52 | 44.0 | — | 0.0006 |
| mms-16 | Exact bundle | 3/3 | 14.18 | 14.25 | 14.26 | 124.8 | 30 | 0.0000 |
| mms-16 | RC-FW | 3/3 | 158.07 | 162.30 | 162.67 | 101.0 | — | 0.0729 |

## Numerical-range stress

| Solver | Success | Median s | Retained gap % | LP welfare gap % | P95 cert. gap $ |
|---|---|---|---|---|---|
| Exact bundle | 10/10 | 0.3452 | 0.0000 | — | 0.0014 |
| RC-FW | 10/10 | 0.3777 | 0.0087 | — | 523.7572 |
