---
adr: 0003
title: Guest-safe / host-only crate split
status: Accepted
date: 2026-07-07
validity_critical: true
supersedes: []
superseded_by: []
---

# ADR-0003 — Guest-safe / host-only crate split

## Context

The state-transition logic runs in **two places that must agree byte-for-byte**:
natively in the sequencer/verifier, and inside the **OpenVM zkVM guest**. The
guest is a constrained `no_std`-ish RISC-V target: it cannot pull in threads,
async runtimes, `tokio`, `commonware` networking, redb, or most of the host
ecosystem. Any dependency that reaches the shared logic must therefore be
guest-compilable — and, more subtly, must compute **identical bytes** on both
targets (no platform-dependent hashing, ordering, or float behavior).

## Decision

Partition the workspace into a **proven core** that is guest-safe and a
**shell** that is host-only, and never let the shell's dependencies leak into the
core:

- **Proven core (guest-safe, byte-identical native↔guest):**
  `matching-engine` (the `Order`/`Fill`/`Problem` types + settlement math),
  `sybil-verifier` (canonical schemas, state-root/witness encodings,
  `commitments`), and `sybil-zk` (the guest-safe transition verifier + qMDB
  proof structs). Settlement (`compute_fill_settlement` / `derive_minting`) is
  shared *verbatim* between sequencer and verifier.
- **Shell (host-only):** `sybil-api(+types)`, `sybil-client`,
  `sybil-polymarket`, `sybil-prover` host tooling, `sybil-l1-*`, the sim crates,
  and the Python `arena/`.

`matching-engine` is the pure leaf every crate depends on; the guest imports the
core and nothing else. Source: `docs/architecture/Crate Dependency Map.md` (Key
Properties), historical `design/archive/planning/architecture-review-2026-07.md` P7/§1.

## Alternatives considered

- **One crate with `#[cfg]`-gated guest/host code.** Rejected: `cfg` soup is
  where byte-divergence bugs hide (a host-only branch subtly changing an
  encoding); a crate boundary makes "is this guest-safe?" a compile-time fact.
- **Duplicate the proven logic in a separate guest crate.** Rejected: two
  copies of the settlement math is the *worst* outcome for a system whose whole
  security rests on native and guest agreeing — they would drift.

## Consequences

**Good:** "guest-safe" is enforced by the dependency graph, not by discipline;
the native verifier and the guest run the *same* code, so the conformance
guarantee is structural; the attack surface inside the proof is minimized.

**Costs / constraints:** the core crates must stay austere — adding a convenient
host dependency to `matching-engine` or `sybil-verifier` can silently break the
guest build or, worse, byte-identity; contributors must know which side of the
line they're on; some code that "wants" to be shared (nice error types, logging)
has to stay in the shell. This is the discipline that makes
[ADR-0008](0008-in-guest-p256-openvm-ecc.md) (in-guest P-256) a careful move —
new crypto crosses into the core.

**Follow-ups:** finishing `sybil-verifier::commitments` / a dedicated
`sybil-commitments` crate to hold every canonical encoding once
(the historical audit's [roadmap](https://github.com/MetaB0y/sybil/blob/main/design/archive/review-2026-07-02/30-roadmap.md) 2.1) — see the consolidation design.
