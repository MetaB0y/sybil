---
tags: [zk, serialization, spec]
layer: verification
crate: sybil-verifier
status: current
last_verified: 2026-07-10
---

# Block Witness Format

`BlockWitness` v9 is the canonical private audit package for a Sybil block. The
sequencer persists it, native verification replays it, and the OpenVM guest
receives it inside `StateTransitionGuestInput`. The proof binds the witness by
recomputing `witness_root` from canonical witness bytes and including that root
in the state-transition public inputs.

The witness proves value-relevant state: order validity, fills, settlement,
post-state qMDB membership, event-root reconstruction, sidecar transition, and
L1 deposit checkpoint binding. It does not prove analytics/read-model
convenience data. `DerivedViewSidecar` is explicitly outside
`canonical_witness_bytes`, outside `witness_root`, outside `da_commitment`, and
outside the guest input; it rides beside sealed blocks for API and analytics
consumers only.

The SYB-216 design produced v3, SYB-225 moved the on-wire format to v4 by
adding `keys_digest`, SYB-253 moved it to v5 for withdrawal refund/prune events
plus the committed observed L1 height, and SYB-270 moved it to v6 for proven key
operations. v6 adds `KeyRegistered`/`KeyRevoked`, `CreateAccount.initial_keys`,
the full post-block `account_keys` universe, and a second guest qMDB proof that
authenticates non-genesis pre-state leaves. SYB-272 then moved the format to v7:
every deposit has a witnessed credit-or-quarantine disposition, the bridge
sidecar opens the single quarantine ledger, and claims are guest-replayed per
ADR-0015. SYB-32 Stage 1a moved the format to v8 by appending committed
`last_clearing_prices` to every market snapshot. Stage 1b moved it to v9 by
adding the signature-bound chain `genesis_hash` needed for in-guest key-op
verification. See
`design/witness-v6-keys-transition.md` and
`docs/adr/0015-deposit-quarantine.md`.

## Encoding

`canonical_witness_bytes(witness)` is not Borsh, MessagePack, bincode, or
OpenVM serde. It is the hand-specified byte vector returned by
`crates/sybil-verifier/src/witness_schema.rs`, using fixed-width
little-endian integers, verbatim byte arrays, ASCII domain strings, and
deterministic sort rules. MessagePack is a storage/transport codec for persisted
`BlockWitness` records and prepared guest-input files; it is not the commitment
encoding. Most `sybil-signing` payloads use Borsh; key operations use the
verifier-owned fixed-width state-bound canonical form. Neither signing encoding
is the witness commitment encoding.

Primitive encodings:

| Type | Bytes |
|---|---|
| `u8` | one byte |
| `u32` | 4-byte little-endian |
| `u64` | 8-byte little-endian |
| `i64` | 8-byte little-endian two's complement |
| `[u8; 20]`, `[u8; 32]` | verbatim |
| `MarketId` | inner `u32` little-endian |
| `Nanos`, `Qty` | inner `u64` little-endian |

## Layout

The first byte is the format version. For v9 it is `0x09`.

```text
canonical_witness_bytes =
    version:u8 = 0x09
 || header
 || previous_header_tag:u8                     // 0 = none, 1 = present
 || previous_header?                           // if tag == 1
 || genesis_hash:[u8;32]                       // key-op signing domain
 || orders_section
 || rejections_section
 || system_events_section
 || deposit_accumulator
 || fills_section
 || clearing_prices_section
 || total_welfare:i64
 || minting_cost:i64
 || mm_constraints_section
 || market_groups_section
 || pre_state_section
 || post_system_state_section
 || post_state_section
 || account_keys_section                       // full post-block active key universe
 || state_sidecar                              // post non-account state
 || pre_state_sidecar                          // pre non-account state
 || resolved_markets_section
```

`header` and `previous_header` have the same 120-byte layout:

```text
header =
    height:u64
 || parent_hash:[u8;32]
 || state_root:[u8;32]
 || events_root:[u8;32]
 || order_count:u32
 || fill_count:u32
 || timestamp_ms:u64
```

Sections are `count:u64 || item_bytes * count`, except where noted:

| Section | Item bytes | Order |
|---|---|---|
| `orders` | `order_accepted_leaf_value` | sort by `order.id` |
| `rejections` | `order_rejected_leaf_value` | sort by `order.id` |
| `system_events` | `system_event_leaf_value` | witness emission order |
| `fills` | `fill_leaf_value` | solver/witness order |
| `mm_constraints` | `mm_id:u64`, `max_capital:u64`, sorted `order_ids`, sorted `(order_id, side)` | sort by `mm_id` |
| `market_groups` | `name`, sorted `markets` | sort by first market id, then name |
| `pre_state`, `post_system_state`, `post_state` | `"sybil/witness/account"` plus account fields | sort by account id |
| `account_keys` | `"sybil/witness/account-keys"`, account id, then sorted `KeyRecord`s | sort accounts by id; keys by pubkey then scheme |
| `resolved_markets` | `market_id:u32` | sort by market id |

`KeyRecord` is `auth_scheme:u8 || pubkey_sec1:[u8;33] ||
capability_mask:u32le`. System-event tags 10 and 11 commit key registration and
revocation; tags 12 and 13 commit `DepositQuarantined` and
`QuarantineClaimed`, respectively. Key events include the complete raw-P256 or WebAuthn
authorization envelope. The guest welds each opened set to the post-state
`keys_digest`, reverse-folds key events to the authenticated pre-state digest,
then forward-replays them over a running globally unique key universe. During
that forward fold it cryptographically verifies each envelope over the
state-bound canonical key-op bytes using `genesis_hash` and the running
key/event digests.

`clearing_prices_section` is:

```text
market_count:u64
|| (market_id:u32 || outcome_count:u32 || price:u64 * outcome_count) * market_count
```

Markets are sorted by `market_id`; prices are in outcome order.

`state_sidecar` starts with the ASCII domain
`"sybil/witness/state-sidecar"`. `pre_state_sidecar` uses the same field
encoding with the distinct domain `"sybil/witness/pre-state-sidecar"`. Each
sidecar carries bridge state, markets sorted by `market_id`, market groups
sorted by `group_id`, resting orders sorted by `order.id`, and account
reservations sorted by `account_id`.

Each market snapshot ends with
`price_count:u64le || price:u64le * price_count`, after the resolution
template. The count is either zero (never cleared) or exactly
`num_outcomes`; every price is at most `NANOS_PER_DOLLAR`. On non-genesis
transitions, witnessed clearing prices must become the post-market prices and
markets without a clearing entry must carry their pre-market prices unchanged.

Bridge state is:

```text
deposit_cursor:u64
|| deposit_root:[u8;32]
|| observed_l1_height:u64
|| next_withdrawal_id:u64
|| withdrawal_count:u64
|| withdrawal_bytes * withdrawal_count          // sorted by withdrawal_id
|| quarantine_entry_count:u64
|| (sybil_account_key:[u8;32] || amount:i64) * quarantine_entry_count
                                                 // sorted by raw key
```

The logical quarantine map is committed as one `sys/quarantine_digest` leaf,
whose SHA-256 digest covers the sorted entry opening. Raw keys do not create
qMDB leaves.

## Deposit Accumulator

v3 replaces the old cumulative `l1_deposits` prefix with a block-start frontier
plus this-block delta:

```text
deposit_accumulator =
    "sybil/witness/deposit-accumulator"
 || pre_frontier:[u8;32] * 32
 || pre_count:u64
 || new_deposits_count:u64
 || l1_deposit_witness * new_deposits_count

l1_deposit_witness =
    "sybil/witness/l1-deposit"
 || deposit_id:u64
 || chain_id:u64
 || vault_address:[u8;20]
 || token_address:[u8;20]
 || sender:[u8;20]
 || sybil_account_key:[u8;32]
 || amount_token_units:u64
 || deposit_root:[u8;32]
```

Semantics:

- `pre_frontier` is the 32-level filled-subtree frontier at block start.
- `pre_count` must equal `pre_state_sidecar.bridge.deposit_cursor`.
- `deposit_root_from_frontier(pre_frontier, pre_count)` must equal
  `pre_state_sidecar.bridge.deposit_root`.
- `new_deposits[i].deposit_id` must equal `pre_count + i + 1`.
- Folding `new_deposits` onto `pre_frontier` with
  `deposit_frontier_prefix_roots` must produce every claimed per-deposit
  `deposit_root`; the last folded root, or the pre root for an empty delta, must
  equal `state_sidecar.bridge.deposit_root` and public `deposit_root`.
- The number of new deposits must advance the post cursor:
  `pre_count + new_deposits.len() == state_sidecar.bridge.deposit_cursor`.
- Exactly one disposition event per new deposit must match the delta by id,
  cumulative root, raw bridge key, and token-unit-to-nanos amount:
  `L1Deposit` credits a committed account, while `DepositQuarantined` parks the
  same value in the system ledger. Both dispositions fold the frontier.
- `QuarantineClaimed` removes the complete accumulated entry and credits the
  account by exactly that amount. The guest derives the expected bridge key as
  `BLAKE3("sybil/bridge/account-key/v1" || account_id:u64le)` from the committed
  account id; no host-side reverse mapping is trusted.

The recurrence is intentionally equivalent to `SybilVault._appendDepositLeaf`.
Solidity hashes deposit leaves as
`keccak256(abi.encode("sybil/l1-deposit/v1", chainid, vault, depositId, token, sender, key, amount))`,
wraps tree leaves as `keccak256(0x00 || leaf)`, hashes internal nodes as
`keccak256(0x01 || left || right)`, and appends through `filledSubtrees` for
depth 32. The Rust mirror in `sybil-l1-protocol` uses the same leaf, node, and
frontier fold.

## Hashing

`hash_header` has one source home:
`crates/sybil-zk/src/header_hash_impl.rs`. It is included by `sybil-zk` and
`sybil-verifier` so the guest, host, and verifier share one byte layout.

`witness_root`:

```text
witness_bytes = canonical_witness_bytes(witness)
witness_root = BLAKE3("sybil/witness" || witness_bytes)
```

`public_input_hash`:

```text
state_transition_public_input_hash =
    keccak256(abi.encode(
        "sybil/openvm/state-transition/v1",
        previous_height,
        new_height,
        previous_state_root,
        new_state_root,
        block_hash,
        events_root,
        witness_root,
        da_commitment,
        deposit_root,
        deposit_count
    ))
```

`da_commitment`:

```text
witness_bytes = canonical_witness_bytes(witness)
witness_root = BLAKE3("sybil/witness" || witness_bytes)
payload_root = BLAKE3(
    "sybil/da/witness-payload/v1"
 || payload_len:u64_le
 || witness_bytes
)
provider_refs_hash =
    BLAKE3("sybil/da/provider-refs/empty/v1")             // empty refs
    or
    BLAKE3(
        "sybil/da/provider-refs/v1"
     || ref_count:u64_le
     || (ref_len:u64_le || ref_bytes) * ref_count
    )                                                     // non-empty refs
da_commitment = BLAKE3(
    "sybil/da-commitment/v1"
 || block_height:u64_le
 || state_root
 || witness_root
 || payload_root
 || payload_len:u64_le
 || provider_refs_hash
)
```

The `StateTransitionPublicInputs` copied from the witness are:
`previous_height`, `new_height`, `previous_state_root`, `new_state_root`,
`block_hash`, `events_root`, `witness_root`, `da_commitment`, `deposit_root`,
and `deposit_count`. The guest verifies this binding before returning the
public-input hash.

## Pre-State Authentication

For non-genesis blocks, the verifier authenticates the full pre-state snapshot:

```text
compute_state_root_with_sidecar(pre_state, pre_state_sidecar)
    == previous_header.state_root
```

Then it checks parent hash chaining:

```text
hash_header(previous_header) == header.parent_hash
```

For genesis, `previous_header` is absent; the genesis header must have zero
`parent_hash` and height `1`.

The post-state sidecar is authenticated separately by:

```text
compute_state_root_with_sidecar(post_state, state_sidecar)
    == header.state_root
```

## Versioning And Compatibility

The version byte is the first byte of `canonical_witness_bytes`. v5 is `0x05`.
Unknown versions must fail closed. This repo does not maintain dual witness
decoders for devnet schema changes; ADR-0011 rejects compatibility wrappers
before launch because they double validity-critical encoder surface.

Any change to `canonical_witness_bytes`, verifier logic compiled by the guest,
deposit binding, public-input marshalling, or the guest's path-dependency
closure changes the OpenVM guest commitment. The required procedure is:

1. Land the schema/guest change as a deliberate batch.
2. Regenerate golden vectors.
3. Rebuild the guest commitment with `just openvm-commit`.
4. Commit the regenerated `zk/openvm-guest/openvm/release/sybil-openvm-guest.commit.json`
   and baseline artifact.
5. Run `scripts/zk-guest-fingerprint.sh --write`, then
   `scripts/zk-guest-fingerprint.sh --check`.
6. Repin or redeploy `OpenVmVerifierAdapter` with the new commitments.
7. Fresh-genesis the devnet. Do not attempt in-place state migration.

For a mid-testnet witness change, the compatibility strategy is the same:
batch the breaking witness and guest changes deliberately, repin commitments,
regenerate goldens, redeploy, and start a fresh genesis. Old witness bytes are
not accepted by a new guest, and a new guest is not accepted by an old adapter
pin.

## Golden Pins

| Pin | Current value | Test or artifact |
|---|---:|---|
| Witness format byte | `6` | `witness_schema::WITNESS_FORMAT_VERSION` |
| Empty canonical witness length | `1583` bytes | `canonical_witness_bytes_are_stable_for_empty_witness` |
| Byte-identity canonical witness length | `3907` bytes | `golden_vectors_pin_header_hash_and_snapshot_encoders` |
| Byte-identity witness SHA-256 length-prefixed digest | `2cc74a0d572c4519b1dcd06e8b230e1cc1b5af488f93324e3297ee8478d8c1f5` | same byte-identity test |
| Empty public-input hash | `e37bee4b2c3e7bb723c01665ccc59fbf098e708c878e1d488a792fdb51858a6f` | `public_input_hash_golden` |
| OL-4 Solidity/Rust public-input hash vector | `42197d0dff7bc2f86a6e359f187adda163fc9b4ffaa0e7cfb9845561bb744830` | Rust test plus `contracts/test/SybilGoldenVectors.t.sol` |
| Current `app_exe_commit` | `0x000a9cb169bb987eb2cdff9cc550179c1d3aae8ebcf0469b9ecb5f56e9a81398` | committed OpenVM `commit.json` and lock |
| Current `app_vm_commit` | `0x007a02fc3055c8beb7aa51187d008991bdec498852b5e1e27f223ee04a72cac5` | committed OpenVM `commit.json` and lock |

The L1 deposit leaf/root vectors live in both
`crates/sybil-l1-protocol/src/lib.rs` and
`contracts/test/SybilGoldenVectors.t.sol`. They pin the Solidity/Rust
equivalence for deposit leaves, tree leaves, prefix roots, and selected
frontier slots.

## Sources Appendix

Concrete claims above were cross-checked against these source ranges:

| Claim area | Source |
|---|---|
| `BlockWitness` field set and v5 `deposit_accumulator`/`pre_state_sidecar` fields | `crates/sybil-verifier/src/types.rs:16-60` |
| `DepositAccumulatorWitness` field semantics and default depth | `crates/sybil-verifier/src/types.rs:176-194` |
| Format version byte `5` and first-byte placement | `crates/sybil-verifier/src/witness_schema.rs:16-30` |
| Exact top-level field order | `crates/sybil-verifier/src/witness_schema.rs:18-74` |
| Header byte order | `crates/sybil-verifier/src/witness_schema.rs:77-85`; `crates/sybil-zk/src/header_hash_impl.rs:1-10` |
| Clearing-price map encoding | `crates/sybil-verifier/src/witness_schema.rs:87-98` |
| MM constraint encoding and sort | `crates/sybil-verifier/src/witness_schema.rs:100-132` |
| Market-group encoding and sort | `crates/sybil-verifier/src/witness_schema.rs:134-163` |
| Account sections sorted by id | `crates/sybil-verifier/src/witness_schema.rs:165-172` |
| Deposit-accumulator byte layout | `crates/sybil-verifier/src/witness_schema.rs:174-192` |
| Empty witness length `1549` | `crates/sybil-verifier/src/witness_schema.rs` test `canonical_witness_bytes_are_stable_for_empty_witness` |
| Snapshot sidecar domains and sort rules | `crates/sybil-verifier/src/snapshot_schema.rs:244-300` |
| Primitive LE append helpers | `crates/sybil-verifier/src/snapshot_schema.rs:302-340` |
| Event section order and leaf encodings | `crates/sybil-verifier/src/event_schema.rs:8-21`, `29-151` |
| Native event root is keyless qMDB over section-order event bytes | `crates/sybil-verifier/src/event_commitment.rs:1-5`, `43-67`, `86-105` |
| Pre-state sidecar authentication | `crates/sybil-verifier/src/block.rs:90-117` |
| Post-state root authentication | `crates/sybil-verifier/src/block.rs:63-75` |
| Genesis parent/height rule | `crates/sybil-verifier/src/block.rs:129-146` |
| Native deposit accumulator verification | `crates/sybil-verifier/src/sidecar.rs:370-483` |
| Guest public input binding and deposit checkpoint checks | `crates/sybil-zk/src/lib.rs:496-580`, `585-758` |
| `witness_root`, DA payload root, provider-ref hash, and `da_commitment` formulas | `crates/sybil-zk/src/lib.rs:346-466` |
| Public-input hash formula and field order | `crates/sybil-zk/src/lib.rs:328-344` |
| Public inputs derived from witness | `crates/sybil-zk/src/lib.rs:468-493` |
| OpenVM guest consumes `StateTransitionGuestInput` and reveals public-input hash | `zk/openvm-guest/src/main.rs:4-26` |
| MessagePack/OpenVM transport distinction | `zk/openvm-tools/src/main.rs:22-30`, `92-113`; `crates/matching-sequencer/src/store.rs:112-115`, `874-875` |
| `sybil-signing` Borsh signable payloads | `crates/sybil-signing/src/lib.rs:1-4`, `75-94` |
| `DerivedViewSidecar` outside witness/root/DA/guest | `crates/matching-sequencer/src/block.rs:78-88`, `147-163` |
| L1 deposit domain, tree depth, frontier alias, leaf/node hashing, and frontier fold | `crates/sybil-l1-protocol/src/lib.rs:11-22`, `133-153`, `156-260` |
| Solidity vault deposit append recurrence | `contracts/src/SybilVault.sol:43-52`, `140-158`, `298-328`, `348-363` |
| Solidity/Rust deposit and public-input golden vectors | `contracts/test/SybilGoldenVectors.t.sol:82-109`, `122-179`; `crates/sybil-l1-protocol/src/lib.rs:481-659`; `crates/sybil-zk/src/lib.rs:1450-1477` |
| Public-input and DA tests | `crates/sybil-zk/src/lib.rs:1439-1545` |
| Byte-identity witness length and digest | `crates/sybil-verifier/src/byte_identity.rs:20-50` |
| Current OpenVM commitment values | `zk/openvm-guest/openvm/release/sybil-openvm-guest.commit.json:1-4`; `zk/openvm-guest/guest.commitment.lock.json:1-7` |
| Guest source fingerprint and repin workflow | `scripts/zk-guest-fingerprint.sh:1-43`, `123-168`, `171-229`; `zk/openvm-guest/README.md:16-35`, `63-83` |
| Fresh-genesis policy | `docs/runbooks/devnet-redeploy.md:31-38`, `76-93`, `116-134` |
| SYB-216 rationale: derived-view outside witness, frontier not MMR, no dual decoders | `design/witness-schema-v2.md:73-103`, `300-327`, `340-376`, `422-443`, `462-476` |

## See Also

- [[Canonical Serialization]]
- [[State Root Schema]]
- [[State Root and Parent Hash]]
- [[Four-Layer Verification]]
- [[ZK Integration Path]]
- [[L1 Settlement and Vault]]
- [[Data Availability]]
- [[Block Lifecycle]]
