---
adr: 0016
title: Public market tape, private canonical blocks, and distinct recovery DA
status: Accepted
date: 2026-07-12
validity_critical: false
supersedes: []
superseded_by: []
---

# ADR-0016 — Public market tape, private canonical blocks, and distinct recovery DA

## Context

ADR-0012 correctly classifies orders, fills, balances, positions, leaf
preimages, and the full witness as private. The implementation nevertheless
served the full canonical `BlockResponse` on public REST, SSE, and WebSocket
routes. That response included account-attributed fills and rejections, key
events, bridge leaves, and derived order-lifecycle rows. Public account reads
and raw witness payloads were later authenticated, but the block feed made most
of the same activity reconstructible.

ADR-0012 also chose "one encrypted blob per account" using a stable account
seed and a passkey PRF-derived view key. That key construction is not complete:
WebAuthn credentials sign rather than perform ECDH, PRF support is optional and
credential-specific, and the design does not define a safe add/revoke flow for
the stable account seed. Separately, operator replacement needs a complete
exchange snapshot, not only user slices. These are different availability
products and must not share one vague "DA" promise.

## Decision

1. **The public block API is an explicit market-tape projection.** It contains
   the hash-chain header, state/events roots, counts, clearing prices, aggregate
   welfare/volume/order statistics, the bridge deposit commitment/count, and a
   sanitized list of resolved market ids. It has no type-level fields for fills,
   rejections, account-bearing system events, individual bridge rows, or
   lifecycle sidecars.
2. **Full canonical blocks are private service data.** The existing v1
   WebSocket shape remains available only behind service authentication. The
   privacy-preserving public replay protocol is `/v2/blocks/ws`; REST and that
   stream return the same public projection. The original live-only SSE route
   was retired after first-party Python consumers migrated to resumable replay.
   Canonicality means the operator stores and proves a row, not that the row
   must be publicly presented.
3. **Exact account financial publication is opt-in.** A leaderboard row exists
   only while the account has a non-empty display name set by the signed profile
   operation. The settings UI must say that opting in publishes the account id,
   PnL, ROI, open-market count, and equity. Clearing the name removes future
   publication. Stable pseudonyms alone are not privacy.
4. **Public Arena data is a deliberate service-owned demo surface.** Bot
   decisions/equity and their metrics are generated from the operator-controlled
   Arena database, not arbitrary user account reads. They are outside the user
   confidentiality promise. Public open-batch price, volume, and participant
   counts are retained as aggregate market data for devnet; exact pre-seal order
   rows remain private. Before real-money launch, low-anonymity aggregate timing
   needs a separate product decision (delay, threshold, or removal).
5. **Production privacy is not claimed in dev mode.** Dev mode intentionally
   bypasses owner/service authorization and mounts diagnostics. Shared or
   real-money environments must run the production profile.
6. **Read bearer tokens are session-scoped.** Non-secret account and passkey
   metadata may persist, but the revocable read bearer is stored in browser
   session storage and old persistent copies are deleted. This reduces
   cross-restart exposure; it is not presented as an XSS defense.
7. **User recovery DA and operator-replacement DA are separate protocols.**
   User recovery needs a `CustodySnapshot`-equivalent bundle encrypted once per
   active recovery credential and fetched through an opaque, non-account-id
   slot. A WebAuthn recipient requires an actually tested PRF ceremony and a
   derived encryption key; the current signing ceremony is insufficient.
   Operator replacement instead needs an encrypted full epoch snapshot and an
   independent key-release/retention policy; emergency threshold release may
   intentionally sacrifice post-failure confidentiality. Neither future
   protocol is represented as implemented by service-gating today's plaintext
   witness artifact.

## Alternatives considered

- **Redact variants inside the existing canonical DTO.** Rejected. A future
  system-event variant could leak by default, and empty private arrays falsely
  look like “no activity.” A separate allowlisted type makes the boundary
  reviewable.
- **Keep the public v1 stream and silently change it.** Rejected. Its versioning
  contract says breaking changes use a new endpoint.
- **Publish every leaderboard account under a pseudonym.** Rejected. Exact
  equity and PnL tied to a stable pseudonym remain private financial data.
- **Implement account-seed encryption exactly as ADR-0012 sketches.** Rejected
  until credential derivation, capability negotiation, and revocation are
  specified and exercised on real authenticators.
- **Use the threshold-released full snapshot for user-private recovery.**
  Rejected. It enables operator replacement but makes all state public when the
  recovery key is released.

## Consequences

Public consumers can verify the header chain and render prices/aggregate market
activity, but cannot replay individual account transitions. Owner activity must
come from authenticated account history. Service consumers that truly require
canonical rows must authenticate to the v1 stream.

The API change is intentionally breaking before launch. It does not alter
consensus state, witness bytes, guest fingerprints, or genesis.

This closes the current structured third-party disclosure, but encrypted user
recovery publication remains a named implementation gap. The next recovery-DA
tranche must include recipient registration, bounded fan-out, HPKE vectors,
opaque storage, complete inclusion/exclusion proofs, custody decryption, and
physical WebAuthn PRF compatibility tests. Operator-replacement snapshot
retention remains a separate large work item.
