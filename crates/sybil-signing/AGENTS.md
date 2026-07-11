# `sybil-signing`

Stable, dependency-light Borsh layouts for client-authorized actions. This is
the only home for ordinary client canonical signing bytes.

## Read first

- [[P256 Authentication]] and [ADR-0007](../../docs/adr/0007-canonical-bytes-domain-separation.md)

## Invariants

- Field order, enum tags, optional encoding, fixed array sizes, and domain
  inputs are signature protocol. Never reorder or “clean up” them casually.
- Genesis-bound actions must remain bound to genesis; deployment/state-bound
  key-operation rules must match their verifier owner.
- Mirror types here to avoid pulling engine/sequencer dependencies into
  clients, then test conversion at both boundaries.
- Do not add hashing/signing policy to canonical byte builders.
- Snapshot changes require an intentional coordinated migration and review.

Run `cargo test -p sybil-signing`; inspect `insta` diffs rather than accepting
snapshot updates blindly.
