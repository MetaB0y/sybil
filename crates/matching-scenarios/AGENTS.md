# `matching-scenarios`

Synthetic and replayable `Problem` generation for tests and solver research.

## Read first

- [[Crate Dependency Map]] and [[Payoff Vectors]]

## Boundaries

- Scenario source owns presets and configuration fields; do not copy their
  changing values into documentation.
- All random generation is seeded and must remain reproducible.
- Scenario sizes are whole shares and are converted to protocol quantity units
  at generation.
- Generated profiles are structural stress tests, not calibrated live order
  flow. Replay corpora have their own schema and pinned fingerprints.
- Keep generation independent of solver behavior; every compared solver must
  receive a byte-identical problem.
