---
tags: [design, ratification, consensus, keys, escape]
layer: core
status: awaiting-ratification
last_verified: 2026-07-07
---

# Keys-digest + Escape-claim — Ratification Packet

One decision pass to unblock the **D-cluster** implementation (SYB-225 proven
key-ops, SYB-32 escape claims). Both designs are otherwise complete:
[`account-keys-digest.md`](../design/account-keys-digest.md),
[`escape-claim-guest.md`](../design/escape-claim-guest.md). Every item below has a
**recommendation** — you can ratify by exception (just flag the ones you'd change).

These two features share one consensus batch: they both add the
account-leaf `keys_digest`, both need in-guest P-256 verification (one OpenVM ECC
integration, moves `app_vm_commit`), and both ride the **fresh-genesis redeploy**.
Ratify them together.

---

## A. The one scope lever that dominates everything: WebAuthn-in-guest?

**D0 — Does the v1 guest verify WebAuthn assertions, or raw-P256 signatures only?**

- **Recommendation: raw-P256 only for the v1 guest.** WebAuthn deferred.
- Why: this is the single biggest effort/risk driver. Raw-P256-only key-ops +
  escape = **~1.5 weeks**; adding WebAuthn assertion verification in-circuit
  (authenticatorData || SHA-256(clientDataJSON) envelope parsing) pushes it to
  **2–3 weeks** and enlarges the guest attack surface.
- Consequence you're accepting: **WebAuthn-only accounts must register a raw-P256
  backup key** before escape mode is credible for them (they can still trade via
  WebAuthn at the API boundary — the API verifies WebAuthn today; only the
  *proven* key-op and *escape* paths would require the raw key). The API can
  enforce "≥1 raw-P256 key" at account creation for funded accounts.
- If you'd rather not impose backup keys: pick WebAuthn-in-guest v1 and eat the
  +1 week. This is the fork; everything downstream (D-cluster effort estimate,
  the guest crypto surface) hangs on it.

This resolves **keys-digest Q2** and **escape-claim Q1(WebAuthn)** together.

---

## B. keys_digest design decisions (from account-keys-digest.md §Open Questions)

| # | Decision | Recommendation | Rationale / cost of the alternative |
|---|---|---|---|
| **D1** | `KeyScope` → consensus authorization, or stays cosmetic? | **Cosmetic in v1.** Do not digest it. | Today every registered key can sign every mutation; `scope` is a UI label. Making it consensus without verifier behavior attached is fake rigor. If agent-keys later need "trade-but-not-withdraw", introduce a real `capability_mask` then — a clean additive move. |
| **D2** | Replay guard for key-ops: state-bound (`pre_keys_digest`+`pre_events_digest`) vs a new proven `auth_nonce` in the account leaf? | **State-bound signatures.** | Adds zero new account-leaf state. Because key-ops update `events_digest`, the same authorization can't replay after the first accepted mutation (unless BLAKE3 breaks). A per-account `auth_nonce` is more familiar but is *more* consensus state to carry and prove. |
| **D3** | Key-op authorization: inline in `SystemEventWitness`, or split public event + private auth witness? | **Inline in the system event, v1.** | Key-ops are rare; auditability matters; a 1:1 canonical binding is simpler to get right. Split only if event-bloat ever bites. |
| **D4** | Forbid zero-key user accounts once v4 is live? | **Yes for funded/trading accounts; MINT + internal/dev may keep the empty-key digest** (and are excluded from user escape claims). | Any account with user funds must be escapable → must have ≥1 key at creation. |
| **D5** | Separate `acct_keys/{id}` typed leaf, or digest-in-`acct/` + witness sidecar only? | **Digest-only in `acct/`, no second leaf.** | The account-leaf `keys_digest` gives escape proofs; the witness key-set sidecar gives replacement-operator recoverability. A second typed leaf duplicates state and raises proof cost for zero gain. |
| **D6** | First-key introduction at account creation | **Add `initial_keys` to the `CreateAccount` system event.** Service/operator is the authority for the first key on service-created accounts; future L1-deposit-created accounts bind the first key to the deposit derivation. | Kills the current *public, unsigned* `POST /keys` first-key path for production (the SYB-229 class of hole, but at the schema level). Subsequent key-ops must be signed by an existing active key. |

---

## C. Escape-claim design decisions (from escape-claim-guest.md §7)

| # | Decision | Recommendation | Rationale / cost of the alternative |
|---|---|---|---|
| **D7** | Input forms: both **P** (full DA payload) and **L** (qMDB leaf proofs), or L-only? | **Both — but build L first.** | L is the *user-floor* path: it works from the two-file custody snapshot with **zero DA dependency** — an escape path that needs the operator's DA isn't an escape path. P shares the reconstruction lib with the custody CLI and is the convenience path. L-only would halve the surface but re-makes DA a single point of failure. Ship L, then P. |
| **D8** | Freshness rule for the claim root R: **newest-accepted-root-only**, or deduction bookkeeping (track normal withdrawals finalized at roots ≥ escape activation and net them out)? | **Newest-accepted-root-only.** | Zero new vault bookkeeping. Binding claims to the freshest accepted root means any already-finalized withdrawal is already reflected in the balance — no double-recovery. The freshest root is exactly what escape mode certifies as stale-but-final. |
| **D9** | Escape claims bypass the **withdrawal delay** and the **pause flag**? | **Confirm both bypasses.** | The delay exists to let the *operator* react; in escape mode the operator is the failure, so no delay. Pause exists for operator/governance response; escape exists for when governance *is* the problem, so `pause()` must **not** gate `escapeClaim` — needs an explicit carve-out from the SYB-96 single `paused` flag (escape checks only `escapeModeActive`). This is a **contract change** to flag in the vault. |
| **D10** | Nullifier granularity | **One claim per account per root** (already in the spec; not per-amount). | X is already the maximum safe amount; partial claims complicate accounting for no benefit. Listed for confirmation only. |

Escape-claim Q4 ("approve adding the key commitment to `acct/` leaves") is **already
answered** — that *is* SYB-225 (keys_digest), which you've approved. Noted here only
so the two docs stay consistent.

---

## D. What's locked regardless (FYI, no decision needed)

- **Digest**: `keys_digest = SHA256("sybil/state/account-keys-digest/v1" ||
  account_id || key_count || sorted(auth_scheme:u8 || pubkey_sec1[33]))`, appended
  after `events_digest` in the account leaf. Empty set = the domain/count hash, not
  `[0;32]`.
- **Consensus batch**: canonical witness **v3 → v4**; account-leaf bytes + every
  state root change; `decode_canonical_witness_bytes` updated; new key-op system
  events (tags 7+); guest repin. Batched with SYB-224's `genesis_hash` domain
  discipline (key-op + escape canonical bytes both lead with `genesis_hash[32]`).
- **In-guest P-256** via OpenVM's **accelerated ECC extension** (secp256**r1**/P-256),
  not soft `p256`. **Moves `app_vm_commit`** — first VM-commit move since
  `0x0026ab66`. (Feasibility/wiring under active investigation — see the OpenVM
  ECC note; if OpenVM turns out not to support P-256, D0 and the whole ECC plan
  reopen, so this is the gating technical unknown.)
- **Rollout**: fresh genesis (pre-redeploy). Recreate genesis with full active
  key-sets; every funded account initialized with ≥1 key; repin guest.

---

## E. Fastest ratification

Reply with just the exceptions, e.g. *"D0 raw-only ✓, D8 newest-root ✓, all recs
accepted except D1 — I want scope as a real capability_mask now."* Everything not
flagged is taken as the recommendation. That unblocks codex to start the SYB-225
increment 1 (commitment types + encoders + golden vectors) the moment the
god-split lands and the OpenVM ECC feasibility is confirmed.
