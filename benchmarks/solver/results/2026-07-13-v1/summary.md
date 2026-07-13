# Generated solver experiment summary

Protocol: `solver-evaluation-v1`. Source revision: `831dda777e1bc11fb66a13d335401284868a3f03`.

Integrity: 675/675 records, 0 duplicates, 0 cross-solver scenario mismatches.

Failed, timed-out, empty, panicking, and verifier-invalid runs remain in every denominator. Gap and runtime summaries use successful runs only and always show `success/declared`.

## Overall robustness and runtime

| Solver | Success | Failed | At cap | Median seconds | Median LP gap % |
|---|---|---|---|---|---|
| Fisher | 46/48 | 2 | 0 | 0.1026 | 0.053 |
| Quasi | 123/158 | 35 | 0 | 0.0868 | 0.034 |
| D-LP | 16/16 | 0 | 11 | 0.5894 | 0.000 |
| D-Quasi | 13/16 | 3 | 8 | 1.3275 | 1.918 |
| EG-FW | 137/137 | 0 | 92 | 0.5710 | 0.681 |
| IterLP | 137/137 | 0 | 10 | 0.2548 | 0.295 |
| LP | 158/158 | 0 | 0 | 0.0269 | 0.000 |
| MILP | 4/5 | 1 | 0 | 0.0529 | 0.000 |

## Quality suite

| Profile | Solver | Success | Median LP gap % | IQR % | Median allocation L1 |
|---|---|---|---|---|---|
| asymmetric | Fisher | 11/12 | 0.032 | [0.011, 0.051] | 0.031 |
| asymmetric | Quasi | 11/12 | 0.000 | [0.000, 0.000] | 0.026 |
| asymmetric | EG-FW | 12/12 | 0.309 | [0.156, 0.525] | 0.051 |
| asymmetric | IterLP | 12/12 | 0.491 | [0.362, 0.640] | 0.062 |
| asymmetric | LP | 12/12 | 0.000 | [0.000, 0.000] | 0.000 |
| balanced | Fisher | 12/12 | 0.467 | [0.030, 0.707] | 0.075 |
| balanced | Quasi | 10/12 | 0.467 | [0.043, 0.621] | 0.075 |
| balanced | EG-FW | 12/12 | 0.446 | [0.270, 1.457] | 0.059 |
| balanced | IterLP | 12/12 | 0.219 | [0.123, 0.329] | 0.040 |
| balanced | LP | 12/12 | 0.000 | [0.000, 0.000] | 0.000 |
| buy-heavy-stress | Fisher | 12/12 | 0.065 | [0.038, 0.861] | 0.054 |
| buy-heavy-stress | Quasi | 9/12 | 0.000 | [0.000, 0.823] | 0.037 |
| buy-heavy-stress | EG-FW | 12/12 | 1.125 | [0.479, 5.123] | 0.075 |
| buy-heavy-stress | IterLP | 12/12 | 0.659 | [0.593, 0.826] | 0.082 |
| buy-heavy-stress | LP | 12/12 | 0.000 | [0.000, 0.000] | 0.000 |
| concentrated | Fisher | 11/12 | 0.045 | [0.009, 0.351] | 0.042 |
| concentrated | Quasi | 9/12 | 0.034 | [0.000, 0.457] | 0.046 |
| concentrated | EG-FW | 12/12 | 0.957 | [0.515, 3.919] | 0.093 |
| concentrated | IterLP | 12/12 | 0.290 | [0.218, 0.355] | 0.049 |
| concentrated | LP | 12/12 | 0.000 | [0.000, 0.000] | 0.000 |

## Scaling suite

| Scale | Solver | Success | Median seconds | IQR seconds |
|---|---|---|---|---|
| large | Quasi | 3/8 | 0.3422 | [0.3251, 0.3637] |
| large | EG-FW | 8/8 | 2.6138 | [2.5495, 2.6638] |
| large | IterLP | 8/8 | 1.0823 | [1.0240, 1.1252] |
| large | LP | 8/8 | 0.1097 | [0.1089, 0.1186] |
| medium | Quasi | 2/8 | 0.0819 | [0.0817, 0.0820] |
| medium | EG-FW | 8/8 | 0.5549 | [0.2475, 0.5813] |
| medium | IterLP | 8/8 | 0.2354 | [0.2202, 0.2543] |
| medium | LP | 8/8 | 0.0260 | [0.0253, 0.0281] |
| small | Quasi | 8/8 | 0.0093 | [0.0091, 0.0094] |
| small | EG-FW | 8/8 | 0.0684 | [0.0406, 0.0692] |
| small | IterLP | 8/8 | 0.0271 | [0.0253, 0.0297] |
| small | LP | 8/8 | 0.0033 | [0.0032, 0.0039] |
| xlarge | Quasi | 2/5 | 1.2923 | [1.2838, 1.3008] |
| xlarge | EG-FW | 5/5 | 5.0078 | [3.6336, 10.6022] |
| xlarge | IterLP | 5/5 | 4.7761 | [4.3968, 4.8365] |
| xlarge | LP | 5/5 | 0.4649 | [0.4599, 0.4895] |

## Budget sweep

| Budget | Solver | Success | Mean LP gap % | Bootstrap 95% CI |
|---|---|---|---|---|
| 0.1× | Quasi | 10/10 | 3.488 | [2.288, 4.753] |
| 0.1× | EG-FW | 10/10 | 3.993 | [2.697, 5.347] |
| 0.1× | IterLP | 10/10 | 0.191 | [0.139, 0.241] |
| 0.1× | LP | 10/10 | 0.000 | [0.000, 0.000] |
| 0.3× | Quasi | 8/10 | 1.660 | [0.849, 2.524] |
| 0.3× | EG-FW | 10/10 | 3.335 | [2.213, 4.473] |
| 0.3× | IterLP | 10/10 | 0.268 | [0.235, 0.299] |
| 0.3× | LP | 10/10 | 0.000 | [0.000, 0.000] |
| 0.5× | Quasi | 8/10 | 0.733 | [0.264, 1.319] |
| 0.5× | EG-FW | 10/10 | 2.000 | [0.984, 3.163] |
| 0.5× | IterLP | 10/10 | 0.280 | [0.252, 0.309] |
| 0.5× | LP | 10/10 | 0.000 | [0.000, 0.000] |
| 1× | Quasi | 9/10 | 0.031 | [0.001, 0.073] |
| 1× | EG-FW | 10/10 | 0.637 | [0.396, 0.886] |
| 1× | IterLP | 10/10 | 0.301 | [0.272, 0.327] |
| 1× | LP | 10/10 | 0.000 | [0.000, 0.000] |
| 1.5× | Quasi | 7/10 | 0.000 | [-0.000, 0.000] |
| 1.5× | EG-FW | 10/10 | 0.689 | [0.406, 1.016] |
| 1.5× | IterLP | 10/10 | 0.301 | [0.272, 0.327] |
| 1.5× | LP | 10/10 | 0.000 | [0.000, 0.000] |
| 3× | Quasi | 8/10 | 0.000 | [0.000, 0.000] |
| 3× | EG-FW | 10/10 | 0.227 | [0.184, 0.273] |
| 3× | IterLP | 10/10 | 0.305 | [0.278, 0.327] |
| 3× | LP | 10/10 | 0.000 | [0.000, 0.000] |
