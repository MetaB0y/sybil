---
tags: [infrastructure, storage, recovery]
layer: sequencer
crate: matching-sequencer
status: current
last_verified: 2026-07-10
---

# Acknowledged-Write WAL Replay

Between block-boundary snapshots, the sequencer protects every write it has
already acknowledged (returned 200 OK for) with a small per-subsystem
write-ahead table in redb. On restart those tables are replayed on top of the
last committed snapshot in a **fixed order**. This note audits every such WAL
path, maps the cross-table ordering dependencies, records the July 1 regression
that motivated the fixed order, and decides whether the separate-tables model is
still sufficient or whether we should collapse to one globally sequenced WAL.

This is the acknowledged-write half of [[Persistence]]; the snapshot/commit-fence
half (redb ↔ qMDB, two-slot A/B, recovery) is documented there and is out of
scope here except where replay interacts with it.

## Landed context this analysis assumes

Read the [consolidated system invariants](../../SPEC.md#11-consolidated-invariants)
first. The relevant persistence invariants are:

- **Block-boundary persistence (inv. 9).** The block is the transactional unit;
  no event sourcing. WAL rows are *not* an event log — they are a between-block
  patch that is folded into the *next* block's normal solve, never replayed as
  their own committed blocks.
- **redb commit fence (inv. 2).** redb is the single commit authority. Every WAL
  table is cleared in the *same* redb write transaction that flips the fence
  (`save_block_inner`), so "snapshot advanced" and "WAL emptied" are atomic.
- **Durable-before-live admission (inv. 10).** An acknowledged write is durable
  before its effect is user-visible (or, for direct admits, is rolled back if the
  durable append fails). This is what makes the WAL rows a faithful record of
  exactly the acknowledged writes.
- **OrderBook is the single reservation authority (inv. 5).** Aggregate
  reservations are re-summed from per-order reservations on replay; they are not
  trusted as standalone aggregates. This is why cross-table replay reordering is
  end-state safe (see the matrix).

## Current tables inventory

The six acknowledged-write WAL tables live under
`crates/matching-sequencer/src/store/`. Each is keyed by a monotonic `u64`
sequence. `resting_orders` is a separate rewritten snapshot row rather than a
WAL. Every WAL is cleared atomically inside `save_block`.

| WAL table | Written by (durable append) | Replayed / applied at | Cleared at |
|-----------|-----------------------------|-----------------------|------------|
| `resting_orders` (snapshot, not a log) | `save_block_inner` rewrites the whole book each block (`crates/matching-sequencer/src/store.rs:894`) | `load_state` reads it (`crates/matching-sequencer/src/store.rs:1626`); `OrderBook::restore` rebuilds reservations (`crates/matching-sequencer/src/order_book.rs:137`) | overwritten each block |
| `admit_log` | actor after `try_admit_direct`, before 200 OK (`crates/matching-sequencer/src/actor.rs:1197`); append in `crates/matching-sequencer/src/store.rs:2037` | `restore` re-inserts each row via `reinsert_for_replay` (`crates/matching-sequencer/src/sequencer.rs:824`) | `save_block_inner` (`crates/matching-sequencer/src/store.rs:1015`) |
| `pending_bundles` | actor on `AdmitOutcome::Deferred`, before 200 OK (`crates/matching-sequencer/src/actor.rs:1253`); append in `crates/matching-sequencer/src/store.rs:2008` | held in `pending_bundles` queue at `restore` (`crates/matching-sequencer/src/sequencer.rs:846`); drained by the **next block's** normal validation, not applied inline | `save_block_inner` (`crates/matching-sequencer/src/store.rs:1007`) |
| `control_plane_log` | actor `persist_control_plane`, before the in-memory mutation (`crates/matching-sequencer/src/actor.rs:1386`); append in `crates/matching-sequencer/src/store.rs:2057` | `restore` replays each `ControlPlaneCommand` in ascending seq (`crates/matching-sequencer/src/sequencer.rs:855`); dispatch in `replay_control_plane_command` (`crates/matching-sequencer/src/sequencer.rs:926`) | `save_block_inner` (`crates/matching-sequencer/src/store.rs:1022`) |
| `pending_l1_deposits` | actor `handle_l1_deposit`, after validate, before ingest (`crates/matching-sequencer/src/actor.rs:1568`); append in `crates/matching-sequencer/src/store.rs:2064` | `restore` re-ingests each deposit (`crates/matching-sequencer/src/sequencer.rs:883`) | `save_block_inner` (`crates/matching-sequencer/src/store.rs:1029`) |
| `pending_bridge_withdrawals` | actor `handle_bridge_withdrawal`, after validate, before request (`crates/matching-sequencer/src/actor.rs:1582`); append in `crates/matching-sequencer/src/store.rs:2068` | `restore` re-requests each withdrawal (`crates/matching-sequencer/src/sequencer.rs:901`) | `save_block_inner` (`crates/matching-sequencer/src/store.rs:1033`) |
| `pending_bridge_l1_inputs` | actor bridge handlers, after transition preflight and before the live mutation | `restore` replays withdrawal events and confirmed-height observations after withdrawal creation rows | the same `save_block_inner` transaction that persists bridge state |

`control_plane_log` carries a single typed enum, `ControlPlaneCommand`
(`crates/matching-sequencer/src/store.rs:479`), covering account create/fund,
pubkey registration, market create/metadata, market-group create, signed cancel,
market resolution (plain + attested), feed registration, and template install.
So it is *already* a partially unified WAL for the control plane; the open
question is only whether the order/deposit/withdrawal subsystems should join it.

Note: qMDB (fenced account + typed-state slots) is a snapshot store, not a WAL,
and is out of scope per the ticket. The off-block analytics tables
(`fill_history`, `equity_points`, `history_events`, `price_points`, the tracker
snapshots) are derived views, not acknowledged-write logs, and do not gate the
200-OK contract.

### The fixed replay order

`BlockSequencer::restore` (`crates/matching-sequencer/src/sequencer.rs:802`)
applies state in exactly this order:

1. committed snapshot state (statuses, metadata, feeds, templates, then
   `OrderBook::restore` from the `resting_orders` snapshot);
2. `admit_log` re-inserted on top of the book (`sequencer.rs:824`);
3. `next_order_id` advanced past every replayed resting id (`sequencer.rs:827`) —
   **before** any pending-bundle ids are assigned next block;
4. `control_plane_log` replayed in seq order (`sequencer.rs:855`);
5. stale resting orders expired for the restored height (`sequencer.rs:870`);
6. `pending_l1_deposits` re-ingested (`sequencer.rs:883`);
7. `pending_bridge_withdrawals` re-requested (`sequencer.rs:901`);
8. `pending_bridge_l1_inputs` replay withdrawal status events and confirmed L1
   heights only after every acknowledged withdrawal request has been restored;
9. `pending_bundles` are *not* applied here — they wait in the queue for the next
   block's normal solve.

Within a single table, ascending `u64` seq preserves exact acknowledgement
order (redb iterates keys ascending). The ordering gaps are only *between*
tables.

## Cross-table dependency matrix

"Earlier" / "Later" are positions in the fixed replay order above. A dependency
exists when the later step validates or mutates against state the earlier step
produces. "Reorder-safe?" asks whether swapping the two would still reach a
correct end state (recall: no block is produced during restore, so only the
*final* restored state must be correct).

| Earlier step | Later step | Dependency | Enforced by | Reorder-safe? |
|--------------|-----------|------------|-------------|---------------|
| `admit_log` | `next_order_id` advance | fresh ids must not collide with replayed resting ids | fold-max over replayed orders (`sequencer.rs:827`) | **No** — must precede pending-bundle id assignment; guarded by `restore_advances_next_order_id_past_replayed_admit_log_before_pending_bundles` |
| `control_plane` create/fund | `pending_l1_deposits`, `pending_bridge_withdrawals` | deposit/withdrawal validate against account existence + balance | replay order (control-plane before bridge WALs) | No — bridge validation needs the acked create/fund visible first |
| `control_plane` signed cancel | `pending_bridge_withdrawals` | cancel releases resting-order reservations that a withdrawal validates against | replay order (**the July 1 fix**) | **No** — this is the regression edge; guarded by `bridge_withdrawal_replays_after_control_plane_cancel_wal` |
| stale-order expiry | `pending_bridge_withdrawals` | TTL-expired resting orders must release reservations before a withdrawal checks free balance | expiry runs at `sequencer.rs:870`, before bridge replay | No — guarded by `restore_expires_stale_resting_orders_before_bridge_wal_replay` |
| `pending_l1_deposits` | `pending_bridge_withdrawals` | a deposit that funds a same-window withdrawal must land first | replay order (deposits before withdrawals) | Yes (monotone) — deposits only *raise* balance, so a valid-at-ack withdrawal stays valid; reorder cannot manufacture an over-withdrawal because each withdrawal was already validated at ack time |
| `pending_bridge_withdrawals` | `pending_bridge_l1_inputs` | a queued/finalized/cancelled event or expiry observation must see the withdrawal leaf it targets | replay order (withdrawal creation before L1 lifecycle inputs) | No — otherwise an acknowledged refund/finalization could be discarded as an unknown leaf; guarded by the actor crash/restart refund test |
| `admit_log` | `control_plane` fund/create-market | a direct admit acked *after* a fund/market-create is replayed *before* it | none (relies on non-validating replay) | Yes (latent) — `reinsert_for_replay` (`order_book.rs:175`) does **not** re-validate against balance or market existence; it re-sums reservations and pushes the row. Aggregates are commutative, no block is produced mid-restore, so the end state is correct regardless of order |
| `control_plane` resolve | resting orders / `admit_log` | resolution must see resting orders on the market to refund/clear them | admit_log replays before control-plane (`sequencer.rs:824` before `:855`) | No — resolve must run after the book is rebuilt |
| any table | `pending_bundles` drain | bundles re-validate against fully restored state | bundles deferred to next block (`sequencer.rs:846`) | Yes — bundles are never trusted; a stale/over-reserved bundle becomes a block rejection, guarded by `restored_pending_bundle_revalidates_against_replayed_admit_reservations` |
| any table | invalid WAL row | one bad row must not abort recovery | per-row crash guards drop + count (`sybil_restore_wal_rows_dropped_total`); guarded by `restore_drops_invalid_bridge_and_deposit_wal_rows` | n/a |

Reading the matrix: there are effectively **four hard cross-subsystem ordering
edges**, plus the intra-bridge rule that withdrawal creation precedes its L1
lifecycle inputs —
(1) admit_log before the id-advance/bundle-drain, (2) control-plane before bridge
WALs, (3) the cancel→withdrawal reservation edge, (4) expiry→withdrawal. Edges 2
and 3 collapse into the same "control-plane/expiry before bridge" rule. The
bridge stage itself is ordered funding → withdrawal creation → L1 lifecycle
input. The remaining reorderings are provably safe: they are either monotone (deposits) or
end-state-commutative because replay never re-validates and never emits a block.

## The July 1 incident (motivating case)

A bridge withdrawal acknowledged in the same inter-block window as a signed
cancel could, on restart, be replayed **before** the cancel released the
resting-order reservations it was validating against. The withdrawal saw the
account's balance still encumbered by an order the user had already cancelled,
and was dropped (or, in the mirror direction, a withdrawal validated against
reservations that no longer semantically existed). The bug was ordering, not
data loss: both rows were durably present; they were just applied in the wrong
relative order.

The narrow fix — already landed — pins the replay order so control-plane
commands (including the cancel) and stale-order expiry always run before the
bridge WALs. That is edges 2/3/4 in the matrix, and the three named tests above
lock it in.

The incident is the reason this note exists: it proved that with N independent
tables, correctness depends on a *documented, tested* replay order, and that a
latent ordering edge can hide until a specific cross-subsystem interleaving
occurs in production.

## Verdict

**Separate tables + the documented, tested replay order are sufficient today.
Do not migrate to a single sequenced WAL now.** Reasoning:

1. **Ordering is not validity-sensitive.** Replayed WAL rows never emit their
   own block header, `events_root`, or `state_root`. They mutate in-memory state
   that is then re-committed by the *next* block's ordinary solve, which
   re-derives every commitment deterministically. The exact cross-subsystem
   *interleaving* a global WAL would preserve does not currently feed any signed
   or hashed artifact, so preserving it buys no correctness.
2. **The ordering rules are bounded and satisfied.** The full set is one rule —
   "book (snapshot + admit_log + expiry) and control-plane before the bridge
   WALs, with the id-advance before bundle drain." Every other cross-table
   reorder is monotone or end-state-commutative (matrix), backed by
   `reinsert_for_replay`'s non-validating re-sum and the fact that no block is
   produced mid-restore.
3. **Ticket non-goal respected.** A rewrite here would be aesthetic
   consolidation, not a correctness fix. The control plane is *already* one typed
   log; the remaining tables are few and their coupling is a single rule.

This verdict is conditional. Revisit — and adopt the contingency design below —
the moment **any** of these triggers fires:

- **T1 — Ordering becomes validity-sensitive.** Any replayed acknowledged-write
  begins contributing to a committed/hashed artifact (block header, `events_root`,
  `state_root`, a witness) whose bytes depend on cross-subsystem event
  interleaving. This is the invariant-9 boundary; crossing it makes per-table
  order a validity bug surface.
- **T2 — A replay step finalizes state per-row.** If replay stops "folding into
  the next block's fresh solve" and a single WAL row can itself commit/witness a
  block, the fresh-solve safety net is gone and exact global order is required.
- **T3 — The ordering rules accumulate.** A *fifth* distinct hard ordering edge
  (beyond the four in the matrix, i.e. a genuinely new pairwise rule not
  reducible to the existing "X before bridge" rule) means the pairwise-rule
  approach is no longer tractable — collapse to one seq.
- **T4 — A new WAL table couples to more than one existing table.** If a new
  acknowledged-write path's correctness depends on interleaving with *two or more*
  existing tables (not a single "runs after X"), a global seq is cheaper than the
  N-way rule.
- **T5 — A reorder stops being monotone.** If any cross-table reorder can change
  the *accept/reject* decision of a replayed row (e.g. a deposit whose ordering
  relative to a withdrawal flips validity, or a replay step that rejects a row
  based on state a later table would have supplied), the commutativity argument
  breaks and exact order is mandatory.

## Contingency design sketch (single sequenced WAL)

If a trigger fires, replace the five tables with one:

- **Table.** `acknowledged_writes: TableDefinition<u64, &[u8]>` in redb, key =
  global monotonic seq, value = `rmp_serde` of a typed enum:

  ```
  enum AckWrite {
      DirectAdmit(RestingOrder),
      DeferredBundle(OrderSubmission),
      ControlPlane(ControlPlaneCommand),
      L1Deposit(L1Deposit),
      BridgeWithdrawal(BridgeWithdrawalRequest),
  }
  ```

  This absorbs today's `ControlPlaneCommand` enum unchanged as one variant.
- **Global sequence.** A `next_ack_write_seq` counter key in the existing
  `counters` table, allocated in the same redb write txn as the append, so seq is
  gap-free and matches durable order across subsystems.
- **Write path.** Each append site keeps its current *durable-vs-live* discipline
  (control-plane/deposit/withdrawal durable-before-live; direct admit
  live-then-durable-with-rollback) — only the *table* changes, not the ack
  contract. One append helper replaces the five.
- **Replay.** A single ascending scan dispatches per variant into the same
  handlers used today (`replay_control_plane_command`, `reinsert_for_replay`,
  `ingest_l1_deposit`, `request_bridge_withdrawal`). Exact global order is now
  preserved for free. Two carry-overs from the current order must remain explicit
  because they are not pure per-row replays:
  - `DeferredBundle` rows still route to the `pending_bundles` queue for
    next-block validation rather than applying inline (invariant 9 is preserved,
    not weakened).
  - The `next_order_id` advance past all replayed `DirectAdmit` rows must still
    happen before any bundle-derived id is assigned. In a single scan this means
    a post-scan max-fold before draining bundles, exactly as today.
- **Snapshot interaction.** Cleared in the same `save_block_inner` redb txn that
  flips the fence — identical to today; one `retain(false)` instead of five. The
  commit-fence crash model (invariant 2) is unchanged.

The single WAL is strictly *more* faithful (exact global order) at the cost of a
coarser-grained table (one hot key range instead of five). It removes the class
of latent ordering edges the July 1 bug came from, which is the real reason to
adopt it once a trigger makes order matter.

## Migration / backfill notes

The migration is a store-layout bump (`store_layout_version`, currently `1`,
checked at open in `crates/matching-sequencer/src/store.rs`):

1. On first open at the new version, in one redb write txn: read the five legacy
   tables and re-emit their rows into `acknowledged_writes` **in exactly today's
   documented replay order** — all `admit_log` rows, then `control_plane_log`,
   then `pending_l1_deposits`, then `pending_bridge_withdrawals` — assigning fresh
   sequential `next_ack_write_seq` values in that order. `pending_bundles` are
   re-emitted as `DeferredBundle` rows (their relative position among themselves
   preserved; they remain deferred at replay).
2. Because today's replay order *is* the correctness order, backfilling in that
   order makes the unified replay behaviorally identical to the current
   per-table replay for any store written before the bump — no semantic change on
   the migration boundary.
3. `resting_orders` stays a snapshot row (it is not an acknowledged-write log);
   only the four logs plus `pending_bundles` fold into the unified table.
4. Drop the legacy tables in the same txn, or leave them empty and unused until a
   later cleanup. Recovery must reject a store that has *both* a populated legacy
   table and a populated `acknowledged_writes` (ambiguous), matching the
   fail-closed posture of the fence recovery.
5. Add a round-trip test: write via legacy tables at version 1, migrate, and
   assert the restored `BlockSequencer` state is byte-identical to a
   pre-migration `restore`. Extend the restart-harness ladder in
   [[Testing Strategy]] with a mixed-subsystem interleaving case (admit, fund,
   cancel, deposit, withdraw acknowledged in that wall-clock order) that only the
   single-seq WAL can replay in true order — it is the direct regression test for
   the class of bug T1/T5 describe.

## Related notes

- [[Persistence]] — the snapshot/commit-fence model this replay sits on top of
- [[Block Lifecycle]] — the block is the transactional unit WAL rows fold into
- [[L1 Settlement and Vault]] — the bridge deposits/withdrawals two of these WALs protect
- [[Pending Orders and TTL]] — resting-order reservations and expiry, the cancel→withdrawal edge
- [[Market Resolution]] — resolution as a control-plane command replayed against the restored book
- [[State Root Schema]] — the committed leaves; why replay order is not yet validity-sensitive
- [[Testing Strategy]] — the restart-harness ladder the replay-order tests extend
