---
tags: [design, validity, keys, witness, escape]
layer: core
status: ratified 2026-07-10
last_verified: 2026-07-10
revision: 3 (rev 2 = post adversarial review — see witness-v5-design-review.md, all BLOCKER/MAJOR findings addressed, revised sections marked ⟲; rev 3 = renumbered v5→v6 and re-verified tag allocations after SYB-253/273 landed and took wire v5 + tags 7–9)
---

# Witness v6 — proven key ops + the non-trade transition audit (SYB-270 / ZK-1)

*Design for [SYB-270](https://linear.app/sybilmarket/issue/SYB-270) (ZK-1, HIGH
soundness), completing the [SYB-225](https://linear.app/sybilmarket/issue/SYB-225)
key-commitment work and closing the last unconstrained account-field transitions.
One validity-schema move (wire `5 → 6`), one guest repin, one fresh genesis.*

> [!IMPORTANT]
> **Landed implementation boundary (2026-07-10).** The v6 implementation lands
> the authenticated pre-state proof, complete key-op witnessing, post-key-set
> opening, reverse/forward key-set replay, global key uniqueness, digest v2,
> and the non-trade value-field audit. Three parts of this design remain
> deliberately deferred and are not claims made by the v6 guest:
>
> 1. **Key-op signature verification in the guest (§3a).** P-256/WebAuthn guest
>    acceleration is a separate ticket and was explicitly out of scope for
>    SYB-270. The witness carries the complete authorization envelope, enforces
>    byte caps, and proves that the declared signer is an active key with the
>    matching authentication scheme at that point in the running key set. Raw
>    P-256 and WebAuthn signatures continue to be verified at API/sequencer
>    admission, but the guest does not cryptographically re-verify them. The
>    state-bound canonical key-op helpers specified in §2a are landed for that
>    follow-up; the live API remains on its nonce-bearing admission messages
>    until the guest-crypto/API migration is made atomically.
> 2. **The D4 funded-account key floor.** v6 makes the first key part of a
>    pending `CreateAccount.initial_keys` event and rejects a first-key bootstrap
>    after the creation block, but does not yet reject every funded zero-key
>    account protocol-wide. Existing internal/dev account construction still
>    depends on empty key sets. Enforcing the floor and migrating genesis data
>    remain landing-time work.
> 3. **Genesis pre-state proof.** Every non-genesis block supplies and verifies a
>    real qMDB proof for `pre_state + pre_state_sidecar` against the previous
>    state root. Height 1 has no previous header/root in the current protocol,
>    so its pre-proof is canonically empty and genesis remains the externally
>    trusted boundary. This replaces the earlier text below that described a
>    synthetic real proof at height 1.

> **Ground truth correction to the roadmap framing.** Roadmap lane 5 described this
> as "the ZK-1 + witness-schema-v2 combined move." The witness-v2 program is in fact
> already landed on main: wire **v3** (pre_state_sidecar + deposit frontier,
> 2026-07-06, `9771218c`+`80acf067`), the unproven `DerivedViewSidecar` (Increment 0,
> lives in `crates/matching-sequencer/src/{analytics.rs,block.rs}`), and wire **v4**
> (`keys_digest` in `AccountSnapshot`), and — landed 2026-07-10 (`060dec4f`),
> after this design's first revisions — wire **v5** (SYB-253/273 withdrawal
> refund/prune events + committed observed L1 height,
> `WITNESS_FORMAT_VERSION = 5`). The only remaining witness-v2
> item is the deferred openings/delta witness (testnet-scale work, explicitly out of
> scope). **This document designs the next and only pending validity move: v6.**

Companion reads: `design/account-keys-digest.md` (the SYB-225 survey — flow
ground truth), `design/keys-and-escape-ratification.md` (**D2–D8 RATIFIED
2026-07-07**), ADR-0009/0011 (fresh genesis is free, break clean), ADR-0012
(encrypted DA), ADR-0013 (escape values positions at last clearing price),
**ADR-0014 (WebAuthn-first, verified in-guest — reverses ratification D0)**,
`design/openvm-p256-integration.md` (in-guest P-256 recipe),
`design/witness-schema-v2.md` (the landed v3 design), SYB-269 review umbrella,
`witness-v5-design-review.md` (the adversarial review this revision answers).

---

## 1. Problem statement and threat model ⟲ *(revised: proven-path description corrected per BLOCKER-1; variant count per MINOR-1)*

`keys_digest` is committed into every `acct/{id}` state leaf (v4) and the digest
formula is shared between host and verifier
(`crates/sybil-verifier/src/account_keys.rs`,
`crates/matching-sequencer/src/digest.rs::refresh_account_keys_digest`). But key
registration and revocation remain **entirely outside the proven state
transition**: `SystemEventWitness` (`crates/sybil-verifier/src/types.rs`)
has ten variants — `CreateAccount`(tag 0), `Deposit`(1), `L1Deposit`(2),
`WithdrawalCreated`(3), `MarketResolved`(4), `OrderCancelled`(5),
`MarketGroupExtended`(6), `WithdrawalRefunded`(7), `WithdrawalFinalized`(8),
`L1BlockObserved`(9) — and **no key op**. Key mutations ride the
control-plane WAL and the redb pubkey tables only.

What the **proven path** actually constrains about account leaves today — and
this is narrower than the native test path, which matters:

- The guest entrypoint `verify_state_transition_input`
  (`crates/sybil-zk/src/lib.rs:305-326`) calls exactly:
  `verify_public_input_binding`, `verify_qmdb_state_root`, then
  `verify_match`, `verify_settlement`, `verify_orders`, `verify_sidecar`.
- `verify_qmdb_state_root` (`crates/sybil-zk/src/guest_commitments.rs:136-168`)
  proves the leaves of **`post_state` + `state_sidecar`** against
  **`new_state_root`** — the *only* qMDB proof in `StateTransitionGuestInput`
  (`crates/sybil-zk/src/lib.rs:59-65`).
- **The guest never authenticates `pre_state` / `pre_state_sidecar`.**
  `verify_block` (`crates/sybil-verifier/src/block.rs:59`, pre-root recompute at
  `:94-95`) is the only code that checks the pre snapshot against
  `previous_header.state_root`, and it runs **only** in the native/test path
  (`verify_full`, `lib.rs:93/111`) — never in the guest.
  `verify_public_input_binding` checks the 32-byte header field
  `previous_state_root == witness.previous_header.state_root` but never binds the
  pre-state *leaves* to that root. (The deposit leg is the one anchored
  exception: post `deposit_root`/`deposit_count` are public inputs checked
  on-chain against the vault tree.)
- `verify_settlement` derives **balances and positions only**, from
  `post_system_state + fills` (`crates/sybil-verifier/src/settlement.rs:381-…`),
  where `post_system_state` and `pre_state` are themselves unauthenticated
  in-guest. Nothing re-derives `events_digest` or `total_deposited`; nothing
  checks the `pre_state → post_system_state` step.

**ZK-1 attack (sharpened):** the sequencer does not even need to "swap a digest
in a transition" — it supplies a *fabricated* `pre_state` with arbitrary
`keys_digest` values, folds zero key events, and commits any post key set it
likes; only `post_state` is proof-checked, against a root the sequencer itself
produced. Once accepted, block N+1 chains from that root, so **one bad block
launders the forged key set permanently**. When the SYB-32 escape hatch ships,
the committed key set authorizes L1 claims valued per ADR-0013; a laundered
digest is a direct theft path with no on-chain tell.

**The generalization (the real ZK-1 sentence):** the guest must constrain
**every non-trade account-field transition**, not only `keys_digest` — today
`events_digest` and `total_deposited` are equally free (§4), and the system-event
application step is unchecked. And per the above, *no* pre→post fold is worth
anything until the guest first gets an **authenticated pre-state**. That keystone
is therefore a first-class v6 deliverable (§2e), not an assumed given.

---

## 2. The v6 schema

All ratified decisions from `keys-and-escape-ratification.md` are taken as given:
D1 (reserve `capability_mask`), D2 (state-bound replay guard — component
semantics revised in §2a per review MAJOR-1), D3 (inline auth in the event), D4
(funded accounts must hold ≥1 key), D5 (digest-only in the account leaf), D6
(`initial_keys` at creation). D0 is governed by ADR-0014: **WebAuthn-first,
verified in-guest**; raw-P256 remains for agent keys.

### 2a. New system events (host + witness mirror) ⟲ *(revised: replay-guard binding semantics per MAJOR-1; KeyRevoked carries the full record; tag numbers per MINOR-1)*

`SystemEvent` (`crates/matching-sequencer/src/system_event.rs`) and
`SystemEventWitness` (`crates/sybil-verifier/src/types.rs`) gain:

```rust
KeyRegistered {                // events_root tag 10
    account_id: u64,
    key: KeyRecord,
    authorization: KeyOpAuth,  // inline (D3)
},
KeyRevoked {                   // events_root tag 11
    account_id: u64,
    key: KeyRecord,            // FULL record, not just the pubkey — needed for the
                               // reverse fold in §3 and for audit completeness
    authorization: KeyOpAuth,
},
```

and `CreateAccount` gains `initial_keys: Vec<KeyRecord>` (D6). The
service/operator is the first-key authority for service-created accounts;
`initial_keys` may be empty only for MINT/internal accounts (D4 — enforced at the
fold, §3). Tag numbering assumes lane A's `WithdrawalRefunded` takes
**tag 7** (SYB-253, in flight); confirm against landed main before increment 1.

```rust
pub struct KeyRecord {
    pub auth_scheme: u8,        // 0 = raw_p256, 1 = webauthn
    pub pubkey_sec1: [u8; 33],  // compressed SEC1 P-256
    pub capability_mask: u32,   // reserved (D1): all-ones = full authority (today's semantics)
}

pub enum KeyOpAuth {
    RawP256  { signer_pubkey: [u8; 33], signature: [u8; 64] },
    WebAuthn { signer_pubkey: [u8; 33],
               authenticator_data: Vec<u8>,   // ≤ 512 B (§7 Q5)
               client_data_json: Vec<u8>,     // ≤ 2 KB  (§7 Q5)
               signature: [u8; 64] },
}
```

**Canonical key-op bytes** (what the signature covers) follow the SYB-224
domain discipline — led by `genesis_hash[32]`:

```
"sybil/keyop/register/v1" || genesis_hash[32] || account_id:u64le
  || key_record (auth_scheme:u8 || pubkey_sec1[33] || capability_mask:u32le)
  || bound_keys_digest[32] || bound_events_digest[32]
("sybil/keyop/revoke/v1" analogous, over the full target key_record)
```

**Binding semantics (revised — review MAJOR-1).** The naïve reading of D2
("bind the pre-application digests") is racy: `events_digest` folds on *every*
account event (deposits, L1 credits, fills, resolutions, cancels — see the
`digest.rs` encoders), so a user cannot pre-sign against a value that moves with
unrelated intra-block activity. And binding `keys_digest` **alone** is unsound:
key sets can *return* to a prior value (register K → revoke K), so an old signed
`register K` authorization would re-verify after the user deliberately revoked K
— a replay that re-installs a possibly-compromised key. The v6 rule keeps both
components and makes them predictable instead:

1. **Key-op events fold first.** Within a block's `system_events`, all key ops
   for an account precede every other event touching that account; the sequencer
   orders them so and **the guest asserts this ordering** (§3).
2. `bound_keys_digest`/`bound_events_digest` are the account's values **at block
   start** (= the last committed block's post values — stable, queryable via the
   API) for the account's *first* key op in a block; each *subsequent* key op in
   the same block binds to the **running** values after the previous op (the
   client computes these deterministically, since only its own ops intervene).

Replay safety: every applied key op advances the account's `events_digest`, and
the BLAKE3 chain never revisits a value, so no accepted authorization can ever
re-verify — including the register-revoke-register case that defeats
`keys_digest`-only binding. Liveness: the bound values move only when a
*committed* block touches the account between signing and admission; the API
rejects the stale op at admission (validate-before-WAL, the lane-C pattern) with
a 409 and the client re-signs — a rare, visible retry instead of an
unpredictable intra-block race.

### 2b. Digest formula v2

`key_record` now carries `capability_mask` (D1), so the digest domain bumps:

```
keys_digest(account_id, active_keys) =
  SHA256("sybil/state/account-keys-digest/v2"
         || account_id:u64le || key_count:u64le
         || concat(sorted key_record bytes))
key_record = auth_scheme:u8 || pubkey_sec1[33] || capability_mask:u32le
```

Sorted by `(pubkey_sec1, auth_scheme)` as today
(`crates/sybil-verifier/src/account_keys.rs:13`). Empty set stays the
domain/count hash, never `[0;32]`. All existing v1-digest call sites move —
mechanical, and golden vectors pin the change.

### 2c. Account-keys witness section (the recoverability half) ⟲ *(revised: moved out of the state sidecar; single copy; reverse-fold derivation; caps per MINOR-4)*

Per the SYB-225 survey's standing requirement: the digest alone lets you *verify*
a key set but not *recover* one. v6 adds **one** full key-set section to the
witness itself — **not** to `StateSidecarSnapshot` and **not** as state leaves,
keeping D5 (digest-only in the root) intact:

```rust
// BlockWitness gains:
pub account_keys: Vec<(u64 /* account_id */, Vec<KeyRecord>)>,
// Active key sets at block END, sorted by id, records sorted as in the digest.
// Encoded in canonical_witness_bytes under "sybil/witness/account-keys"
// → committed by witness_root and da_commitment, NOT by the state root.
```

Why this shape (changes from revision 1, which duplicated full sets into both
sidecars):

- **Authenticity comes from the weld, not from state leaves:** the guest asserts
  `digest(id, account_keys[id]) == post_state[id].keys_digest` for every account;
  post leaves are proof-checked against `new_state_root` (§2e gives pre the same
  footing). A set that welds to a committed digest is authentic up to SHA-256
  collision — no second leaf, no root growth (D5).
- **Pre sets are derived, not carried:** for accounts touched by key ops this
  block, the guest *reverse-folds* the ops over the post set (un-register =
  remove the record; un-revoke = re-insert the full `KeyRecord` — this is why
  `KeyRevoked` carries the whole record) to obtain the block-start set, then
  welds it against the **pre**-leaf `keys_digest` (§2e-authenticated). Untouched
  accounts need no set at all for the transition check: the guest asserts
  `post.keys_digest == pre.keys_digest` leaf-to-leaf. One copy of the key
  universe per block, zero duplication.
- **Recoverability:** the witness (and thus the DA payload) carries the full
  active key universe every block, so `import_witness_genesis` can rebuild
  `pubkey_registry` — which it explicitly does not today
  (`crates/matching-sequencer/src/store/import.rs:274` sets
  `pubkey_registry: HashMap::new()`). The import read-back is an increment-2
  deliverable (§5).

**Caps (review MINOR-4), enforced at admission and asserted in the fold:**
`MAX_KEYS_PER_ACCOUNT = 16`, `MAX_KEY_OPS_PER_BLOCK = 64` (recommended values,
§7 Q5). Size accounting, honestly: a `KeyRecord` is 38 B; the section costs
`38 B × total keys + 16 B × accounts`, once. At 1 000 accounts × 2 keys ≈ ~92 KB
— about a fifth of the tri-state account snapshots' footprint, bounded by the
per-account cap, and it shrinks with everything else when the deferred openings
witness lands.

### 2d. Event tags and wire version ⟲ *(revised per MINOR-1/MINOR-2/MINOR-5)*

Two **independent** tag namespaces move; the first revision conflated them:

1. **`events_root` leaves** (`crates/sybil-verifier/src/event_schema.rs`,
   `system_event_leaf_value`): tags 0–9 in use on landed main —
   `MarketGroupExtended = 6`, and lane A (SYB-253/273) took **7**
   (`WithdrawalRefunded`), **8** (`WithdrawalFinalized`), **9**
   (`L1BlockObserved`). `KeyRegistered` → **10**, `KeyRevoked` → **11**.
2. **Per-account `events_digest` arms**
   (`crates/matching-sequencer/src/digest.rs`): 0x01–0x09 in use on landed main
   (`fill=0x01 … withdrawal_created=0x07, order_cancelled=0x08`, lane A's
   refund arm = **0x09**; `WithdrawalFinalized`/`L1BlockObserved` fold no
   per-account arm). `key_registered` → **0x0a**, `key_revoked` → **0x0b**.
   §3b's verifier-side mirror of this encoder must reproduce the scheme exactly,
   including the asymmetry that `mint (0x05)` folds into MINT's `events_digest`
   but has **no** `events_root` leaf.

Both allocations confirmed against landed main (`2793d35c`, 2026-07-10 — rev 3).

`WITNESS_FORMAT_VERSION` bumps `5 → 6` **at increment 1**, together with every
encoder change and regenerated goldens — not at the end. Rationale (review
MINOR-5): the version byte lives inside fingerprinted verifier source, so
flipping it moves the source-fingerprint gate exactly when the encoders move
(§5); holding "v6 encoders under a v5 byte" would suspend the decoder's
`UnknownVersion` tripwire (`witness_schema.rs`) for the whole transition.
Main simply holds v6 code that **is not deployed** until increment 5's fresh
genesis; the devnet keeps running the v5 binary meanwhile. There is no
mixed-format window on any running system.

### 2e. The keystone: pre-state authentication in-guest ⟲ *(NEW — review BLOCKER-1)*

`StateTransitionGuestInput` gains a second proof:

```rust
pub struct StateTransitionGuestInput {
    pub public_inputs: StateTransitionPublicInputs,
    pub witness: BlockWitness,
    pub da_provider_refs: Vec<Vec<u8>>,
    pub state_root_proof: QmdbStateRootProof,       // post leaves vs new_state_root (existing)
    pub pre_state_root_proof: QmdbStateRootProof,   // NEW: pre leaves vs previous_state_root
}
```

`verify_state_transition_input` gains, immediately after the existing
`verify_qmdb_state_root` call:

```rust
verify_qmdb_state_root_for(
    &input.public_inputs.previous_state_root,
    state_schema::state_root_leaves(&witness.pre_state, &witness.pre_state_sidecar),
    &input.pre_state_root_proof,
)?;
```

— the same per-leaf inclusion + next-key exact-keyspace cover
(`guest_commitments.rs:136-168`) the post proof already enforces, over the
**pre** snapshot against the **previous** root (already a public input,
`lib.rs:543`). The prover generates it from the previous block's committed qMDB
state exactly as it generates the post proof today (host-side witgen,
`crates/sybil-prover`). Every non-genesis block proves against the previous
committed root. Height 1 has no previous header/root in the current protocol;
its canonical pre-proof is empty and its state is the externally trusted
genesis boundary.

**Cost, stated plainly:**

- *Input bytes:* the proof section roughly **doubles**. Per leaf:
  ~32 B activity chunk + range digests/peaks (≈ 2·log₂N × 32 B) + ~90 B fixed +
  next_key ⇒ ~0.5–0.7 KB/leaf. At devnet scale (~100–200 leaves) the pre proof
  adds **~60–130 KB** per block, matching the existing post proof.
- *Guest cycles:* the state-root leg roughly **×2** (a second full-keyspace
  verification). This is exactly the "second full root" tax the landed v3 design
  already forecast in `design/witness-schema-v2.md` §5 — that document *designed*
  pre-root authentication, but only the native `verify_block` half was ever
  wired; the guest half ships here. The deferred openings/delta witness
  (witness-v2 Increment 2, testnet-scale) is what later removes this tax; v6
  makes that migration no harder (the proof is a self-contained input field).
- *Side benefit:* v3's `verify_sidecar` pre→post transition checks (resting
  orders, withdrawals, markets, groups) currently also start from an
  unauthenticated pre sidecar in-guest; this proof retroactively puts them on
  authenticated footing at no extra design cost.

Without §2e, **every** fold in §3/§3b compares two attacker-chosen values and
the design closes nothing. With it, the pre end of every fold is as strong as
the post end.

---

## 3. Guest re-derivation: the fold ⟲ *(revised: authenticated pre-state prerequisite; unified uniqueness incl. CreateAccount per BLOCKER-2; Phase-0 ordering; caps)*

New verifier stage `verify_key_transitions` (native in
`crates/sybil-verifier/src/`, executed by the guest inside
`verify_state_transition_input` **after** both root proofs of §2e):

```
// Inputs already authenticated at this point:
//   pre_state / pre_state_sidecar leaves  vs previous_state_root   (§2e — keystone)
//   post_state / state_sidecar leaves     vs new_state_root        (existing)
// witness.account_keys is welded below; it needs no leaf of its own.

post_keys: Map<id, SortedSet<KeyRecord>> := witness.account_keys
assert |post_keys[id]| ≤ MAX_KEYS_PER_ACCOUNT ∀ id
assert all pubkeys across ALL of post_keys are pairwise distinct        // global uniqueness, full universe
assert ∀ acct ∈ post_state:  acct.keys_digest == digest(id, post_keys[id] or ∅)   // post weld

key_ops := key events in witness.system_events, in order
assert |key_ops| ≤ MAX_KEY_OPS_PER_BLOCK
assert ordering: for every account, its key ops precede every other system event
       touching that account (§2a rule 1)

// Reverse fold: recover block-start sets for touched accounts
pre_keys := post_keys restricted to touched accounts
for ev in key_ops REVERSED:
    KeyRegistered { id, key }          => assert key ∈ pre_keys[id];  pre_keys[id] -= key
    KeyRevoked    { id, key }          => assert key ∉ pre_keys[id];  pre_keys[id] += key
    CreateAccount { id, initial_keys } => assert pre_keys[id] == initial_keys; delete pre_keys[id]

assert ∀ touched acct: pre_state[acct].keys_digest == digest(id, pre_keys[id])    // pre weld (authenticated!)
assert ∀ untouched acct ∈ post_state: acct.keys_digest == pre_state[acct].keys_digest
       (accounts created this block are by definition touched)

// Forward semantics: replay ops over the derived pre sets with a RUNNING map
running := pre_keys
running_pubkeys := all pubkeys in post_keys, minus this block's net additions,
                   plus this block's net removals   // = the authenticated block-start universe
for ev in key_ops (in order):
    CreateAccount { id, initial_keys, .. } =>
        assert id ∉ running;  assert initial_keys pairwise distinct
        assert ∀ k ∈ initial_keys: k.pubkey ∉ running_pubkeys                    // BLOCKER-2 fix:
        running[id] := initial_keys;  running_pubkeys += their pubkeys           // same uniqueness
    KeyRegistered { id, key, authorization } =>                                  // source for BOTH arms
        verify_keyop_auth(authorization, running[id], canonical_register_bytes(..., running digests))
        assert key.pubkey ∉ running_pubkeys
        running[id] += key;  running_pubkeys += key.pubkey
    KeyRevoked { id, key, authorization } =>
        verify_keyop_auth(authorization, running[id], canonical_revoke_bytes(...))
        assert key ∈ running[id]  and  |running[id]| > 1        // last-key lockout, as today
        running[id] -= key;  running_pubkeys -= key.pubkey

assert running == post_keys restricted to touched accounts                       // fold closes
// DEFERRED at v6 landing (see implementation boundary above):
funded-zero-key rule (D4): ∀ acct ∈ post_state with balance > 0 or positions ≠ ∅
    and id ∉ {MINT (= u64::MAX, never CreateAccount'd), internal}: post_keys[id] ≠ ∅
```

The reverse-then-forward structure means the *same running map* is the single
uniqueness authority for `initial_keys` and `KeyRegistered` alike (review
BLOCKER-2): a `CreateAccount` smuggling a victim's pubkey, a duplicate within
`initial_keys`, or a cross-event collision inside the batch all fail the same
assertions. Global uniqueness is checked over the **full post universe** (first
assert) *and* maintained incrementally through the fold, so it holds at every
intermediate state. Uniqueness is validity-relevant, not merely operational:
future L1-deposit-created accounts bind deposits to keys (D6), so a duplicated
pubkey is a deposit-routing ambiguity — i.e., money.

No key events for an account ⇒ its digest is provably unchanged
(leaf-to-leaf equality between two *authenticated* snapshots) — that single
property, now actually enforced end-to-end, kills the ZK-1 forgery. Fold cost is
O(total keys + key-ops) per block plus one digest recompute per account with a
key op; untouched accounts cost one 32-byte compare.

### 3a. `verify_keyop_auth` — ADR-0014 compliance ⟲ *(revised: rpIdHash, flags, escape-aware challenge extraction per MINOR-3)*

**Implementation status: deferred to the separate guest-P256 ticket.** v6
currently enforces only signer membership, authentication-scheme agreement, and
authorization-envelope caps in the guest; admission continues to perform the
cryptographic checks. The requirements below remain the normative follow-up,
not a property of the landed v6 circuit.

The signer must be a **current** member of the account's *running* key set (an
earlier op in the same block can authorize a later one). Two arms:

- **RawP256**: ECDSA-P256 verify of `SHA256(canonical_bytes)` under
  `signer_pubkey`, via OpenVM's accelerated secp256r1
  (ADR-0008, `design/openvm-p256-integration.md` — `verify_prehash` recipe).
- **WebAuthn** (primary UX per ADR-0014), verifying
  `signature` over `authenticator_data || SHA256(client_data_json)` under
  `signer_pubkey`, with **all** of:
  1. `challenge = base64url(SHA256(canonical_bytes))` extracted from the
     **actual** `client_data_json` bytes via a minimal RFC 8259 string parser:
     locate the top-level `"challenge"` member, **decode JSON string escapes**
     (`\/`, `\uXXXX`, `\"`, …) before comparison, so a crafted sibling field
     cannot spoof a naïve substring scan;
  2. top-level `"type"` member decodes to `webauthn.get`;
  3. **rpIdHash binding:** `authenticator_data[0..32] ==
     expected_rp_id_hash`, a genesis-pinned guest constant (it changes only with
     a deliberate RP migration, which is a repin event anyway) — without this,
     a passkey minted for *any* relying party could sign Sybil key ops;
  4. **flags:** `authenticator_data[32]` must have UP set, and UV set (key ops
     are security-critical; passkeys perform UV by default — §7 Q7 if this is
     too strict for some authenticators);
  5. size caps already enforced at admission re-asserted in-guest (§2a).

SHA-256 is already a guest extension
(`crates/sybil-zk/src/guest_commitments.rs:76-89` extern); the envelope
scanner is the only genuinely new guest parsing surface — keep it
allocation-light and fuzz it host-side against a real browser corpus (Chrome /
Safari / Firefox `clientDataJSON` samples, plus adversarial escape-sequence
cases).

The API keeps verifying signatures at admission exactly as now
(`crates/sybil-api/src/routes/accounts.rs`, `webauthn.rs`) — the guest check is
the soundness backstop, not a replacement for admission-time rejection.

**OpenVM 2.0.0 rider:** lane B (in flight) moves the toolchain to v2.0.0 final.
The P-256 integration recipe was validated on v2.0.0-beta.2; re-validate the
`p256` guest crate + ECC extension against 2.0.0 in the increment-4 lane, and
note that any change in extern/intrinsic mechanics lands there, not here.

### 3b. Completing the audit: events_digest, total_deposited, event application ⟲ *(revised: digest.rs namespace made explicit per MINOR-2; pre-proof dependency stated)*

Same move, no additional wire change (the inputs are already witnessed), and all
of it **depends on §2e** — each fold below starts from the now-authenticated
pre snapshot:

- **`events_digest`**: `sybil-verifier` gains a mirror of the
  `crates/matching-sequencer/src/digest.rs` per-account BLAKE3 chain — an
  encoder that does not exist verifier-side today. It must reproduce the
  `digest.rs` tag namespace exactly (§2d item 2: 0x01–0x08 landed, 0x09 refund,
  0x0a/0x0b key ops), including the `mint = 0x05` asymmetry (folds into MINT's
  digest, no `events_root` leaf). Check:
  `pre.events_digest ⟶ fold(this block's witnessed events, in system-then-fills
  order) ⟶ post.events_digest`, per account.
- **`total_deposited`**: monotone fold of `Deposit`/`L1Deposit` amounts over
  `pre.total_deposited`, asserted against post.
- **System-event application** (`pre_state → post_system_state`): assert the
  balance/position deltas of the system-event phase equal the witnessed events'
  effects (deposit and L1-deposit credits, withdrawal debits incl. lane A's
  refund credits, resolution payouts). This closes the currently-unchecked first
  hop; `verify_settlement` already constrains the second hop
  (`post_system_state → post_state` via fills) — and gains a sound starting
  point, since `post_system_state` is now pinned from both sides.
- **Account existence**: accounts in `post_state` but not `pre_state` must have
  a `CreateAccount` event; accounts never vanish (no deletion op exists; MINT is
  `u64::MAX`, inserted at genesis, never created by event).

With these, every field of `AccountSnapshot` — `id`, `balance`,
`total_deposited`, `positions`, `events_digest`, `keys_digest` — has a
witnessed, re-derived transition between two root-authenticated snapshots.

---

## 4. Constraint coverage table (the audit) ⟲ *(revised per NIT-1: every "after v6" claim is conditional on the §2e keystone; new uniqueness row)*

All "after v6" entries assume **both** §2e proofs verified in-guest; without the
pre proof, every row marked ⚑ reverts to unconstrained (review BLOCKER-1).

| Account field | Mutation source | Constrained today by | Constrained after v6 by |
|---|---|---|---|
| `balance` | fills (trade) | `verify_settlement` derivation vs post ✅ (pre end unauthenticated in-guest ⚠️) | same derivation, both ends authenticated ⚑✅ |
| `balance` | deposits / L1 deposits / withdrawals / refunds / resolutions | **nothing** — `post_system_state` on faith ❌ | §3b application check ⚑✅ |
| `positions` | fills (trade) | `verify_settlement` ✅ (same ⚠️) | same, authenticated ⚑✅ |
| `positions` | resolutions | **nothing** ❌ | §3b application check ⚑✅ |
| `total_deposited` | Deposit / L1Deposit | **nothing** ❌ | §3b monotone fold ⚑✅ |
| `events_digest` | every account event | **nothing** (events_root commits the list, not the per-account chain) ❌ | §3b BLAKE3 mirror fold ⚑✅ |
| `keys_digest` | register / revoke / create | **nothing** — ZK-1 ❌ | §3 fold + double weld ⚑✅ |
| account existence | CreateAccount | event witnessed; linkage unchecked ⚠️ | §3b existence rule ⚑✅ |
| global pubkey uniqueness | register / initial_keys | host `HashMap` only — not proven ❌ | §3 full-universe + running-map check ✅ |
| L1 deposit legitimacy | vault deposits | v3 frontier fold, anchored on-chain ✅ | unchanged ✅ |

Verdict, honestly stated: **after v6 — including the §2e pre-state proof — no
account field remains unconstrained.** The claim is exactly as strong as the
keystone: increments 1–2 alone (§5) deliver *no* soundness improvement; the
table's right column becomes true at the end of increment 4 and deployed at 5.
Residual trusted surface is operator-authority *policy* (service `Deposit` on
devnet, market administration) — witnessed and digest-folded, but authorized by
the single-operator trust model by design (ADR-0011).

---

## 5. Implementation increments (staged codex lanes) ⟲ *(revised: pre-proof deliverables added; gate accounting per MAJOR-3; export tool per MAJOR-2)*

Prereqs — **all landed 2026-07-10**: lanes A (SYB-253 — tags 7–9/0x09, refund
digest arm; `060dec4f`), C (SYB-271 — first-key preflight this fold's uniqueness
rule mirrors; `11987bd1`), and B (OpenVM 2.0.0; `2793d35c`).

**Gate accounting up front (three distinct artifacts — review MAJOR-3):**

| Artifact | Moves at |
|---|---|
| Source fingerprint (`guest.commitment.lock.json` `source_sha256`; CI `--check`) — closure = `sybil-zk`, `sybil-verifier`, `matching-engine`, `sybil-l1-protocol` (`scripts/zk-guest-fingerprint.sh:76`) | increments **1, 3, 4** — each ends with `--write` to re-green `--check`; the refreshed lock carries a new source hash over stale commit hashes, which the script tolerates by design until the rebuild |
| Built commitments (`app_exe_commit`/`app_vm_commit`, `sybil-openvm-guest.commit.json`, via `just openvm-commit`) | increment **5** only |
| On-chain pin (`OpenVmVerifierAdapter` constructor) + deployed wire format | increment **5** only (fresh genesis) |

Increment 2 touches only `matching-sequencer` / `sybil-api` / `sybil-prover` —
outside the closure; no gate moves.

1. **Types + digests + vectors** (ws2, codex high, ~½ day):
   `WITNESS_FORMAT_VERSION = 6` (§2d), `KeyRecord`, `KeyOpAuth`, both event
   variants + `initial_keys`, digest v2, canonical key-op bytes,
   `BlockWitness.account_keys` encoder, **`pre_state_root_proof` field on
   `StateTransitionGuestInput`** (type only), goldens regenerated. Gate: golden
   vectors + `cargo test -p sybil-verifier -p sybil-zk`; fingerprint `--write`.
2. **Sequencer + API end-to-end** (ws2/ws3, codex xhigh, ~1–1.5 days): key ops
   emit system events (validate → control-plane WAL → apply, the lane-C
   pattern); Phase-0 ordering + block-start binding rule (§2a); `initial_keys`
   through `create_account`; **atomic create-with-key API — the SYB-271
   residual — and retire the unsigned public first-key path for funded accounts
   (D4/D6)**; digest.rs key-op arms (0x0a/0x0b); `account_keys` witness
   assembly; **import read-back** (fix `import.rs:274`); **prover-side pre-proof
   generation** in the witgen path (`crates/sybil-prover` — outside the
   closure). Gate: crash_harness, restore, api integration.
3. **Verifier native folds** (ws2, codex xhigh, ~1 day): §2e verification
   routine (shared, root-parametric), §3 key fold, §3b events_digest mirror /
   total_deposited / application / existence checks; new `ViolationKind`s;
   red→green forgery tests (fabricated pre-state, digest swap, initial_keys
   smuggling a registered pubkey, replayed key-op incl.
   register-revoke-register, zero-key funded account, fabricated deposit
   credit, key-op after a same-account event). Fingerprint `--write`.
4. **Guest crypto + wiring** (main ws, codex xhigh, ~1–2 days): OpenVM P-256
   (re-validated on 2.0.0) + WebAuthn envelope verification (§3a items 1–5,
   incl. the escape-aware JSON scanner + host-side fuzz corpus) + both root
   proofs and all folds wired into `verify_state_transition_input`. Gate:
   `cargo test --workspace`, guest/host parity on shared vectors. Fingerprint
   `--write`.
5. **The move** (main ws only, orchestrator + codex, ~½–1 day): **genesis
   key-export tool** (new — review MAJOR-2: xtask/just target reading the redb
   `PUBKEY_REGISTRY`/`PUBKEY_AUTH_SCHEMES`/`PUBKEY_META` tables into genesis
   `account_keys` + `initial_keys`; no such path exists today), goldens
   re-pinned, `just openvm-commit` → `zk-guest-fingerprint.sh --write` +
   `--check`, adapter repin from `sybil-openvm-guest.commit.json`, **fresh
   genesis** with every funded account holding ≥1 key (D4). Rider: resolve
   SYB-228 first (agg-key determinism under OpenVM 2.0.0) — repin only from the
   main workspace.

Golden vectors move at increments 1 and 5; the **built** guest commitment and
the on-chain pin move only at 5. Increments 1–4 live on main undeployed — safe
because the devnet keeps running the v5 binary until the increment-5 redeploy,
not because of any ADR-0011 property (the earlier revision misattributed this).

---

## 6. Migration ⟲ *(revised per MAJOR-2)*

Fresh genesis per ADR-0009 as relaxed by ADR-0011 — no migration tooling beyond
the export tool below, no coexistence decoding, version byte is the tripwire.
Choreography (existing runbook `docs/runbooks/devnet-redeploy.md`): land
increment 5 → **export live key sets** with the increment-5 tool (redb pubkey
tables → genesis `account_keys` + per-account `initial_keys`; devnet accounts
are dev accounts, so carrying them is optional but recommended — it exercises
the same import path a replacement operator would use, end to end) → rebuild
guest, read `app_exe_commit`/`app_vm_commit` from
`zk/openvm-guest/openvm/release/sybil-openvm-guest.commit.json` → deploy adapter
with both commitments → fresh genesis → post-deploy smoke, including one real
key-op (register + revoke) through the deployed stack.

D4 note: "funded accounts ≥1 key" at genesis is satisfiable **only** via that
export tool (or by starting empty and having dev accounts re-register before
funding); the tool is therefore on the critical path of the recommendation, not
an optional nicety.

This remains a **separate, later** redeploy from the one currently owed
(SYB-266 + SYB-253 + OpenVM). v6 was ratified on 2026-07-10 and implemented in
the working tree; the guest commitment/adapter repin and fresh genesis remain
landing-time orchestrator work.

---

## 7. Open questions / decision points (each with a recommendation) ⟲ *(revised: Q1 is now the pre-proof keystone; Q5 expanded; Q7 new)*

1. **Ratify the keystone: second qMDB proof (pre-state) in-guest, ~2× state-root
   proving cost + ~60–130 KB/block input at devnet scale (§2e)?** Recommend
   **yes** — it is the difference between this design closing ZK-1 and not; the
   cost was already forecast (and accepted in design) by the landed witness-v2
   plan, and the deferred openings witness removes it later.
2. **Batch the full audit (§3b) into v6, not keys-only?** Recommend **yes** —
   same commitment move, and keys-only would leave
   `events_digest`/`total_deposited`/application unconstrained (the same review
   finding class, guaranteed to come back).
3. **WebAuthn in-guest for key ops from day one?** ADR-0014 already decides the
   direction; the open point is only sequencing. Recommend **yes** (both arms in
   increment 4).
4. **Genesis key seeding** — carry current devnet key sets (via the new export
   tool, §5-5/§6) or start empty? Recommend **carry** — it exercises the
   operator-replacement import path; the export tool is now an explicit
   deliverable either way, since D4-at-genesis needs it.
5. **Caps** — `client_data_json` ≤ 2 KB, `authenticator_data` ≤ 512 B,
   `MAX_KEYS_PER_ACCOUNT = 16`, `MAX_KEY_OPS_PER_BLOCK = 64`; all enforced at
   admission and asserted in-guest. Recommend **yes, these values**.
6. **SYB-225 ticket hygiene** — close SYB-225 as folded into SYB-270? Recommend
   **yes** (comment, no new ticket — Linear cap).
7. **UV flag strictness (§3a-4)** — require UV on key-op assertions, or UP only?
   Recommend **UV required** (key ops are rare and security-critical; platform
   passkeys do UV by default); accept the risk that some roaming authenticators
   prompt twice.

---

*Prepared 2026-07-10, revision 3 (Fable meta-lane; adversarially reviewed at
rev 2, see `witness-v5-design-review.md` — filename kept for history; the design
is now numbered v6). Rev-2 baseline was origin/main `9297903d`
(`WITNESS_FORMAT_VERSION = 4`, seven variants, digest arms 0x01–0x08). Rev 3
re-verified against landed main `2793d35c` (2026-07-10): `WITNESS_FORMAT_VERSION
= 5`; **ten** `SystemEventWitness` variants, tags 0–9 (`WithdrawalRefunded = 7`,
`WithdrawalFinalized = 8`, `L1BlockObserved = 9`); `digest.rs` arms 0x01–0x09
(refund = 0x09); OpenVM 2.0.0 toolchain with combined guest fingerprint
`app_exe_commit 0x002bc246…`. Still true on `2793d35c`: guest path =
`verify_state_transition_input` with a **single** post-state qMDB proof and no
`verify_block` call; `import.rs` zeroes `pubkey_registry`; fingerprint
closure = sybil-zk, sybil-verifier, matching-engine, sybil-l1-protocol;
`account_keys.rs` digest v1 without mask; settlement compares balance+positions
only; `events_digest`/`total_deposited` re-derived by nothing.*
