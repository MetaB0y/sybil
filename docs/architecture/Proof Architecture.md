---
tags: [zk, infrastructure]
layer: verification
status: planned
last_verified: 2026-04-30
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

The extended header gives two authenticated roots:
1. **State tree** (`state_root`): "after this block, this complete validium state leaf has value V" — inclusion or exclusion proof against the typed state root.
2. **Events tree** (`events_root`): "fill F happened in block N" or "these are ALL fills in block N" — inclusion/completeness proof against the events Merkle root.

Combined with `parent_hash` chaining, a prover can make claims spanning any range of blocks.

## Three Authenticated Structures

### 1. State Tree

A typed authenticated key-value tree over complete validium state, updated
incrementally each block. The current [[State Root Schema]] implementation is
a native qMDB root over accounts, bridge leaves, markets, market groups,
active resting orders, and aggregate reservations.

**Keys**: typed namespaces such as `acct/{account_id}`,
`acct_resv/{account_id}`, `order/{order_id}`, `market/{market_id}`,
`market_group/{group_id}`, `withdrawal/{withdrawal_id}`, plus `sys/*`.

**Values**: canonical Sybil bytes for each leaf type.

**What it proves**:
- Account X has balance B at block N (inclusion proof)
- Account X does not exist at block N (non-inclusion proof, requires sparse or sorted tree)
- Resting order Y is active or absent at block N
- Market M has lifecycle/resolution state S
- Withdrawal W exists and can be claimed against an accepted root
- The complete validium state at block N (full tree)

**Current implementation**: ordered qMDB authenticated key-value store using
SHA-256 for the native state root. The committed leaves cover accounts,
bridge state, markets, market groups, resting orders, and reservations.
Account leaves include `events_digest`, a running BLAKE3 accumulator over
fills and admin events that touched the account.

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

### Current: Typed qMDB State Root

`BlockHeader.state_root` is the native qMDB root over the typed state leaves
specified in [[State Root Schema]]. It covers accounts, reservations, resting
orders, market lifecycle state, market groups, bridge counters, deposit root,
and active withdrawal leaves.

The verifier currently recomputes that root from the full witness by inserting
the typed leaves into a fresh qMDB and comparing the native qMDB root. Runtime
persistence stores the same keyspace in a dedicated typed-state qMDB, so proof
APIs can verify directly against the header root.

### Next: Proof API

Expose endpoints for requesting authenticated data:

- `GET /v1/proofs/state/{key}?height={N}` → typed state inclusion/exclusion proof
- `GET /v1/proofs/events?height={N}` → events tree for block N
- `GET /v1/proofs/events/{account_id}?from={A}&to={B}` → account's events with Merkle proofs

The sequencer needs to retain enough history to serve these: state tree
history at each height, or enough data to reconstruct it. This interacts with
the persistence tiers because authenticated state snapshots need to be stored
or reconstructable.

### Later: Integration with ZK Pipeline

The [[Block Witness]] evolves: instead of full `pre_state` / `post_state` snapshots, it includes qmdb paths for the typed leaves touched by the block. The ZK circuit verifies the paths against the state root, applies settlement and order-book/market-state changes, and verifies the new state root.

This is when the authenticated data layer and the validity proof pipeline converge.

## Candidate: qmdb

[QMDB](https://commonware.xyz/blogs/qmdb) (LayerZero research + Commonware productionization) is an append-only authenticated database: a log of key updates with an MMR (Merkle Mountain Range) overlay.

Key properties:
- Append-only — merklization only touches the right side of the tree (minimal memory, no disk reads for new writes)
- Supports current state proofs and historical state proofs
- Single Rust implementation (MIT/Apache 2.0), rapidly maturing
- Available as `commonware-storage::qmdb`

It's a natural fit for the state tree: typed state leaves are keys, canonical
state values are values, and blocks produce updates. The MMR structure means
we get historical state proofs within the retained journal window — "account
X had balance B at block 1000" or "order Y was absent at block 1000" without
storing a separate tree snapshot per height.

Stability: ALPHA. It is already in the sequencer as the account snapshot
store, so the typed state store should reuse it unless the ZK/bridge
implementation proves qMDB proof verification is too expensive.

## What Changes in Sybil

| Component | Current | With events root | With proof API |
|-----------|---------|------------------|----------------|
| `BlockHeader` | state_root, parent_hash | + events_root | same |
| `Block` | orders, fills, prices, rejections | + system_events | same |
| `compute_state_root()` | native typed qMDB root | same | same |
| `Fill` struct | order_id, qty, price, account_id | same | same |
| `Account` | balance, positions, total_deposited, events_digest | same | same |
| `AccountStore` | HashMap | same | mirrored into authenticated KV |
| `store.rs` | redb tables + account qMDB snapshots | + events tree storage | dedicated typed-state qMDB |
| Verifier Layer 3 | checks state_root, parent_hash | + checks events_root | verifies qMDB paths |
| `BlockWitness` | full pre/post state snapshots + system_events | same | qMDB paths for touched state leaves |
| API | no proof endpoints | same | + proof endpoints |

## Resolved Decisions

1. **Events tree structure**: flat list of leaves, sorted by canonical block order. Per-account indexing can be added later if scans become too expensive.

2. **Events hash function**: BLAKE3. It's already the event/header hash and remains acceptable for per-block event authentication.

3. **Typed state-root hash function**: SHA-256. This matches the current qmdb instantiation and is easier to route through ZK/EVM verification paths than BLAKE3.

4. **Range inactivity compression**: implemented today as `events_digest` on `Account`. Equal digests at two trusted heights imply no account-level activity in between.

## Remaining Open Question

1. **Historical state retention**: how much qmdb journal history is retained locally and on DA. This is a DA/recovery policy question, not a state-root schema question.
2. **Operator replacement**: state root commits to complete state, but state data must be available independently for a replacement operator. See SYB-116 and SYB-76.

## Related Notes

- [[ZK Integration Path]] — the validity proof pipeline this feeds into
- [[L1 Settlement and Vault]] — how accepted roots and withdrawal proofs are used on-chain
- [[Block Witness]] — evolves to use Merkle paths instead of full snapshots
- [[State Root and Parent Hash]] — state-root concept and qMDB commitment
- [[Four-Layer Verification]] — gains events_root check in Phase 1
- [[Persistence]] — storage requirements for authenticated structures
- [[Settlement]] — fills need account_id for events tree
