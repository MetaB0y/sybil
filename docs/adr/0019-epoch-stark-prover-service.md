---
adr: 0019
title: Epoch STARK proofs from a durable standalone prover service
status: Accepted
date: 2026-07-14
validity_critical: true
supersedes: []
superseded_by: []
---

# ADR-0019 — Epoch STARK proofs from a durable standalone prover service

## Context

The transition guest currently proves one block, while the host worker only
prepares guest inputs and marks proof generation `not_started`. Submitting an
EVM-wrapped proof for every ten-second block is operationally and economically
unacceptable today, and the EVM/Halo2 wrapping path has a materially higher
memory requirement than the root STARK path. At the same time, delaying proof
work must not make gaps irrecoverable: the sequencer retains only two qMDB slots,
so an old block witness is insufficient to reconstruct its authenticated leaf
proofs after those slots rotate.

Ordinary order and cancel signatures are checked at admission but discarded
before witness construction. Their cross-block replay nonce is persisted but is
not committed by `state_root`. A proof therefore establishes transition
consistency without establishing those user intents.

## Decision

Sybil proves **fixed, contiguous multi-block epochs** with one monolithic OpenVM
guest execution. The normal production-capable mode produces and locally
verifies an aggregated OpenVM **STARK** proof. EVM/Halo2 wrapping is an explicit
backend switch over the same epoch input and is disabled until its resource and
submission path are intentionally enabled. Per-block proofs remain a diagnostic
tool, not the settlement cadence.

The prover is a standalone, restart-safe service. The sequencer transactionally
persists a portable proof job for every locally produced committed block before the qMDB slot can
rotate. The prover ingests those jobs idempotently, assembles deterministic
epochs, leases attempts, writes content-addressed artifacts, and resumes from its
durable state after a crash. A typed mock backend emits the same proof envelope
for end-to-end tests, but mock and STARK envelopes are structurally ineligible
for L1 submission; only the EVM variant can cross that boundary.

A non-genesis witness imported into an empty store is an explicit recovery
checkpoint, not a replay of its incoming transition; capture resumes with its
first locally produced child.

The epoch guest also proves ordinary Raw-P256/WebAuthn order and cancel intent.
The next witness/state schema commits the per-account replay nonce and retains
the exact authorization envelopes until the block. Because these actions make
acknowledgement order validity-sensitive, they ride one globally sequenced
acknowledged-write log rather than independently ordered WAL tables.

The detailed formats, state machines, crash model, and staged implementation are
in the repository document `design/epoch-prover-service.md`.

## Alternatives considered

- **One proof and one L1 submission per block.** Architecturally simpler and may
  become viable later, but it is too expensive now and couples block cadence to
  the slowest proof/wrap/submission stage. Epoch size stays configurable so it
  can converge to one without another protocol redesign.
- **Produce EVM/SNARK proofs immediately.** Deferred. It adds memory, key
  material, trusted-setup artifacts, and L1 concerns before the durable STARK
  pipeline itself is proven. The backend switch preserves the path.
- **Recursively combine independently produced block STARKs.** Deferred until
  parallel proving is necessary. OpenVM's deferral path adds another guest,
  verifier variant, key family, and SDK-only orchestration. One epoch execution
  is the smallest system today.
- **Let a watcher reconstruct jobs from old witnesses.** Rejected. qMDB's A/B
  slots make proof material unrecoverable after rotation, and polling creates a
  correctness race during outages.
- **Keep user authorization admission-only.** Rejected. It leaves the operator
  able to prove a transition containing user actions whose intent the guest
  never checked.

## Consequences

**Good:** proof cost is amortized across blocks; the initial service avoids the
EVM wrapper's memory burden; every committed block remains provable after a
long prover outage; crash recovery and retries have one durable authority; mock,
STARK, and EVM use the same envelope and observability; signatures, active keys,
and replay nonces enter the validity statement together.

**Costs / constraints:** proof-job construction becomes part of the block
commit path and must be benchmarked; epochs add proof latency and a larger
re-prove unit; the nonce and authorization work changes the state root, witness,
guest commitment, and genesis; the acknowledged-write log must be unified before
authorization order becomes validity-sensitive; L1 cannot accept STARK or mock
envelopes, so testnet root submission still waits for the EVM backend switch.

**Follow-ups:** implement the phases in the linked plan; repin the guest once
epoch verification and ordinary authorization land together; run the STARK
service through a sustained crash/restart soak before enabling the EVM backend.
