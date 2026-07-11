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
```

Run `--help` for the current preset and flag list; do not duplicate clap's complete contract here.

## Solver names

| Name | Implementation |
|---|---|
| `lp` | HiGHS LP plus budget-linearized re-solve; production default |
| `iter-lp` | Damped fixed-point LP |
| `eg` | Eisenberg–Gale / Frank–Wolfe |
| `conic` | Clarabel conic modes |
| `milp` | Feature-gated SCIP reference |
| `decomposed` | Per-group coordination experiment |
| `all` | Compare enabled implementations |

## Invariants

- Compare integer landed outputs and recomputed welfare, not raw floating objectives.
- Treat a timeout/incumbent separately from a proven MILP optimum.
- Keep scenario generation in `matching-scenarios`; keep reporting/export in this crate.
- Full-sequencer behavior belongs in `matching-sequencer`'s simulation path or API restart tests.

## Testing

```bash
cargo test -p matching-sim
cargo run --release -p matching-sim -- --preset quick --solver all
```

