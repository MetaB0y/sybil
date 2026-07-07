---
adr: 0008
title: In-guest P-256 verification via OpenVM accelerated ECC
status: Accepted
date: 2026-07-07
validity_critical: true
supersedes: []
superseded_by: []
---

# ADR-0008 — In-guest P-256 verification via OpenVM accelerated ECC

## Context

Today the guest verifies **no signatures at all** — order/cancel/withdrawal/key
authorization all happens at the API/actor boundary, and the guest only proves
the state transition (its `openvm.toml` enables `rv32i, rv32m, io, sha2` and
nothing else). Two roadmap features break that assumption: **proven key-ops**
(SYB-225) and **escape claims** (SYB-32) both need to verify a **secp256r1 /
P-256 ECDSA signature *inside* the guest** — the escape guest to bind a claim to
a registered signer, the key-op path to prove a key mutation was authorized by an
existing key. A pure-Rust `p256` verify inside the zkVM would cost tens of
millions of cycles.

## Decision

Verify P-256 signatures **in-guest using OpenVM's accelerated ECC extension**
(the dedicated EC STARK chips), not a soft `p256` crate. Feasibility is
confirmed: OpenVM **v2.0.0-beta.2** — the tag already pinned — ships secp256r1 as
a first-class accelerated curve (`P256Point`, `NistP256`, drop-in `openvm-p256`
guest crate, `verify_prehash`), at parity with secp256k1. The concrete wiring
(the `openvm.toml` `modular`+`ecc` stanza with P-256's `a = -3`/`b` constants,
guest deps, generated init macros, and the `verify_prehash` API against our
compressed-SEC1 keys) is in `design/openvm-p256-integration.md`.

## Alternatives considered

- **Soft-Rust `p256` inside the guest.** Rejected: field arithmetic unrolled into
  RISC-V is enormously expensive per verify; batches of key-ops in a block
  multiply it.
- **Custom short-Weierstrass curve config.** Not needed — P-256 is built-in — but
  it's the fallback mechanism if OpenVM ever dropped it.
- **Keep all signature verification at the API boundary (status quo).** Rejected:
  it cannot support a *trustless* escape claim — the guest must independently
  bind account → signer, which means verifying the signature in the proof.

## Consequences

**Good:** trustless escape and proven key-ops become possible; one ECC
integration serves both SYB-225 and SYB-32; cost is a bounded number of
accelerated chip rows, not soft-crypto cycles.

**Costs / constraints:** enabling a new VM extension **moves `app_vm_commit`** —
the first VM-commit move since `0x0026ab66` — a *deeper* commitment change than
the source-only `app_exe_commit` repins we've done, and it drags in the SYB-228
reproducibility caveats (untracked `agg_prefix.pk`, build-path dependence; repin
in `~/sybil`). New crypto crosses into the guest-safe proven core
([ADR-0003](0003-guest-host-crate-split.md)), enlarging the in-proof attack
surface. WebAuthn-in-guest is a further step (envelope parsing) and is deferred
(ratification D0: raw-P256-only v1). Rides the fresh-genesis window
([ADR-0009](0009-fresh-genesis-for-consensus-changes.md)).

**Follow-ups:** SYB-225 increment 4 (guest support), SYB-32 escape guest;
`design/keys-and-escape-ratification.md` D0 gates the WebAuthn scope.
