---
tags: [zk, serialization, spec]
layer: verification
status: current
last_verified: 2026-04-17
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
  [[State Root and Parent Hash|state root Merkle tree]]. Taking SSZ means either
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
follow-up RFC alongside the events tree.

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
| `0x06` – `0xFE` | reserved for future events | — | — |
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
- v2 state root: `BLAKE3("sybil/state-root/v2" || concat(sorted_account_bytes))`

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

- [[State Root and Parent Hash]] — what the state root commits to and how the Merkle
  tree (Phase 2) hashes canonical account bytes
- [[Block Witness]] — the witness uses this spec for canonical witness bytes
- [[Proof Architecture]] — authenticated data layer consuming these encodings
- [[Nanos and Integer Arithmetic]] — motivates the integer-only rule
- [[ZK Integration Path]] — ZK circuits operate on canonical bytes
