---
tags: [zk, spec]
layer: verification
crate: sybil-verifier
status: current
last_verified: 2026-04-17
---

# State Root Schema

Normative spec for the state root: what it commits to, how bytes are produced
from accounts, and how the scheme evolves from the current flat hash
(Phase 1) to an authenticated Merkle tree anchored in
[commonware-storage qmdb](https://commonware.xyz/blogs/qmdb) (Phase 2).

[[State Root and Parent Hash]] is the concept introduction — one-page story
of *why* we have a state root and a parent hash. This doc is the *how*.

## What's committed to

The state root is a 32-byte digest over **all account state after
settlement, including system-event effects**. Specifically, the fields of
`AccountSnapshot`:

- `id` (u64)
- `balance` (i64)
- `total_deposited` (i64)
- `positions` — non-zero position triples `(market_id, outcome, qty)`
- `events_digest` — per-account running BLAKE3 accumulator over events

Anything not in `AccountSnapshot` is explicitly **not** covered: the mempool,
the resting order book, market metadata, oracle state, sequencer clock. Those
are either reproducible from the witness (order book), irrelevant to balances
(mempool), or live in separate commitments (markets are referenced by id
inside positions, their full metadata is not part of state root).

This matters for [[Order Types|time-in-force]]. The current verifier can check
that an order included in a block was not expired at `header.height`, because
time-in-force lives in the private witness order. It cannot, from the current
state root alone, prove that a post-block IOC remainder was not kept in the
off-chain resting book. Phase 2 should either include resting orders in the
authenticated state tree or publish a separate order-book root when that
property becomes bridge-critical.

## Phase 1 (current): flat canonical hash

Implementation: `crates/sybil-verifier/src/block.rs::compute_state_root`.

```
state_root_v1 = BLAKE3( concat(account_bytes for account in accounts sorted by id) )
```

Where `account_bytes` follows [[Canonical Serialization]] v1. Implementations
MUST sort accounts by `id` ascending before concatenation; they MUST filter
zero-qty positions; they MUST sort positions by `(market_id, outcome)`.

**Properties.**

- O(n) recompute per block, where n = number of accounts.
- No per-account inclusion proof: a third party cannot verify "account X had
  balance B at height H" without receiving the full state.
- Fine up to ~10k accounts on the current sequencer hardware. Breaks beyond
  that (memory + time both grow linearly, and the full state must fit in
  a single hash pass).
- Matches the [[Block Lifecycle]] step order: settlement finishes, then the
  sequencer calls `compute_state_root(accounts)` and writes the result into
  the header.

**Known limitations.**

- Changing one account requires rehashing everything.
- The bytes hashed are not themselves a Merkle tree, so Merkle paths are not
  extractable.
- Historical proofs ("account X had balance B at height H−1000") require
  replaying old state or keeping full snapshots — neither is production-ready.

Phase 2 addresses all of these by promoting an authenticated data structure
that already exists in the sequencer into the commitment scheme.

## What already exists in the sequencer

Before proposing anything new it's worth grounding the discussion in the
current code:

- `crates/matching-sequencer/src/qmdb_accounts.rs` wraps an
  [`OrderedVariableDb`](https://docs.rs/commonware-storage/latest/commonware_storage/qmdb/current/ordered/variable/struct.Db.html)
  — qmdb's MMR-backed ordered key-value store — and uses it to persist
  account snapshots at block boundaries.
- `crates/matching-sequencer/src/account_storage.rs::FencedAccountStorage`
  is the `AccountStateStore` implementation wired into the sequencer today.
  It keeps two snapshot slots (A/B) and flips between them under a redb
  fence.
- Keys are `slot_prefix || 'a' || account_id_be_u64`; values are
  `rmp_serde::to_vec(&account)` (MessagePack).
- The qmdb type alias pins the hasher to `commonware_cryptography::Sha256`.
- The MMR root produced by `batch.merkleize(...).apply_batch(...)` exists
  **but is not currently exposed or used** — state root is still the flat
  BLAKE3 of §Phase 1.

So qmdb is shipping today as a storage layer. It's not yet the source of
truth for the block header's `state_root`.

## Phase 2 (target): promote qmdb's Merkle root to `state_root`

The cheapest path to authenticated state is to reuse the MMR that qmdb
already maintains. Write once, hash once.

### Design

1. After settlement, commit the batch to `QmdbAccounts` as today.
2. Read the MMR root back from qmdb (the `merkleize` step already builds it;
   we just need to surface it).
3. Publish that MMR root in the block header as `state_root`.
4. The per-account serialization inside qmdb stays
   `rmp_serde` for now **but** is wrapped so that what qmdb commits to is
   `BLAKE3("sybil/account/v1" || canonical_account_bytes)` — see §Encoding
   alignment.

Result: every persisted account ends up under an authenticated MMR whose
root is the state root. Per-account inclusion proofs come for free from
qmdb's MMR proof API. Historical proofs come for free from qmdb's
append-only structure.

### Hasher: swap SHA-256 for BLAKE3

qmdb is generic over the hasher. The current alias:

```rust
type AccountDb = OrderedVariableDb<
    MmrFamily, commonware_tokio::Context, Vec<u8>, Vec<u8>,
    commonware_cryptography::Sha256, OneCap, CHUNK_SIZE,
>;
```

`commonware-cryptography` ships a BLAKE3 module
(`commonware_cryptography::blake3`). Switching the hasher parameter aligns
qmdb's MMR hash with the rest of the system's hashing (events digest,
parent hash, v1 state root) and keeps a single hash in the ZK circuit.

**Open question:** whether to keep SHA-256 to match RISC Zero / SP1's
native-precompile story and thereby keep proving cheap. BLAKE3 circuit
support is progressing but less mature. Decide in the proving-system
selection issue (SYB-27).

### Encoding alignment

Today's qmdb values are `rmp_serde`-serialized `Account` structs. For
persistence that's fine — the store round-trips the Rust struct.

For a state-root commitment we need the on-disk bytes to be an artifact of
the canonical spec, not of the serde derive order. Two paths:

**(a) Double-encode.** Keep qmdb values as `rmp_serde` for storage
ergonomics; publish `BLAKE3("sybil/account/v1" || canonical_account_bytes)`
as the MMR leaf value. qmdb then commits to the canonical digest, not the
MessagePack blob. Storage size unchanged; one extra BLAKE3 per touched
account per block (cheap).

**(b) Replace the encoding.** Store canonical bytes directly in qmdb;
drop `rmp_serde`. Saves the extra hash but ties the on-disk format to the
canonical spec forever, so every encoding change is a storage migration.

Recommendation: **path (a)**. It keeps storage changes cheap, lets the
canonical spec evolve independently (via version bumps), and adds a thin
BLAKE3 layer whose performance cost is far below settlement.

### Incremental update

qmdb already does the right thing: `new_batch` / `write` / `merkleize` /
`apply_batch`. The sequencer already calls this sequence on every
boundary. The change is just:

- After `apply_batch`, ask qmdb for the resulting MMR root.
- Write that root into `BlockHeader.state_root`.

Cost per block: bounded by the number of touched accounts × hash-per-level
— same ballpark as any Merkle KV store. No change to Big-O.

### Proof API

qmdb exposes MMR proofs natively. Wrap them in a Sybil-facing endpoint:

```
GET /v1/proofs/state/{account_id}?height={N}
  → {
      "account_id": 42,
      "block_height": N,
      "state_root": "0x...",
      "leaf": "0x...",                         // BLAKE3 of canonical account bytes
      "account_snapshot": { ... },              // canonical fields, optional
      "mmr_proof": { ... }                      // qmdb's native proof bytes
    }
```

Verifier runs qmdb's proof verifier with the supplied hasher; then asserts
`leaf == BLAKE3("sybil/account/v1" || canonical_account_bytes(snapshot))`.

For consumers that can't link qmdb (e.g., a Solidity verifier), we export
either (a) a self-contained MMR proof verifier compiled via
`commonware-storage` helpers, or (b) a ZK proof wrapping the verification.
The choice depends on the ZK settlement architecture (SYB-27, SYB-30) and
is out of scope for this RFC.

## Alternatives considered

### Build a fresh Sparse Merkle Tree

Cost: new dependency surface, duplicate storage, reimplement proof API,
reimplement incremental updates. Benefit: decouples state root from
qmdb's evolution. Rejected — qmdb is production today, actively maintained
by Commonware, and gives us Merkle-for-free.

### Verkle tree

Smaller proofs (vector commitments via KZG) but pulls in elliptic-curve
crypto and a KZG ceremony. Premature when on-chain proof size is not the
bottleneck and the ZK circuit is recursive anyway.

### Keep qmdb for persistence only; build a separate commitment tree

Two trees, two roots, two sets of invariants. Only justifiable if we need
radically different hashing (e.g. Poseidon for ZK) on the commitment side
while qmdb stays in its native form. That's a real possibility long-term;
for Phase 2 we can get away with one tree.

## Migration from Phase 1 to Phase 2

Hard fork at a chosen block height `H`:

- Blocks with `height < H` use `state_root_v1` (flat hash).
- Block `H` uses `state_root_v2` — the qmdb MMR root (hashed with whichever
  hasher we commit to in §Hasher) under domain prefix
  `"sybil/state-root/v2"`. The v2 root is computed from the full account
  set at end of settlement at block `H`; it does not chain back to the v1
  root.
- Blocks with `height > H` continue with qmdb-rooted state.

Verifiers dispatch on `header.height`: read block header, pick the
algorithm, re-verify. Both implementations retained in `sybil-verifier`
during the migration window (forever, in practice — historical blocks need
to remain verifiable).

Domain-separation ensures no collision between v1 and v2 roots even if they
cover the same account set.

## Retention and historical proofs

Phase 1: current state only; historical proofs not supported without
replay.

Phase 2 (qmdb): qmdb is append-only by design, so historical state proofs
come **for free** within the retained journal window. Retention is
configured by `items_per_blob` / journal-partition pruning; the current
sequencer keeps the full log. A future "prune to last N blocks" policy is
a storage question, not a commitment-scheme question.

## Relation to events and witness roots

| Root | Commits to | Primary consumer |
|---|---|---|
| `state_root` (this doc) | post-settlement account set | escape hatch, on-chain settlement |
| `events_root` ([[Proof Architecture]]) | this block's event stream | external verifiers of "did F happen" |
| `witness_root` ([[Block Witness]]) | the full audit package | provers, replayers |

All three are expected to live in the extended block header
(`BlockHeader v2`, defined in [[Block Witness]]).

## Open questions

1. **Hasher.** Keep SHA-256 (matches RISC Zero / SP1 native precompiles,
   expensive-but-established ZK cost) or swap to BLAKE3 (system-wide
   consistency, smaller native cost, less mature in-circuit). Defer to the
   proving-system selection issue (SYB-27).
2. **Leaf encoding path (a) vs (b).** Wrap-with-BLAKE3 (current
   recommendation) or replace rmp_serde with canonical bytes directly.
   Decision affects every future on-disk schema change.
3. **Exposing qmdb's MMR root.** The `merkleize` API builds the root but
   `QmdbAccounts` does not currently surface it. Needs a new method on
   `AccountStateStore`. Small change; implementation issue, not a design
   issue.
4. **Solidity verifier.** Verifying an MMR proof on-chain is viable but
   non-trivial. Needs an implementation plan in the ZK settlement RFC
   (SYB-30) before we can commit to Phase 2 going on-chain.
5. **Events tree co-habitation.** The events tree from
   [[Proof Architecture]] is orthogonal but we should decide whether it
   also lives in qmdb (simpler) or in a standalone Merkle structure
   (separation of concerns).

## Test vectors

**Phase 1 — empty state:** `state_root_v1 = BLAKE3("") = af1349b9...e41f3262`.

**Phase 1 — one account (same as [[Canonical Serialization]] Vector 2):**

```
account_bytes = (72 bytes, see Canonical Serialization Vector 2)
state_root_v1 = BLAKE3(account_bytes)
```

**Phase 2 — concrete vectors depend on hasher choice (§Open question 1) and
are committed alongside the first implementation PR.** The spec-level
assertion is:

```
for each account A with id i:
    leaf_i = H("sybil/account/v1" || canonical_account_bytes(A))

mmr_root = qmdb_mmr_root( {(key_i, leaf_i) for each i}, hasher = H )
state_root_v2 = H("sybil/state-root/v2" || mmr_root)
```

where `H ∈ {SHA-256, BLAKE3}` per Open Question 1.

## Where this lives

> `crates/sybil-verifier/src/block.rs` — `compute_state_root` (Phase 1)
> `crates/matching-sequencer/src/block.rs` — writes `state_root` into the block header
> `crates/matching-sequencer/src/canonical_state.rs` — canonical account ordering used by Phase 1
> `crates/matching-sequencer/src/qmdb_accounts.rs` — qmdb wrapper (Phase 2 will surface its MMR root)
> `crates/matching-sequencer/src/account_storage.rs` — `AccountStateStore` trait; Phase 2 extends this to return a commitment root

## See also

- [[State Root and Parent Hash]] — the concept intro this doc normalizes
- [[Canonical Serialization]] — byte-level rules for `canonical_account_bytes`
- [[Block Witness]] — the witness the state root lives inside
- [[Proof Architecture]] — events tree (complementary) and proof-composition patterns
- [[ZK Integration Path]] — how the state root anchors the on-chain proof chain
- [[Persistence]] — storage tiers and the qmdb wrapper
