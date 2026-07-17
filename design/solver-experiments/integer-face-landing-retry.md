# Full tangent-face integer landing

Date: 2026-07-17

Status: accepted development change; public-snapshot, synthetic, and replay
evidence, not held-out paper evidence.

## Experiment IFL-001 — conditional full-face retry

### Counterexample

The frozen public-depth baseline contains a loose-budget culture case where
both structural retained-cash solvers certify a continuous objective of
`$123.963552` with effectively zero gap, then land at `$121.010180`. The
`$2.953372` loss is repeated inside the connected 50-market portfolio.
Clarabel and LP-SLP land the culture case at the continuous objective, so this
is not an infeasible integer optimum.

Landing diagnostics show that all three restricted candidates—nearest tangent
face, primary LP basis, and rounded certified target—have the same
`$121.010180` retained objective. The restriction
`max_fill_i = ceil(q_i)` has excluded an integer-friendly point on the larger
supporting face; changing the selector among those three candidates cannot
recover it.

### Hypothesis

At a converged retained-cash optimum, the final gradient defines a global
supporting matching LP. Re-solving on its full order bounds and selecting the
point nearest the certified target remains on that tangent face. Its nonlinear
retained objective may be worse because the tangent face can span different MM
utility vectors, so the expanded result must be compared against the ordinary
restricted landing and accepted only when its actual verifier-ready retained
objective is higher.

To avoid routine latency cost, retry only when restricted landing loses more
than one basis point of the continuous objective or fails post-processing.
Zero-budget MM orders remain closed.

### Candidate

`support_and_finalize_target_with_face_retry` performs the existing restricted
landing first, computes its actual retained objective, and conditionally calls
the same supporting-price and integer-landing pipeline with original order
bounds. It introduces no new optimizer, tolerance, or fallback solver.

### Protocol

The candidate was evaluated on the exact recorded baselines preceding this
change:

| Matrix | Candidate artifact | Records |
|---|---|---:|
| Frozen public depth | `/tmp/public-depth-face-retry` | 60 |
| Sequencer replay | `/tmp/solver-replay-full-face-landing` | 576 |
| Pacing development | `/tmp/pacing-full-face-landing` | 630 |
| Structural-oracle development | `/tmp/structural-full-face-landing` | 244 |
| Price-pacing development | `/tmp/price-pacing-full-face-landing` | 236 |
| **Total** | | **1,746** |

The public matrix used
`benchmarks/solver/protocol-public-depth-development.json`; the remaining
commands followed the corresponding checked-in protocol and the standard
runner:

```bash
cargo run --release -p matching-sim --all-features \
  --bin solver-experiments -- \
  --protocol <protocol.json> \
  --source-revision full-face-landing \
  --output-dir <artifact> --overwrite
python3 scripts/benchmarks/analyze_solver_experiments.py <artifact>
```

All declared records were present with matching fingerprints. No candidate
became verifier-invalid and the retry introduced no new failure.

### Quality and availability

The frozen culture event and its connected-portfolio embedding both recovered
the full `$123.963552` continuous objective:

| Case | Restricted loss | Expanded-face loss |
|---|---:|---:|
| Culture, loose budget | `$2.953372` (`2.382452%`) | `$0` |
| Connected 50-market portfolio, loose budget | `$2.953372000156` | `$0.000000000156` |

Consequently, bundle maximum retained gap on the public corpus fell from
`2.3825%` to zero; RC-FW maximum fell from `2.3825%` to `0.1678%`.
The remaining RC-FW maximum is continuous-solver error rather than integer
landing loss.

The complete replay had 19 retained-cash rows above the one-basis-point retry
threshold. Seven expanded candidates were accepted and all seven strictly
improved landed retained objective; none became worse. The pacing matrix
removed a `$205.636586` bundle landing loss and recovered numerical-range seed
`16203` from `PostProcessingFailure`. The structural matrix recovered the
HiGHS and structural bundle variants of seed `21200`; the price-pacing matrix
recovered bundle seed `19303`. These four availability gains retained
minting-duality residuals below the existing `$0.05` gate.

Other improvements were smaller but broad: replay changed seven rows, and
pacing/structural/price-pacing contained additional RC-FW and bundle rows whose
landed objective rose. Existing RC-FW iteration-limit and Clarabel numerical
failures were unaffected, as expected.

### Runtime trade-off

The retry is deliberately conditional, but a failed expanded candidate still
costs an additional landing LP. In the public run it added about `4.4 ms` to
the isolated culture event and about `92 ms` to its 50-market portfolio.
Across the replay, 19 of 288 retained-cash rows retried and only seven accepted
the candidate. Sequential baseline/candidate runs suggested a higher retained
solver P95 and one higher RC-FW maximum, but process noise and basis
nondeterminism make those timings directional rather than paired latency
evidence. The other broad matrices did not show a consistent slowdown.

### Decision

Accept IFL-001. It removes material quality loss and converts four recorded
post-processing failures into supported, verifier-valid results with a small
and localized implementation. The hierarchy remains:

```text
same certified final tangent
    > hard feasibility and price support
    > actual landed retained objective
    > proximity to one continuous representative
```

Do not add a more elaborate pre-gate yet. A cheap predictor might avoid the 12
replay retries that were ultimately rejected, but it could also suppress the
availability recoveries, and the current one-basis-point rule is simple and
auditable. Revisit a face-opportunity gate or reuse of the primary LP model if
paired production-like latency shows that the extra tail solve is material.
