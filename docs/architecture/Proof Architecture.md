---
tags: [zk, infrastructure]
layer: verification
status: design
last_verified: 2026-04-10
---

# Proof Architecture

Sybil is a [[ZK Integration Path|validium]]: off-chain execution, on-chain state commitments, validity proofs. This note defines the **authenticated data layer** — the cryptographic structures that make arbitrary account-level proofs possible.

## Design Philosophy

**Authenticate data, not proof types.** We don't enumerate specific proofs ("prove PnL > X", "prove I didn't trade market M"). Instead, we provide authenticated data primitives — Merkle commitments over state and events — and any proof is composed from these by an external prover.

The sequencer's job: produce blocks, authenticate every piece of data in them, commit roots to a trust anchor. A prover's job: extract the relevant authenticated data, run computation, produce a claim with proof. This could be a ZK circuit, an AI agent, or a human with a script.

Analogy: we build the database with authenticated indexes. We don't pre-define the queries.

## Trust Anchors

The **block header** is the root of trust. If a verifier trusts a block header (via the on-chain state root chain or a validity proof), they transitively trust anything provable from its commitments.

Current header:
```
height | parent_hash | state_root | order_count | fill_count | timestamp_ms
```

Extended header:
```
height | parent_hash | state_root | events_root | order_count | fill_count | timestamp_ms
```

Two new guarantees from `events_root`:
1. **State tree** (`state_root`): "after this block, account X has balance B and positions P" — inclusion proof against the state Merkle root.
2. **Events tree** (`events_root`): "fill F happened in block N" or "these are ALL fills in block N" — inclusion/completeness proof against the events Merkle root.

Combined with `parent_hash` chaining, a prover can make claims spanning any range of blocks.

## Three Authenticated Structures

### 1. State Tree

A Merkle tree over all account state, recomputed (incrementally) each block.

**Key**: `AccountId` (u64)
**Value**: `hash(balance || total_deposited || sorted_positions || events_digest)`

**What it proves**:
- Account X has balance B at block N (inclusion proof)
- Account X does not exist at block N (non-inclusion proof, requires sparse or sorted tree)
- The complete account set at block N (full tree)

**Current implementation**: flat BLAKE3 hash over all accounts — O(n) per block, no per-account proofs. The hashed value already includes a per-account `events_digest`, a running BLAKE3 accumulator over fills and admin events that touched the account.

**Target**: authenticated key-value store (e.g., sparse Merkle tree, qmdb). O(k log n) per block where k = accounts touched. Per-account Merkle paths are O(log n).

**Why this enables flexible proofs**:
- PnL: state proof at block A + state proof at block B → `portfolio_value_B - total_deposited_B` (note: `total_deposited` is already on the Account struct)
- Sharpe ratio: state proofs at blocks A, A+k, A+2k, ..., B → compute returns series → mean/std
- Current positions: single state proof at latest block
- Solvency: state proof showing `balance ≥ 0` and no negative positions
- Inactivity: if `events_digest_A == events_digest_B`, the account had no recorded fills/deposits/resolutions in that range (assuming collision resistance)

### 2. Events Tree

A Merkle tree over everything that happened in this block, built fresh each block.

**Leaves** (canonical encoding of each event):

| Event type | Fields |
|------------|--------|
| `Fill` | order_id, fill_qty, fill_price, account_id, market_ids |
| `OrderAccepted` | order (full), account_id |
| `OrderRejected` | order, account_id, reason |
| `CreateAccount` | account_id, initial_balance |
| `Deposit` | account_id, amount |
| `Withdrawal` | account_id, amount |
| `MarketResolved` | market_id, payout_nanos, affected_accounts |
| `MintAdjustment` | market_id, outcome, position_delta, balance_delta |

Current groundwork already landed:
- `Fill` carries `account_id`
- Blocks carry `system_events`
- Each account carries `events_digest`, so range-inactivity proofs can often use state snapshots instead of scanning every block event

**What it proves**:
- Fill F happened in block N (inclusion proof)
- These are ALL events in block N (tree completeness — leaf count matches `order_count + fill_count + ...`)
- Account X had no fills in block N (enumerate all fills, show none match — or maintain per-account sub-index)

**Why this enables flexible proofs**:
- "I didn't trade market M between blocks A and B": for each block in range, show events tree has no fills for my account on market M
- "I didn't receive deposits": for each block in range, show events tree has no Deposit events for my account
- Trade history: collect all Fill events for my account across blocks
- Volume: sum fill quantities from events proofs

### 3. Block Chain

Already exists: `parent_hash` links blocks into a hash chain. The on-chain contract stores the latest state root. Walking backwards from any trusted header reaches genesis.

**What it proves**:
- Block N exists and has specific commitments (header chain)
- Block ordering and timestamps
- No blocks were inserted or removed

**No changes needed** — the current chain structure works. The chain becomes more useful once state and events trees provide per-block proofs to anchor to.

## Proof Composition

A proof of an arbitrary claim follows the pattern:

```
Claim: "Account 42 had PnL > $500 between blocks 1000 and 2000"

Data:
  1. Block header at height 1000 (trusted via chain)
  2. State Merkle proof: account 42 at block 1000 → {balance: X, deposited: D, positions: [...]}
  3. Block header at height 2000 (trusted via chain)
  4. State Merkle proof: account 42 at block 2000 → {balance: Y, deposited: D', positions: [...]}
  5. Clearing prices at block 2000 (from events tree or header extension)

Computation:
  portfolio_value = balance_2000 + Σ(position * clearing_price)
  pnl = portfolio_value - total_deposited_2000
  assert pnl > 500 * NANOS_PER_DOLLAR
```

The verifier checks: (a) Merkle proofs verify against trusted state roots, (b) computation is correct.

For a ZK proof, the computation runs inside a circuit and the Merkle paths are private inputs. For a non-ZK attestation, the prover just provides the data and computation in the clear.

## Proof Sketches

### "My Sharpe ratio is > 2.0 over the last 30 days"

Data: state proofs at daily intervals (30 snapshots). Computation: daily returns → annualized sharpe.

### "I never traded market M"

Data: for each block in range, events tree proof showing no fills for my account on market M. If blocks are frequent (1/sec), this could be compressed by providing state proofs showing my position on market M is 0 at start and 0 at end, plus events proofs at a coarser granularity.

### "I had no deposits after block 1000"

Data: state proof at block 1000 showing `total_deposited = D`, state proof at block N showing `total_deposited = D`. Since `total_deposited` is monotonically non-decreasing and only increases on deposit, equality proves no deposits occurred.

### "My account was never funded by account X"

This requires provenance tracking not currently in the data model. Deposits are admin-only operations with no on-chain linkage to source. Once deposits come from L1 bridge (Phase 4), the deposit events in the events tree will include the L1 sender address, making this provable.

## Implementation Plan

### Phase 0: Events on Fill (done)

`Fill` now carries `account_id`. This was the prerequisite for useful event authentication.

### Phase 1: Events Tree (simplest, most useful)

Build a Merkle tree over block events and commit `events_root` in the block header. This is a pure addition — doesn't change existing state management.

- Canonical encoding for each event type
- Simple binary Merkle tree (balanced, padded)
- `events_root` added to `BlockHeader`
- Verifier checks `events_root` matches (new Layer 3 check)
- BLAKE3 as leaf/node hash (consistent with existing choices)

### Phase 2: Authenticated State Tree

Replace `compute_state_root()` with an authenticated key-value store.

- Sparse Merkle tree or qmdb (LayerZero/Commonware collaboration, see https://commonware.xyz/blogs/qmdb)
- Incremental updates: only rehash accounts touched by this block's fills
- Per-account Merkle path generation
- State root computation becomes O(k log n) instead of O(n)
- `AccountStore` backed by authenticated structure
- Verifier's state root check adapts to new tree format

This is the deeper change — it touches `AccountStore`, `compute_state_root()`, persistence (`store.rs`), and the verifier's Layer 3.

### Phase 3: Proof API

Expose endpoints for requesting authenticated data:

- `GET /v1/proofs/state/{account_id}?height={N}` → state Merkle proof
- `GET /v1/proofs/events?height={N}` → events tree for block N
- `GET /v1/proofs/events/{account_id}?from={A}&to={B}` → account's events with Merkle proofs

The sequencer needs to retain enough history to serve these (state tree at each height, or ability to reconstruct). This interacts with the persistence tiers — authenticated state snapshots need to be stored or reconstructable.

### Phase 4: Integration with ZK Pipeline

The [[Block Witness]] evolves: instead of full `pre_state` / `post_state` snapshots, it includes Merkle paths for the accounts touched by this block's fills. The ZK circuit verifies the Merkle paths against the state root, applies settlement, and verifies the new state root.

This is when the authenticated data layer and the validity proof pipeline converge.

## Candidate: qmdb

[QMDB](https://commonware.xyz/blogs/qmdb) (LayerZero research + Commonware productionization) is an append-only authenticated database: a log of key updates with an MMR (Merkle Mountain Range) overlay.

Key properties:
- Append-only — merklization only touches the right side of the tree (minimal memory, no disk reads for new writes)
- Supports current state proofs and historical state proofs
- Single Rust implementation (MIT/Apache 2.0), rapidly maturing
- Available as `commonware-storage::qmdb`

It's a natural fit for the state tree: accounts are keys, state is values, blocks produce updates. The MMR structure means we get historical state proofs for free — "account X had balance B at block 1000" without storing per-block snapshots.

Stability: ALPHA. Worth prototyping against, but we should be prepared to implement our own sparse Merkle tree if qmdb's API doesn't fit.

## What Changes in Sybil

| Component | Current | After Phase 1 | After Phase 2 |
|-----------|---------|---------------|---------------|
| `BlockHeader` | state_root, parent_hash | + events_root | same |
| `Block` | orders, fills, prices, rejections | + system_events | same |
| `compute_state_root()` | flat BLAKE3 over all accounts + `events_digest` | same | Merkle tree root |
| `Fill` struct | order_id, qty, price, account_id | same | same |
| `Account` | balance, positions, total_deposited, events_digest | same | same |
| `AccountStore` | HashMap | same | authenticated KV |
| `store.rs` | redb tables | + events tree storage | + state tree storage |
| Verifier Layer 3 | checks state_root, parent_hash | + checks events_root | state root via Merkle |
| `BlockWitness` | full pre/post state snapshots + system_events | same | Merkle paths for touched accounts |
| API | no proof endpoints | same | + proof endpoints (Phase 3) |

## Resolved Decisions

1. **Events tree structure**: flat list of leaves, sorted by canonical block order. Per-account indexing can be added later if scans become too expensive.

2. **Hash function**: BLAKE3. It's already the system hash, fast in the sequencer, and acceptable for the data layer even if the eventual circuit uses a different hash internally.

3. **Range inactivity compression**: implemented today as `events_digest` on `Account`. Equal digests at two trusted heights imply no account-level activity in between.

## Remaining Open Question

1. **Historical state retention**: do we store state trees for all blocks, or just recent ones? qmdb's append-only model gives history for free. A standard Merkle tree would need per-block snapshots or a rollback mechanism.

## Related Notes

- [[ZK Integration Path]] — the validity proof pipeline this feeds into
- [[Block Witness]] — evolves to use Merkle paths instead of full snapshots
- [[State Root and Parent Hash]] — current commitment scheme, replaced by Phase 2
- [[Four-Layer Verification]] — gains events_root check in Phase 1
- [[Persistence]] — storage requirements for authenticated structures
- [[Settlement]] — fills need account_id for events tree
