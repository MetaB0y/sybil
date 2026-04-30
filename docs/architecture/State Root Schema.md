---
tags: [zk, spec]
layer: verification
crate: sybil-verifier
status: current
last_verified: 2026-04-30
---

# State Root Schema

Normative spec for `BlockHeader.state_root`: what it commits to, how typed
state bytes are produced, and how proofs are expected to work.

[[State Root and Parent Hash]] is the concept introduction. This note is the
byte-level commitment contract.

## Commitment

`state_root` is the native current-qmdb root over sorted typed state leaves.
The root uses commonware's ordered current qMDB with SHA-256 and variable-size
`Vec<u8>` keys and values.

Implementation:

- `crates/sybil-verifier/src/block.rs::compute_state_root_with_sidecar`
- `crates/sybil-verifier/src/block.rs::state_root_leaves`
- `crates/sybil-verifier/src/block.rs::state_root_from_leaves`
- `crates/matching-sequencer/src/block.rs::compute_complete_state_root`

For verifier-side recomputation, the typed leaves are inserted into a fresh
empty qMDB in bytewise key order and the resulting qMDB root is the
`state_root`. Runtime persistence stores the same leaves in a dedicated
typed-state qMDB whose active keyspace exactly matches the header root.

## Key Families

| Key family | Commits to |
|---|---|
| `acct/{account_id}` | `id`, `balance`, `total_deposited`, non-zero `positions`, `events_digest` |
| `acct_resv/{account_id}` | aggregate reserved cash and positions from active resting orders |
| `market/{market_id}` | binary market definition, lifecycle status/resolution, metadata digest, resolution template |
| `market_group/{group_id}` | mutually exclusive market group name and member markets |
| `order/{order_id}` | active resting order, owner, effective expiry, remaining quantity, and reservation metadata |
| `sys/deposit_cursor` | highest consumed L1 deposit cursor |
| `sys/deposit_root` | deposit log root used by the bridge sidecar |
| `sys/next_withdrawal_id` | next withdrawal id counter |
| `withdrawal/{withdrawal_id}` | normal L1 withdrawal claim, recipient, token, amount, expiry, and nullifier |

The committed surface covers the exchange's active trading state: accounts,
bridge withdrawals, markets, groups, resting orders, and reservations. This is
what lets a verifier check order presence/absence, market tradeability, and
withdrawal claims from the header commitment rather than trusting witness-only
context.

## Leaf Encoding

Keys are byte strings. Collections are canonicalized before leaf construction:

- accounts by `account_id`
- withdrawals by `withdrawal_id`
- markets by `market_id`
- market groups by `group_id`
- resting orders by `order_id`
- reservations by `account_id`

Values are canonical Sybil bytes under explicit type domains defined in
[[Canonical Serialization]]. Runtime storage may keep ergonomic MessagePack
copies elsewhere, but authenticated state values must be canonical leaf bytes.

The qMDB root commits to the key/value pairs themselves. There is no separate
sorted-leaf digest layer.

## Sequencer Storage

Current storage has two qMDB roles:

- The block header root is computed from the typed leaves through native qMDB.
- `crates/matching-sequencer/src/qmdb_accounts.rs` persists account snapshots
  under a fenced account-qMDB slot for crash recovery.
- `crates/matching-sequencer/src/qmdb_state.rs` persists the typed state
  leaves in fenced A/B qMDBs whose unprefixed keyspace is exactly the
  `state_root` keyspace.

The account qMDB slot currently stores:

- legacy account rows: `slot_prefix || 'a' || account_id_be_u64`
- metadata rows: slot height and `next_account_id`

`QmdbState` exposes the committed typed-state root plus typed-leaf inclusion
and exclusion proofs. Those proofs verify directly against
`BlockHeader.state_root` for the fenced slot recorded by redb.

## Proof API

qMDB exposes current-value and exclusion proofs natively. A Sybil-facing API
should wrap them for off-chain verifiers and ZK provers:

```
GET /v1/proofs/state/{key}?height={N}
  -> {
      "key": "acct/42",
      "block_height": N,
      "state_root": "0x...",
      "leaf": "0x...",
      "leaf_type": "acct",
      "qmdb_proof": { ... }
    }
```

Verifier logic checks the qMDB proof with SHA-256, the supplied key/value (or
exclusion key), and the header `state_root`. For bridge withdrawals, the L1
contract should verify a ZK proof over the relevant qMDB membership/exclusion
checks rather than reimplementing qMDB proof verification directly in
Solidity.

Normal bridge withdrawals should prove a committed
`withdrawal/{withdrawal_id}` leaf. Emergency cash exits should expose
withdrawable cash, not just raw balance, which is why reservations or
equivalent open-exposure data are committed state. See
[[L1 Settlement and Vault]] for the contract boundary.

## Retention and Historical Proofs

qMDB is append-only, so historical state proofs come from the retained journal
window. Retention is configured by `items_per_blob` and pruning policy. A
future "prune to last N blocks" policy is a storage and data-availability
question, not a commitment-scheme question.

## Recovery Boundary

The state root is a commitment, not a data availability mechanism. If the
operator disappears and users cannot obtain the state data, neither a qMDB
root nor a ZK proof system can reconstruct it.

The preferred recovery shape is DA-backed operator replacement: fetch
published state snapshots/deltas, verify the reconstructed typed state against
the latest accepted `state_root`, and start a replacement sequencer.
Individual cash-only force exits are a conservative fallback because
unresolved prediction-market positions cannot be cleanly unwound on L1
without moving market resolution and settlement logic onto L1.

Out of scope here:

- DA provider and publication cadence: SYB-76.
- Escape reconstruction tooling: SYB-80.
- Operator replacement and encrypted emergency disclosure: SYB-116.
- L1 vault/settlement contracts: [[L1 Settlement and Vault]] and SYB-31/SYB-32.

## Alternatives Considered

**Account-only qMDB root.** Rejected as incomplete. It cannot prove active
resting orders, reservations, market lifecycle state, market-group membership,
or that an expired order is absent after a block.

**Sparse Merkle tree.** Rejected for now because it duplicates storage and
proof APIs while qMDB already gives the sequencer an authenticated ordered
key-value store.

**Separate commitment tree alongside qMDB.** Rejected unless direct Solidity
membership proofs become more important than keeping one authenticated state
store.

## Where This Lives

> `crates/sybil-verifier/src/block.rs` - typed leaf construction and native qMDB root recomputation
> `crates/matching-sequencer/src/block.rs` - writes `state_root` into the block header
> `crates/matching-sequencer/src/canonical_state.rs` - canonical account ordering used by account leaves
> `crates/matching-sequencer/src/qmdb_accounts.rs` - fenced account recovery snapshots
> `crates/matching-sequencer/src/qmdb_state.rs` - fenced typed-state qMDB and proofs
> `crates/matching-sequencer/src/account_storage.rs` - account snapshot and typed-leaf persistence boundary

## See Also

- [[State Root and Parent Hash]]
- [[Canonical Serialization]]
- [[Block Witness]]
- [[Proof Architecture]]
- [[ZK Integration Path]]
- [[L1 Settlement and Vault]]
- [[Persistence]]
