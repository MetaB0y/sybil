---
tags: [zk, spec]
layer: verification
crate: sybil-verifier
status: current
last_verified: 2026-04-29
---

# Block Witness

The Block Witness is the self-contained input to block verification. The
sequencer produces one per block. The [[Four-Layer Verification]] logic
consumes it today; the [[ZK Integration Path|ZK circuit]] will consume it
tomorrow. Everything else in this doc follows from two invariants:

1. **Self-contained.** Given a witness and its parent header (or nothing, for
   genesis), any third party can re-run settlement and verify every claim the
   block makes — without access to sequencer state, mempool, or history
   beyond the parent.
2. **Reproducible.** `apply_fills(pre_state, system_events, fills) == post_state`,
   and `compute_state_root_with_sidecar(post_state, state_sidecar) ==
   header.state_root`. If either equation fails, the witness is invalid.

## Rust type

`crates/sybil-verifier/src/types.rs::BlockWitness` holds 16 fields:

| Field | Purpose |
|---|---|
| `header` | block header being verified |
| `previous_header` | parent header (or `None` for genesis) |
| `orders` | orders accepted into this batch, with account mapping |
| `rejections` | orders rejected, with reasons |
| `system_events` | state changes applied between blocks (create account, dev deposit, L1 deposit, withdrawal creation, resolution) |
| `fills` | solver output |
| `clearing_prices` | per-market clearing prices produced by the solver |
| `total_welfare` | sum of `(limit_price - clearing_price) * fill_qty` |
| `minting_cost` | minting cost not captured in fill welfare (MILP only) |
| `mm_constraints` | MM budget constraints active this batch |
| `market_groups` | market group definitions (for complete-set logic) |
| `pre_state` | account snapshots at block start |
| `post_system_state` | after system events, before fills |
| `post_state` | after fills — what the header's `state_root` commits to |
| `state_sidecar` | non-account state committed by the header's `state_root`: bridge, resting orders, and reservations |
| `resolved_markets` | markets resolved/voided; orders/fills must not reference |

The sequencer builds this in `matching-sequencer::sequencer` at the end of
each block. Tests and `matching-sim` run the 4-layer verifier over it.

## ZK public/private partition

In a SNARK, witness fields split into **public inputs** (available to every
verifier, checked against on-chain commitments) and **private inputs** (only
seen inside the circuit, never exposed).

| Field | Public or private |
|---|---|
| `header` | public |
| `previous_header` | public (just its hash, really) |
| `clearing_prices` | public |
| `resolved_markets` | public |
| `total_welfare`, `minting_cost` | public |
| `header.order_count`, `header.fill_count` | public |
| `orders` (individual, including expiry) | private |
| `rejections` | private |
| `system_events` (individual) | private (shape is public via events_root; bridge public inputs carry deposit/withdrawal commitments separately) |
| `fills` (individual) | private (shape is public via events_root) |
| `mm_constraints`, `market_groups` | private |
| `pre_state`, `post_system_state`, `post_state` | private |
| `state_sidecar` | private (deposit root/count, withdrawal commitments, and selected order/reservation claims can be exposed through dedicated proof public inputs where needed) |

Rationale: the public side is "what was the market's observable outcome" —
clearing prices, how many orders, welfare. The private side is "which
specific users did what" — individual orders, fills, balances. Selective-reveal
ZK proofs (see [[Proof Architecture]] and future "Selective Reveal ZK" doc)
let an account-holder reveal their own slice without exposing anyone else's.

The `events_root` (proposed in [[Proof Architecture]] Phase 1) is a public
commitment over the private event list, which is how external verifiers can
prove "fill F happened in block N" without seeing the whole witness.

## Canonical witness bytes

Under [[Canonical Serialization]] v1. The witness encodes as a fixed outer
layout with variable-length sections, each prefixed with `count:u64`.

```
witness_v1_bytes =
    version:u8 = 0x01
 || header_bytes                                              (88 bytes, see Canonical Serialization)
 || previous_header_tag:u8                                    (0x00 = none, 0x01 = present)
 || previous_header_bytes?                                    (88 bytes if present)
 || section[orders]
 || section[rejections]
 || section[system_events]
 || section[fills]
 || section[clearing_prices]                                  (see below)
 || total_welfare:i64
 || minting_cost:i64
 || section[mm_constraints]
 || section[market_groups]
 || section[pre_state]
 || section[post_system_state]
 || section[post_state]
 || state_sidecar_section
 || section[resolved_markets]
```

Where `section[T]` = `count:u64 || item_bytes<T> * count`, items in canonical
sort order (specified below). `section[clearing_prices]` is the only irregular
one because it's a map:

```
clearing_prices_section =
    market_count:u64
 || (market_id:u32 || outcome_count:u32 || price:u64 * outcome_count) * market_count
```

with markets sorted by `market_id` ascending and prices in outcome order.

**Item encodings** (all defined in [[Canonical Serialization]]; deferred items
carry a TODO there too):

| Section | Item encoding | Sort order |
|---|---|---|
| `orders` | `WitnessOrder` (TODO, see Canonical Serialization §composites) | by `order.order_id` ascending |
| `rejections` | `WitnessRejection` (TODO) | by `order.order_id` ascending |
| `system_events` | `SystemEventWitness` (tag-dispatched like events registry) | by emission order |
| `fills` | `Fill` (see Canonical Serialization) | solver output order (stable) |
| `mm_constraints`, `market_groups` | TODO | by first market_id ascending |
| `pre_state`, `post_system_state`, `post_state` | `AccountSnapshot` (see Canonical Serialization) | by `id` ascending |
| `state_sidecar` | `StateSidecarSnapshot` (see Canonical Serialization) | withdrawal/order/reservation leaves by id ascending |
| `resolved_markets` | `market_id:u32` | by `market_id` ascending |

Once every item encoding is pinned, `witness_root = BLAKE3("sybil/witness/v1" || witness_v1_bytes)`.

## `witness_root` in the block header

Today the block header commits to `state_root` and `parent_hash`. It does
**not** commit to the witness. That means the sequencer could produce an
internally consistent block (state_root valid) while feeding a different
witness to downstream verifiers. The verifier would catch the mismatch
(post_state must rehash to state_root), but the witness itself is not
cryptographically anchored.

**Proposal — BlockHeader v2.** Extend the header:

```
BlockHeader v2 =
    height:u64
 || parent_hash:[u8; 32]
 || state_root:[u8; 32]
 || events_root:[u8; 32]          (new — see Proof Architecture Phase 1)
 || witness_root:[u8; 32]          (new — this doc)
 || order_count:u32
 || fill_count:u32
 || timestamp_ms:u64
```

Chaining hash uses a domain-separation prefix `"sybil/block-header/v2"` so v2
chains don't collide with v1 chains. Migration: hard fork at a chosen height,
same as the [[Canonical Serialization]] version-bump pattern.

This makes the witness a first-class part of the commitment chain. Anyone who
trusts the header transitively trusts the witness, and the sequencer can no
longer equivocate about what happened in a block without changing the header
(and thereby the on-chain state-root chain).

## Versioning

- **v1 witness** is the shape in this doc. Implementations MUST reject
  witness bytes whose first byte is not `0x01`.
- **v2 witness** will bump the leading byte to `0x02` and may rearrange
  sections. Verifiers dispatch on the version byte.
- **Adding a field** to the witness that affects correctness = new version.
  Adding a purely-observational field (e.g., timing info for debugging) that
  the verifier ignores = no version bump, but the verifier MUST skip unknown
  trailing bytes gracefully.

## Size budget

Order-of-magnitude estimate for a mid-sized block (1k orders, 2k fills, 500
accounts touched with ~5 positions each):

| Section | ~Bytes |
|---|---|
| Header + prev_header | 176 |
| 1k orders (est. 64B each) | 64,000 |
| 2k fills (52B each) | 104,000 |
| 3× account snapshots × 500 × ~80B | 120,000 |
| Other | ~10,000 |
| **Total** | **~300 KB** |

At 1 block per 2s, that's ~13 GB/day of witness data. Most of it is highly
compressible (canonical bytes are regular). Whether to post the full witness
to DA or just the `witness_root` + a compressed events summary is decided in
the Data Availability RFC (sibling, M3 · Validium Foundations). See also the
open-questions section below.

## Relation to events and state roots

Three hash roots in play, each with a different scope:

| Root | Scope | Primary consumer |
|---|---|---|
| `state_root` | v1: accounts; v2: complete typed validium state | ZK settlement, bridge claims, recovery checks |
| `events_root` | everything that happened in this block | external verifiers asking "did F happen" |
| `witness_root` | the full audit package | prover; anyone reconstructing the block |

They're complementary. A minimal on-chain commitment would include only
`state_root` (chain-valid) + `witness_root` (auditable) and use `events_root`
as a caller-supplied input to prove derived claims. Exact layout is decided
in [[Proof Architecture]], [[L1 Settlement and Vault]], and the Data
Availability RFC (sibling, M3).

Order expiry lives in the private `orders` section. The verifier can check
that an order included in a batch is eligible for `header.height`. The current
v2 [[State Root Schema]] also commits post-block active resting orders, so
presence or absence of an order is provable against `state_root` instead of
being only an implementation and witness property.

## Test vectors

Minimal genesis witness: zero accounts, zero orders, zero fills. Expected
`state_root` is the BLAKE3 of empty input (see [[Canonical Serialization]]
test vector 1). With the genesis header in [[Canonical Serialization]] test
vector 4, expected `witness_root = BLAKE3("sybil/witness/v1" ||
witness_v1_bytes)` — concrete hex lands in a follow-up test file once the
deferred item encodings are pinned.

## Open questions

1. **Full witness on DA or just root?** Posting 13 GB/day to Celestia is
   viable; posting it to Arweave is not. Two tiers of DA (root on L1, full
   witness on a cheaper layer) is probably the answer but needs the Data
   Availability RFC (sibling, M3) to close.
2. **Should the witness be split into public/private Rust structs?** The
   partition table in §3 lives only in prose today. Enforcing it at the type
   level (a `WitnessPublic` + `WitnessPrivate` pair, combined into
   `BlockWitness` for the sequencer's convenience) would make ZK compilation
   mechanical. Good idea; worth a dedicated follow-up.
3. **`post_system_state` redundancy.** It's recoverable from `pre_state +
   system_events`. Keeping it in the witness is convenient for the verifier
   but inflates bytes and proof-generation time. Could be dropped once the
   verifier is robust.
4. **Item encodings for Order / MmConstraint / MarketGroup.** Deferred from
   [[Canonical Serialization]] v1. Until these land, `witness_root` cannot be
   computed from spec — a gap to close before the events tree ships.

## Where this lives

> `crates/sybil-verifier/src/types.rs` — `BlockWitness`, `WitnessBlockHeader`, `AccountSnapshot`
> `crates/matching-sequencer/src/block.rs` — `produce_block` builds the witness; `hash_header` is the current (v1) header hash
> `crates/sybil-verifier/src/block.rs` — `verify_block` runs Layer 3 checks against the witness

## See also

- [[Canonical Serialization]] — the byte spec this doc builds on
- [[State Root Schema]] — normative spec for the `state_root` field
- [[State Root and Parent Hash]] — concept intro for state root and chaining
- [[Proof Architecture]] — events_root + authenticated data layer
- [[L1 Settlement and Vault]] — how witness-backed roots drive bridge custody
- [[Four-Layer Verification]] — current consumer of the witness
- [[ZK Integration Path]] — future consumer (the prover)
- [[Block Lifecycle]] — where in block production the witness is built
