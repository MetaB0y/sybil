# `sequencer-sim`

Dev-only agent simulation that drives the real `matching-sequencer` across
many batches. It must not become a dependency of `sybil-api` or production
sequencing.

## Read first

- [[Block Lifecycle]] and [[Testing Strategy]]
- `matching-sequencer/AGENTS.md`

## Boundaries

- Agents and metrics may use floating point; exchange mutation still goes
  through the real sequencer and integer settlement.
- Preserve deterministic seeded scenarios and report the seed/config needed to
  reproduce a run.
- Keep synthetic policy in `agent/` and orchestration in `simulation.rs`; do
  not add simulation shortcuts to the production sequencer.

Main entry point: `cargo run -p sequencer-sim --bin sybil-sim -- --help`.
Run `cargo test -p sequencer-sim`.
