---
tags: [zk, serialization, spec]
layer: verification
status: current
last_verified: 2026-04-29
---

# Canonical Serialization

Single normative reference for how every Sybil value becomes bytes. Every
other serialization-related spec ([[State Root and Parent Hash]], [[Block Witness]],
[[Proof Architecture]]) refers to this one.

This document describes **Canonical Bytes v1**, which matches the encoding
already implemented in `matching-sequencer` and `sybil-verifier`. The goal is
to turn silent convention into a contract.

## Why one spec

The [[State Root and Parent Hash|state root]], the block-chain parent hash, the
per-account [[Block Witness|events digest]], and the future
[[ZK Integration Path|ZK circuit inputs]] all depend on byte-identical
encoding — across Rust crates, across languages, and across implementations.
Without a normative spec, drift is silent and catastrophic: two honest
implementations can compute different state roots from the same logical state.

Today the rules are scattered across five files:

- `crates/matching-sequencer/src/block.rs::hash_header`
- `crates/matching-sequencer/src/canonical_state.rs::CanonicalState`
- `crates/matching-sequencer/src/digest.rs` (5 event encoders)
- `crates/sybil-verifier/src/block.rs::compute_state_root`
- `crates/sybil-verifier/src/types.rs::AccountSnapshot`

This RFC collects them in one place and pins the rules.

## Principles

1. **All-integer arithmetic.** No floats ever appear in canonical bytes. See
   [[Nanos and Integer Arithmetic]].
2. **Little-endian, fixed-width integers.** Matches Rust's `.to_le_bytes()`.
   Chosen for zero-cost encoding in the sequencer and straightforward
   reproduction in EVM precompiles.
3. **No framing, no length prefixes on top-level hashes.** Producers concatenate
   field bytes and feed them to BLAKE3. Consumers of a hash don't need to
   reparse. Framing is a caller responsibility (e.g., the witness adds
   `count:u64` prefixes to variable-length sections; see §§6 and 7 of
   [[Block Witness]]).
4. **Deterministic ordering on every collection.** Sort rule specified per
   type. Implementations MUST sort before hashing; consumers MUST NOT trust
   input order.
5. **Type-tag byte for sum types.** Events are tagged (§6). Product types
   (structs with fixed field set) are untagged.
6. **Versioning is explicit.** A breaking change bumps the version via a
   domain-separation prefix on the top-level hash (§8). Silent breakage is a
   bug.
7. **No third-party codec.** We considered borsh, SSZ, and canonical CBOR
   (§ below). For a system where byte layout is load-bearing for ZK
   correctness, hand-rolled + documented beats any framework.

## Alternatives considered

- **borsh.** Ergonomic (`derive(BorshSerialize)`) but too intimate with Rust
  struct definitions: a field reorder changes bytes silently. We want sort
  rules explicit at the spec level, not implicit in Rust declaration order.
- **SSZ.** Good ZK fit, but introduces Merkleization rules orthogonal to our
  [[State Root Schema|state root Merkle tree]]. Taking SSZ means either
  adopting its tree shape or fighting it. Net overhead > benefit.
- **Canonical CBOR.** Framing + type tags built in, but the spec surface is
  large and we'd leverage ~none of the ecosystem.

**Decision:** keep the current hand-rolled encoding, write this RFC, add test
vectors. Revisit if a second-language implementation (TS frontend verifier,
Solidity precompile) proves the manual rules unergonomic.

## Primitive encodings

| Type | Bytes | Notes |
|---|---|---|
| `u8` | 1 | — |
| `u32` | 4 | little-endian |
| `u64` | 8 | little-endian |
| `i64` | 8 | little-endian, two's complement |
| `[u8; 32]` | 32 | verbatim |
| `Nanos` | 8 | alias for `u64` |
| `Qty` | 8 | alias for `u64` |
| `MarketId(u32)` | 4 | encoded as its inner `u32` |

No Boolean in v1 — if needed, encode as `u8` with 0x00 / 0x01.

## Composite encodings

### `AccountSnapshot`

Source: `sybil-verifier::block::compute_state_root`.

```
account_bytes =
      id:u64
   || balance:i64
   || total_deposited:i64
   || positions_bytes
   || events_digest:[u8; 32]
```

`positions_bytes` is the concatenation of position triples, each:

```
position = market_id:u32 || outcome:u8 || qty:i64
```

Rules:

- **Sort order:** ascending by `(market_id, outcome)`.
- **Zero filter:** triples with `qty == 0` MUST be omitted.
- **Length:** implicit — reader knows the account boundary from its caller.

When a list of accounts is hashed (state root), accounts are sorted ascending
by `id` and concatenated.

### `BlockHeader v1`

Source: `matching-sequencer::block::hash_header`.

```
header_bytes =
      height:u64
   || parent_hash:[u8; 32]
   || state_root:[u8; 32]
   || order_count:u32
   || fill_count:u32
   || timestamp_ms:u64
```

Total: 8 + 32 + 32 + 4 + 4 + 8 = **88 bytes**, fixed.

[[Block Witness]] proposes a `BlockHeader v2` that adds `events_root` and
`witness_root`. The v2 encoding is a strict prefix of v1 for forward-compat
(see §8 of this doc).

### `Fill` (deferred)

**TODO v1.1.** `Fill` is used inside the [[Block Witness]] but is currently
serialized only via serde-for-debugging. The events tree (Proof Architecture
Phase 1) will need a canonical encoding. Proposed:

```
fill_bytes =
      order_id:u64
   || fill_qty:u64
   || fill_price:u64
   || account_id:u64
   || market_count:u32
   || market_id:u32 * market_count
```

Lists of fills sorted by `order_id` ascending; ties broken by original solver
output index.

### `Order`, `MmConstraint`, `MarketGroup` (deferred)

**TODO v1.1.** These only matter for the [[Block Witness]] canonical bytes and
the ZK circuit. Not needed for state root. Encodings will be added in a
follow-up RFC alongside the events tree. The Rust signing path already uses
`sybil-canonical::Order`, including `expires_at_block`, so P256 signed orders
cover resolved IOC/GTD expiry semantics even before the full witness byte spec
is frozen.

### State leaves for `state_root_v2`

[[State Root Schema]] fixes the v2 commitment shape. The current
implementation commits a typed subset: accounts, resting orders, aggregate
reservations, market definitions/lifecycle, market groups, bridge counters,
deposit root, and active withdrawal leaves. Each committed value begins with
an ASCII domain string identifying the leaf type and version, followed by
canonical fixed-width fields and deterministically sorted collections.

Reserved domains:

| Key family | Value domain |
|---|---|
| `acct/{account_id}` | `sybil/state/acct/v1` |
| `acct_resv/{account_id}` | `sybil/state/acct-resv/v1` |
| `order/{order_id}` | `sybil/state/order/v1` |
| `withdrawal/{withdrawal_id}` | `sybil/state/withdrawal/v1` |
| `market/{market_id}` | `sybil/state/market/v1` |
| `market_group/{group_id}` | `sybil/state/market-group/v1` |
| `sys/*` | `sybil/state/sys/v1` |

Implemented key encodings:

| Logical key | Bytes |
|---|---|
| `acct/{account_id}` | `"acct/" || account_id:u64_be` |
| `acct_resv/{account_id}` | `"acct_resv/" || account_id:u64_be` |
| `market/{market_id}` | `"market/" || market_id:u32_be` |
| `market_group/{group_id}` | `"market_group/" || group_id:u64_be` |
| `order/{order_id}` | `"order/" || order_id:u64_be` |
| `sys/deposit_cursor` | ASCII literal |
| `sys/deposit_root` | ASCII literal |
| `sys/next_withdrawal_id` | ASCII literal |
| `withdrawal/{withdrawal_id}` | `"withdrawal/" || withdrawal_id:u64_be` |

`acct` value:

```text
account_leaf_value =
    "sybil/state/acct/v1"
 || id:u64_le
 || balance:i64_le
 || total_deposited:i64_le
 || position_count:u64_le
 || (market_id:u32_le || outcome:u8 || qty:i64_le) * position_count
 || events_digest:[u8;32]
```

Positions with `qty == 0` MUST be omitted. Remaining positions are sorted by
`(market_id, outcome)`.

`sys` value:

```text
sys_leaf_value =
    "sybil/state/sys/v1"
 || name_len:u8
 || name:ascii_bytes
 || raw_value
```

`deposit_cursor` and `next_withdrawal_id` use `raw_value:u64_le`.
`deposit_root` uses `raw_value:[u8;32]`.

`withdrawal` value:

```text
withdrawal_leaf_value =
    "sybil/state/withdrawal/v1"
 || withdrawal_id:u64_le
 || account_id:u64_le
 || recipient:address
 || token:address
 || amount_token_units:u64_le
 || amount_nanos:u64_le
 || expiry_height:u64_le
 || nullifier:[u8;32]
```

`acct_resv` value:

```text
account_reservation_leaf_value =
    "sybil/state/acct-resv/v1"
 || account_id:u64_le
 || reserved_balance:i64_le
 || reserved_position_count:u64_le
 || (market_id:u32_le || outcome:u8 || qty:i64_le) * reserved_position_count
```

Reserved positions with `qty == 0` MUST be omitted. Remaining reserved
positions are sorted by `(market_id, outcome)`.

`order` value:

```text
resting_order_leaf_value =
    "sybil/state/order/v1"
 || account_id:u64_le
 || created_at:u64_le
 || expires_at_block:u64_le
 || reserved_balance:i64_le
 || reserved_position_count:u64_le
 || (market_id:u32_le || outcome:u8 || qty:i64_le) * reserved_position_count
 || order_bytes
```

`order_bytes` is the admitted remaining order:

```text
order_bytes =
    order_id:u64_le
 || num_markets:u8
 || market_id:u32_le * num_markets
 || num_states:u8
 || payoff:i8 * num_states
 || limit_price:u64_le
 || max_fill:u64_le
 || condition_tag:u8
 || condition_bytes?
 || expires_at_block_tag:u8
 || raw_expires_at_block:u64_le?
```

`condition_tag = 0` for no condition. `condition_tag = 1` is:

```text
condition_bytes =
    market_id:u32_le
 || threshold:u64_le
 || direction:u8   // 0 = Above, 1 = Below
```

`expires_at_block_tag = 0` for `None`; `1` for `Some(raw_expires_at_block)`.
The resting-order wrapper always also commits the effective
`expires_at_block` used by the sequencer.

`market` value:

```text
market_leaf_value =
    "sybil/state/market/v1"
 || market_id:u32_le
 || name_len:u64_le
 || name:utf8_bytes
 || num_outcomes:u8
 || market_status_bytes
 || metadata_digest:[u8;32]
 || resolution_template_len:u64_le
 || resolution_template:utf8_bytes
```

`metadata_digest = SHA256("sybil/state/market-meta/v1" || payload_len:u64_le ||
payload)`. The sequencer payload is tagged: `0x00` for no metadata, or `0x01`
followed by description, category, sorted tags, resolution criteria,
expiry timestamp, creation timestamp, and effective resolution template. Text
fields use `len:u64_le || utf8_bytes`.

`market_status_bytes` is tag-dispatched:

```text
0x00 = Active
0x01 = Proposed   || resolution_proposal || challenge_deadline_ms:u64_le
0x02 = Challenged || resolution_proposal || challenge
0x03 = Resolved   || resolution_record
0x04 = Voided
```

Resolution helper encodings:

```text
oracle_source =
    0x00                     // Admin
  | 0x01 || feed_id:u64_le   // DataFeed
  | 0x02                     // AutomatedL0

resolution_proposal =
    proposal_id:u64_le
 || market_id:u32_le
 || payout_nanos:u64_le
 || oracle_source
 || proposed_at_ms:u64_le
 || reason_tag:u8
 || reason_len:u64_le || reason:utf8_bytes   // only when reason_tag = 1

challenge =
    challenge_id:u64_le
 || challenger:u64_le
 || proposal_id:u64_le
 || bond_amount:u64_le
 || proposed_payout_nanos:u64_le
 || reason_len:u64_le
 || reason:utf8_bytes
 || challenged_at_ms:u64_le

resolution_record =
    market_id:u32_le
 || payout_nanos:u64_le
 || oracle_source
 || resolved_at_ms:u64_le
 || proposal_tag:u8 || resolution_proposal?
 || challenge_tag:u8 || challenge?
```

`market_group` value:

```text
market_group_leaf_value =
    "sybil/state/market-group/v1"
 || group_id:u64_le
 || name_len:u64_le
 || name:utf8_bytes
 || market_count:u64_le
 || market_id:u32_le * market_count
```

Market ids inside a group are sorted ascending before encoding.

## Event encoding registry

Events are tag-dispatched single-byte sum types. The running
`events_digest` accumulates these into each account via BLAKE3, updated by
`matching-sequencer::digest::update_digest`.

| Tag | Event | Body | Source |
|---|---|---|---|
| `0x01` | Fill | `order_id:u64 \|\| fill_qty:u64 \|\| fill_price:u64 \|\| block_height:u64` | `encode_fill_event` |
| `0x02` | Deposit | `amount:i64 \|\| block_height:u64` | `encode_deposit_event` |
| `0x03` | Resolution | `market_id:u32 \|\| payout_nanos:u64 \|\| block_height:u64` | `encode_resolution_event` |
| `0x04` | CreateAccount | `initial_balance:i64 \|\| block_height:u64` | `encode_create_account_event` |
| `0x05` | Mint | `count:u64 \|\| (market_id:u32 \|\| outcome:u8 \|\| position_delta:i64 \|\| balance_delta:i64) * count \|\| block_height:u64` | `encode_mint_event` |
| `0x06` | L1Deposit | `deposit_id:u64 \|\| amount:i64 \|\| deposit_root:[u8;32] \|\| block_height:u64` | `encode_l1_deposit_event` |
| `0x07` | WithdrawalCreated | `withdrawal_id:u64 \|\| amount:i64 \|\| nullifier:[u8;32] \|\| block_height:u64` | `encode_withdrawal_created_event` |
| `0x08` – `0xFE` | reserved for future events | — | — |
| `0xFF` | reserved as sentinel (do not use) | — | — |

Adding an event type consumes the next free tag. Removing or re-tagging is a
major version bump.

## The running digest

```
events_digest_new = BLAKE3(events_digest_old || event_bytes)
```

This is *not* a Merkle root; it is a non-commutative running hash. Equal
digests at two trusted state roots imply no account-level event activity in
between — the Proof Architecture inactivity-proof primitive.

## Versioning

Canonical Bytes v1 has no version byte in individual encodings. Breaking
changes are handled at the **hash-domain level**:

- v1 state root: `BLAKE3(concat(sorted_account_bytes))`
- v2 state root: `SHA256("sybil/state-root/v2" || qmdb_root(typed_state_leaves))`

This means existing hashes remain valid; only new hashes land under new
domain separation strings. Verifiers pick the algorithm by block height (see
[[State Root and Parent Hash]] migration section).

Adding a field to a struct is always a version bump for any root that covers
it. There is no silent "skip unknown field" rule — consumers that don't know
about the field MUST reject.

## Test vectors

Minimal worked examples. All outputs are BLAKE3.

### Vector 1: empty state root

No accounts → empty input → `BLAKE3(empty)`:

```
state_root_v1 = af1349b9 f5f9a1a6 a0404dea 36dcc949 9bcb25c9 adc112b7 cc9a93ca e41f3262
```

(The BLAKE3 of the empty string.)

### Vector 2: one account, no positions

```
account:
  id               = 1
  balance          = 100
  total_deposited  = 100
  positions        = []
  events_digest    = [0; 32]

account_bytes (hex):
  01 00 00 00 00 00 00 00    # id: u64 LE
  64 00 00 00 00 00 00 00    # balance: i64 LE
  64 00 00 00 00 00 00 00    # total_deposited: i64 LE
  00 00 00 00 00 00 00 00    # events_digest[0..8]
  00 00 00 00 00 00 00 00    # events_digest[8..16]
  00 00 00 00 00 00 00 00    # events_digest[16..24]
  00 00 00 00 00 00 00 00    # events_digest[24..32]

state_root_v1 = BLAKE3(account_bytes)   # implementations MUST match
```

### Vector 3: fill event digest

```
event:
  tag              = 0x01 (Fill)
  order_id         = 7
  fill_qty         = 10
  fill_price       = 500_000_000
  block_height     = 12

event_bytes (hex):
  01                          # tag
  07 00 00 00 00 00 00 00     # order_id: u64 LE
  0a 00 00 00 00 00 00 00     # fill_qty: u64 LE
  00 65 cd 1d 00 00 00 00     # fill_price: u64 LE (500_000_000)
  0c 00 00 00 00 00 00 00     # block_height: u64 LE

events_digest_new = BLAKE3([0; 32] || event_bytes)
```

### Vector 4: block header

```
header:
  height           = 1
  parent_hash      = [0; 32]
  state_root       = [1; 32]
  order_count      = 5
  fill_count       = 3
  timestamp_ms     = 1000

header_bytes length = 88 bytes, as defined above.
header_hash = BLAKE3(header_bytes)
```

A follow-up issue will land a `canonical_bytes_vectors.rs` test in
`crates/sybil-verifier/tests/` that asserts these exact bytes and hashes
against the implementation. This RFC is pure spec.

## Adding a field without breaking hashes

Two patterns:

1. **New top-level root.** Don't extend the existing structure — introduce a
   new root that covers the new field. `events_root` is the canonical
   example: it's a sibling of `state_root`, not an extension of it.
2. **Version bump.** If a new field fundamentally belongs inside an
   existing structure (e.g., adding a `last_seen_block` to the account
   snapshot), bump the version and domain-separate the new root. Dual-support
   during migration.

Bad pattern: adding a field to `AccountSnapshot` without version-bumping. Old
hashers produce v1 bytes, new hashers produce v2 bytes, and everything breaks
silently. Don't do this.

## Implementation note

Current code uses `.to_le_bytes()` directly without a shared helper. A future
refactor could introduce a `CanonicalBytes` trait per type, matching this
spec. Not required for correctness today; it's a readability / drift-safety
improvement.

## See also

- [[State Root Schema]] — consumes this spec to define Phase 1 and Phase 2 state roots
- [[State Root and Parent Hash]] — concept introduction
- [[Block Witness]] — the witness uses this spec for canonical witness bytes
- [[Proof Architecture]] — authenticated data layer consuming these encodings
- [[Nanos and Integer Arithmetic]] — motivates the integer-only rule
- [[ZK Integration Path]] — ZK circuits operate on canonical bytes
