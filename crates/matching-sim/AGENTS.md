# AGENTS.md

## Purpose

`matching-sim` is the solver-level CLI harness. It creates synthetic `Problem`s through `matching-scenarios`, dispatches one or more solvers, builds verification material, compares trusted integer results, and optionally exports visualization JSON. It does not exercise the full sequencer lifecycle.

## Read first

- [[Solver Landscape]]
- [[Crate Dependency Map]]
- [[Four-Layer Verification]]

## Common commands

```bash
just sim-quick
just sim-small
just compare
cargo run --release -p matching-sim --features milp -- --preset small --solver milp --milp-timeout 60 --mm-mode exact
cargo run --release -p matching-sim --bin solver-experiments -- --output-dir /tmp/solver-smoke --smoke --overwrite
```

Run `--help` for the current preset and flag list; do not duplicate clap's complete contract here.

## Solver names

| Name | Implementation |
|---|---|
| `retained-cash` / `rfw` | Certified retained-cash generalized Frank–Wolfe; default |
| `lp` | HiGHS LP plus budget-linearized re-solve; risk-neutral baseline |
| `conic` | Clarabel conic modes |
| `milp` | Feature-gated SCIP reference |
| `decomposed` | Per-group coordination experiment |
| `all` | Compare enabled implementations |

## Invariants

- Compare integer landed outputs and recomputed welfare, not raw floating objectives.
- Compare the retained-cash objective on every allocation when evaluating the
  paper algorithm; do not rank it only by linear welfare.
- Treat a timeout/incumbent separately from a proven MILP optimum.
- Keep algorithm termination separate from verifier validity: a capped iterate
  may verify, and an empty numerical failure may be vacuously verifier-valid.
- Controlled experiments must not enable or emulate cross-solver fallback.
- Keep scenario generation in `matching-scenarios`; keep reporting/export in this crate.
- Full-sequencer simulations belong in `sequencer-sim`; durability/restart
  behavior belongs in `matching-sequencer` and API integration tests.

## Testing

```bash
cargo test -p matching-sim
cargo run --release -p matching-sim -- --preset quick --solver all
cargo test -p matching-sim --bin solver-experiments
```

The preregistered protocol, integrity rules, full-run command, and generated
artifact contract are documented in `benchmarks/solver/README.md`.
