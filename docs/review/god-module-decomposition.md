---
layer: core
status: planned
---

# God-Module Decomposition — matching-sequencer

Three files in `crates/matching-sequencer/src/` dominate the crate and concentrate its
review debt:

| File | Lines | Role | Test tail |
|---|---|---|---|
| `sequencer.rs` | ~7,164 | `BlockSequencer` core (block production, settlement, state root) | ~3,444 (48%) |
| `store.rs` | ~6,683 | redb persistence layer + qMDB fence | ~2,517 (38%) |
| `actor.rs` | ~6,045 | ractor message loop + client handle + supervisor | ~1,949 (32%) |

This is a **responsibility/boundary analysis**, not a line-count exercise. The founder's
framing holds: oversized files signal tight coupling and mixed responsibilities. The good
news, after reading all three end to end, is that **each file is mostly cleanly
separable** — the bulk of every file is either (a) a large test tail, (b) mechanical
client/accessor shim code, or (c) co-located-but-independent concerns. The genuinely
entangled core in each is small, cohesive, and *should stay big*.

Every proposed split is tagged **SAFE-MOVE** (pure relocation, byte-identical behavior,
compile-verified) or **REFACTOR** (changes a type/signature/boundary — needs the full test
gate plus a block-fingerprint check). The default posture is: **split into child modules
that preserve field privacy so nothing changes but the file a function lives in.**

> ### Consensus guardrail (read first)
> Per [`40-do-not-break.md`](40-do-not-break.md), several code paths are **one hop from the
> guest commitment** — state-root computation, witness/event byte encodings, settlement
> purity (shared verbatim with `sybil-verifier`), the redb commit fence, and the
> DA-commitment encoding. A **pure code-move does not change the commitment**; a refactor
> that touches a type, signature, encoding, or ordering *might*. Every such path is flagged
> 🔴 below. The gate for any 🔴 change is: full test suite **plus** a block-fingerprint
> check (produce identical `state_root` / `events_root` / witness bytes on a fixed vector)
> **plus** the native↔guest golden-root tests and canonical/insta snapshots.

---

## File 1 — `sequencer.rs` (~7,164 lines)

### 1.1 Responsibility map

File shape: lines 1–836 = free functions + types + the `BlockSequencer` struct; 837–3696 =
one monolithic `impl BlockSequencer` (~2,860 lines, ~70 methods); 3699–3717 = free helpers +
`BatchSequencer` alias; 3720–7164 = `#[cfg(test)] mod tests` (~3,444 lines).

| Cluster | ~LOC | Lines | Key items |
|---|---|---|---|
| **A. Config & wire types** | ~250 | 40–299, 750–835 | `SequencerConfig` (+`Default`), `AnalyticsMemoryStats`, `OrderSubmission`, `AdmitOutcome`, `BatchResult`, `PendingOrderInfo`, `LeaderboardBase/Row`, the `BlockSequencer` struct def (778–835), phase-local structs (`PreparedBlock`, `SolvedBatch`, `FinalizedBlockState`, `WitnessArtifacts`, `WitnessAssemblyInput`) |
| **B. Construction / restore / replay** | ~340 | 838–1217 | `new`, `with_default_solver`, `restore` (920–1057), `rebuild_api_key_index`, `replay_control_plane_command` (1072–1217, 21-arm dispatcher) |
| **C. Account / key / API-key / profile / nonce** | ~380 | 1460–1807 (scattered) | pubkey registry, `create/revoke_api_key`, `set_profile`, `validate/advance_replay_nonce`, `create/fund_account`, `capture_system_account_baseline`/`capture_missing_system_account` |
| **D. Bridge / deposit / withdrawal** | ~240 | 1866–2105 + 301–336 | `ingest_l1_deposit`, `request_bridge_withdrawal`, `apply_bridge_withdrawal_l1_event`, `validate_*`, free `bridge_block_data`/`l1_deposit_witness` 🔴 |
| **E. Market lifecycle / resolution / feeds** | ~330 | 1284–1424, 2335–2526 | `create_market(_group)`, `extend_market_group`, `resolve_market` / `resolve_market_attested` (near-duplicate ~55-line bodies, SEQ-6), `shrink_market_groups_after_resolution`, feeds/templates |
| **F. Order admission / STP / cancel** | ~300 | 656–745, 2107–2333, 2562–2573 | `GroupCoverageTracker` (656–745), `try_admit_direct`, `seed_group_coverage_*`, `cancel_pending_order(_at)`, `record_system_event`, `open_batch_unique_placers` |
| **G. Analytics / portfolio / leaderboard reads** | ~120 | 1262–1282, 1426–1456, 1809–1864 | `portfolio_summary`, `leaderboard_bases`, thin `AnalyticsState` delegations |
| **H. Trivial accessors** | ~90 | 1219–1435 (interleaved) | `height`, `genesis_hash`, `snapshot`, `last_header`, `order_book`, etc. |
| **I. Block production pipeline** | ~1,160 | 2528–3696 | the god cluster — see below |

**The block-production cluster (I)** is the heart. `prepare_block` (2538) **clones the whole
sequencer**, drains `pending_bundles`, calls `produce_block_in_place`, then
`validate_prepared_block_for_commit` (2618, the commit gate that recomputes
`compute_complete_state_root`). `produce_block_in_place` (2940–3695, **~755 lines**) is a
12-step straight-line dataflow:

1. system-event application + per-account `events_digest` loop (2945–3045) 🔴
2. book maintenance — expire + revalidate (3086–3155)
3. submission processing — MM clamp, STP checks, `order_book.accept` (3157–3380)
4. stats capture (3382–3456)
5. `Problem` build (3458)
6. **Phase 1** `solve_batch_phase` (def 2680–2756)
7. witness pre/post snapshots via `build_witness_phase_snapshots` (485) 🔴
8. **Phase 2** `finalize_block_state_phase` (def 2762–2843) — settle + minting 🔴
9. post-settle bookkeeping (3488–3581)
10. **Phase 3** `assemble_witness_artifacts` (def 2845–2938) — `state_root` + `events_root` 🔴
11. commit-to-self (3609–3614)
12. observe + `verify_full` + invariant gate (3616–3694)

Supporting free helpers used by this cluster: `expected_balance_delta_from_fills` (399),
`collect_account_invariant_failures` (434, shared with the commit gate),
`convert_system_event` (573) 🔴, `convert_rejection_reason` (504), `classify_order_side`
(382), `build_witness_phase_snapshots` (485) 🔴.

### 1.2 Coupling analysis

**Genuinely entangled (the real glue):**

- **The system-event staging protocol.** `capture_system_account_baseline` /
  `capture_missing_system_account` (1714/1725) and `record_system_event` (2107), plus the
  fields `pending_system_events` and `pending_system_account_baselines`, form a
  *cross-cutting mutation protocol*: every state-changing op (fund 1770; bridge 1982/2086;
  cancel 2306; resolution 2380/2447; market-group 1380) must register itself so block
  production can digest it into `events_digest` and compute correct pre-state witness
  snapshots. **This is the single strongest force keeping C/D/E/F/I together.**
- **`restore` / `replay_control_plane_command`** (920–1217) call the public mutation API of
  *every* cluster, in a load-bearing order (control-plane before deposits/withdrawals,
  comment 981–986). B depends on C+D+E+F.
- **`GroupCoverageTracker`** (656–745) is shared by admission (`try_admit_direct` 2182) and
  production (3161). STP semantics must be identical on both paths — a shared invariant.
- **`collect_account_invariant_failures`** (434) is the one conservation definition, called
  by both the finalize phase (2829) and the commit gate (2623).

**Merely co-located (easy to move):** clusters G, H, the read halves of C/D/E, the bridge
read accessors, and the free-floating data types (A).

**Key Rust enabler:** a struct's private fields are visible to the **defining module and all
descendant modules**. Declaring `BlockSequencer` in `sequencer/mod.rs` and placing
per-cluster `impl BlockSequencer` blocks in *child* modules keeps every private field
accessible with **zero visibility changes**. `&mut self` is therefore *not* a reason to keep
clusters in one file.

### 1.3 Proposed module structure

```
sequencer/
├── mod.rs              // struct BlockSequencer + fields; new/with_default_solver;
│                       // trivial accessors (H); the system-event staging helpers
│                       // (capture_*baseline + record_system_event) as the ONE shared def;
│                       // re-exports. Public surface: BlockSequencer, BatchSequencer.
├── config.rs          // SequencerConfig(+Default), AnalyticsMemoryStats, DEFAULT_ORDER_TTL_BLOCKS
├── types.rs           // OrderSubmission, AdmitOutcome, BatchResult, PendingOrderInfo, Leaderboard*
├── accounts.rs        // impl block — cluster C
├── bridge_ops.rs      // impl block — cluster D (named _ops to avoid clash with crate::bridge)
├── markets.rs         // impl block — cluster E
├── admission.rs       // impl block — cluster F + GroupCoverageTracker
├── restore.rs         // impl block — cluster B (restore, replay_control_plane_command, snapshot)
├── views.rs           // impl block — cluster G
└── production/
    ├── mod.rs         // impl block — prepare_block, commit_prepared_block, try_produce_block,
    │                  //   produce_block, validate_prepared_block_for_commit,
    │                  //   produce_block_in_place (orchestrator); PreparedBlock;
    │                  //   batch_result_from_block; invariant free-helpers
    ├── solve.rs       // solve_batch_phase + SolvedBatch
    ├── finalize.rs    // finalize_block_state_phase + FinalizedBlockState +
    │                  //   expected_balance_delta_from_fills + collect_account_invariant_failures
    └── witness.rs     // assemble_witness_artifacts + WitnessArtifacts/WitnessAssemblyInput +
                       //   convert_system_event, l1_deposit_witness, build_witness_phase_snapshots,
                       //   convert_rejection_reason, classify_order_side, verifier_failures
```

**New types — propose only what already exists latent:**
- **Keep `GroupCoverageTracker`** (already a type) → `admission.rs`. Correct STP boundary; no
  invention needed.
- **Reject a `WitnessAssembler` struct.** `assemble_witness_artifacts` already takes
  `WitnessAssemblyInput` and borrows 7 `&self` fields. A standalone struct means threading
  those through a constructor — a REFACTOR with no clarity gain over "method in
  `production/witness.rs`."
- **Reject a `BlockProductionPipeline` owning type.** The pipeline is inherently `&mut self`
  over the whole sequencer (it commits back into `last_header`/sidecars/analytics). A separate
  owner just re-borrows every field.

### 1.4 SAFE-MOVE vs REFACTOR

| Change | Tag | Notes |
|---|---|---|
| Clusters C/D/E/F/B/G → their modules; A/H → `config.rs`/`types.rs`/`mod.rs` | **SAFE-MOVE** | Struct stays in `mod.rs`; child modules see private fields; zero signature change |
| Move `GroupCoverageTracker` + phase structs + the 3 phase methods + witness encoders into `production/*` | **SAFE-MOVE** 🔴 | Relocation only, but the encoders/state-root path are consensus surface — gate with a block-fingerprint check even though bytes are unchanged |
| Keep system-event staging helpers as one shared def in `mod.rs` | **SAFE-MOVE** | Do NOT duplicate per module |
| Extract `produce_block_in_place` inline sections 1–5 & 9 into named phase fns (review idea #6) | **REFACTOR** 🔴 | Changes the events-digest path; highest risk; do last, one section at a time, each behind a fingerprint gate |
| De-dup `resolve_market` / `resolve_market_attested` into `settle_resolution` (SEQ-6) | **REFACTOR** | New private fn, changed call graph; touches settlement + system-event recording |
| Consolidate `capture_*baseline` + `record_system_event` into a single "staged event" API | **REFACTOR** | Touches the cross-cluster protocol — defer, out of scope for the split |

🔴 **Loud flags (one hop from the guest commitment):** `assemble_witness_artifacts` (2845,
computes `state_root` via `compute_state_root_with_sidecar` + `events_root`);
`convert_system_event` (573), `l1_deposit_witness` (325), `bridge_block_data` (301),
`build_witness_phase_snapshots` (485), `convert_rejection_reason` (504) (canonical byte
encodings pinned by ZK `events_root`); `finalize_block_state_phase` (2762)
(`derive_minting`/`apply_minting`/`settle_batch` → `post_state` → `state_root`, settlement
purity shared with the verifier); `validate_prepared_block_for_commit` (2618, the
`compute_complete_state_root` gate); the **system-event `events_digest` loop (2952–3044)** via
`crate::digest::encode_*`.

### 1.5 Test tail (3720–7164, ~3,444 lines)

Move each cluster into a `#[cfg(test)] mod tests` **beside** its new module. Sibling test
modules see `pub(crate)` and private items of the parent chain, so private-internal tests
keep compiling. Tests that reach private internals — **force sibling unit modules, not
`tests/`:**
- commit-gate tests (4022–4109) use private `validate_prepared_block_for_commit`
- `test_expected_balance_delta_*` (4281–4318) call private `expected_balance_delta_from_fills`
- header/phase proptests (5758–5872) call private `build_witness_phase_snapshots`
- `restored_pending_bundle_revalidates` uses `pending_bundles_for_test` (`pub(crate)`)

Public-API-only clusters (validation/reservation 4637–4810; market-group lifecycle
6003–6264) *could* migrate to `crates/matching-sequencer/tests/`, but the split is cleaner if
tests move module-by-module with their code. Shared fixtures (`make_sequencer`, `setup`, `q`,
`sequencer_from_scenario_problem`, …) go to a `#[cfg(test)]` `sequencer/testutil.rs`.

### 1.6 What should stay big
`produce_block_in_place`'s 12-step pipeline is **one cohesive responsibility** — a
straight-line dataflow whose intermediates (`witness_orders`, `order_account_map`,
`mm_order_ids_set`, `derived_view_sidecar`, `block_orders_by_market`) thread through many
steps and would become a wide param/return struct if force-split. The already-extracted phase
methods (`solve`/`finalize`/`assemble_witness`) are the *right* seams; the orchestrator should
land at **~250–350 lines, not <100** — pushing lower scatters the guest-commitment dataflow
across files and *increases* consensus risk. The system-event staging protocol and
`GroupCoverageTracker` also stay whole.

---

## File 2 — `store.rs` (~6,683 lines)

### 2.1 Responsibility map

One public `Store { db: Arc<Database> }` struct; its `impl` spans 1895–3355; ~1,700 lines of
free functions surround it; the test module is 4166–6683 (~2,517 lines, **38%**). The `Store`
methods themselves are only ~1,460 lines.

| Cluster | ~LOC | Lines | Key items |
|---|---|---|---|
| **A. Table schema + key codecs** 🔴 | ~370 | 93–494 | 38 `TableDefinition`s, counter-key consts, `STORE_LAYOUT_VERSION`, manual big-endian key encoders (`fill_history_key`, `price_point_key`, `price_candle_key` + variants). **Durable byte formats — row ordering & retention correctness.** |
| **B. Row serialization (enum/tag codecs)** 🔴 | small | 955–993 | `key_scope_to/from_store`, `account_auth_scheme_to/from_store`, `PubkeyMetaRow` (hand-rolled `u8` tags). Values are `rmp_serde` (additive), inlined at ~40 call sites |
| **C. Atomic block commit (the fence)** 🔴 | ~500 | 1109–1617, 2036–2110 | `RedbBlockCommit`, `build_redb_block_commit` (1151, pure), `write_redb_block_commit_inner` (1326–1617, **the single write txn**, fence flip at 1611 before `commit()` 1615), `save_block_inner` (2036, qMDB→root-verify→write→flip) |
| **D. Admission / bridge / control-plane WAL** | ~120 | 850–952, 3107–3378 | `ControlPlaneCommand` enum (durable payload), `append_pending_bundle/admit_log/control_plane_command/pending_l1_deposit/pending_bridge_withdrawal` → `append_msgpack_row_bytes` (3357). WAL *clears* live inside the commit txn (1524–1543) |
| **E. Snapshot/restore assembly** 🔴 | ~450 | 997–1107, 2569–3097 | `RestoredState`, `AnalyticsRestoredState`, `SequencerSnapshot`, `load_state` (2569–3013, 444-line fence-driven recovery), `ensure_state_qmdb_root` (3016), `read_recovery_metadata`, `read_account_state_fence`, `initialize_or_validate_layout` |
| **F. Witness-genesis import** 🔴 | ~600 | 2459–2566, 3421–4002 | `import_witness_genesis`, `ensure_import_target_empty`, `restored_state_from_witness` + all `*_from_snapshot`/`*_from_witness` mappers. Recomputes `compute_state_root_with_sidecar`, fails closed |
| **G. DA artifacts** 🔴 | ~150 | 675–824, 2201–2228 | `DaArtifact`/`DaArtifactManifest`/`DaProviderRef`, `DA_*` consts, `from_witness`, `verify_payload_integrity`, **`file_da_provider_ref_bytes` (814–824, feeds `da_commitment`/`public_input_hash` via `sybil_zk`)**, `save/load_da_artifact` |
| **H. Derived views / analytics reads** | ~250 | 3154–3326 | `equity_series`, `account_events`, `account_fills(_after)`, `append_offblock_rows` (read-only bar the last) |
| **I. History retention / pruning / compaction** | ~400 | 461–673, 826–848, 1619–1893, 2241–2375 | `HistoryRetentionPolicy`, `prune_history_redb` (1619), `prune_historical_block_rows` (461), `backfill_price_history_indexes` (1810, one-time index migration), `load_block(_page)`, `load_price_history/candles` |
| **J. Auto-resolution records** | ~50 | 524–558, 3327–3355 | `AutoResolutionRecord/Action` + 2 methods (explicitly off-block, not in state root) |
| **K. qMDB passthroughs** | ~60 | 2112–2172, 3016 | `state_qmdb_root/_leaves/_leaf_proof`, thin delegation to `AccountStateStore` |
| **L. Error + infra** | ~90 | 560–588, 1897–2003, 3384–3433 | `StoreError`, `Store::open`, `redb_write` (`spawn_blocking` seam), fault-injection scaffolding |

### 2.2 Coupling analysis

**What actually holds it together:** the single `db` handle (a *resource*, shallow
coupling); file-private table `const`s referenced across 3–5 clusters each (the biggest
mechanical blocker — must promote to a shared `tables` module); key codecs shared by commit
(build), prune (index walks), and view reads (range bounds); `redb_write` + fault injection
used by commit/WAL/prune/DA/auto-resolution.

**Genuinely entangled (must stay together):**
- `write_redb_block_commit_inner` (1326–1617) touches ~30 tables in one txn — **this IS the
  commit fence.** Per [`40-do-not-break.md`](40-do-not-break.md) item 2, no second commit
  point; atomic-by-requirement, not by accident.
- `load_state` + `ensure_state_qmdb_root` + fence readers — strictly ordered, fail-closed
  recovery; the qMDB-repair step must run after core state is read, against the committed
  header root.
- The witness-import free functions (F) — a self-checking reconstruction pipeline; each
  mapper is only meaningful as part of `restored_state_from_witness`.

**Merely co-located (safe to move):** DA (G), retention/pruning (I), analytics view reads
(H), auto-resolution (J), enum/tag codecs (B).

### 2.3 Proposed module structure

```
store/
├── mod.rs           // pub struct Store, open(), redb_write(), save_block* orchestration,
│                    //   StoreError, re-exports. The narrow public surface.
├── tables.rs        // ALL TableDefinition consts + counter-key/string consts +
│                    //   STORE_LAYOUT_VERSION. pub(crate). [DURABLE — do not rename]
├── codec.rs         // all *_key / *_bounds fns + scope/scheme tag codecs + PubkeyMetaRow. [DURABLE]
├── commit.rs        // RedbBlockCommit, build_redb_block_commit, write_redb_block_commit(_inner),
│                    //   PersistedCoreCounters, AccountStateFence, write_core_counters. [FENCE — whole]
├── restore.rs       // RestoredState, AnalyticsRestoredState, SequencerSnapshot, load_state,
│                    //   ensure_state_qmdb_root, recovery metadata readers, layout validation.
├── import.rs        // import_witness_genesis, ensure_import_target_empty + witness→state free fns.
├── wal.rs           // ControlPlaneCommand, append_msgpack_row_bytes, all append_* methods.
├── da.rs            // Da* types, DA_* consts, from_witness, verify_payload_integrity,
│                    //   file_da_provider_ref(_bytes), save/load_da_artifact. [CONSENSUS-ADJACENT]
├── retention.rs     // HistoryRetention*, prune_history(_redb), prune_historical_block_rows,
│                    //   backfill_price_history_indexes, load_block(_page), load_price_history/candles.
├── views.rs         // equity_series, account_events, account_fills(_after), append_offblock_rows.
├── auto_resolution.rs // AutoResolutionRecord/Action + 2 methods.
└── fault.rs (#[cfg(test)]) // StoreFaultPoint/Injection — pub(crate), used by crash_harness.
```

Submodules use `impl Store { … }` blocks in separate files. `db`, `account_state_store`, and
`fault_injection` fields must become `pub(crate)` (or gain `pub(crate)` accessors) — the one
behavioral (visibility-only) change.

**`BlockStore` trait — verdict: reject as speculative.** The crash harness and tests do not
swap the redb backend; they construct a real `Store` and inject faults via
`inject_next_save_block_fault` / `StoreFaultPoint`. A trait over redb adds a vtable across the
commit fence for zero consumers and risks obscuring the "single commit authority" invariant.
The only genuinely useful seams already exist: `account_state_store: Box<dyn
AccountStateStore>` (the qMDB side) and the `redb_write` `FnOnce` closure.

### 2.4 SAFE-MOVE vs REFACTOR

Almost the entire split is **SAFE-MOVE** — relocation of consts, codecs, and methods verbatim.
No `rmp_serde` call-site changes are needed (inlined serialization moves with its method); do
**not** attempt to "centralize encoding" as part of the split — that would be a REFACTOR
touching durable formats.

| Change | Tag | Notes |
|---|---|---|
| Table consts → `tables.rs` | **SAFE-MOVE** 🔴 | `TableDefinition::new("name")` strings are the durable identity — do not rename |
| Key codecs + tag codecs → `codec.rs` | **SAFE-MOVE** 🔴 | Durable byte layouts; relocate verbatim, no endianness/field-order change |
| Commit fence → `commit.rs` | **SAFE-MOVE** 🔴 | Move as one unit; preserve table write order + `before_commit()`→`commit()` (1614–1615); do not split the txn |
| `load_state` + recovery readers → `restore.rs` | **SAFE-MOVE** 🔴 | Preserve ordering incl. `ensure_state_qmdb_root` call site (2824) |
| Witness-import → `import.rs` | **SAFE-MOVE** 🔴 | Keep root re-checks; verifier-boundary reconstruction |
| DA → `da.rs` | **SAFE-MOVE** 🔴 | `file_da_provider_ref_bytes` feeds `da_commitment`/`public_input_hash`; move bytes-for-bytes; `DA_*` strings are manifest surface |
| Retention / views / auto-resolution / WAL → their modules | **SAFE-MOVE** | Independent; `ControlPlaneCommand` keep field order + `#[serde(default)]` |
| Widen `db`/`account_state_store`/`fault_injection` to `pub(crate)` | **REFACTOR** (trivial) | Visibility-only, invisible outside the crate |

### 2.5 Test tail (4166–6683, ~2,517 lines)
`#[cfg(test)] mod tests` uses `use super::*`, so it sees private items. Shared fixtures
(`temp_db_path`, `sample_header`, `sample_witness`, `sample_sealed_block`, and the ~220-line
`coherent_header_and_witness` 4299–4521) → a `store/testutil.rs`. Move each cluster beside its
code: fence/recovery → `commit.rs`/`restore.rs`; retention → `retention.rs`; witness/DA →
`da.rs`/`commit.rs`; import drill (5703–6065, the single largest test) → `import.rs`; WAL →
`wal.rs`; views → `views.rs`. Tests poking `store.db` / `store.account_state_store` /
`TEST_COUNTERS` need the same `pub(crate)` widening the split already requires — keep as
in-crate siblings, not `tests/`. **Move code first (compile-verifiable SAFE-MOVE), then tests.**

### 2.6 What should stay big
`write_redb_block_commit_inner` (~290 lines, 30 tables, one txn — the fence),
`load_state` (444 lines, fence-driven ordered recovery), and the witness-import pipeline
(~600 lines, self-checking reconstruction) are each **one cohesive responsibility**. Large
because the responsibility is large, not because tangled. (A light internal cleanup — grouping
the ~15 near-identical "single-blob snapshot with default-on-missing" reads at 2847–2933
behind one generic helper — is a *possible* future REFACTOR, out of scope for the split.)

---

## File 3 — `actor.rs` (~6,045 lines)

### 3.1 Responsibility map

Macro-structure: ~1,450 lines of handler logic (`impl SequencerActorState`, 752–2194); ~630
lines of `impl Actor` dispatch (2196–2828); ~1,070 lines of `SequencerHandle` client shim
(3026–4095); ~1,949 lines of tests (32%); ~600 lines of types/infra/helpers.

The `SequencerMsg` enum has **52 production + 3 test variants**, but this undercounts: the
single `Query(SequencerReadQuery)` variant (342) is a Trojan horse carrying **~40 read-only
ops** as boxed closures over `&SequencerActorState`. True operation count ≈ 90.

| Cluster | ~LOC | Key items |
|---|---|---|
| **A. Block production / commit** 🔴 | ~260 | `on_tick`/`on_tick_inner` (765–856, prepare→persist→commit→broadcast), `halt_after_invariant_failure`, `persist_block` (954–1070), `record_metrics`, `push_to_history`; ticker spawn in `post_start` (2231) |
| **B. Order submission / cancel** 🔴 | ~230 | `handle_signed/authenticated_order`, cancels, **`admit_or_defer` (1404–1449, durable-before-live)**, `check_*_submission_limits`, `accept_replay_nonce`, `persist_control_plane` |
| **C. Account / auth / key / API-key** | ~370 | create/fund account + all key/profile/api-key mutations, `resolve_signer_account` (shape: verify → resolve → nonce → WAL → apply) |
| **D. Bridge deposits / withdrawals** | ~60 | `handle_l1_deposit`, `handle_bridge_withdrawal(_l1_event)`, signed/authenticated variants |
| **E. Market admin / resolution / feeds** | ~120 | create market(_group)/extend/resolve(_attested)/register_feed/install_template |
| **F. State proofs** 🔴 | ~70 | `handle_state_proof` (2123–2193, reads committed qMDB root, inclusion/exclusion, dual root/slot guard), `SequencerStateProof(Kind)` |
| **G. Indicative / shadow-solve** | ~180 | `IndicativeSnapshot/SolveGate`, `build_indicative_snapshots`, `on_indicative_tick` (`spawn_blocking`) |
| **H. Query serving — async store-backed** | ~230 | own enum variants: `GetBlock`, `GetPriceHistory/Candles`, `GetAccountFills(After)`, `Leaderboard` (~55 LOC inline in the arm), `GetAccountEvents`, `GetDaArtifact`, auto-resolution; `handle_search_markets` (~110 LOC) |
| **I. Query serving — sync read-model** | ~40 methods | via the `Query` closure (no enum variant): `get_account`, `get_state_root`, `list_markets`, portfolio, all analytics rollups (3319–4094) |
| **J. Lifecycle / supervision** | ~200 | `SequencerActorState` struct, `pre_start`/`post_start`, `SequencerSupervisor` (+`restart_from_store`), `SequencerHandle::spawn*`, `stop_and_wait` |
| **K. Infra** | ~170 | `TokenBucket`, `MailboxMonitor`, `MailboxPressureLevel` |

### 3.2 Coupling analysis

**What forces one file:** the `ractor::Actor` trait requires exactly one `handle()` (2275–2827,
~550 lines) — the only *structural* forcing function. But the arms **already delegate** to
`SequencerActorState` methods; most are 1–3 lines. The exceptions that inline real logic
(`GetBlock` ~35, `Leaderboard` ~55, `GetAccountEvents`, `GetDaArtifact`, `GetPriceHistory/
Candles`) are the arms worth extracting to methods first.

**`SequencerActorState`** (561–580) is the shared hub. Its `sequencer: BlockSequencer` field is
used by everything; `store` by A/B/C/D/E/F/H; `block_history`/`block_broadcast` by A + block
queries; but **`global/account_submission_bucket*` are B-private and `indicative_cache/gate`
are G-private** — those could move behind sub-structs. The dominant coupling (every handler
needs `&mut sequencer`) is real but does not block a *module* split; it blocks splitting into
*separate actors* (out of scope — would require splitting `BlockSequencer`).

**Query serving (I) is genuinely a read-model concern merely co-located** with mutation. The
`SequencerReadQuery` closure (`FnOnce(&SequencerActorState) -> T`, immutable borrow) already
decouples ~40 reads from the enum; the only obstacle to moving them out is visibility, trivially
satisfied by submodules under `actor/`.

**`SequencerHandle` is a near-pure client shim** (~1,070 LOC): every method is
`rpc(|reply| Msg::X(..))` or `read_query(|state| ..)`. It can live entirely in `actor/handle.rs`
— its only coupling is the `SequencerMsg` variants it constructs and the closures naming
`SequencerActorState` fields. **This shim + its ~1,950 lines of tests are ~50% of the file and
are barely coupled to handler logic — the single biggest, cleanest extraction.**

**Genuinely entangled:** the durable-before-live sequence in `admit_or_defer` (admit →
`append_admit_log` → rollback-on-failure = the 200-OK contract); `on_tick_inner`'s
prepare→persist→commit ordering with halt-vs-retry; each mutation's WAL-before-apply ordering
(validate-on-clone → burn nonce → WAL → apply). All *within* a handler; handlers are
independent of each other.

### 3.3 Proposed module structure

```
actor/
├── mod.rs         // SequencerActor, SequencerActorState(Args), impl Actor (dispatch ONLY),
│                  //   re-exports preserving public actor::{...} paths.
├── messages.rs    // SequencerMsg enum (KEEP FLAT), SequencerReadQuery, BlockTickOutcome,
│                  //   SequencerStateProof(Kind), MarketSearchResult, IndicativeSnapshot.
├── infra.rs       // TokenBucket, MailboxMonitor, MailboxPressureLevel, IndicativeSolveGate.
├── production.rs  // on_tick(_inner), halt_after_invariant_failure, persist_block,
│                  //   record_metrics, push_to_history, on_indicative_tick, build_indicative_snapshots.
├── handlers/
│   ├── orders.rs  // signed/auth order+cancel, admit_or_defer, check_*_limits, accept_replay_nonce,
│   │              //   persist_control_plane, submission/cancel metrics.
│   ├── accounts.rs// cluster C.
│   ├── bridge.rs  // cluster D.
│   ├── admin.rs   // cluster E.
│   └── proofs.rs  // handle_state_proof. CONSENSUS-CRITICAL.
├── queries.rs     // handle_search_markets, extracted async query bodies, page helpers.
├── handle.rs      // SequencerHandle client shim, SequencerHandleInner, rpc/read_query.
└── supervisor.rs  // SequencerSupervisor(+restart_from_store), SequencerHandle::spawn*.
```

Methods split across files as multiple `impl SequencerActorState { … }` blocks; `handle()` in
`mod.rs` calls them unchanged. **No signatures change** — the property that makes most of it a
SAFE-MOVE. `SequencerActorState` fields become `pub(crate)`.

**Keep `SequencerMsg` flat** — do not shard into sub-enums. `Actor::Msg` is one type; wrapping
sub-enums (`SequencerMsg::Orders(OrderMsg)`) cascades to every `rpc()` call site and the match
structure for zero comprehension gain. The enum is a data catalogue, not where complexity lives.

### 3.4 SAFE-MOVE vs REFACTOR

| Change | Tag | Notes |
|---|---|---|
| Infra → `infra.rs` | **SAFE-MOVE** | Self-contained |
| `messages` types + `SequencerMsg` → `messages.rs` (flat) | **SAFE-MOVE** | Re-export preserves paths |
| `SequencerHandle` shim → `handle.rs` | **SAFE-MOVE** | Biggest win (~1,070 LOC); closures need `SequencerActorState` `pub(crate)` |
| Supervisor + `spawn_*` → `supervisor.rs` | **SAFE-MOVE** | Cohesive lifecycle |
| Handlers C/D/E → `handlers/{accounts,bridge,admin}.rs` | **SAFE-MOVE** | Preserve WAL-before-apply ordering per fn verbatim |
| Query helpers + sync read methods → `queries.rs` | **SAFE-MOVE** | Read-only |
| Extract inline `handle()` arm logic (GetBlock, Leaderboard, GetAccountEvents, GetDaArtifact, GetPriceHistory/Candles) into methods | **REFACTOR** (light) | New signatures + call-site change; do *before* moving so `mod.rs` stays a thin dispatcher |
| Split `SequencerMsg` into sub-enums | **REFACTOR — do NOT** | Cascades to every `rpc()` call site; no benefit |
| Widen `SequencerActorState` fields to `pub(crate)` | **REFACTOR** (trivial) | Visibility-only |

🔴 **Loud flags:** `admit_or_defer` (1404, the durable-before-live 200-OK contract — move only
byte-identical, do not tidy the rollback branch); `on_tick_inner` (765, prepare/persist/commit
ordering driving the single-commit-authority guarantee — do not reorder, do not drop a
crashpoint); `persist_block` (954, save-awaited-then-DA-fire-and-forget ordering is
load-bearing); `handle_state_proof` (2123, `SequencerStateProof` generation, one hop from the
guest commitment — the root/slot equality guard must not be dropped or reordered); the
WAL-before-apply ordering in every C/D/E handler.

### 3.5 Test tail (4097–6045, ~1,949 lines)
Single `#[cfg(test)] mod tests`. Keep **in-crate** (tests touch private `MailboxMonitor`,
`IndicativeSolveGate`, `build_indicative_snapshots`, and the `*_for_test` crash helpers at
3143–3226) and split across the new submodules as sibling `#[cfg(test)] mod tests` blocks.
Shared fixtures (`make_test_sequencer`, `temp_store_path`) → `actor/testutil.rs`. The three pure
unit tests (`build_indicative_snapshots_*`, `indicative_solve_gate`, `mailbox_monitor`) are the
easiest to relocate first. Do **not** move to `tests/` — private-internal dependence makes that a
visibility REFACTOR with no upside.

### 3.6 What should stay big
The block-production commit sequence (`on_tick_inner` + `persist_block` +
`halt_after_invariant_failure` + `commit_prepared_block`, ~260 LOC, one indivisible ordering
constraint — metrics interleave because they read post-commit `BlockProduction`);
`handle_state_proof` (~70 LOC, one atomic op — the guard duplication between inclusion/exclusion
branches is intentional, leave inline); `admit_or_defer` + its limit checks (one admission state
machine); the flat `SequencerMsg` enum (the actor's protocol); and the `handle()` dispatch
(a `ractor` requirement — should read as an index, not hold logic).

---

## Recommended landing sequence

Each step is individually reviewable and individually green. **SAFE-MOVEs first, REFACTORs
last.** Within each file, move production code first (compile-verifiable), then relocate its
tests, so no commit is both a code-move and a test-move.

**Phase 0 — net the trapeze (prerequisite).** Ensure a block-fingerprint check exists (fixed
vector → assert identical `state_root`/`events_root`/witness bytes) and the native↔guest
golden-root tests + canonical/insta snapshots are green on `main`. Every 🔴 commit below runs
this gate. (This aligns with [`40-do-not-break.md`](40-do-not-break.md) §"safe refactor order"
step 1.)

**Phase 1 — `actor.rs` (lowest consensus risk, biggest LOC win).**
1. Extract `TokenBucket`/`MailboxMonitor`/`IndicativeSolveGate` → `actor/infra.rs`. SAFE-MOVE.
2. Move `SequencerMsg` + message types → `actor/messages.rs` (flat). SAFE-MOVE.
3. Move `SequencerHandle` shim → `actor/handle.rs`; widen `SequencerActorState` to `pub(crate)`.
   SAFE-MOVE (halves the file).
4. Move `SequencerSupervisor` + `spawn_*` → `actor/supervisor.rs`. SAFE-MOVE.
5. Move sync read-model + query helpers → `actor/queries.rs`. SAFE-MOVE.
6. Move handler clusters C/D/E → `actor/handlers/{accounts,bridge,admin}.rs`. SAFE-MOVE.
7. Move `handle_state_proof` → `actor/handlers/proofs.rs`. SAFE-MOVE 🔴 (fingerprint/proof gate).
8. Move production + indicative → `actor/production.rs`; move `admit_or_defer` +
   limits → `actor/handlers/orders.rs`. SAFE-MOVE 🔴.
9. Relocate tests into sibling `#[cfg(test)]` modules + `actor/testutil.rs`. SAFE-MOVE.
10. *(Optional, later)* Extract fat `handle()` arms (GetBlock/Leaderboard/…) into methods.
    REFACTOR (light).

**Phase 2 — `store.rs` (mostly SAFE-MOVE, durable-format flags).**
1. Promote table consts → `store/tables.rs`. SAFE-MOVE 🔴 (do not rename `TableDefinition`
   strings).
2. Move key + tag codecs → `store/codec.rs`. SAFE-MOVE 🔴 (durable byte layouts).
3. Move independent clusters: `retention.rs`, `views.rs`, `auto_resolution.rs`, `wal.rs`,
   `da.rs`. SAFE-MOVE (`da.rs` is 🔴 — `file_da_provider_ref_bytes`).
4. Move commit fence → `store/commit.rs`. SAFE-MOVE 🔴 (do not split the txn).
5. Move `load_state` + recovery → `store/restore.rs`. SAFE-MOVE 🔴 (preserve ordering).
6. Move witness-import → `store/import.rs`. SAFE-MOVE 🔴.
7. Move fault scaffolding → `store/fault.rs` (`pub(crate)`, crash_harness dep). SAFE-MOVE.
8. Widen `db`/`account_state_store`/`fault_injection` to `pub(crate)`. REFACTOR (trivial).
9. Relocate tests + `store/testutil.rs`. SAFE-MOVE.

**Phase 3 — `sequencer.rs` (largest consensus surface; do last).**
1. Move types → `sequencer/config.rs`, `sequencer/types.rs`. SAFE-MOVE.
2. Move read clusters G/H accessors; keep the system-event staging helpers as one shared def in
   `mod.rs`. SAFE-MOVE.
3. Move clusters C/D/E/F/B → `accounts.rs`/`bridge_ops.rs`/`markets.rs`/`admission.rs`/
   `restore.rs`. SAFE-MOVE (D touches witness free fns — 🔴 those go to `production/witness.rs`).
4. Move production cluster → `sequencer/production/{mod,solve,finalize,witness}.rs`.
   SAFE-MOVE 🔴 (full fingerprint gate on every commit).
5. Relocate tests into sibling modules + `sequencer/testutil.rs`. SAFE-MOVE.
6. *(Optional, later, one at a time)* Extract `produce_block_in_place` inline sections into
   named phase fns; de-dup `resolve_market`/`resolve_market_attested` (SEQ-6). REFACTOR 🔴.

**Ordering rationale:** `actor.rs` first because ~50% of it (shim + tests) is mechanically
separable with near-zero consensus risk — it proves the child-module pattern and the
`pub(crate)` field widening on the least dangerous file. `store.rs` second: durable-format
flags but a clean cluster boundary and an existing fault-injection seam. `sequencer.rs` last:
it holds the most guest-commitment surface, so it benefits from the pattern being proven and the
fingerprint gate exercised on the earlier files.

---

## Honest summary: how much is really "god file"?

Across all three, the tangled, consensus-load-bearing core is **small and cohesive**:
- `sequencer.rs`: ~1,160 lines of production pipeline (one dataflow) + the system-event
  staging protocol. The other ~2,500 non-test lines split cleanly on field-privacy-preserving
  child modules.
- `store.rs`: ~1,340 lines that must stay whole (commit fence + `load_state` + witness import).
  The rest (~2,800 non-test lines) is co-located-but-independent.
- `actor.rs`: ~260 lines of production commit sequence + per-handler ordering. ~50% of the file
  (client shim + tests) is barely coupled to it.

**These files are big mostly because of large test tails, mechanical shim/accessor code, and
several independent concerns sharing one handle — not because the core domain logic is
irreducibly tangled.** The irreducible core in each *should* stay big and is called out above.
The decomposition is therefore ~90% SAFE-MOVE; the only mandatory behavioral change is widening
a handful of struct fields to `pub(crate)`. The REFACTORs (phase-fn extraction, SEQ-6 de-dup,
`handle()` arm extraction) are genuine boundary improvements but are optional and must run
behind the block-fingerprint gate, since each touches — or lives one hop from — the guest
commitment.
