# `sybil-proof-protocol`

Portable, versioned handoff types shared by the sequencer-side proof-job
outbox and the standalone prover service.

## Read first

- [[ZK Integration Path]], [[Block Witness]], and
  [ADR-0019](../../docs/adr/0019-epoch-stark-prover-service.md)

## Boundaries

- Types and deterministic validation only: no filesystem, network, process,
  clock, sequencer store, or OpenVM SDK dependency.
- IDs are content-derived. Never let callers assign trust-bearing identifiers.
- `ProofKind` is the trust boundary. Only `OpenVmEvm` is L1-submittable; do not
  replace that distinction with a boolean or free-form string.
- Format/domain changes are protocol migrations and require explicit version
  bumps and fixtures.

Run `cargo test -p sybil-proof-protocol` and the relevant `sybil-zk` checks.
