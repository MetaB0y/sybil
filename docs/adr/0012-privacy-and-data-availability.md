---
adr: 0012
title: Privacy & data-availability model — public root+proof, private contents
status: Accepted
date: 2026-07-07
validity_critical: true
supersedes: []
superseded_by: []
---

# ADR-0012 — Privacy & data-availability model

> **Founder-audit note.** This is the headline decision of the 2026-07-07 reset.
> The one place your steer most changes the shape is the **DA fork** in §Decision
> point 4 (self-custody vs encrypted-DA) — that's the thing to veto if my read is
> wrong. Everything else here is "stop leaking + keep it simple."
>
> **Resolved 2026-07-07 (founder confirmed encrypted-DA, no veto).** The concrete
> construction — HPKE per-account blobs sealed to a passkey-PRF-derived *view
> key*, stored at blinded slots in a mirrorable bucket, with a one-word-per-block
> L1 DA-commitment + availability challenge, and self-custody demoted to an
> optional robustness cache so **users need not self-custody anything** — is
> specified in [`design/data-availability-design.md`](https://github.com/MetaB0y/sybil/blob/main/design/data-availability-design.md).
> Cost: ~1 hash/block on-chain + ~300 MB static storage at 1 M accounts.

## Context

Sybil is a **private** validium: individual balances, positions, orders, and
fills must never be visible to third parties. **Today they are fully exposed** —
the DA payload is a plaintext full-state snapshot served by *unauthenticated*
public endpoints (`/v1/da/{height}/payload`), and `/v1/accounts/{id}`,
`/portfolio`, and `/proofs/state` return any account's data to anyone. This is
acknowledged scaffolding (SYB-120 encrypted DA, SYB-60 auth), but it is the exact
leak the private-validium requirement forbids.

The tension a validium must resolve: users must be able to **exit even if the
operator vanishes** (that needs data to be *available*), while **nothing
per-account leaks** (that needs data to be *private*). The insight that dissolves
it: a state inclusion proof is made of **opaque hashes** — publishing the hash
structure reveals *nothing about contents*; only the **leaf preimage** (the
account's actual balance/positions) is secret.

## Decision

1. **Public (leaks nothing):** the **state root**, the **validity proof**, and
   the **opaque qMDB hash structure** a user needs to build an inclusion proof for
   their own leaf. Hashes are one-way; this reveals at most rough account *count*,
   never contents.
2. **Private (never public):** all **leaf preimages** (balances, positions,
   reservations) and the **full block witness** (which carries per-account orders
   and fills). The witness is a **private input to the prover**, consumed inside
   the guest — it is *not* a DA payload. This directly corrects
   [ADR-0006](0006-witness-v3-full-snapshot.md)'s "publish the full snapshot to
   DA" framing: the *snapshot* stays private; only the *root+proof+hashes* are public.
3. **Close the current leaks now:** every per-account read
   (`/accounts/{id}`, `/portfolio`, `/proofs/state`, block/witness endpoints)
   becomes **owner-authenticated** — you can only read *your own* account, proven
   by your key or a read-scoped token. Remove the plaintext full-witness public DA
   endpoint. (This is mostly finishing SYB-60 + SYB-120's intent.)
4. **The exit-data guarantee — the fork:**
   - **(Recommended target) Encrypted-per-account DA.** Publish each account's
     state **encrypted to a key the owner can derive from their account key**
     (e.g. ECIES to their P256 pubkey) to cheap storage, alongside the public hash
     structure. Anyone-can-exit: a user pulls their blob, decrypts, rebuilds their
     proof — robust *and* private, even if they kept no local copy.
   - **(Simpler fallback) Self-custody.** Publish only the hash structure; the
     client silently caches the user's own leaf preimage + path each session.
     Simplest and fully private, but a user who loses their cache *and* the
     operator is gone is stuck.
   - **Decision (resolved 2026-07-07): encrypted-per-account DA**, concretely the
     three-layer HPKE-to-view-key scheme in
     [`design/data-availability-design.md`](https://github.com/MetaB0y/sybil/blob/main/design/data-availability-design.md).
     Self-custody is retained only as **Layer 1** (an optional local cache), *not*
     the exit requirement — the operator-hosted encrypted blobs let a user on a
     fresh device re-fetch-and-decrypt with nothing kept. Phased:
     Phase 1 = HPKE blobs + auth-gated reads (also fixes the live leak);
     Phase 2 = the per-block L1 DA-commitment + availability challenge
     (no-backward-compat, [ADR-0011](0011-validium-stance-no-backcompat.md), makes
     the Phase-2 upgrade free).

## Alternatives considered

- **Public plaintext DA (today's build).** Rejected — total leak; incompatible
  with a private validium.
- **Full data on-chain (a rollup).** Rejected — that's not a validium; expensive
  and still public.
- **No DA at all.** Rejected — users couldn't exit if the operator vanished.

## Consequences

**Good:** privacy becomes a structural property (only opaque hashes are public);
the "you can always exit" guarantee is preserved (via encrypted-DA) without
leaking; the witness is correctly reframed as a private prover input, shrinking
the public surface to root+proof+hashes.

**Costs / constraints:** requires (a) auth-gating every per-account read — a real
API change, and a footgun to get right (no endpoint may return another user's
data); (b) an encryption scheme for the DA target — kept simple (per-account blob
sealed to a user-derivable key), but it is new crypto + key-management; (c) the
matching engine still *sees* all orders to clear a batch — this ADR makes the
data private **at rest and in transit**, not from the operator; operator-blind
batches are a separate, later step
([sealed-bid-batch-auctions](https://github.com/MetaB0y/sybil/blob/main/design/sealed-bid-batch-auctions.md)).

**Follow-ups:** SYB-60 (auth-gate reads), SYB-120 (encrypted DA); update
[ADR-0006](0006-witness-v3-full-snapshot.md) framing; the escape guest
([ADR-0013](0013-exit-and-escape-model.md)) consumes a private leaf preimage +
public hash path.
