# `matching-sim`

Solver-level CLI and experiment runner; it does not exercise full sequencing,
persistence, or restart behavior.

## Read first

- [[Solver Landscape]], [[Four-Layer Verification]], and
  `benchmarks/solver/README.md`

## Boundaries

- Clap and protocol files own solver names, flags, scenarios, and defaults.
- Compare landed integer outputs and verifier-recomputed welfare, not raw
  floating objectives.
- Retained-cash experiments must evaluate that objective on every allocation;
  linear welfare alone is not the algorithm-ranking metric.
- Keep timeout, iteration cap, numerical failure, and verifier validity as
  distinct outcomes. Research solvers never silently fall back.
- Publishable experiments require a versioned preregistered protocol, untouched
  seeds, complete failure retention, and an immutable implementation revision.
- Scenario generation belongs in `matching-scenarios`; full-sequencer behavior
  belongs in `sequencer-sim` and sequencer/API integration tests.
