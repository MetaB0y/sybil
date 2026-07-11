---
adr: 0015
title: Deposit quarantine — unresolvable deposit keys park in a committed quarantine ledger; the frontier fold never skips
status: Accepted
date: 2026-07-10
validity_critical: true
supersedes: []
superseded_by: []
---

# ADR-0015 — Deposit quarantine for unresolvable account keys

Resolves [SYB-272](https://linear.app/sybilmarket/issue/SYB-272) (L1-1, HIGH
griefing DoS). Option 1 (quarantine bucket) ratified by Valery 2026-07-10;
this ADR fixes the concrete shape after checking the design against the
deposit-frontier soundness constraints.

## Context

`SybilVault.deposit` accepts any nonzero amount with any `bytes32
sybilAccountKey`. The L1 indexer resolves that key to an account via
`GET /v1/bridge/accounts/by-key/{key}` (sequencer-maintained
account↔bridge-key mapping) and refuses to skip an unresolvable deposit,
because the cursor is gapless (SYB-254/255) and the sequencer only accepts
sequential deposit ids. Two correct properties — gapless cursor,
sequential ingest — combine into a wedge: `deposit(1 wei, random_bytes32)`
becomes deposit N, resolution fails forever, and **every later deposit is
uncredited**.

The constraint that shapes the fix: the guest's deposit accumulator
(`pre_frontier`, mirroring `SybilVault.filledSubtrees`) folds deposit
leaves **in L1 leaf order** and must reproduce the vault's deposit root.
A "skip" that omits leaf N from the fold can never reconcile with L1
again. So the only sound version of "quarantine" is one where **the leaf
still folds; only the value's destination changes.**

## Decision

1. **Every deposit folds into the frontier in order, unconditionally.**
   Disposition — credit vs quarantine — is decided per deposit; the fold
   is not.
2. **Unresolvable deposits are credited to a single system quarantine
   ledger committed in validium state**: one system-side structure (not a
   per-key account leaf) holding `raw bytes32 key → accumulated amount`,
   with its binding digest committed the same way other system/bridge
   leaves are (`state_schema.rs` sys leaves). No loss: value is inside the
   state commitment from the moment of quarantine.
3. **Disposition is witnessed and guest-constrained.** Two new system
   events (wire tags allocated after witness v6's `KeyRegistered=10` /
   `KeyRevoked=11`; expected `DepositQuarantined=12`,
   `QuarantineClaimed=13`): the guest re-derives the quarantine ledger's
   digest from `previous digest + witnessed events` exactly like the v6
   `keys_digest` re-derivation, and enforces conservation (deposit amount
   → quarantine entry on quarantine; entry → account balance on claim,
   entry removed). An unwitnessed quarantine-ledger mutation is a rejected
   block. This is **wire v7**, batched with v6 into the same fingerprint
   refresh and fresh genesis (ADR-0009: one commitment move).
4. **Claims are automatic on later key registration.** When a bridge key
   matching a quarantined entry becomes resolvable, the sequencer emits
   `QuarantineClaimed` crediting the mapped account. The guest checks the
   claim against **committed** state: the claiming account's key material
   as committed in its leaf (v6 makes key-set transitions proven). If the
   bridge-key↔account binding turns out not to be derivable from
   committed leaf fields, the implementation must add it to the leaf (or
   fold it into `keys_digest`) as part of the same v7 move — the claim
   check must never trust an uncommitted host-side mapping.
5. **Cursor semantics change from "applied" to "disposed".** The gapless
   invariant becomes: the cursor advances only past deposits that are
   either applied or durably quarantined (both state-committed). Liveness
   restored, gaplessness preserved.
6. **L1 refunds are a deferred hook, not part of this change.** Because
   quarantine entries live under the state root, a future L1 claim can
   prove membership against a submitted root and refund on-chain
   (escape-hatch machinery). Nothing in this design blocks that; nothing
   in this change implements it.

## Alternatives rejected

- **Per-key synthetic account leaves** (quarantine as real accounts keyed
  by `H(bytes32)`): attacker mints unbounded state leaves at 1 wei each —
  swaps a liveness grief for a state-growth grief.
- **Off-state indexer-side quarantine table**: the guest cannot see it, so
  value would later "appear" in an account with no constrained source —
  a soundness hole worse than the bug being fixed (vault balance ≥ L2
  supply would hold only by operator honesty).
- **Vault-side registration gate** (ticket option 2): requires an L1 view
  of L2 key registrations and breaks deposit-before-register UX.
- **Skip-with-record** (ticket option 3): "skip" is unsoundable against
  the frontier fold, per Context.

## Consequences

- A malicious sequencer *can* quarantine a resolvable deposit. That is a
  **delay, not a theft**: conservation is guest-enforced, the entry stays
  claimable, and a single sequencer can already delay any credit by
  stalling — quarantine adds no new power. Documented as accepted.
- Dust-key spam accumulates entries in the quarantine ledger (bounded
  cost: one map entry per unique key, no qMDB leaf) — cheap for us,
  purchasable-but-pointless for the attacker. A metrics counter +
  vmalert rule on quarantine-ledger size keeps it observable.
- Wire v7 ships in the same fresh genesis as v6, so devnet sees exactly
  one schema move.

## Implementation notes (for the lane)

Verify during implementation, fail closed on each: (a) exact frontier
fold call sites (`sidecar.rs` / `witness_schema.rs`) treat quarantined
deposits identically to credited ones; (b) how `get_bridge_account_id_by_key`
is populated and whether the binding is committed in the leaf today (see
Decision 4); (c) indexer retry loop terminates for quarantined deposits
(disposition recorded → cursor advances → no head-of-line retry);
(d) `SYB-255` stall alarm updated to distinguish "stalled" (bug) from
"quarantining" (working as designed, but still worth a low-priority
signal).

## Implementation verification (SYB-272)

- **(a) Frontier fold:** `sidecar.rs` and the guest's direct public-input
  binding consume the complete `deposit_accumulator.new_deposits` sequence.
  They require one `L1Deposit` or `DepositQuarantined` disposition per leaf;
  neither disposition changes the frontier recurrence.
- **(b) Binding:** `get_bridge_account_id_by_key` is populated by scanning
  committed account ids and comparing the deterministic bridge key
  `BLAKE3("sybil/bridge/account-key/v1" || account_id:u64le)`. The guest derives
  the same value from the claiming account's committed `id`; no account-leaf
  extension or uncommitted host mapping is needed.
- **(c) Cursor:** successful credit and successful quarantine both update the
  durable bridge cursor/root and return success to the indexer. A unit test
  asserts the confirmed scan range advances after a quarantined deposit.
- **(d) Observability:** unresolvable-key handling logs
  `l1.indexer.deposit_quarantining`; actual resolution/submission failures log
  `l1.indexer.deposit_pipeline_stalled`. Ledger size and amount are gauges, and
  `DepositQuarantineLedgerGrowing` is the low-severity growth alert.
