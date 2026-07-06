# Operator Replacement & Emergency State Disclosure

Status: **DRAFT — for ratification** (SYB-116)
Author: Claude (Fable 5), 2026-07-06. Companion to
`design/escape-hatch-reconstruction.md` (SYB-80, drafted same day) — that doc
owns the reconstruction *mechanics*; this one owns *who runs them, when, and
under what disclosure model*. Cross-refs: SYB-32 (escape claim), SYB-120
(encrypted DA), SYB-222 (witness/DA API exposure).

## 1. Honest framing for the current trust phase

Today Sybil has one operator, one admin key, and the same human behind both.
"Operator replacement" therefore means two different things, and conflating
them produces fake security:

- **R-A: Disaster recovery** (real, buildable now): the operator's *infra* dies
  — box lost, store corrupted, region gone — and the SAME party restarts the
  exchange from DA-reconstructed state. No adversary; the problem is data and
  procedure.
- **R-B: Trustless replacement** (not real yet): the operator is malicious or
  permanently absent and a DIFFERENT party takes over against the operator's
  will. This requires an appointment mechanism L1 currently does not have (the
  admin appoints; if the admin IS the failed operator, there is no one to
  appoint a successor). R-B is gated on a governance decision that is
  explicitly out of scope until there is more than one operator in practice.

This doc fully specifies R-A, and shapes every artifact so R-B needs only the
appointment layer added — no new data paths.

## 2. Replacement flow (R-A, normative)

Preconditions: L1 intact (`SybilSettlement` roots, `SybilVault` funds), at
least one retrievable DA payload for an accepted root (SYB-80 §5 retention).

1. **Choose the recovery root**: newest accepted root whose payload is
   retrievable (SYB-80 §3 walk-back). Call it height H. Everything after H is
   the at-risk delta — bounded by DA retention lag, alertable (SYB-223 item 3).
2. **Reconstruct**: run the SYB-80 §3 procedure. Output: complete typed state
   at H — accounts, positions, reservations, resting orders, withdrawals,
   markets, groups, lifecycle/status, deposit frontier, `sys/` counters. Every
   family is in the witness payload; nothing needs the dead operator's disks.
3. **Boot a fresh sequencer from the snapshot** ("genesis-from-witness"): a
   boot path that imports the decoded state instead of empty genesis. The
   store already persists/restores every one of these families (they round-trip
   through it in normal operation); the missing piece is only the importer
   that populates a fresh store from a decoded witness. Ticketed as an R-A
   implementation increment (§6).
4. **Re-arm L1**: the new sequencer's prover continues from H. `parent` linkage
   and the deposit frontier make the first new block verifiable against the
   old chain; the settlement contract does not care who runs the prover — only
   that proofs verify against the pinned guest commitments.
5. **Reconcile the L1 bridge edge**: deposits that landed on L1 after H are
   picked up by the indexer from the vault log (frontier catches up
   naturally). Withdrawals already queued on L1 are unaffected (finalization
   is permissionless). Withdrawal leaves created between H and the outage are
   the one loss class: those users' funds are still accounted in their
   balances at H (leaf creation debits at creation — if the leaf is post-H it
   never existed at H, so the balance still contains the money). Net: **no
   fund loss at H; in-flight intents after H must be re-submitted.**
6. **Users**: nothing to do. Keys unchanged, balances at H proven. In-flight
   orders after H are gone (batch-auction orders are short-lived by design).

### What survives, family by family (SYB-116 acceptance item)

| Family | Source at recovery | Fidelity |
|---|---|---|
| Balances, positions | `post_state` accounts in payload | exact at H |
| Reservations | `acct_resv` in sidecar | exact at H |
| Resting orders | sidecar `resting_orders` | exact at H |
| Markets, groups, status/lifecycle | sidecar | exact at H |
| Pending withdrawals | `withdrawal/` leaves | exact at H |
| Deposit cursor/frontier | `sys/` + frontier | exact; L1 log replays the gap |
| Replay nonces | **NOT in proven state** | ⚠ gap — §4 |
| Analytics/history (candles, fills, equity) | derived views / unproven | lost beyond payload; acceptable (unproven by design, SYB-216) |

## 3. Disclosure model recommendation (SYB-116 acceptance item)

**Devnet (now): plaintext DA payloads are acceptable and already flagged as
scaffolding** (`Data Availability.md`). Dev funds, house bots, no user privacy
to protect yet.

**Before real users (public testnet): encrypt payloads; do NOT ship plaintext
full state.** Recommended shape (aligned with the SYB-80 §6 reservation so it
is purely additive):

- Payload encrypted with a per-era content key K. `da_commitment` keeps
  binding the PLAINTEXT bytes; the manifest gains `ciphertext_hash` and key
  metadata. Nothing about proofs or the guest changes.
- **Key custody, recommended for the single-operator phase**: K is escrowed as
  a 2-of-3 Shamir split — Valery + two independent holders (people or a
  KMS/HSM under separate credentials). Disclosure rule: shares may be combined
  ONLY when `escapeModeActive` is true on L1 (publicly checkable), which the
  vault already gates on root-staleness timeout. This gives R-A recovery even
  if the operator is incapacitated, without making state public in normal
  operation, and without threshold-cryptography engineering (Shamir on one
  32-byte key is a lunch-break implementation).
- Rejected for now: MPC/threshold-decryption networks (heavy, premature),
  TEE-held keys (TEE track is parked), pure governance delay (no governance
  exists), per-user encrypted leaves (breaks the one-payload reconstruction
  property and multiplies key management by user count).

**Trigger** (SYB-116 acceptance item): reuse the existing, already-deployed
condition — `activateEscapeMode()`'s root-staleness timeout — as THE single
emergency trigger for both escape claims and disclosure. One trigger, one
clock, already on chain, callable by anyone. No second mechanism until R-B.

## 4. Gaps found while writing this (both verified in code)

1. **Cross-genesis replay of signed orders/cancels.** `canonical_order_bytes`
   (and cancels) include the SYB-191 nonce but NO chain/genesis/operator-epoch
   identifier — only bridge withdrawals carry `chain_id`
   (`crates/matching-sequencer/src/crypto.rs:149,173`). Nonces reset on every
   fresh genesis (they are not in the state root — see 2), so an order signed
   before a fresh-genesis redeploy verifies again after it. Devnet impact:
   trivial. Real-funds impact: an old captured order re-executes against the
   victim's re-funded account. **Fix: fold a `genesis_hash` (or operator-epoch
   id) into the canonical signed bytes of orders and cancels — a deliberate
   canonical-bytes change to batch with the NEXT guest-commitment move.**
2. **Replay nonces are unproven state.** `acct/` leaves carry balance,
   deposits, positions, events digest — no nonce
   (`crates/sybil-verifier/src/state_schema.rs`). A replacement operator
   restores nonces from nothing (reset) or from unproven store data. With fix
   1 in place this is merely cosmetic (fresh epoch = fresh nonce space,
   cross-epoch replay dead by domain); WITHOUT fix 1 it is the enabler of the
   replay above. Decision: take fix 1; do NOT put nonces in the state root
   (they'd bloat every account leaf for a value fix 1 makes epoch-local).

Both are ticketed (§6). Fix 1 is cheap insurance and should ride whatever
consensus-touching batch comes next — not its own commitment move.

## 5. Unresolved before public testnet (SYB-116 acceptance item)

- Ratify the disclosure model (§3) — including WHO the two non-Valery share
  holders are. This is a people decision, not a code decision.
- R-B appointment mechanism: deliberately unspecified; unblock = "more than
  one credible operator exists."
- Retention SLO constants (shared open question with SYB-80 §8).
- The genesis-from-witness importer must exist and be DRILLED (restore drill
  extension of SYB-223) before we claim R-A works.
- Fix 1 (§4) must land before any deployment where signed orders move real
  value across a genesis boundary.

## 6. Implementation increments

- **OR-1 (with SYB-222/R0):** genesis-from-witness importer — decoded payload
  → fresh store; drill wired into `restore-store-drill.sh`.
- **OR-2 (next consensus batch):** `genesis_hash` in order/cancel canonical
  bytes (§4 fix 1). Rides a batched commitment move, never alone.
- **OR-3 (pre-testnet, after §3 ratified):** payload encryption + Shamir
  escrow + escape-gated disclosure procedure (implements SYB-120's decision).
- **OR-4 (parked):** R-B appointment layer.
