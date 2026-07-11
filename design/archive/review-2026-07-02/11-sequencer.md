# The Sequencer

**Crate:** `matching-sequencer` (~23.6k LOC — 37% of the workspace, the largest crate)

This crate is the production node: it collects orders, produces blocks, settles fills, computes the state root, and persists to disk. It also — confusingly — contains a full agent simulator. Two survey passes covered it (lifecycle/orchestration and state/settlement); this doc merges them.

## Verdict

A genuinely good skeleton buried in three god-files and carrying several real bugs. The prepare/commit block split, the redb-fenced A/B qMDB slots, and the single canonical admissions WAL with revalidating replay are the right shapes. But block production runs entirely inline on one actor task at O(total state) per tick, the safety invariants only log, and the untested seams (checkpoint interval, WAL replay) harbor the crash-loop and data-loss bugs. This crate is where [Themes 2, 3, 4, and 5](02-cross-cutting-themes.md) all land.

## Architecture as built

**Actor stack (`actor.rs`, 3.1k).** `SequencerHandle` (Clone) wraps an `ActorRef` behind an `RwLock`, a broadcast channel for sealed blocks, and a mailbox monitor. `SequencerSupervisor` (ractor) relinks and respawns the actor on failure, reloading state from the store. `SequencerActor` owns the sync `BlockSequencer` core, a 100-block history ring, an admission limiter, and a ~60-variant `SequencerMsg` god-enum mixing protocol writes, bridge/oracle ops, and ~25 analytics reads. Two timers: the block tick (prod 500ms; deployed 10s) and a hardcoded 750ms indicative tick that shadow-solves the resting book off-thread via `spawn_blocking` (the only off-thread solve in the system).

**Admission (`admission.rs`, `sequencer.rs`).** Post-refactor: submit → global token bucket → per-account caps via the `AdmissionView` trait → `plan_admission`, which direct-admits single-order/single-market/non-MM submissions and **defers** (does not reject) everything else into the batch. Direct plans are durably appended to the `ADMISSIONS` redb stream (one fsync each) before insertion into the live book.

**Block production (`sequencer.rs`, 5.3k).** `on_tick` → `prepare_block` **clones the entire `BlockSequencer`** (accounts, book, analytics, metadata) and drains pending bundles on the clone. `produce_block_in_place` (~750 lines) applies system events, expires+revalidates the book, processes fresh submissions, builds a `Problem`, runs `solver.solve` **inline on the actor task**, settles, derives minting, runs conservation checks (log-only) and `verify_full` (log-only), and assembles the witness + header. `CanonicalState::from_accounts` (full scan+sort) runs ≥3× per block; the witness carries three full account-snapshot vectors.

**State & settlement.** `AccountStore` is an in-memory `HashMap<AccountId, Account>` plus a reserved `MINT` account; balances are `i64` and may go negative (no floor enforced). `OrderBook` (1,073 lines) is the single source of truth for reservations, with aggregate maps that must equal the sum of per-order reservations. Settlement math is the shared pure `matching-engine` module. Minting is derived from position totals that **include** MINT's existing shorts, so each block adjusts only the incremental imbalance — correct, and it matches the verifier exactly.

**Persistence.** The redb-fenced two-slot qMDB model (documented accurately in `Persistence.md`): write the inactive qMDB slot, verify its root == `header.state_root` before the redb commit, then flip the fence. Recovery is fence-driven and self-heals a typed-root mismatch. This is a strong, honest boundary.

**Divergence from docs:** `Mempool.md`/`Persistence.md`/`Block Data Boundaries.md` are accurate; `Block Lifecycle.md` still describes the deleted mempool ("drains the mempool — pulling orders up to drain limits"); the crate `AGENTS.md` is badly stale (claims a `mempool.rs`, TTL "default 3" vs actual 63,072,000, wrong `produce_block` signature, "agent-based simulation engine").

## Strengths

- The **prepare/persist/commit split** gives clean crash atomicity: a persistence failure discards the clone and retries next tick while the live sequencer retains pending work.
- The **redb-fenced A/B qMDB design** is honest about lacking cross-db atomicity, fail-closed on metadata mismatch, and validates/repairs the typed-root against the committed header on restore.
- The **single `ADMISSIONS` stream** unifies direct and deferred recovery; replay re-derives reservations rather than trusting WAL rows and repairs `next_order_id` with a metric.
- `qmdb_state.rs`/`qmdb_accounts.rs` use proper tokio actors per store.
- Minting/conservation is modeled coherently (MINT in position totals) and matches the verifier's derivation.
- Strong in-crate test coverage of cross-block STP, direct-admission durability, and restore round-trips; `MailboxMonitor` + supervisor restart are solid operational touches.

## Findings

| ID | Kind | Sev | Summary |
|----|------|-----|---------|
| [H1](01-critical-bugs.md) | design | **high** | Conservation checks + `verify_full` are advisory-only; invalid blocks seal, persist, broadcast |
| [H7](01-critical-bugs.md) | bug | **high** | Bridge-withdrawal WAL replay `.expect`s on a stale snapshot → crash loop |
| [H8](01-critical-bugs.md) | bug | **high** | Checkpoint-interval persistence silently discards history/equity/fill deltas for skipped blocks |
| [H13](01-critical-bugs.md) | bug | medium | Resolving one market deletes its entire market group |
| SEQ-1 | bug | high | `open_batch_unique_placers` ignores the market filter for resting orders (`sequencer.rs:908-928`), inflating participation counts for every market |
| [D1](01-critical-bugs.md) | design | high | Block production (solve + verify + full clone + fsync) runs inline on the single actor task; a slow solve stalls all reads and the tick timer bursts queued blocks |
| [D2](01-critical-bugs.md) | design | high | O(total state) per block: full clone every tick, ≥3 canonical scans, full qMDB `ReplaceLeaves`, six full tracker-blob rewrites; caused the documented multi-GiB restart incident |
| SEQ-2 | bug | medium | `order_book.rs:575-592` uses f64 ratio+ceil for partial-fill reservations on a **state-root-committed** path (loses precision > 2^53 nanos, determinism hazard) — see [Theme 3](02-cross-cutting-themes.md) |
| SEQ-3 | bug | medium | Indicative shadow-solve gate is never released if the solver panics inside `spawn_blocking` → indicative prices freeze silently until restart |
| SEQ-4 | design | medium | One durable redb write txn (fsync) per admission, serialized on the actor; P256 verification also on the actor task → ~500/s throughput cap on cloud disk |
| SEQ-5 | bug | medium | `ProduceBlock` RPC returns the previous block (200 OK) when paused or when persistence fails, so callers can't distinguish "produced" from "stale" |
| SEQ-6 | inconsistency | medium | Duplicated ~40-line `SettleNow` bodies in `resolve_market` and `resolve_market_attested`; triplicated order-side classification; duplicated `HistoryEvent` construction |
| SEQ-7 | debt | medium | Actor remains a general read-model proxy; the hot/cold split is half-migrated (fills/events/equity read store-first but still via the actor mailbox) — see [Theme 4](02-cross-cutting-themes.md) |
| SEQ-8 | bloat | medium | The agent simulator (`simulation.rs`, `scenario.rs`, `agent/`, `metrics.rs`, `bin/sybil_sim.rs`, ~1.7k LOC) lives inside the production crate; sim-only deps (`clap`, `comfy-table`, `rand`) compile into every prod build |
| SEQ-9 | inconsistency | low | Live `GetStateRoot` mixes uncommitted mutations into a root that matches no committed header; adjacent `GetStateProof` verifies against the committed fence root — two notions of "current root" |
| SEQ-10 | bug | low | `pause_count`, block history, latest block, and rate-limiter buckets reset on supervisor restart — a crash while paused resumes production immediately |
| SEQ-11 | bloat | low | Legacy `PENDING_BUNDLES`/`ADMIT_LOG` tables + `try_admit_direct`/`AdmitOutcome` + `reinsert_for_replay` linger after the decoupling refactor; `witness.minting_cost` hardcoded 0 |
| SEQ-12 | inconsistency | low | `settle_batch` silently skips fills for missing accounts (`else { continue }`) while the verifier creates them (`or_default`) — a latent settlement asymmetry |
| SEQ-13 | doc-drift | medium | Crate `AGENTS.md` and `Block Lifecycle.md` describe a deleted mempool architecture; TTL comments contradict each other |
| SEQ-14 | test-gap | medium | No tests cover checkpoint-interval semantics, stale-snapshot WAL replay, or per-market open-batch placers (exactly where H7/H8/SEQ-1 live); order-book reservation property tests absent on this branch |

## Ambitious ideas

1. **Split the crate along its real seams:** `sybil-sequencer` (node core), `sequencer-runtime` (actor/supervisor/handle/mailbox), `sequencer-store` (store/account_storage/qmdb_*), `sequencer-analytics`, and `sybil-sim` (the agent harness). A 23.6k-line crate becomes four ~5k crates with enforced dependency direction and a rewritten `AGENTS.md` per crate.
2. **Replace whole-sequencer cloning with an explicit `BlockDelta`:** `produce_block` computes `{touched accounts, book mutations, analytics deltas, header, witness}` against `&self`, and commit applies the delta. This makes atomicity structural, kills the O(total state) clone, gives persistence exactly the delta to write (fixing H8 and the full-qMDB rewrite together), and is the single biggest lever on both block-production and restart cost.
3. **Make the solve a first-class async stage** like the indicative path already is: a `Solving` state machine (prepare on-actor, `spawn_blocking` solve+witness+verify, commit on completion) with tick coalescing. Reads stay fast during solves; cadence degrades gracefully to solve-time instead of bursting.
4. **Finish the hot/cold migration aggressively:** delete ~25 read variants from `SequencerMsg`, serve cold reads from `ReadModelStore` in `sybil-api`, persist blocks/price-history so SSE catch-up survives restart. The actor enum should fit on one screen.
5. **Make verification the gate, not a logger** (H1): run the conservation checks + `verify_full` as a precondition of commit, sharing one enforcement path with the verifier. Adopt integer-only reservation math (SEQ-2) enforced by a `deny(float_arithmetic)` lint on the core modules and a `debug_assert` that aggregate reservations equal the per-order sum after every `settle`.
6. **Extract `produce_block_in_place`'s four inlined concerns into named phase modules** (system-event application, batch assembly, solve/settle/minting, witness assembly) so the orchestrator drops under 100 lines and each phase is independently testable.
