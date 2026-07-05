---
tags: [zk, spec]
layer: verification
crate: sybil-verifier
status: current
last_verified: 2026-07-03
---

# Block Witness

The Block Witness is the self-contained input to block verification. The
sequencer produces one per block and persists the qMDB proof material needed
for the post-state commitment. The [[Four-Layer Verification]] logic consumes
the witness today; `sybil-prover` combines it with retained qMDB proofs into
a portable `StateTransitionProofJob`, then builds OpenVM guest input from
that job. Everything else in this doc follows from two invariants:

1. **Self-contained.** Given a witness and its parent header (or nothing, for
   genesis), any third party can re-run settlement and verify every claim the
   block makes — without access to sequencer state, mempool, or history
   beyond the parent.
2. **Reproducible.** `apply_fills(pre_state, system_events, fills) == post_state`,
   and `compute_state_root_with_sidecar(post_state, state_sidecar) ==
   header.state_root`, and `compute_events_root(system_events, orders,
   rejections, fills) == header.events_root`. If any equation fails, the
   witness is invalid.

## Rust type

`crates/sybil-verifier/src/types.rs::BlockWitness` holds 17 fields:

| Field | Purpose |
|---|---|
| `header` | block header being verified |
| `previous_header` | parent header (or `None` for genesis) |
| `orders` | orders accepted into this batch, with account mapping |
| `rejections` | orders rejected, with reasons |
| `system_events` | state changes applied between blocks (create account, dev deposit, L1 deposit, withdrawal creation, resolution) |
| `l1_deposits` | private L1 deposit-log prefix through `state_sidecar.bridge.deposit_cursor`, used by the guest to reconstruct the vault checkpoint root |
| `fills` | real accepted-order fills produced by the solver; synthetic minting fills are not allowed |
| `clearing_prices` | per-market clearing prices produced by the solver |
| `total_welfare` | net welfare: gross order-value objective minus settlement-derived `minting_cost` |
| `minting_cost` | settlement-derived reporting cost from real-fill cash flow and `derive_minting` adjustments |
| `mm_constraints` | MM budget constraints active this batch |
| `market_groups` | market group definitions (for complete-set logic) |
| `pre_state` | account snapshots at block start |
| `post_system_state` | after system events, before fills |
| `post_state` | after fills — what the header's `state_root` commits to |
| `state_sidecar` | non-account state committed by the header's `state_root`: bridge, markets, market groups, resting orders, and reservations |
| `resolved_markets` | markets resolved/voided; orders/fills must not reference |

The sequencer builds this in `matching-sequencer::sequencer` at the end of
each block. Sequencer tests run the 4-layer verifier over it; `matching-sim`
builds a witness and runs the match layer.
`sybil-prover` is the prover-input boundary; its `sequencer-store` feature
consumes a committed witness plus qMDB proofs from storage and emits
`StateTransitionProofJob`, while the default builder consumes only that
portable job and produces `StateTransitionGuestInput` for `sybil-zk`.

Minting/burning is not encoded as synthetic orders or synthetic fills. The
solver may use minting variables internally to discover welfare-maximizing
fills and clearing prices, but the witness records only real participant
orders/fills. Settlement verification independently replays those fills, derives
the required protocol counterparty adjustment, checks the reserved MINT account
in `post_state`, and checks that `minting_cost` equals the shared
settlement-derived reporting cost. Layer 1 welfare verification then checks
`total_welfare = gross_order_value - minting_cost`.

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
| `l1_deposits` | private (the guest proves the prefix root equals public `depositRoot`/`depositCount` and that credited L1 deposit events match included leaves) |
| `fills` (individual) | private (shape is public via events_root) |
| `mm_constraints`, `market_groups` | private |
| `pre_state`, `post_system_state`, `post_state` | private |
| `state_sidecar` | private (deposit root/count, withdrawal commitments, market status, and selected market/order/reservation claims can be exposed through dedicated proof public inputs where needed) |

Rationale: the public side is "what was the market's observable outcome" —
clearing prices, how many orders, welfare. The private side is "which
specific users did what" — individual orders, fills, balances. Selective-reveal
ZK proofs (see [[Proof Architecture]] and future "Selective Reveal ZK" doc)
let an account-holder reveal their own slice without exposing anyone else's.

The `events_root` is a public qMDB commitment over the private event list,
which is how external verifiers can prove "fill F happened in block N" without
seeing the whole witness.

## Canonical witness bytes

Under [[Canonical Serialization]], the witness encodes as a fixed outer layout
with variable-length sections, each prefixed with `count:u64`.

```
witness_bytes =
    version:u8 = 0x02
 || header_bytes                                              (120 bytes, see Canonical Serialization)
 || previous_header_tag:u8                                    (0x00 = none, 0x01 = present)
 || previous_header_bytes?                                    (120 bytes if present)
 || section[orders]
 || section[rejections]
 || section[system_events]
 || section[l1_deposits]
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

**Item encodings** are defined by [[Canonical Serialization]] and the
executable schema in `crates/sybil-verifier/src/witness_schema.rs`:

| Section | Item encoding | Sort order |
|---|---|---|
| `orders` | accepted-order event leaf bytes | by `order.order_id` ascending |
| `rejections` | rejected-order event leaf bytes | by `order.order_id` ascending |
| `system_events` | `SystemEventWitness` (tag-dispatched like events registry) | by emission order |
| `l1_deposits` | L1 deposit leaf inputs plus cumulative post-deposit root | by append order, with `deposit_id == index + 1` |
| `fills` | `Fill` (see Canonical Serialization) | solver output order (stable) |
| `mm_constraints` | `MmConstraint` canonical bytes | by `mm_id` ascending |
| `market_groups` | `MarketGroup` canonical bytes | by first market_id, then name |
| `pre_state`, `post_system_state`, `post_state` | `AccountSnapshot` (see Canonical Serialization) | by `id` ascending |
| `state_sidecar` | `StateSidecarSnapshot` (see Canonical Serialization) | market, market-group, withdrawal, order, and reservation leaves by id ascending |
| `resolved_markets` | `market_id:u32` | by `market_id` ascending |

`witness_root = BLAKE3("sybil/witness" || witness_bytes)`. The implemented
schema lives in `crates/sybil-verifier/src/witness_schema.rs` and is exposed
through `sybil_verifier::commitments::witness_schema`.

## `witness_root` in the block header

Today the block header commits to `state_root`, `events_root`, and
`parent_hash`. It does **not** commit to the full witness. That means the
sequencer could produce an internally consistent block while feeding a
different non-event witness section to downstream verifiers. The verifier
would catch state/event mismatches, but the full witness package itself is not
cryptographically anchored.

The OpenVM state-transition public inputs now include `witness_root`, and the
guest recomputes it from the private `BlockWitness`. This binds the proof to a
canonical full witness package, but the root is still not part of the block
header hash chain until the header extension below lands.

**Proposal - witness root.** Add `witness_root` to the header:

```
BlockHeader =
    height:u64
 || parent_hash:[u8; 32]
 || state_root:[u8; 32]
 || events_root:[u8; 32]
 || witness_root:[u8; 32]
 || order_count:u32
 || fill_count:u32
 || timestamp_ms:u64
```

Chaining hash uses a domain-separation prefix `"sybil/block-header"` so the
extended header has explicit bytes.

This would make the full witness a first-class part of the commitment chain.
Anyone who trusts the header transitively trusts the witness, and the
sequencer can no longer equivocate about non-event witness data without
changing the header.

## Format Changes

- The witness bytes begin with a format byte. The shape in this doc uses
  `0x02`; implementations MUST reject unknown format bytes. `0x02` adds the
  private `l1_deposits` prefix section after `system_events`.
- Before launch, changing the witness layout updates the format byte, hash
  domain, and verifier together.
- Adding a purely observational field that the verifier ignores must be an
  explicitly skipped trailing section.

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
to DA or only a recovery-oriented subset is part of [[Data Availability]] and
future operator-replacement design. The current proof binds the canonical
witness payload into `da_commitment` but does not yet require a specific DA
provider.

## Relation to events and state roots

Three hash roots in play, each with a different scope:

| Root | Scope | Primary consumer |
|---|---|---|
| `state_root` | complete typed validium state | ZK settlement, bridge claims, recovery checks |
| `events_root` | qMDB event log for everything that happened in this block | external verifiers asking "did F happen" |
| `witness_root` | the full audit package | prover; anyone reconstructing the block |

They're complementary. A minimal on-chain commitment would include only
`state_root` (chain-valid) + `witness_root` (auditable) and use `events_root`
as a caller-supplied input to prove derived claims. Exact layout is decided
in [[Proof Architecture]], [[L1 Settlement and Vault]], and the Data
Availability RFC (sibling, M3).

Order expiry lives in the private `orders` section. The verifier can check
that an order included in a batch is eligible for `header.height`. [[State Root Schema]]
also commits post-block active resting orders, so presence or absence of an
order is provable against `state_root` instead of being only an implementation
and witness property.

## Test vectors

Minimal genesis witness: zero accounts, zero orders, zero fills. Expected
`state_root` is the native qMDB root over the default typed state leaves.
With the genesis header in [[Canonical Serialization]] test vector 3,
expected `witness_root = BLAKE3("sybil/witness" || witness_bytes)`. The
current executable vectors live in `sybil-zk`'s public-input golden test and
`sybil-verifier`'s `witness_schema` tests.

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
## Where this lives

> `crates/sybil-verifier/src/types.rs` — `BlockWitness`, `WitnessBlockHeader`, `AccountSnapshot`
> `crates/matching-sequencer/src/block.rs` — `produce_block` builds the witness and imports the shared header hash
> `crates/sybil-verifier/src/block.rs` — `verify_block` runs Layer 3 checks against the witness
> `crates/sybil-verifier/src/event_schema.rs` — canonical event leaves
> `crates/sybil-verifier/src/event_commitment.rs` — native keyless-qMDB `events_root`
> `crates/sybil-verifier/src/witness_schema.rs` — canonical full witness bytes
> `crates/sybil-zk/src/header_hash_impl.rs` — shared header hash source
> `crates/sybil-zk/src/lib.rs` — `witness_root` computation and public input binding

## See also

- [[Canonical Serialization]] — the byte spec this doc builds on
- [[State Root Schema]] — normative spec for the `state_root` field
- [[State Root and Parent Hash]] — concept intro for state root and chaining
- [[Proof Architecture]] — events_root + authenticated data layer
- [[L1 Settlement and Vault]] — how witness-backed roots drive bridge custody
- [[Four-Layer Verification]] — current consumer of the witness
- [[ZK Integration Path]] — future consumer (the prover)
- [[Block Lifecycle]] — where in block production the witness is built
