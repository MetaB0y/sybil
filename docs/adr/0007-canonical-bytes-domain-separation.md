---
adr: 0007
title: Domain-separated canonical bytes with genesis binding
status: Accepted
date: 2026-07-07
validity_critical: true
supersedes: []
superseded_by: []
---

# ADR-0007 — Domain-separated canonical bytes with genesis binding

## Context

Every mutating action (order, cancel, key-register, key-revoke, withdrawal,
escape-claim) is authorized by a **P256 signature over canonical bytes**. Two
failure modes threaten any such scheme:

1. **Cross-action replay** — bytes that verify as one action type also verifying
   as another, or a signature for message A being accepted for message B.
2. **Cross-deployment replay** — a signature captured on one deployment (devnet,
   a fork, a prior genesis) being replayed on another that shares key material.

Bearer tokens compound the risk if they can authorize writes; they must not.

## Decision

Canonical signing bytes are **domain-separated and genesis-bound**:

- Each action type prefixes its canonical bytes with a **distinct domain string**
  (e.g. `"sybil/signing/account-key-op/v1"`, `"sybil/escape-claim/v1"`), so
  bytes for one action can never verify as another.
- Every action's canonical bytes lead with the **32-byte `genesis_hash`** as a
  domain separator (SYB-224/229/231), binding the signature to one deployment's
  genesis. A signature from another genesis simply doesn't verify.
- **Bearer tokens are read-only**; all writes require the signature. Replay
  within a deployment is additionally guarded (nonce for orders; the
  state-bound `pre_keys_digest`+`pre_events_digest` for key-ops, per the
  ratification packet D2).

Order/cancel/key-register/key-revoke canonical bytes all now lead with
`genesis_hash` (SYB-224/229/231, landed).

## Alternatives considered

- **Chain-id / deployment-id integer instead of genesis hash.** Weaker: an
  integer can collide or be reused across a re-genesis; the genesis *hash* is
  unique to the actual initial state.
- **Per-action nonces only, no domain string.** Rejected: nonces stop replay of
  the *same* action but not cross-action-type confusion; domain separation is
  cheap and closes that class entirely.
- **Let bearer tokens authorize writes for convenience.** Rejected: a leaked
  read token would become a write capability.

## Consequences

**Good:** cross-action and cross-deployment replay are closed by construction;
the discipline is uniform, so new signed actions get it by following the
pattern; escape-claim and key-op canonical bytes inherit the same rule
([ADR-0005](0005-escape-via-operator-replacement.md),
[ADR-0008](0008-in-guest-p256-openvm-ecc.md)).

**Costs / constraints:** the canonical-bytes builders are **validity-critical surface**
and today are **duplicated** in spots (the `hash_header`-×3 / divergent
reservation-encoder problem, recorded in the [historical audit](https://github.com/MetaB0y/sybil/blob/main/design/archive/review-2026-07-02/02-cross-cutting-themes.md) Theme 6) —
divergent copies are a genuine bug class this ADR makes urgent to consolidate;
every client (Rust, Python, frontend) must construct identical bytes or signing
silently fails; changing a domain string or field order is a breaking change for
every signer.

**Follow-ups:** consolidate all canonical encoders into one crate
(`sybil-commitments`, roadmap 2.1 / task #6) so there is exactly one definition
per encoding.
