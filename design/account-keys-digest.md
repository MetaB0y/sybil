# SYB-225 Survey and Design: `keys_digest` in Account Leaves

Date: 2026-07-06

Scope: read-only survey plus design. No implementation was performed.

## Executive Recommendation

Add a new `keys_digest: [u8; 32]` field to `AccountSnapshot` and therefore to the `acct/{account_id}` state leaf. The digest should commit to the active signing-key set for that account. Do not overload `events_digest`: it is a history/activity accumulator, not recoverable key state, and today the verifier does not derive it from fills or system events.

The digest alone is not enough. The canonical witness sidecar must also carry the full active key set for every account, and witness import must rebuild `pubkey_registry` from that sidecar. Without the full key set, a replacement operator can verify that some key set existed but cannot recover which keys are authorized.

Make key registration/revocation proven system operations. Subsequent key mutations must be signed by an existing active account key and verified by the guest against the pre-operation key set. The first key must be introduced at account creation, because today account creation is service/operator driven and the existing `POST /v1/accounts/{id}/keys` path is public and unsigned.

This is a consensus change: bump canonical witness format v3 to v4, change account leaf bytes and state roots, update `decode_canonical_witness_bytes`, update the OpenVM guest and repin the guest commitment. Batch it with SYB-224.

## 1. Key Registration Flow Today

### HTTP route and API behavior

`POST /v1/accounts/{id}/keys` is mounted on public routes, not service routes: `crates/sybil-api/src/app.rs:747` opens `public_routes`, and the key route is registered at `crates/sybil-api/src/app.rs:790-794`. Account creation/funding are service routes at `crates/sybil-api/src/app.rs:940-948`.

The register-key handler parses `public_key_hex`, accepts compressed or otherwise valid SEC1 P256 input via `Sec1Point`/`VerifyingKey`, optionally validates WebAuthn registration, then calls `register_pubkey_with_meta`: `crates/sybil-api/src/routes/accounts.rs:117-174`. There is no signer, signature, nonce, or existing-key authorization in this request. The request DTO confirms the fields are just public key, auth scheme, optional WebAuthn registration, label, and scope: `crates/sybil-api-types/src/request.rs:134-160`.

For WebAuthn registration, the API extracts the COSE EC2 key from attestation data and requires it to match `public_key_hex`: `crates/sybil-api/src/routes/accounts.rs:144-156` and `crates/sybil-api/src/webauthn.rs:157-176`.

Revocation is different: `POST /v1/accounts/{id}/keys/revoke` is signed. The route normalizes the target to compressed SEC1 bytes, computes canonical revocation bytes, and either verifies raw P256 or WebAuthn intent before calling the sequencer: `crates/sybil-api/src/routes/accounts.rs:621-674`. The request fields include target pubkey, signer pubkey, auth scheme, signature/WebAuthn assertion, and nonce: `crates/sybil-api-types/src/request.rs:201-218`.

### Actor and sequencer flow

The actor messages include `RegisterPubkey`, `RegisterPubkeyWithMeta`, and signed/authenticated revoke variants: `crates/matching-sequencer/src/actor.rs:195-221`.

For registration, the actor checks the account exists and the key is not already registered, persists a control-plane WAL command, then mutates in-memory state: `crates/matching-sequencer/src/actor.rs:1614-1638` and `crates/matching-sequencer/src/actor.rs:1832-1859`.

For revocation, the actor verifies the signed revocation, resolves the signer to the claimed account, validates ownership and last-key lockout against a clone, burns the replay nonce, persists the WAL row, then removes the key: `crates/matching-sequencer/src/actor.rs:1913-1948`. Shared signer resolution is by `lookup_pubkey`: `crates/matching-sequencer/src/actor.rs:1861-1876`.

The sequencer holds `pubkey_registry: HashMap<PublicKey, RegisteredPubkey>` as separate state: `crates/matching-sequencer/src/sequencer.rs:810-811`. Registration inserts into that map only: `crates/matching-sequencer/src/sequencer.rs:1478-1496`. Revocation removes from that map and refuses to remove the last signing key: `crates/matching-sequencer/src/sequencer.rs:1499-1529`.

Signed order admission uses the registry but does not put signer material into the witness. The actor verifies the P256 signature, looks up the signer pubkey, advances nonce, and creates an `OrderSubmission` with only the account id: `crates/matching-sequencer/src/actor.rs:1194-1221`. `WitnessOrder` contains only `order`, `account_id`, and `is_mm`: `crates/sybil-verifier/src/types.rs:74-80`.

### Store and WAL

The store has separate redb tables for signing keys:

- `PUBKEY_REGISTRY`: compressed SEC1 pubkey -> account id, `crates/matching-sequencer/src/store.rs:125-126`
- `PUBKEY_AUTH_SCHEMES`: compressed SEC1 pubkey -> scheme tag, `crates/matching-sequencer/src/store.rs:128-129`
- `PUBKEY_META`: compressed SEC1 pubkey -> msgpack label/scope/created_at metadata, `crates/matching-sequencer/src/store.rs:131-134`

SYB-60 metadata is serialized as `label`, `scope`, and `created_at_ms`: `crates/matching-sequencer/src/store.rs:949-958`. The store tags auth schemes as RawP256=0 and WebAuthn=1: `crates/matching-sequencer/src/store.rs:976-987`.

Every block save rewrites the pubkey tables from the current in-memory registry, clearing removed keys first so revocation is durable: `crates/matching-sequencer/src/store.rs:1373-1397`. Restore reads these tables back into `pubkey_registry`: `crates/matching-sequencer/src/store.rs:2627-2665`.

The control-plane WAL contains register/revoke variants: `crates/matching-sequencer/src/store.rs:845-947`. The actor appends WAL rows with `append_control_plane_command`: `crates/matching-sequencer/src/store.rs:3092-3098`. On block save the control-plane log is cleared: `crates/matching-sequencer/src/store.rs:1517-1520`. On restore it is replayed before bridge WALs: `crates/matching-sequencer/src/sequencer.rs:977-996`.

### Does key registration/revocation enter the block or witness?

No, except indirectly by allowing later signed requests to be admitted.

`SystemEvent` has variants for account creation, deposit, L1 deposit, withdrawal creation, market resolution, order cancellation, and market-group extension. It has no key registration or revocation variant: `crates/matching-sequencer/src/system_event.rs:6-50`.

`SystemEventWitness` mirrors those same variants and also has no key variant: `crates/sybil-verifier/src/types.rs:118-162`. `convert_system_event` has no key arm: `crates/matching-sequencer/src/sequencer.rs:573-640`. `event_schema::system_event_leaf_value` assigns tags 0 through 6 to the existing variants; no key op is encoded into `events_root`: `crates/sybil-verifier/src/event_schema.rs:29-124`.

The witness assembled for a block includes `system_events`, account snapshots, fills, sidecars, etc., but no control-plane WAL and no pubkey registry: `crates/sybil-verifier/src/types.rs:16-60`. Witness assembly populates `system_events` from `pending_system_events`, deposits from `L1Deposit` events, and state root from `post_state + state_sidecar`: `crates/matching-sequencer/src/sequencer.rs:2862-2924`.

Conclusion: key registration and revocation are entirely outside the proven state transition today. The verifier and guest see no key material and no key-op trace.

## 2. What the Guest and Verifier Currently Prove About Accounts

`AccountSnapshot` currently contains `id`, `balance`, `total_deposited`, `positions`, and `events_digest`: `crates/sybil-verifier/src/types.rs:198-207`. There is no nonce, pubkey, auth scheme, or key digest.

The account state leaf is keyed as `b"acct/" || account_id:be`: `crates/sybil-verifier/src/state_schema.rs:88-93`. `state_root_leaves` sorts accounts by id and emits `(account_leaf_key, account_leaf_value)` for each account: `crates/sybil-verifier/src/state_schema.rs:15-26`.

The account leaf value is `b"sybil/state/acct"` followed by account fields. The fields are encoded as `id`, `balance`, `total_deposited`, canonical positions, and `events_digest`: `crates/sybil-verifier/src/snapshot_schema.rs:28-34` and `crates/sybil-verifier/src/snapshot_schema.rs:159-162`. Integers in snapshot values are little-endian: `crates/sybil-verifier/src/snapshot_schema.rs:335-340`.

State roots are commonware ordered-current qMDB roots using SHA-256. The verifier builds sorted leaves, writes each key/value to a temporary ordered qMDB, and returns `db.root().0`: `crates/sybil-verifier/src/block.rs:205-223` and `crates/sybil-verifier/src/block.rs:242-269`. The qMDB type uses `QmdbSha256`: `crates/sybil-verifier/src/block.rs:37-44`.

`verify_block` recomputes the post-state root from `witness.post_state + witness.state_sidecar` and checks it against `header.state_root`: `crates/sybil-verifier/src/block.rs:58-75`. It also authenticates the pre-state against `previous_header.state_root` when a previous header exists: `crates/sybil-verifier/src/block.rs:90-104`.

Canonical witness format is currently v3: `crates/sybil-verifier/src/witness_schema.rs:27`. The decoder rejects any other version and re-encodes the decoded witness to enforce canonical order: `crates/sybil-verifier/src/witness_schema.rs:63-133`. Account decoding reads exactly the current five account fields: `crates/sybil-verifier/src/witness_schema.rs:614-622`. Account witness encoding uses the same account fields under `b"sybil/witness/account"`: `crates/sybil-verifier/src/snapshot_schema.rs:164-166`.

The OpenVM-facing `StateTransitionGuestInput` carries public inputs, a full `BlockWitness`, DA provider refs, and a state-root proof: `crates/sybil-zk/src/lib.rs:59-65`. `verify_state_transition_input` checks public input binding, verifies the qMDB state root proof, then runs match, settlement, order, and sidecar verification: `crates/sybil-zk/src/lib.rs:306-325`. The guest recomputes witness bytes from `witness_schema::canonical_witness_bytes`: `crates/sybil-zk/src/lib.rs:346-370`.

The qMDB proof checks all state leaves derived from `witness.post_state + witness.state_sidecar`, verifies each key/value proof, and checks the next-key ring for exact keyspace: `crates/sybil-zk/src/guest_commitments.rs:136-167`.

Changing the account leaf encoding changes every affected state root. Changing witness fields changes canonical witness bytes, witness root, DA commitment, guest code, and the guest commitment. The v2-to-v3 design already treats canonical witness changes this way: `design/witness-schema-v2.md:410-414` and `design/witness-schema-v2.md:426-431`.

## 3. How Other Per-Account Mutations Become Witness-Visible

Account creation is service-driven today. `POST /v1/accounts` calls `sequencer.create_account`: `crates/sybil-api/src/routes/accounts.rs:41-63`. The actor persists `CreateAccountAt`, then calls `create_account_at`: `crates/matching-sequencer/src/actor.rs:1574-1593`. The sequencer creates the account, captures a missing pre-state baseline, and records `SystemEvent::CreateAccount`: `crates/matching-sequencer/src/sequencer.rs:1720-1730`.

Funding/deposits similarly capture a pre-state baseline, mutate account balance and `total_deposited`, and record a system event: `crates/matching-sequencer/src/sequencer.rs:1753-1772`. L1 deposits validate and advance bridge state, mutate balance and `total_deposited`, then record `SystemEvent::L1Deposit`: `crates/matching-sequencer/src/sequencer.rs:1968-1999`. Withdrawals debit balance, create a withdrawal sidecar leaf, and record `SystemEvent::WithdrawalCreated`: `crates/matching-sequencer/src/sequencer.rs:2057-2092`.

At block production, pending system events are drained and applied to `events_digest`: account creation, deposits, L1 deposits, withdrawals, market resolutions, and cancellations all have digest update arms; market-group extension does not touch accounts: `crates/matching-sequencer/src/sequencer.rs:2936-3032`.

Fills update account state and `events_digest` during settlement: `crates/matching-sequencer/src/settlement.rs:37-54`. Mint adjustments update MINT's digest: `crates/matching-sequencer/src/settlement.rs:57-69`.

`events_digest` itself is a BLAKE3 hash chain over the previous digest and encoded event bytes: `crates/matching-sequencer/src/digest.rs:3-8`. Current digest event tags include fills, deposits, L1 deposits, withdrawals, resolutions, create account, mint, and order cancellation: `crates/matching-sequencer/src/digest.rs:10-115`.

`build_witness_phase_snapshots` makes account system events visible by using captured baselines for `pre_state` and live accounts for `post_system_state`: `crates/matching-sequencer/src/sequencer.rs:480-500`. Later witness assembly includes `pre_state`, `post_system_state`, and `post_state`: `crates/matching-sequencer/src/sequencer.rs:3577-3596`.

Important verifier gap: settlement verification derives only balances and positions from `post_system_state + fills`; it does not derive `events_digest` or `total_deposited`. The derived account state contains only balance and positions: `crates/sybil-verifier/src/settlement.rs:31-56`, and post-state comparison checks only balance and positions: `crates/sybil-verifier/src/settlement.rs:381-448`.

Can key ops ride `events_digest` instead of a new field? No. `events_digest` is an append-only activity accumulator, not a recoverable set commitment. It can prove "something in this hash chain changed" only if the verifier recomputes it, and it cannot tell the escape guest which keys are currently active. It also cannot support operator replacement: a digest alone cannot rebuild `pubkey_registry`. Key ops should update `events_digest` as account activity, but the current signer set needs a separate `keys_digest` plus a full key-set sidecar.

## 4. WebAuthn Key Identity and Consensus Fields

The registered auth schemes are `RawP256` and `WebAuthn`: `crates/matching-sequencer/src/crypto.rs:20-28`. The stored scheme tags already use RawP256=0 and WebAuthn=1: `crates/matching-sequencer/src/store.rs:976-987`.

The key identity is the compressed SEC1 P256 public key. `PublicKey` equality and hashing use `to_sec1_point(true)`, and `compressed_bytes()` returns those 33 bytes: `crates/matching-sequencer/src/crypto.rs:167-195`. The registry is keyed globally by `PublicKey`, so one compressed key cannot be registered twice: `crates/matching-sequencer/src/sequencer.rs:1491-1495`.

WebAuthn registration extracts a P256 COSE EC2 key and converts it to compressed SEC1 bytes. The COSE checks require kty=2, alg=-7, crv=1, and 32-byte x/y coordinates: `crates/sybil-api/src/webauthn.rs:260-280`.

WebAuthn assertions sign `authenticatorData || SHA-256(clientDataJSON)`, and the API challenge is `base64url(SHA-256(canonical_bytes))`: `crates/sybil-api/src/webauthn.rs:108-153`. The architecture note confirms the witness and guest currently contain neither WebAuthn envelopes nor raw signatures: `docs/architecture/P256 Authentication.md:19-35`.

SYB-60 metadata fields are label, scope, and created_at: `crates/matching-sequencer/src/crypto.rs:59-76`. The code says scope is descriptive and every registered key can sign every mutation: `crates/matching-sequencer/src/crypto.rs:30-35`; account.rs says the same for agent keys: `crates/matching-sequencer/src/account.rs:38-46`.

Consensus relevance recommendation:

- In: `auth_scheme` and compressed SEC1 pubkey. These determine whether a supplied authorization can be verified and which public key verifies it.
- Out: `label`, `created_at_ms`, `credential_id_b64url`. These are cosmetic or client-side round-trip fields.
- Out for v1: existing `scope`, unless the orchestrator promotes it from descriptive metadata to an authorization rule. If scope is intended to constrain escape/withdraw/trade authority, replace it with a consensus `capability_mask` and digest that. Digesting today's UI scope would make a cosmetic label-classification field consensus without any verifier behavior attached.
- Out: `revoked_at`. Signing-key revocation removes the key from the active registry today; active signer set is the state. Historical revocation should be proven by key-op events, not by retaining revoked keys in the active set digest.

## Recommended Design

### `keys_digest` definition

Add `keys_digest: [u8; 32]` to `AccountSnapshot`, encoded after `events_digest` in both state and witness account encodings.

Use SHA-256, matching the existing state-content digest pattern used for market metadata (`crates/sybil-verifier/src/state_schema.rs:154-165`) and the qMDB state root hash. Define:

```text
keys_digest(account_id, active_keys) =
  SHA256(
    "sybil/state/account-keys-digest/v1"
    || account_id:u64le
    || key_count:u64le
    || concat(sorted_key_records)
  )

key_record =
    auth_scheme:u8       // 0 raw_p256, 1 webauthn
 || pubkey_sec1[33]      // compressed SEC1 P256
```

Sort `active_keys` by `(pubkey_sec1 lexicographic, auth_scheme)`. Reject duplicate pubkeys globally and duplicate pubkeys within an account. The digest of an empty key set is the domain/account/count hash above with `key_count = 0`, not `[0; 32]`.

Do not include labels, created_at, credential ids, or revoked markers. Do not include existing `scope` in v1 unless it is ratified as a consensus authorization field.

### Full key-set sidecar

Add a canonical key-registry sidecar to the witness, preferably inside `StateSidecarSnapshot` as `account_key_sets: Vec<AccountKeySetSnapshot>` so pre and post snapshots travel with the existing `pre_state_sidecar` and `state_sidecar`.

Suggested structs:

```rust
pub struct AccountKeySetSnapshot {
    pub account_id: u64,
    pub active_keys: Vec<AccountSigningKeySnapshot>,
}

pub struct AccountSigningKeySnapshot {
    pub auth_scheme: AccountAuthSchemeSnapshot, // raw_p256=0, webauthn=1 on the wire
    pub pubkey_sec1: [u8; 33],
}
```

The sidecar carries the full active set for recoverability. The account leaf carries `keys_digest` for qMDB/account proofs. Verification must recompute `keys_digest` from every sidecar key set and compare it to the corresponding `AccountSnapshot.keys_digest`. If an account has no key-set sidecar entry, it is treated as an empty key set.

This avoids a separate `acct_keys/{account_id}` typed leaf in v1. The full key set is authenticated by the account leaf digest, while the canonical witness payload carries enough data for replacement operators to rebuild the registry.

### Proven key operations

Use system events, not a disconnected operation family, for the consensus-visible key mutations. Key ops are account/system state changes like deposits and withdrawals, and the existing `system_events -> events_root -> witness_root/DA` path is the right public audit trail.

Add system event variants:

```rust
KeyRegistered {
    account_id,
    signer_pubkey_sec1,
    signer_auth_scheme,
    new_pubkey_sec1,
    new_auth_scheme,
    pre_keys_digest,
    pre_events_digest,
    expires_at_height,
    authorization, // raw signature or WebAuthn assertion envelope
}

KeyRevoked {
    account_id,
    signer_pubkey_sec1,
    signer_auth_scheme,
    target_pubkey_sec1,
    pre_keys_digest,
    pre_events_digest,
    expires_at_height,
    authorization,
}
```

If event bloat becomes a concern, split public event details from private authorization witnesses, but keep a one-to-one canonical binding. The simpler first design is to carry authorization in the system event.

Canonical signed bytes for key ops should be state-bound, not rely on today's unproven `last_nonce`:

```text
"sybil/signing/account-key-op/v1"
|| genesis_hash[32]
|| account_id:u64le
|| op:u8                 // 0 register, 1 revoke
|| target_auth_scheme:u8 // register only; 0 for revoke or omitted by op-specific schema
|| target_pubkey_sec1[33]
|| pre_keys_digest[32]
|| pre_events_digest[32]
|| expires_at_height:u64le
```

The `pre_events_digest` binding is the replay guard. Key ops must update `events_digest`, so the same authorization cannot be replayed after the first accepted mutation unless BLAKE3 is broken. If the orchestrator dislikes state-bound signatures, add a proven `key_op_nonce` or `auth_nonce` to the account leaf. Do not rely on the existing sequencer `last_nonce`; it is not in `AccountSnapshot`.

Guest/verifier checks for each key op:

1. Process key events in witness order, before fills, using the pre key sidecar and pre account snapshots as the working state.
2. `pre_keys_digest` equals the working key digest for `account_id`.
3. `pre_events_digest` equals the working account `events_digest`.
4. The signer pubkey exists in the working active key set for `account_id`, with the stated auth scheme.
5. Raw P256 signatures verify directly; WebAuthn assertions verify the same challenge discipline as the API (`base64url(SHA-256(canonical_key_op_bytes))`) against the registered P256 key.
6. `expires_at_height >= witness.header.height`.
7. Register: target key is well-formed, not already active for any account, and is inserted.
8. Revoke: target key exists for this account and removal leaves at least one active signing key.
9. Update `keys_digest` from the new working set and update `events_digest` using a new canonical key-op event digest encoding.
10. After all system events, the derived account `keys_digest` and `events_digest` match `post_system_state`; after fills, `post_state.keys_digest` remains unchanged from `post_system_state` unless later key ops are ever allowed after fills.
11. The post key sidecar matches the derived working key registry and every post account's `keys_digest`.

### In-guest P256 verification — OpenVM ECC extension (Valery, 2026-07-06)

Check 5 is **new crypto in the proving path**. Today the guest verifies **no
signatures at all** — order/cancel/withdrawal/key-op auth all happens at the
API/actor boundary, and the guest only proves the state transition
(`sha2` is the sole crypto extension: `zk/openvm-guest/openvm.toml` enables
`rv32i`, `rv32m`, `io`, `sha2` and nothing else). Proving a P256 ECDSA
verification with a **pure-Rust `p256` crate inside the zkVM would be
enormously expensive** (field arithmetic unrolled into RISC-V cycles).

**Requirement: use OpenVM's accelerated P256.** Enable OpenVM's ECC extension
configured for **secp256r1 / P-256** (short-Weierstrass) plus the modular-
arithmetic extension it depends on, and verify key-op signatures through the
OpenVM ECDSA guest path (the patched/accelerated `p256`), NOT a soft
implementation. This is the same primitive the SYB-32 escape-claim guest needs
(it verifies the claimant's P256 signature in-guest) — do them on one ECC
integration.

**Consequence — this moves `app_vm_commit`, not just `app_exe_commit`.**
Enabling a new VM extension changes the VM configuration, so this is the first
`app_vm_commit` change since `0x0026ab66…`. That is fine — it's a deliberate
consensus change riding the fresh-genesis window (§ Migration) — but it is a
*deeper* commitment move than the source-only exe repins we have been doing,
and SYB-228's reproducibility caveats (untracked `agg_prefix.pk`, build-path
dependence) apply to the repin. Budget proving-cost measurement for the ECDSA
verification: it is per-key-op, so batches of key registrations in one block
multiply it.

### First key and account creation

Today account creation is service/operator driven and key registration is a separate public unsigned call. The witness-visible account creation event has only account id and initial balance: `crates/matching-sequencer/src/sequencer.rs:1720-1730`, converted to `SystemEventWitness::CreateAccount { account_id, initial_balance }` at `crates/matching-sequencer/src/sequencer.rs:573-581`.

Do not keep the current unauthenticated first-key path for production. Add initial keys to account creation:

```rust
CreateAccount {
    account_id,
    initial_balance,
    initial_keys: Vec<AccountSigningKeySnapshot>,
}
```

For service-created accounts, the service/operator is the authority for assigning the first key. For future L1 deposit-created accounts, the first key should be bound to the L1 deposit/account-key derivation in that flow. Any account with user funds should have at least one active key at creation. MINT and internal system accounts may use the empty key digest and be excluded from user escape claims.

Subsequent key registration/revocation must be signed by an existing active key. Self-revocation is allowed only when another key remains, preserving the current last-key lockout.

### Escape claim impact

The escape-claim spec explicitly needs key binding and currently identifies the gap: `design/escape-claim-guest.md:47-59`. With `keys_digest` in `acct/{A}`, the escape guest can take:

- qMDB proof for `acct/{A}`;
- qMDB proof or exclusion proof for `acct_resv/{A}`;
- claimed key set or claimed signer record plus enough sibling key records to recompute `keys_digest`;
- raw P256 signature or WebAuthn assertion over escape-claim canonical bytes.

The guest verifies the account leaf, recomputes `keys_digest`, proves the signer is in that committed set, verifies the claim authorization, and computes withdrawable cash. For WebAuthn-only accounts, the escape guest must support WebAuthn assertion verification or those accounts need a raw backup key.

## Blast Radius

| Area | Required change | Evidence / reason |
|---|---|---|
| `AccountSnapshot` | Add `keys_digest` field with serde default only for transitional decoding if needed. | Current fields end at `events_digest`: `crates/sybil-verifier/src/types.rs:198-207`. |
| State leaf encoding | Append `keys_digest` to `append_account_fields`; every account leaf and state root changes. | Account field encoder is `crates/sybil-verifier/src/snapshot_schema.rs:28-34`; state account domain is `crates/sybil-verifier/src/snapshot_schema.rs:159-162`. |
| Witness format | Bump v3 to v4; account decoder/encoder reads/writes `keys_digest`; add key sidecar and key system events. | Version is hard-coded v3 at `crates/sybil-verifier/src/witness_schema.rs:27`; unknown versions are rejected at `crates/sybil-verifier/src/witness_schema.rs:63-68`. |
| Canonical decoder | Update `decode_canonical_witness_bytes` and NonCanonical round trip tests. | Decoder reads account fields at `crates/sybil-verifier/src/witness_schema.rs:614-622` and re-encodes for canonicality at `crates/sybil-verifier/src/witness_schema.rs:131-133`. |
| Event schema | Add key-op system event tags and event digest encodings. | Current tags 0-6 are assigned in `crates/sybil-verifier/src/event_schema.rs:29-124`. |
| Verifier | Verify key sidecar digest consistency; verify key-op transitions and authorizations; extend system-event replay to check `events_digest` and `keys_digest`. | Current settlement compares only balances/positions: `crates/sybil-verifier/src/settlement.rs:381-448`. |
| Guest | Same checks as native verifier; add P256/WebAuthn verification for key ops; public input hash unchanged unless public input shape changes, but guest commitment changes. | Guest calls verifier layers at `crates/sybil-zk/src/lib.rs:306-325`; witness bytes feed `witness_root` at `crates/sybil-zk/src/lib.rs:346-370`. |
| Sequencer account model | Add `Account.keys_digest` or equivalent cached field; update it on account creation/key ops; snapshot it in `snapshot_account`. | `Account` has `events_digest` but no key digest: `crates/matching-sequencer/src/account.rs:73-98`; snapshot currently emits no key field: `crates/matching-sequencer/src/canonical_state.rs:86-97`. |
| Sequencer key registry | Stage key ops as pending system events; maintain live registry for immediate auth; derive pre/post key sidecars. | Current registry mutates outside system events: `crates/matching-sequencer/src/sequencer.rs:1478-1529`. |
| API | Replace public unsigned key registration with signed existing-key registration; add create-account-with-initial-keys service path; keep revocation shape but update canonical bytes. | Current register request has no signer/signature: `crates/sybil-api-types/src/request.rs:134-160`. |
| Store | Persist key sidecar in witness; keep redb pubkey tables as an operational cache derived from committed key state; WAL key ops until block commit. | Current pubkey tables are outside state root: `crates/matching-sequencer/src/store.rs:125-134`. |
| Import/recovery | Rebuild `pubkey_registry` from witness key sidecar, not empty. Validate every key set against account `keys_digest`. | Current witness import returns `pubkey_registry: HashMap::new()`: `crates/matching-sequencer/src/store.rs:3470-3480`. |
| Docs/runbooks | Update escape-claim, operator replacement, P256 auth, state root schema, block witness, and devnet redeploy docs. | Operator replacement already notes import resets nonces and signed submissions must be re-signed: `docs/architecture/Operator Replacement.md:92-97`. |

## Migration and Rollout

Recommended rollout is pre-redeploy/fresh genesis. The repo is still in early dev, and this changes the account leaf schema, witness schema, guest binary, and accepted roots. Existing roots cannot support escape-claim key binding because their account leaves do not contain `keys_digest`.

Fresh genesis path:

1. Implement v4 commitments and guest.
2. Recreate genesis from an operator-controlled snapshot that includes full active key sets.
3. Initialize every funded user account with at least one key.
4. Repin the OpenVM guest commitment and deploy against the new genesis/root series.

Post-deploy migration path, if forced:

1. Freeze writes.
2. Read current redb `pubkey_registry`, `pubkey_auth_schemes`, and `pubkey_meta`.
3. Build full key-set sidecar, compute `keys_digest` per account, and rewrite every account leaf.
4. Produce a migration block or synthetic genesis root that all verifiers treat as a hard version boundary.
5. Invalidate old escape-claim targets for key binding; old roots remain non-escapable by signer set.

This is not just a store migration. `state_root` changes for every account, and old v3 witnesses decode differently from v4. The design/witness precedent says canonical witness changes force a guest repin and fresh genesis in devnet: `design/witness-schema-v2.md:426-431`.

SYB-224 interaction: `/home/anonymous/sybil-ws2/REPORT-SYB224.md` did not exist at the time of this report. Abstractly, SYB-224 and SYB-225 should be batched. SYB-224 introduces `genesis_hash` into order/cancel canonical bytes; key-op canonical bytes and escape-claim canonical bytes should follow the same domain discipline. Operator replacement docs already plan `genesis_hash` as the next consensus batch: `docs/architecture/Operator Replacement.md:181-184`.

## Effort Estimate by Increment

| Increment | Scope | Estimate |
|---|---|---|
| 1. Commitment types and encoders | `keys_digest` helper, snapshot structs, state/witness schema v4, golden vectors, decoder tests. | 1-2 days |
| 2. Sequencer state and sidecar | Account cached digest, key sidecar construction, system-event staging, registry/cache consistency. | 2-3 days |
| 3. Proven key-op verifier | Native verifier transition checks, digest checks, global uniqueness, first-key account creation rules. | 2-3 days |
| 4. Guest support | Port checks into guest path; raw P256 verification; WebAuthn support if in scope. | Raw P256: 2-3 days; WebAuthn: +3-5 days |
| 5. API and signing bytes | Signed register-key endpoint, create-account initial keys, updated clients/tests, deprecate unsigned registration. | 1-2 days |
| 6. Store/import/recovery | Persist/import full key sidecar, rebuild registry, update restore drill. | 1-2 days |
| 7. Rollout/docs | Architecture notes, escape-claim update, operator replacement runbook, adapter repin docs. | 1-2 days |

Raw-P256-only path is roughly 1.5 weeks. Full WebAuthn escape/key-op verification is closer to 2-3 weeks because the guest must verify WebAuthn assertion envelopes, not just P256 signatures.

## Open Questions for Orchestrator

1. Should existing `KeyScope` become consensus authorization, or remain cosmetic? Recommendation: keep it out of v1 and introduce a real `capability_mask` later if agent keys should not be able to withdraw/escape.
2. Should escape claims support WebAuthn assertions in the first guest? If not, WebAuthn-only accounts need a raw backup key before escape mode is credible.
3. Accept state-bound key-op signatures using `pre_keys_digest + pre_events_digest`, or add a proven `auth_nonce`/`key_op_nonce` to account leaves? Recommendation: state-bound signatures avoid adding nonce state and solve replay for key-set cycles as long as key ops update `events_digest`.
4. Should key-op authorization be included directly in `SystemEventWitness`, or split into public key-op events plus private authorization witnesses? Recommendation: direct inclusion first; key ops are rare and auditability matters.
5. Should zero-key user accounts be forbidden once v4 is live? Recommendation: yes for funded/trading accounts; only MINT/internal/dev accounts may use the empty key digest.
6. Should there be a separate `acct_keys/{id}` typed leaf in addition to `acct/{id}.keys_digest`? Recommendation: no for v1. It duplicates state and increases proof cost; the digest in `acct/` plus full witness sidecar gives both escape proofs and recoverability.
