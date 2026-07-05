# Witness Schema v2 — pre_state_sidecar, removal/admit streams, deposit accumulator

*Design draft for [SYB-216](https://linear.app/sybilmarket/issue/SYB-216). One deliberate
witness-schema bump that closes the H4 sidecar-transition gap, unblocks the SYB-166 c+d
view extraction, folds in SYB-212 group-membership derivation, and fixes the SYB-214
O(total-deposits) witness growth — arranged so **exactly one guest-commitment move** happens.*

Companion reads: `docs/review/02-cross-cutting-themes.md` (Theme 2, "verify don't log"),
`docs/review/15-verification-zk.md` (H4, ZK-9), `design/architecture-review-2026-07.md`
(§P1 kernel/views split). Ground truth for the current encoding:
`crates/sybil-verifier/src/witness_schema.rs`, `crates/sybil-verifier/src/snapshot_schema.rs`,
`crates/sybil-verifier/src/sidecar.rs`, `crates/sybil-zk/src/lib.rs`.

---

## 0. Naming and the version byte

The ticket calls this "v2", but the on-wire `WITNESS_FORMAT_VERSION` in
`crates/sybil-verifier/src/witness_schema.rs` is **already `2`** (it reached 2 when
`l1_deposits` landed). "v2" here names the *design generation*, not the version byte. This
bump takes the wire version **`2 → 3`**. Everywhere below, "v3" means the on-wire format and
"this bump" / "the v2 design" means the SYB-216 batch. Keep the version byte as a loud
tripwire: a decoder that reads a `2` where it expects `3` must hard-fail, not silently
mis-parse.

## 1. What is missing today, precisely

The witness (`BlockWitness`, `crates/sybil-verifier/src/types.rs`) already carries three full
**account** snapshot sets — `pre_state`, `post_system_state`, `post_state` — but only **one**
sidecar: `state_sidecar`, the *post* non-account state (bridge, markets, groups, resting
orders, reservations). There is **no pre-state sidecar**. So `verify_sidecar`
(`crates/sybil-verifier/src/sidecar.rs`) can only re-derive facts from *activity* in the
current block, and it self-documents the resulting holes in `SIDECAR_WITNESS_GAPS`:

1. a resting order deleted together with its aggregate reservation — undetectable (both sides
   of the reservation-rollup equality move together);
2. a pre-existing withdrawal leaf deleted with no `WithdrawalCreated` event this block —
   undetectable (the check only walks *this block's* events);
3. market metadata/group/status edits for markets absent from `resolved_markets`/events —
   undetectable. **SYB-212's `ExtendMarketGroup`** membership change sits in exactly this hole.

Separately, three derived-view inputs (SYB-166 c+d) are simply absent from the witness:
the block-start `expire`/`revalidate` removal stream (removed `RestingOrder` + owner + exit
reason + amounts — see `sequencer.rs:2707-2745`, `order_book.rs:292-330`), the removed-order
`has_been_matched` flag (`order_book.rs:63`), and direct-admit timing (admit height, and the
new-vs-carried split computed at `sequencer.rs:2758` / `:3004`). These feed
`analytics.record_order_exit` / `record_order_admitted` today — they never enter the witness.

And `l1_deposits` rides in **full** every block (the whole cumulative prefix,
`sequencer.rs:2486-2491`), at 172 bytes/deposit (SYB-214).

The end state the review wants (Theme 2, §P1) is one deterministic
`apply_block(pre_state, pre_sidecar, inputs) -> (post_state, post_sidecar)` that the sequencer
runs, the verifier re-runs, and the guest proves. This bump supplies the missing **input**
(`pre_sidecar`) and the missing **binding** to make that STF checkable.

---

## 2. Recommendation summary (the seven decisions)

1. **New proven fields:** add `pre_state_sidecar: StateSidecarSnapshot` to the witness,
   encoded identically to the existing post sidecar under a new domain tag; replace the full
   `l1_deposits` prefix with a **deposit frontier + this-block delta**. Both are the only two
   additions to `canonical_witness_bytes`. Bump the version byte `2 → 3`.
2. **Full vs commitment+openings:** carry `pre_state_sidecar` in **full**, authenticated by
   recomputing the pre-state root and asserting equality with the already-committed
   `previous_header.state_root` (no extra proof bytes). Openings/delta is the right *eventual*
   answer (ambitious idea #3) but is a second, bigger commitment move; do it at testnet scale,
   not now. Devnet books are tens of orders — full pre-sidecar adds single-digit KB.
3. **Removal/admit streams go UNPROVEN.** They ride in a separate `derived_view_sidecar`
   stream that is *not* in `canonical_witness_bytes`, *not* hashed into `witness_root`/
   `da_commitment`, and *never* seen by the guest. Most of their content becomes *derivable*
   from the now-present `pre_state_sidecar` anyway; only genuinely non-derivable view fields
   (`has_been_matched`, fine exit-reason classification) are original data. **This is the key
   decision: SYB-166 c+d proceeds with NO commitment move.**
4. **Deposit accumulator:** keep the on-chain **fixed-depth incremental Merkle tree** (it
   already exists — `SybilVault._appendDepositLeaf`, depth 32) and carry its **frontier**
   (32 filled-subtree hashes) + this-block leaves in the witness. Do **not** invent an MMR;
   an MMR would *diverge* from the on-chain tree. O(depth + block-deposits), flat forever.
5. **Verifier changes:** `verify_sidecar` gains the full pre→post transition check; a new
   pre-root authentication lands in the block layer; deposit binding switches to a
   frontier-fold. Guest cost roughly doubles on the state-root leg (a second full root) and
   *drops* on the deposit leg.
6. **Migration:** fresh-genesis devnet (existing policy, `docs/runbooks/devnet-redeploy.md`),
   version byte `2 → 3` as the tripwire. No migration tooling — none exists and none is worth
   building for devnet.
7. **Increments:** (0) unproven derived-view stream — no commitment move; (1) the v3 schema
   bump — **the single commitment move**; (2) delta/openings witness — a deferred, separate
   future move, explicitly out of scope here.

---

## 1 (detail). Exact new witness fields + canonical encodings

All encodings follow the existing length-prefixed, little-endian, domain-tagged style in
`witness_schema.rs` / `snapshot_schema.rs` (`append_u64` = 8 LE, `append_u32` = 4 LE,
`append_i64` = 8 LE, `append_string` = u64 len + bytes, arrays = u64 count + elements sorted
by a stable key). Two structural additions and one replacement.

### 1a. `pre_state_sidecar` (new proven field)

`BlockWitness` gains one field, mirroring the existing post sidecar:

```rust
pub struct BlockWitness {
    // ... unchanged ...
    pub state_sidecar: StateSidecarSnapshot,        // post (unchanged)
    pub pre_state_sidecar: StateSidecarSnapshot,    // NEW: non-account state at block start
    pub resolved_markets: Vec<MarketId>,
}
```

Encoding reuses `append_witness_state_sidecar` verbatim, under a distinct domain tag so the
two sidecars can never be confused on the wire. In `canonical_witness_bytes`, insert **after**
the post sidecar (line 65) and before `resolved_markets`:

```
append_witness_state_sidecar(out, &witness.state_sidecar)        // existing, tag "sybil/witness/state-sidecar"
append_witness_pre_state_sidecar(out, &witness.pre_state_sidecar) // NEW,     tag "sybil/witness/pre-state-sidecar"
```

`append_witness_pre_state_sidecar` is byte-identical to `append_witness_state_sidecar`
(`snapshot_schema.rs:244-275`) except the leading domain string is
`b"sybil/witness/pre-state-sidecar"`. It carries, in sorted order: bridge
(deposit_cursor u64, deposit_root [32], next_withdrawal_id u64, withdrawals[]), markets[]
(sorted by `market_id.0`), market_groups[] (sorted by `group_id`), resting_orders[] (sorted by
`order.id`), account_reservations[] (sorted by `account_id`). No new element types — the
snapshot visitor already covers every one.

The sequencer builds it from the **block-start** bridge/order-book/markets/groups/lifecycle
snapshot, i.e. exactly what `state_sidecar_snapshot(&self.bridge, &self.order_book, …)`
(`sequencer.rs:2446`) returns *before* this block's system events and settlement mutate them.
Capture it at the top of `produce_block_in_place`, symmetric to how `pre_state` accounts are
already captured.

### 1b. Deposit frontier + delta (replaces full `l1_deposits`)

Replace the `l1_deposits: Vec<L1DepositWitness>` field and its `append_l1_deposits`
(`witness_schema.rs:174-187`, 172 bytes/deposit) with:

```rust
pub struct DepositAccumulatorWitness {
    /// Filled-subtree frontier at block start (mirrors SybilVault.filledSubtrees).
    pub pre_frontier: [[u8; 32]; 32],   // DEPOSIT_TREE_DEPTH = 32
    pub pre_count: u64,                  // deposits before this block  (== pre bridge cursor)
    /// Deposit leaves credited *this block only*, in id order.
    pub new_deposits: Vec<L1DepositWitness>,
}
```

Canonical encoding (domain-tagged, replaces the `append_l1_deposits` call site at
`witness_schema.rs:50`):

```
out.extend_from_slice(b"sybil/witness/deposit-accumulator")
for h in pre_frontier { out.extend_from_slice(&h) }   // 32 * 32 = 1024 bytes, fixed
append_u64(out, pre_count)
append_u64(out, new_deposits.len())
for d in new_deposits {                                 // same per-leaf layout as today
    out.extend_from_slice(b"sybil/witness/l1-deposit")
    append_u64(out, d.deposit_id); append_u64(out, d.chain_id)
    out.extend_from_slice(&d.vault_address); out.extend_from_slice(&d.token_address)
    out.extend_from_slice(&d.sender); out.extend_from_slice(&d.sybil_account_key)
    append_u64(out, d.amount_token_units); out.extend_from_slice(&d.deposit_root)
}
```

The guest folds `new_deposits` onto `pre_frontier` exactly as
`sybil_l1_protocol::deposit_prefix_roots` / `SybilVault._appendDepositLeaf` do (identical
`hash_node` / `filledSubtrees` recurrence, `crates/sybil-l1-protocol/src/lib.rs:157-178`),
producing the intermediate and final roots. It checks: `fold(pre_frontier, pre_count) ==
pre_state_sidecar.bridge.deposit_root`, the last folded root `== state_sidecar.bridge.deposit_root`,
and each `new_deposits[i].deposit_root` equals its intermediate. The pre-frontier is
authenticated *by the fold*: a forged frontier that reproduced the committed pre-root would be
a keccak collision.

### 1c. `derived_view_sidecar` (new UNPROVEN field — §3)

Rides in the block record / DA envelope alongside the witness but **outside**
`canonical_witness_bytes`. It never touches `witness_root`, `da_commitment`, or the guest.

```rust
pub struct DerivedViewSidecar {
    pub removed_orders: Vec<RemovedOrderView>,   // block-start expire + revalidate stream
    pub admits: Vec<AdmitTimingView>,            // direct-admit timing
}
pub struct RemovedOrderView {
    pub order_id: u64,
    pub account_id: u64,
    pub exit_reason: u8,             // 0 Expired, 1 RevalidateInsufficientBalance,
                                     // 2 RevalidateInsufficientPosition, 3 MarketInactive,
                                     // 4 AccountGone  (mirrors order_book RestingExit / reason)
    pub has_been_matched: bool,
    pub reserved_balance_released: i64,
    pub reserved_positions_released: Vec<(MarketId, u8, i64)>,
    pub active_markets: Vec<MarketId>,
}
pub struct AdmitTimingView {
    pub order_id: u64,
    pub account_id: u64,
    pub admit_height: u64,
    pub is_new: bool,                // false == carried resting from a prior block
}
```

Encoding is ordinary length-prefixed LE; because it is unproven, its byte layout is *not*
consensus-critical and may evolve without a commitment move (its only constraint is that
`sequencer` and the view consumers agree).

---

## 2 (detail). Full inclusion vs commitment+openings — size analysis

### Measured baseline

The empty-witness canonical encoding is **349 bytes** (`witness_schema.rs:226`, asserted). The
SYB-216 reference "golden" loaded block is **~2.5 KB**. Per-element encoded sizes, derived
from the actual encoders:

| Element | Encoder | Bytes (typical single-market binary) |
|---|---|---|
| Block header | `append_header` | 120 |
| Account snapshot (witness) | `append_witness_account`, 2 positions | ~111 |
| `Order` (canonical) | `append_order`, 1 mkt / 2 states / no cond | ~42 |
| Resting-order snapshot | order + acct + 3×u64 + i64 + 1 pos | ~95 |
| Account reservation | acct + i64 + 1 pos | ~40 |
| Market snapshot | id + name + status + digest + template | ~90 (Active) / ~150 (Resolved) |
| Withdrawal leaf | id + acct + recip20 + token20 + 3×u64 + null32 | ~112 |
| L1 deposit leaf | domain24 + fields | 172 |

The state sidecar dominates any active block. For a devnet resting book of size **R** with
**M** markets and **W** live withdrawals, the post sidecar is roughly
`135·R + 90·M + 112·W` bytes (order ≈95 + its reservation ≈40 per resting order).

### The two options

**Full inclusion (recommended).** Duplicate the sidecar as `pre_state_sidecar`. Extra bytes
≈ the post-sidecar size. No extra proof bytes: authenticity comes from recomputing the pre
root and comparing to `previous_header.state_root`, which the witness already carries.

**Commitment + openings (deferred).** Carry only the pre-leaves the STF touches/deletes, each
with a qMDB inclusion branch (+ next-key ring for the completeness argument) against
`previous_header.state_root`. Extra bytes ≈ `touched · (leaf + ~32·32 branch)`. Wins big when
`touched ≪ R`, loses when a block churns most of the book, and needs the exact-keyspace
completeness proof extended to the *pre* root — materially more guest code.

### Numbers

| Book (R, M, W) | Witness today | + full pre-sidecar | + openings (touched=20) |
|---|---|---|---|
| Small (10, 5, 2) | ~2.5 KB | **~4.3 KB** (+1.8 KB) | ~2.5 KB + ~22 KB proof |
| Medium (100, 30, 20) | ~19 KB | **~35 KB** (+16 KB) | ~19 KB + ~22 KB proof |
| Large (2000, 200, 200) | ~310 KB | **~600 KB** (+290 KB) | ~310 KB + ~22 KB proof |

Openings' fixed ~22 KB (20 touched leaves × ~1.1 KB branch+ring) is *worse* than full at
devnet scale and only pays off in the "large book, few touches" regime — i.e. testnet, not
devnet. The 2 GB Linode holds ~500 blocks/s-worth of full-pre-sidecar medium blocks for days;
the binding constraint is not disk but **guest proving cost**, and there the second full root
(below) is the real tax — which openings would eventually remove. That is the v3-vs-future
tradeoff, not a reason to start with openings.

**Decision: full inclusion now.** It reuses `compute_state_root_with_sidecar` unchanged,
keeps the witness symmetric (pre/post accounts already symmetric; make sidecars symmetric
too), and closes H4 with the smallest diff. Structure the pre-sidecar as its own top-level
field (not interleaved) so the future openings migration replaces one field cleanly.

### The `l1_deposits` fix is the *urgent* size lever

Unlike the sidecar (O(current state)), the full deposit prefix is **O(total deposit
history), re-sent every block**. At 172 B/deposit and N=1000 lifetime deposits, that is
**172 KB per block, forever** — 14 GB/day at 1 s cadence, which alone exhausts the 2 GB
devnet. The frontier form is **1024 B + 172·(deposits this block)** ≈ ~1 KB flat. This is the
single highest-value byte in the bump and the concrete reason SYB-214 rides this batch.

---

## 3 (detail). Removal/admit streams — proven or unproven?

This is the load-bearing design call. The removal/admit data (SYB-166 c+d) exists **only** to
let derived views (order-stats, history feed, exit accounting) move *out* of block production
(§P1). It is not value-conservation data; no invariant depends on it; the guest would never
consult it.

**Argument for unproven.** Once `pre_state_sidecar` is in the *proven* witness, most of the
removal/admit content is **derivable**, so proving it again is redundant:

- *Removed-order identity + owner + released amounts:* a removed resting order is present in
  `pre_state_sidecar.resting_orders` and absent from `state_sidecar.resting_orders`; its owner
  and reservation amounts are right there in the pre snapshot. **Derivable.**
- *New-vs-carried:* carried ⟺ present in `pre_state_sidecar.resting_orders`; new ⟺ present in
  post but not pre. Admit height for carried = the pre snapshot's `created_at`. **Derivable.**
- *`has_been_matched`:* not in `RestingOrderSnapshot`, and *not* reconstructable from pre/post
  balances alone (a fully-cancelled-after-partial order looks like a plain removal).
  **Not derivable — original data.**
- *Fine exit-reason:* "expired" vs "revalidated: insufficient balance" vs "market inactive"
  distinguishes causes that produce the *same* pre/post delta. **Not derivable — original data.**

So the genuinely-original view fields are two flags/enums per removed order. Putting the
*entire* stream in the proven witness to carry those two would force a commitment move and pay
guest cost for data no invariant reads. Putting it in an **unproven** `derived_view_sidecar`:

- costs **zero** guest cycles and **zero** proven bytes;
- lets **SYB-166 c+d land with no commitment move at all** (Increment 0);
- keeps the trust model honest — analytics were *always* sequencer-trusted; nothing about
  moving them out of the block path should make them attested. The proven witness still binds
  every balance/position/reservation via H4, so a sequencer that lies in the derived stream
  corrupts *views*, never *value*.

**Trap to avoid:** do not let a view silently start trusting the unproven stream for something
value-relevant. The rule: the `derived_view_sidecar` may carry *only* fields that are either
(a) derivable from the proven pre/post sidecar, or (b) presentational (`has_been_matched`,
exit-reason label). Anything a settlement or reservation invariant depends on belongs in the
proven witness, full stop.

**Which verifier layer consumes each:**

| Field | Home | Consumer |
|---|---|---|
| pre resting orders / reservations / withdrawals / markets / groups / deposit_root | proven `pre_state_sidecar` | `verify_sidecar` (transition check) + pre-root auth in block layer + guest |
| deposit frontier + this-block leaves | proven `DepositAccumulatorWitness` | deposit binding (`verify_deposit_prefix` / guest `verify_public_input_binding`) |
| removed-order identity, owner, amounts, new-vs-carried, admit height | unproven `derived_view_sidecar` | analytics/order-stats view (`AnalyticsState::observe_block`, per §P1) — **no** verifier layer |
| `has_been_matched`, exit-reason label | unproven `derived_view_sidecar` | same view; original data, view-trusted |

---

## 4 (detail). Deposit accumulator: frontier, not MMR

The on-chain side is **already** a fixed-depth (32) append-only incremental Merkle tree:
`SybilVault._appendDepositLeaf` (`contracts/src/SybilVault.sol:348-363`) folds each new leaf
through `filledSubtrees`/`zeroHashes`; the vault publishes `currentDepositRoot` and a
`depositRootByCount[count] => root` checkpoint map (`:46`, `:152-154`). The Rust mirror is
`deposit_prefix_roots` (`crates/sybil-l1-protocol/src/lib.rs:157-178`), byte-identical
recurrence.

An MMR/peaks accumulator would be a *different* structure with a *different* root — it would
**diverge from the deployed contract** and force a vault redeploy and a new inclusion-proof
format for no benefit. The frontier of the existing tree is the natural, contract-faithful
witness:

- **Witness carries:** `pre_frontier` (32 hashes = the exact `filledSubtrees` at block start),
  `pre_count`, and the leaves credited this block (§1b).
- **Guest recomputes:** folds this-block leaves onto `pre_frontier`, checks the result
  reproduces `pre_state_sidecar.bridge.deposit_root` (before) and `state_sidecar.bridge.deposit_root`
  (after). Both roots are already committed in the (now pre and post) bridge sidecars.
- **Composition with `SybilVault`:** the guest's post-root must equal the vault's
  `depositRootByCount[post_count]`; the fold recurrence is the same code, so agreement is
  structural. No new opening format, no contract change.
- **SYB-190 indexer reconciliation:** the `sybil-l1-indexer` feeds ordered deposit leaves to
  the sequencer; the frontier is a pure fold of that ordered stream, so indexer and sequencer
  agree by construction. The frontier also lets the indexer's reconciliation check a single
  block's credited deposits against `depositRootByCount` without replaying all history.
- **SYB-159 WAL/replay:** the frontier is derivable from the persisted deposit log on replay
  (fold the first `pre_count` leaves); no new durable-write ordering is introduced. The
  `deposit_log` may still be persisted for indexer/debug use — it just stops riding the
  witness.

**Keep-full-prefix-for-devnet is rejected:** the growth is O(history)·per-block, the worst
scaling in the whole witness, and the frontier is *simpler* to verify (fixed 1 KB fold) than
the current O(N) prefix recomputation in `verify_public_input_binding`
(`crates/sybil-zk/src/lib.rs:589-660`).

---

## 5 (detail). Verifier layer changes + guest cost

**Layer 3 / block (`crates/sybil-verifier/src/block.rs`).** Add **pre-state-root
authentication**: recompute `compute_state_root_with_sidecar(&witness.pre_state, &witness.pre_state_sidecar)`
and assert it equals `previous_header.state_root`. Today `previous_header` is used only for
parent-hash chaining (`:91-114`); this makes the pre snapshot load-bearing. Genesis (no
previous header) carries an empty pre-sidecar.

**Layer 5 / sidecar (`crates/sybil-verifier/src/sidecar.rs`).** Upgrade `verify_sidecar` from
"derive-what-you-can" to a **full pre→post transition check**, deleting all three
`SIDECAR_WITNESS_GAPS`:

- *Resting orders:* every order in `pre.resting_orders` is either present unchanged in
  `post.resting_orders`, or accounted for by activity — filled to completion (a fill exists),
  expired (`pre.expires_at_block < height`), or cancelled (`OrderCancelled` event). A deletion
  with no explanation now fails. New post orders must correspond to accepted `orders`.
- *Reservations:* the existing rollup equality (`:81-110`) now runs against **both** pre and
  post, so deleting an order + its reservation together no longer nets out.
- *Withdrawals:* pre withdrawals must persist into post unless expired/claimed by rule;
  deleting a pre-existing leaf with no event now fails (gap #2 closed).
- *Markets/groups:* status/metadata/membership changes must correspond to `MarketResolved` /
  `ExtendMarketGroup` / admin events; a silent edit fails (gap #3 + **SYB-212** closed).

**Deposit binding.** `verify_deposit_prefix` (native, `sidecar.rs:249-302`) and the guest's
`verify_public_input_binding` deposit block (`crates/sybil-zk/src/lib.rs:589-660`) switch from
"recompute the whole prefix" to "fold this block's leaves onto `pre_frontier` and check both
endpoints" (§1b/§4).

**Guest (`verify_state_transition_input`, `crates/sybil-zk/src/lib.rs:305-326`).** Gains the
pre-root authentication and the transition check; the four `ensure_valid` layers stay but
`verify_sidecar` now does real work. `witness_root` / `da_commitment` recompute over the new
`canonical_witness_bytes`; the guest exe/vm commitment therefore changes → **adapter repin**
(this is the one deliberate move).

**Guest cost (qualitative).** The state-root leg roughly **doubles**: the guest now
authenticates a *second* full root (pre) in addition to the post root — both O(total state)
qMDB hashing (already the dominant cost, ZK-4/ZK-9). The transition check adds a linear
pre/post set-difference over resting orders/withdrawals/markets — cheap next to hashing. The
deposit leg **drops** from O(total deposits) to O(depth + block deposits). Net: proving cost
rises, dominated by the second root — which is precisely what the deferred openings/delta
witness (Increment 2) exists to remove.

---

## 6 (detail). Migration: fresh-genesis, version byte 2 → 3

Devnet policy is **fresh-genesis redeploy — no migration path** (`docs/runbooks/devnet-redeploy.md`,
"Devnet policy is fresh-genesis redeploy"). This bump changes `canonical_witness_bytes` and the
guest exe, so it already forces an adapter repin and (per the runbook's guest-commitment +
consensus-surface rules) a fresh genesis. No state-migration tooling exists and none is worth
writing for devnet. **Recommendation: fresh-genesis, following the existing runbook.**

- Bump `WITNESS_FORMAT_VERSION` `2 → 3` (§0) as a hard tripwire against a stale decoder.
- Regenerate the golden witness vectors and the guest commitment; run
  `scripts/zk-guest-fingerprint.sh --check` and repin the `OpenVmVerifierAdapter` constructor
  per Step 1 of the runbook.
- Note: neither addition changes the **state-leaf** schema — `pre_state_sidecar` is
  authenticated against the *already-committed* `previous_header.state_root`, and the deposit
  frontier is authenticated against the *already-committed* `bridge.deposit_root`. So the
  state root itself is schema-stable; the break is in the witness + guest, which is what forces
  the repin. Fresh genesis is still the clean path (heights/roots chain from empty).

Reject schema-versioned coexistence (running v2 and v3 decoders side by side): it doubles the
consensus-critical encoder surface — exactly the hand-synchronized-encoder hazard Theme 6
warns is a soundness bug waiting to happen — to save a devnet reset that is already routine.

---

## 7 (detail). Implementation split — landable increments

Ordered so that **at most one guest-commitment move** occurs, and it is Increment 1.

### Increment 0 — Unproven derived-view stream (NO commitment move)

Add `DerivedViewSidecar` to the block record / DA envelope, outside `canonical_witness_bytes`.
Emit the block-start `expire`/`revalidate` removal stream, `has_been_matched`, exit-reason,
and direct-admit timing into it from `produce_block_in_place`. Move the view consumers
(order-stats, history) to read the stream via `AnalyticsState::observe_block(&SealedBlock, …)`
(§P1) instead of inline calls.

- **Gate:** view-parity test — analytics produced from the stream are byte-identical to those
  produced inline today, over the scenario suite. No golden-witness or guest change.
- **Unblocks:** SYB-166 c+d view extraction, with zero commitment cost.

### Increment 1 — Witness schema v3 (THE one commitment move)

Add `pre_state_sidecar` (§1a) and replace `l1_deposits` with the deposit frontier+delta
(§1b). Bump the version byte. Land the verifier changes (§5): pre-root authentication, the
full `verify_sidecar` transition check, the frontier deposit binding. Land the guest changes,
regenerate goldens + guest commitment, repin the adapter, fresh-genesis the devnet.

- **Gates:** (a) golden witness vectors regenerated and pinned; (b)
  `scripts/zk-guest-fingerprint.sh --check` green and adapter repinned; (c) new
  deletion-attack tests fail-closed — resting-order+reservation co-deletion, pre-existing
  withdrawal deletion, silent market/group/status edit, and **SYB-212** `ExtendMarketGroup`
  membership change (the three `SIDECAR_WITNESS_GAPS` become three passing red→green tests);
  (d) a frontier-equivalence test asserting the guest fold matches
  `SybilVault._appendDepositLeaf` / `deposit_prefix_roots` on shared vectors.
- **This is the deliberate batch:** H4 completion + SYB-212 + SYB-214, one commitment move.

### Increment 2 — Delta/openings witness (DEFERRED — a separate future move)

Ambitious idea #3 / §P1 scale fix: replace the full tri-state snapshots **and** the full
`pre_state_sidecar` with touched-leaf qMDB openings against `previous_header.state_root`, and
drop `post_system_state` (recoverable from `pre_state` + system events). Removes the
second-full-root proving tax (§5) and makes proving cost O(activity), not O(total state).

- **Explicitly out of scope for SYB-216.** It is a *second* commitment move and a materially
  bigger lift (extend the exact-keyspace completeness proof to the pre root). Sequence it for
  testnet, when book size makes the full pre-sidecar's proving cost bite. Increment 1 is
  structured (pre-sidecar as its own top-level field) so this is a clean field replacement.

---

## Open questions

1. **Withdrawal lifecycle rule.** The full transition check needs an exact rule for when a pre
   withdrawal leaf may legitimately leave `post` (expiry height reached? claim event? are
   withdrawals append-only until expiry?). `sidecar.rs` currently only checks *creation*. The
   deletion-detection depends on pinning this — confirm against the settlement/claim path.
2. **Second-root proving budget.** Is the ~2× state-root proving cost acceptable on the real
   prover at devnet book sizes, or does it push Increment 2 forward? Needs a measured proving
   run on a representative block, not just the qualitative estimate in §5.
3. **`has_been_matched` provenance.** Confirmed non-derivable from pre/post here — but if a
   later change adds a per-order fill-count to the proven resting-order snapshot for another
   reason, `has_been_matched` becomes derivable and could leave the unproven stream. Track as a
   possible future simplification, not a blocker.
4. **Frontier vs `depositRootByCount` authority.** The guest authenticates the frontier against
   the *sidecar's* `deposit_root`. Should it additionally bind to the on-chain
   `depositRootByCount[count]` inside the proof, or is that the settlement contract's job at
   root-submission time? Leaning "settlement's job" (keeps the guest chain-agnostic), but
   confirm with the SYB-190 reconciliation design.
5. **Exit-reason enum stability.** The unproven `exit_reason` codes mirror the sequencer's
   `RestingExit` / revalidation reasons; since the stream is unproven its layout can drift, but
   a shared enum between sequencer and view avoids a silent skew. Decide whether to hoist it
   into a shared type now or later.
