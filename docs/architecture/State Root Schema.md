---
tags: [zk, spec]
layer: verification
crate: sybil-verifier
status: current
last_verified: 2026-04-29
---

# State Root Schema

Normative spec for the state root: what it commits to, how bytes are produced,
and how the scheme evolves from the current typed leaf root to a complete
validium-state commitment anchored in
[commonware-storage qmdb](https://commonware.xyz/blogs/qmdb).

[[State Root and Parent Hash]] is the concept introduction — one-page story
of *why* we have a state root and a parent hash. This doc is the *how*.

## What's committed to

The current implementation is `state_root_v2`: a SHA-256 digest over sorted,
typed key/value leaves. It commits to account state after settlement,
including system-event effects, plus the bridge sidecar state needed for
normal withdrawals.

Current key families:

| Key family | Commits to |
|---|---|
| `acct/{account_id}` | `id`, `balance`, `total_deposited`, non-zero `positions`, `events_digest` |
| `sys/deposit_cursor` | highest consumed L1 deposit cursor |
| `sys/deposit_root` | deposit log root used by the bridge sidecar |
| `sys/next_withdrawal_id` | next withdrawal id counter |
| `withdrawal/{withdrawal_id}` | normal L1 withdrawal claim, recipient, token, amount, expiry, and nullifier |

The current implementation does **not** yet commit the resting order book,
reservation/open-exposure state, market metadata, or oracle lifecycle state.
Those are still required before the state root is a complete production
validium commitment.

The production target is a single typed qmdb root over the complete validium
state needed to verify, recover, and restart the exchange:

| Key family | Commits to |
|---|---|
| `acct/{account_id}` | balance, positions, total deposited, account event digest, withdrawal/nullifier metadata |
| `acct_resv/{account_id}` | aggregate open reservations or equivalent data needed to derive withdrawable cash |
| `order/{order_id}` | active resting order, owner, remaining quantity, expiry, and reservation metadata |
| `withdrawal/{withdrawal_id}` | normal L1 withdrawal claim, recipient, token, amount, expiry, and nullifier |
| `market/{market_id}` | market lifecycle, resolution state, and compact metadata commitment |
| `sys/*` | schema version, height marker, next ids, and global counters |

This matters for [[Order Types|order expiry]]. In the current subset the
verifier can check that an included order was not expired at `header.height`,
but cannot prove from `state_root` alone that a post-block expired remainder
was not kept in the off-chain resting book. In the complete typed root, active
resting orders are committed state, so order absence/existence becomes a state
proof rather than a witness-only claim.

## Current root: typed v2 subset

Implementation:

- `crates/sybil-verifier/src/block.rs::compute_state_root_with_bridge`
- `crates/sybil-verifier/src/block.rs::state_root_v2_leaves`
- `crates/matching-sequencer/src/block.rs::compute_state_root_v2`

```
state_root_v2 =
  SHA256(
      "sybil/state-root/v2"
   || leaf_count:u64
   || (key_len:u32 || key || value_len:u32 || value) * sorted_leaves
  )
```

Keys are sorted bytewise ascending. Account and withdrawal collections are
sorted by id before leaf construction. Values are canonical Sybil bytes under
the domains in [[Canonical Serialization]].

The sequencer also writes these same typed v2 leaves into the active fenced
qmdb slot under `slot_prefix || "v2:" || leaf_key`. Because the current
storage wrapper keeps A/B slots in the same qmdb and still stores legacy
MessagePack account copies alongside the typed leaves, the public
`state_root_v2` is **not yet** qmdb's native MMR root. The qmdb leaves exist
so the storage keyspace and public commitment use the same typed bytes before
we switch to native qmdb roots and proofs.

## Historical v1: flat canonical hash

Implementation: `crates/sybil-verifier/src/block.rs::compute_account_state_root_v1`.

```
state_root_v1 = BLAKE3( concat(account_bytes for account in accounts sorted by id) )
```

Where `account_bytes` follows [[Canonical Serialization]] v1. Implementations
MUST sort accounts by `id` ascending before concatenation; they MUST filter
zero-qty positions; they MUST sort positions by `(market_id, outcome)`.

This root is retained for tests, historical compatibility, and migration
discussion. It is no longer the root written into newly produced block
headers.

**Properties.**

- O(n) recompute per block, where n = number of accounts.
- No per-account inclusion proof: a third party cannot verify "account X had
  balance B at height H" without receiving the full state.
- Fine up to ~10k accounts on the current sequencer hardware. Breaks beyond
  that (memory + time both grow linearly, and the full state must fit in
  a single hash pass).
- Matched the original [[Block Lifecycle]] step order: settlement finished,
  then the sequencer called an account-only root helper and wrote the result
  into the header.

**Known limitations.**

- Changing one account requires rehashing everything.
- The bytes hashed are not themselves a Merkle tree, so Merkle paths are not
  extractable.
- Historical proofs ("account X had balance B at height H−1000") require
  replaying old state or keeping full snapshots — neither is production-ready.

The native qmdb target addresses all of these by promoting an authenticated
data structure that already exists in the sequencer into the commitment
scheme.

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
- Legacy account copies use keys `slot_prefix || 'a' || account_id_be_u64`
  with values `rmp_serde::to_vec(&account)` (MessagePack).
- Typed commitment leaves use keys `slot_prefix || "v2:" || leaf_key` and the
  same canonical values hashed into `state_root_v2`.
- The qmdb type alias pins the hasher to `commonware_cryptography::Sha256`.
- The MMR root produced by `batch.merkleize(...).apply_batch(...)` exists
  **but is not currently exposed or used** — the block header uses the
  canonical SHA-256 typed leaf digest described in §Current root.
- Bridge deposits and withdrawals now persist as redb sidecar state
  (`BridgeState`) plus pending WALs. Blocks expose the sidecar transition data
  for proof-generation, and the header root now commits
  `sys/deposit_cursor`, `sys/deposit_root`, `sys/next_withdrawal_id`, and
  active `withdrawal/{withdrawal_id}` leaves.

So qmdb is shipping today as a storage layer. It's not yet the source of
truth for the block header's `state_root` because the public root does not use
qmdb's native MMR root or proof format yet.

## Target: native typed global qmdb root

The cheapest complete path to authenticated state is to reuse the ordered
qmdb MMR that already exists in the sequencer, but widen the keyspace from
account snapshots to typed validium state. The production state root is one
root, not one root per subsystem.

### Design

1. After settlement, write every touched state leaf to qmdb under a typed key.
2. Delete leaves that are no longer active, such as fully filled or expired
   resting orders.
3. Read the MMR root back from qmdb after `merkleize` / `apply_batch`.
4. Publish `state_root_v3 = SHA256("sybil/state-root/v3" || qmdb_root)` in
   the block header, or choose an explicit migration height and domain if the
   naming changes before mainnet.

Result: accounts, reservations, active orders, market lifecycle, and global
counters share one authenticated commitment. Inclusion and exclusion proofs
come from qmdb's ordered proof APIs. Historical proofs come from qmdb's
append-only structure within the retained journal window.

### Hasher: SHA-256

qmdb is generic over the hasher. The current alias:

```rust
type AccountDb = OrderedVariableDb<
    MmrFamily, commonware_tokio::Context, Vec<u8>, Vec<u8>,
    commonware_cryptography::Sha256, OneCap, CHUNK_SIZE,
>;
```

**Decision:** keep SHA-256 for typed state roots. This matches the qmdb
instantiation already wired into the sequencer, keeps the authenticated
database on a conservative hash, and is easier to route through ZK/EVM
verification paths than BLAKE3. Historical v1 roots, block parent hashes, and
account `events_digest` remain BLAKE3; verifiers dispatch by root version or
migration height.

### Encoding alignment

Today's qmdb values are `rmp_serde`-serialized `Account` structs. For
persistence that's fine, but it is not acceptable as a public commitment
format because serde field order is not a protocol.

For v2, qmdb keys and values committed by `state_root_v2` MUST be canonical
Sybil bytes under explicit type domains. Runtime storage may keep ergonomic
MessagePack copies elsewhere, but the authenticated value is the canonical
commitment value.

The state leaf domain rules are defined in [[Canonical Serialization]].
Every leaf starts with a type/version domain, for example
`"sybil/state/acct/v1"` or `"sybil/state/order/v1"`, followed by fixed-width
canonical fields and deterministically sorted collections. Exact byte-level
field layouts for non-account v2 leaves are still implementation follow-ups;
the key families and commitment shape are fixed here.

### Incremental update

qmdb already does the right thing: `new_batch` / `write` / `merkleize` /
`apply_batch`. The sequencer already calls this sequence for account
snapshots at block boundaries. The v2 change is:

- Replace the account-only snapshot wrapper with a typed state writer.
- Write every touched account, reservation, active/resting order,
  market-state, and system leaf in the same block-boundary batch.
- After `apply_batch`, ask qmdb for the resulting MMR root.
- Write the domain-wrapped root into `BlockHeader.state_root`.

Cost per block: bounded by the number of touched accounts × hash-per-level
— same ballpark as any Merkle KV store. No change to Big-O.

### Proof API

qmdb exposes current-value and exclusion proofs natively. Wrap them in a
Sybil-facing endpoint for off-chain verifiers and ZK provers:

```
GET /v1/proofs/state/{key}?height={N}
  → {
      "key": "acct/42",
      "block_height": N,
      "state_root": "0x...",
      "leaf": "0x...",                         // canonical state leaf bytes or digest
      "leaf_type": "acct/v1",
      "mmr_proof": { ... }                      // qmdb's native proof bytes
    }
```

Verifier runs qmdb's proof verifier with SHA-256 and the supplied key/value
or exclusion proof. For bridge withdrawals, the L1 contract should verify a
ZK proof over the relevant qmdb membership/exclusion checks rather than
reimplement qmdb proof verification directly in Solidity.

Normal bridge withdrawals should prove a committed `withdrawal/{withdrawal_id}`
leaf. Emergency cash exits should expose withdrawable cash, not just raw
balance. That is why reservations or equivalent open-exposure data are
committed state. See [[L1 Settlement and Vault]] for the contract boundary.

## Alternatives considered

### Account-only qmdb root

Rejected as the production target. It is simple, but incomplete: it cannot
prove active resting orders, reservations, market lifecycle state, or that an
expired order is absent after a block. It remains the Phase 1 historical
scheme only.

### Build a fresh Sparse Merkle Tree

Cost: new dependency surface, duplicate storage, reimplement proof API,
reimplement incremental updates. Benefit: decouples state root from
qmdb's evolution and may produce simpler Solidity proofs. Rejected for the
main v2 commitment because withdrawals are expected to use ZK proofs and qmdb
already gives the sequencer an authenticated ordered key-value store.

### Verkle tree

Smaller proofs (vector commitments via KZG) but pulls in elliptic-curve
crypto and a KZG ceremony. Premature when on-chain proof size is not the
bottleneck and the ZK circuit is recursive anyway.

### Keep qmdb for persistence only; build a separate commitment tree

Two trees, two roots, two sets of invariants. Only justifiable if we need
direct Solidity membership proofs that cannot economically verify qmdb
semantics inside a ZK proof. Keep as a fallback if SYB-27/SYB-30 prove the
ZK path too expensive or operationally fragile.

## Migration path

Pre-mainnet blocks now use `state_root_v2`, the typed leaf digest described
above. If we need to verify older local/dev blocks, the verifier retains
`compute_account_state_root_v1`.

The future native-qmdb-root switch must be a versioned migration because the
current `state_root_v2` domain already means "SHA-256 over sorted typed
key/value leaves." A native qmdb root should therefore use a new root domain
such as `sybil/state-root/v3`, unless we decide before mainnet to rename the
current pre-production scheme.

Verifiers dispatch on `header.height` and root version: read block header,
pick the algorithm, re-verify. Historical implementations remain in
`sybil-verifier` during the migration window.

Domain separation and verifier dispatch ensure no collision between root
versions even when they cover overlapping state data.

## Retention and historical proofs

Current v2 subset: current state only; historical proofs not supported
without replay.

Native qmdb root: qmdb is append-only by design, so historical state proofs
come from the retained journal window. Retention is configured by
`items_per_blob` / journal-partition pruning; the current sequencer keeps the
full log. A future "prune to last N blocks" policy is a storage and DA
recovery question, not a commitment-scheme question.

## Relation to events and witness roots

| Root | Commits to | Primary consumer |
|---|---|---|
| `state_root` (this doc) | post-settlement complete validium state | ZK settlement, bridge claims, recovery checks |
| `events_root` ([[Proof Architecture]]) | this block's event stream | external verifiers of "did F happen" |
| `witness_root` ([[Block Witness]]) | the full audit package | provers, replayers |

All three are expected to live in the extended block header
(`BlockHeader v2`, defined in [[Block Witness]]).

## Recovery boundary

The state root is a commitment, not a data availability mechanism. If the
operator disappears and users cannot obtain the state data, neither a qmdb
root nor a ZK proof system can reconstruct it.

The preferred recovery shape is DA-backed operator replacement: fetch
published state snapshots/deltas, verify the reconstructed typed state
against the latest accepted `state_root`, and start a replacement sequencer.
Individual cash-only force exits are a conservative fallback because
unresolved prediction-market positions cannot be cleanly unwound on L1
without moving market resolution and settlement logic onto L1.

Out of scope here:

- DA provider and publication cadence: SYB-76.
- Escape reconstruction tooling: SYB-80.
- Operator replacement and encrypted emergency disclosure: SYB-116.
- L1 vault/settlement contracts: [[L1 Settlement and Vault]] and SYB-31/SYB-32.

## Open questions

1. **Typed leaf encodings.** The key families are fixed here, but byte-level
   encodings for account reservation, resting order, withdrawal, market
   lifecycle, and system leaves must be pinned in [[Canonical Serialization]]
   before v2 is implemented.
2. **Exposing qmdb's MMR root and proofs.** The `merkleize` API builds the
   root but the current `QmdbAccounts` wrapper does not surface it. The v2
   implementation should replace that wrapper with a typed state wrapper.
3. **Events tree co-habitation.** The events tree from
   [[Proof Architecture]] is orthogonal. It may remain a separate per-block
   tree even though long-lived state lives in qmdb.

## Test vectors

**Phase 1 — empty state:** `state_root_v1 = BLAKE3("") = af1349b9...e41f3262`.

**Phase 1 — one account (same as [[Canonical Serialization]] Vector 2):**

```
account_bytes = (72 bytes, see Canonical Serialization Vector 2)
state_root_v1 = BLAKE3(account_bytes)
```

**Current v2 — typed leaf digest.** The spec-level assertion is:

```
for each typed state leaf L:
    value_L = canonical_state_value_bytes(L)

state_root_v2 = SHA256(
    "sybil/state-root/v2"
 || leaf_count:u64
 || sorted(key_len:u32 || key || value_len:u32 || value)
)
```

Native qmdb-root vectors land when that versioned migration is implemented.

## Where this lives

> `crates/sybil-verifier/src/block.rs` — `compute_state_root_with_bridge` and `state_root_v2_leaves` (current v2)
> `crates/sybil-verifier/src/block.rs` — `compute_account_state_root_v1` (historical v1)
> `crates/matching-sequencer/src/block.rs` — writes `state_root` into the block header
> `crates/matching-sequencer/src/canonical_state.rs` — canonical account ordering used by v2 account leaves
> `crates/matching-sequencer/src/qmdb_accounts.rs` — fenced qmdb wrapper that persists legacy account copies plus typed v2 leaves
> `crates/matching-sequencer/src/account_storage.rs` — current account snapshot and typed-leaf persistence boundary

## See also

- [[State Root and Parent Hash]] — the concept intro this doc normalizes
- [[Canonical Serialization]] — byte-level rules for account bytes and future state leaf bytes
- [[Block Witness]] — the witness the state root lives inside
- [[Proof Architecture]] — events tree (complementary) and proof-composition patterns
- [[ZK Integration Path]] — how the state root anchors the on-chain proof chain
- [[L1 Settlement and Vault]] — bridge contract assumptions and withdrawal proof shape
- [[Persistence]] — storage tiers and the qmdb wrapper
