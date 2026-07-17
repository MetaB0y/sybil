# Direct scalar pacing dual

Date: 2026-07-17

Status: rejected research branch; retain the derivation and counterevidence.

## Experiment SPD-001 — certified scalar bisection

### Hypothesis

When a connected problem has one effective positive-budget MM, the
retained-cash dual has one scalar pacing factor:

```text
min_{0 < alpha <= 1} H(alpha) - B ln(alpha),
H(alpha) = max_q L(q) + alpha U(q).
```

An oracle allocation supplies subgradient `U(alpha) - B / alpha`. Directly
bracketing its zero might need fewer matching-LP calls than generalized
Frank--Wolfe or a general multi-MM bundle. Retaining every oracle allocation
as an atom and fully correcting their convex mixture preserves a feasible
primal lower bound; the best evaluated scalar dual is a global upper bound.
This is a specialized solver, not a Clarabel or HiGHS parameter change.

### Candidate

Source during development: working change `vqlrkzzr`, parent
`bd284d40545d`.

The disposable `ScalarPacingSolver`:

- accepted exactly one effective positive-budget MM;
- reused the warm-started HiGHS matching oracle and existing restricted master;
- started at `alpha = 1`, then bisected a certified scalar bracket;
- stopped only when best dual minus corrected primal met the bundle's
  `$0.0001` absolute / `1e-8` relative tolerance;
- used the shared price discovery, full-face integer retry, hard-budget fixed
  point, and verifier boundary.

The temporary protocol selected every one-MM slice already declared in
`protocol-pacing-development.json`: 15 market-like budget rows, 15 small flash
rows, and four fixed-2,000-order rows. It compared RC-FW, the existing bundle,
and scalar pacing on 34 byte-identical cases (102 records):

```bash
cargo run --release -p matching-sim --all-features \
  --bin solver-experiments -- \
  --protocol /tmp/protocol-scalar-pacing.json \
  --source-revision scalar-pacing-working-copy \
  --output-dir /tmp/scalar-pacing-development --overwrite
python3 scripts/benchmarks/analyze_solver_experiments.py \
  /tmp/scalar-pacing-development
```

### Result

All 102 records were present and fingerprint-consistent. Each scalar row
succeeded, landed, and verified. Scalar pacing matched the bundle's landed
quality: effectively zero retained gap, zero measured landing loss, and
`0.000329%` maximum allocation movement.

| Method | Success | Oracle calls P50 / P95 / max | Wall P50 / P95 / max |
|---|---:|---:|---:|
| Pacing bundle | 34/34 | 4 / 12 / 13 | 72.80 / 179.21 / 181.70 ms |
| Scalar bisection | 34/34 | 10 / 19 / 20 | 76.16 / 194.19 / 306.91 ms |

Decision: reject. Direct bisection spends extra oracle calls locating kinks in
the piecewise-linear matching-LP value function. Restricting the algorithm to
one MM adds conceptual and implementation surface without a Pareto gain.

## Experiment SPD-002 — safeguarded semismooth step

### Hypothesis

On one fixed LP basis, `U(alpha)` is constant and the dual-subgradient root is
exactly `alpha = B / U`. Taking that step when it remains inside the certified
bracket, and falling back to bisection across basis changes, should avoid most
of SPD-001's search calls without weakening its certificate.

### Result

The same 102-row protocol again completed and verified. The step reduced scalar
median oracle calls from 10 to 4, but did not remove the tail:

| Method | Success | Oracle calls P50 / P95 / max | Wall P50 / P95 / max |
|---|---:|---:|---:|
| Pacing bundle | 34/34 | 4 / 12 / 13 | 84.76 / 178.72 / 183.04 ms |
| Scalar semismooth | 34/34 | 4 / 18.7 / 21 | 94.80 / 192.29 / 302.27 ms |

Scalar retained-objective gap remained negligible (maximum approximately
`9.83e-6` basis points), and its maximum certificate was `$0.000077204`, inside
the declared tolerance. Those quality results do not compensate for a worse
call and latency tail plus a one-MM-only solver path.

Decision: reject and remove the implementation. The fully corrective pacing
bundle already adapts its oracle query to the current primal mixture; on these
books that is a better kink-finding policy than explicitly optimizing the
scalar dual.

### Revisit condition

Do not retry ordinary bisection, `B/U` fixed-point iteration, or their guarded
combination. Revisit scalar pacing only if the matching oracle exposes exact
parametric basis-validity intervals or adjacent breakpoint information, so a
query can jump to the next dual kink with a proof rather than search for it.
That would be a materially different specialized LP capability.
