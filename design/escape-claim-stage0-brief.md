---
tags: [sybil, design, escape, custody, zk, shape-freeze, syb-32]
status: stage-0 brief — interfaces FROZEN on merge; two ADR-0013 readings + two sketch deltas await Valery's one-word ratify
date: 2026-07-10
tickets: [SYB-32, SYB-80, SYB-116]
author: design lane (Fable), per design/escape-claim-plan.md §Stage 0 — no new design decisions except the two readings the plan assigns here
sources: design/escape-claim-plan.md, design/escape-claim-guest.md, design/keys-and-escape-ratification.md (D4/D7–D10), ADR-0007/0008/0013/0014, design/witness-v6-keys-transition.md §2a–2b/3a, repo @ main 01396f00
---

# Escape-claim Stage 0 — implementation brief + shape freeze

Frozen interfaces for parallel lanes 1a/1b/2/3; deltas vs the plan's sketch in §8.
Byte order: **integers little-endian in canonical/leaf bytes** (repo-wide,
`crates/sybil-verifier/src/snapshot_schema.rs:333-343`), **big-endian only inside
32-byte ABI words** for keccak input hashes (`crates/sybil-zk/src/lib.rs:849-853`).

## 1. `EscapeClaimPublicInputs` (frozen)

Mirrors `SybilTypes.WithdrawalPublicInputs` (`contracts/src/SybilTypes.sol:32-40`)
minus `token` (vault is single-token immutable, `SybilVault.sol:31`) and `claimKind`
(dispatch = **dedicated entrypoint + dedicated domain string**, the plan's stage-3
recommendation — fail-closed by construction, no kind constant).

```solidity
struct EscapeClaimPublicInputs {      // contracts/src/SybilTypes.sol
    bytes32 stateRoot;   // R — must equal settlement.latestStateRoot() (D8)
    uint64  height;      // H — must equal settlement.acceptedRootHeight(R)
    uint64  accountId;   // A
    address recipient;   // D
    uint256 amount;      // X — token units (u64 range), NOT nanos; see §6
    bytes32 nullifier;   // §3
}
```

**Input hash** (Rust + Solidity must agree byte-for-byte, golden-vectored):

```
escapeClaimPublicInputHash = keccak256(abi.encode(
    "sybil/openvm/escape-claim/v1", stateRoot, height, accountId,
    recipient, amount, nullifier))
```

Domain follows `sybil/openvm/<statement>/v1` (`"sybil/openvm/withdrawal/v1"`,
`SybilVault.sol:379`; `"sybil/openvm/state-transition/v1"`, `sybil-zk/src/lib.rs:26`).
Field order is the plan's §Stage-2 sketch, frozen. Rust side: the
`AbiWord::{Uint,Bytes32,Address}` encoder pattern (`matching-sequencer/src/bridge.rs:326`,
`sybil-l1-protocol/src/lib.rs:501-`); the helper is private in all three existing
copies (ADR-0007 "Theme 6" debt) — stage 2 `pub`-exports `sybil-l1-protocol`'s
(already in the guest closure), does NOT mint copy #4. The escape guest reveals
exactly this 32-byte hash via `reveal_bytes32` (`zk/openvm-guest/src/main.rs:8`) —
unchanged `OpenVmVerifierAdapter` 32-byte reveal check
(`OpenVmVerifierAdapter.sol:8,72-85`), second adapter pinned to the escape guest.

## 2. Claim canonical bytes + signing envelopes (frozen)

Style: raw-concat with fixed-width LE ints — the **guest-verified** convention
(`sybil-verifier/src/account_keys.rs:71-89` `canonical_key_op_bytes`), not the
borsh admission style. Domain is ADR-0007's own example, `"sybil/escape-claim/v1"`.

```
claim_bytes =
  "sybil/escape-claim/v1"          (21 bytes, no length prefix — key-op style)
  || genesis_hash[32]              ADR-0007: every signed action leads with it
  || chain_id: u64le               ┐ L1-bound-intent convention, per
  || vault_address[20]             ┘ BridgeWithdrawalRequest (sybil-signing/src/lib.rs:132-141) — §8 delta 1
  || state_root[32]                R
  || height: u64le                 H
  || account_id: u64le             A
  || recipient[20]                 D
  || amount_token_units: u64le     X (token units, == public-input amount; see §6)
```

No nonce: replay is closed by R-binding + the one-shot nullifier (D8+D10), the
state-bound philosophy of D2. No nullifier-salt field — see §3.

**Envelopes** — reuse `sybil_verifier::KeyOpAuth` verbatim
(`crates/sybil-verifier/src/types.rs:220-235`; 33-byte SEC1 pubkey, fixed 64-byte
raw r||s signature) and the v6 §3a verification rules with `claim_bytes`
substituted for the key-op bytes (both arms, ADR-0014):

- **RawP256** (`auth_scheme=0`): ECDSA-P256 `verify_prehash` over
  `SHA256(claim_bytes)` under `signer_pubkey` — the ADR-0008 recipe
  (`design/openvm-p256-integration.md`).
- **WebAuthn** (`auth_scheme=1`): signature over
  `authenticator_data || SHA256(client_data_json)`, with v6 §3a items 1–5:
  challenge = base64url(`SHA256(claim_bytes)`) via the escape-safe RFC 8259
  extractor, `type == "webauthn.get"`, `rpIdHash == expected_rp_id_hash` (the
  **same** guest constant lane 1b allocates — cross-lane dependency, not a fork),
  UP+UV flags, caps re-asserted (`authenticator_data ≤ 512 B`,
  `client_data_json ≤ 2 KiB`, `account_keys.rs:10-11`).

Signer must be a member of the account's key set welded to the proven `acct/{A}`
leaf's `keys_digest` (same weld as `key_transition.rs::weld_post_keys`; digest v2
per `account_keys.rs:14-31`), with matching `auth_scheme`. `capability_mask` is
NOT checked (D1: cosmetic in v1; future scoped-delegation hook). Empty welded set
⇒ fail closed (D4). The bytes builder + envelope verifier live in `sybil-verifier`
(guest/host split per ADR-0003) so CLI, main guest (1b), and escape guest (2)
share one definition — ADR-0007's one-definition rule.

## 3. Nullifier (frozen) — and shared `nullifierUsed`

```
escape_nullifier = keccak256(abi.encode(
    "sybil/escape-nullifier/v1", chain_id, vault_address, accountId, stateRoot))
```

Exactly one claim per account per root (D10) ⇒ deterministic in (A, R): **no
salt, and recipient is deliberately excluded** (either would permit multiple
claims per (A,R)). Shape mirrors `withdrawal_nullifier` (`bridge.rs:317-338`);
Solidity twin mirrors `depositLeaf`'s `(block.chainid, address(this), …)` style
(`SybilVault.sol:347-358`). The **vault recomputes** it from its own
`block.chainid`/`address(this)` + public `accountId`/`stateRoot` and requires
equality with `inputs.nullifier` — transitively anchoring the guest's private
`chain_id`/`vault_address` inputs (and thus the claim bytes' deployment binding,
§8 delta 2).

**Shared vs separate map: SHARED `nullifierUsed` — recommended, verified safe.**
Normal withdrawals use `"sybil/withdrawal-nullifier/v1"` (bridge.rs:327; consumed
at `SybilVault.sol:186,191`); both formulas are `keccak256(abi.encode(<distinct
string>, …))`, so distinct dynamic-string heads give disjoint preimage sets —
cross-domain collision requires a keccak256 collision. Vault-path audit:
`cancelWithdrawal` (the only `nullifierUsed[n]=false` writer, `SybilVault.sol:253`)
requires a `withdrawals[n]` entry (`:246`); escape claims never create one
(immediate payout, no queue), so admin can never un-spend an escape nullifier,
and neither kind can block the other (disjoint domains). Stage-3 gate already
planned: named forge test for cross-domain non-collision.

## 4. Market leaf `last_clearing_prices` (frozen — lane 1a)

`MarketSnapshot` (`sybil-verifier/src/types.rs:357-365`) gains
`pub last_clearing_prices: Vec<Nanos>` — indexed by outcome; **empty vector =
never cleared** (§5b). Sequencer populates it from `price_tracker.last_clearing_prices`
(`matching-sequencer/src/price_tracker.rs:57`) at block build; genesis markets
start empty unless `--import-witness` carries them.

**Encoding** — appended **last** in `append_market_snapshot_fields`
(`snapshot_schema.rs:57-64`), so the state leaf (domain `"sybil/state/market"`)
and the witness encoding both pick it up through the shared visitor:

```
... || resolution_template (existing tail)
    || price_count: u64le || price_0: u64le || ... || price_{n-1}: u64le
```

Shape validity (fail closed): `price_count == 0 || price_count == num_outcomes`;
each `price ≤ NANOS_PER_DOLLAR` (precedent: `match_verifier.rs:426-437` already
enforces this range plus binary YES+NO == exactly `NANOS_PER_DOLLAR` on every
witnessed `clearing_prices` entry).

**Transition constraint** (extend `sidecar.rs::verify_market_transition`, the
`:719-806` pattern; gated on `previous_header.is_some()` like the rest):

- `m ∈ witness.clearing_prices` ⇒ `post.last_clearing_prices == clearing_prices[m]`
  (witnessed prices are themselves range/coherence-checked, above).
- `m ∉ witness.clearing_prices` ⇒ `post.last_clearing_prices ==
  pre.last_clearing_prices` (silent-mutation ⇒ violation — the plan's red→green gate).
- Documented residual: a `clearing_prices` entry for a zero-fill market is only
  coherence-bounded (uniform-price check binds via fills, `match_verifier.rs:360-421`),
  and price conditions legitimately read no-fill markets' prices (`:546`), so
  "entry ⇒ fills" must NOT be added. Redistribution-not-solvency exposure; flag
  to stage 2's adversarial review.

Wire: bump `WITNESS_FORMAT_VERSION` (`witness_schema.rs:28`, now 6) — batch as
v7 with SYB-272 if its genesis hasn't shipped, else v8. No new event tags.
Golden vectors regenerate; `fingerprint --write`; fresh genesis per plan ordering.

## 5. The two ADR-0013 readings (ratify with one word each)

**(a) "capped at the account's backing" = floor-0 + checked arithmetic + price
coherence; NO additional per-account cap.** In particular X is NOT capped at
`total_deposited` — trading gains are escapable; "backing" is systemic (YES+NO
mint from exactly $1 and coherent prices sum to $1, so valuing everyone at last
price ≈ conserves vault collateral, ADR-0013 §Why). An explicit per-account cap
would need a committed "backing" quantity that doesn't exist and would confiscate
profits. Dust is the accepted ADR-0013 bend, bounded per §6. — *Recommended: yes.*

**(b) Never-cleared markets value at 0, encoded as the empty price vector.**
`last_clearing_prices == []` ⇒ every position in that market contributes exactly
0 to X (ADR-0013's stated bend: "never cleared → treat as 0"). No mint-reference
fallback, no special genesis case. — *Recommended: yes.*

## 6. Valuation formula (frozen — exact types and rounding)

Units: `balance`, `reserved_balance` are **i64 nanodollars** (`NANOS_PER_DOLLAR
= 1_000_000_000`); position `qty` is **i64 share-units** (`SHARE_SCALE = 1_000`
per share); prices are **u64 nanos ≤ 1e9** (`matching-engine/src/types.rs:11-36`).

```
per-position value (i64 nanos):
  v(m, o, qty) = checked_signed_notional_nanos(price[m][o], qty)
               = sign(qty) · floor(price · |qty| / SHARE_SCALE)     // trunc toward 0
               // matching-engine/src/types.rs:279-286 — the consensus-canonical
               // signed helper; None ⇒ fail closed (no claim)
  never-cleared market (empty vector) ⇒ v = 0 for all its positions (§5b)

X_nanos: i128 = i128(balance)
              + Σ_{(m,o,qty) ∈ positions} i128(v(m,o,qty))    // checked adds
              − i128(reserved_balance)     // from acct_resv/{A}; 0 iff leaf
                                           // absence is PROVEN by exclusion proof
X_nanos_clamped: u64 = u64::try_from(max(X_nanos, 0))  // floor 0; Err ⇒ fail closed
X (public amount)    = X_nanos_clamped / NANOS_PER_TOKEN_UNIT   // floor; = /1000,
                       // matching-sequencer/src/bridge.rs:12,247-253 — L1 pays
                       // token units (SybilVault.sol:12; queue pays amount raw)
```

Notes the lanes must not re-derive: `reserved_positions` are NOT subtracted
(reserved shares are still owned and valued in `positions`; only cash reservations
offset). Valuation is **market-status-blind** (ADR-0013 decision 3: escape needs
a price, never an outcome; resolution settlement zeroes positions in normal
operation). Dust: truncation-toward-zero deviates < 1 nano per position and the
nanos→token floor strands < 1000 nanos in the vault — both inside the ratified
ADR-0013 dust bend. Market-leaf inclusion proof required for **every** market
with a nonzero position; a missing proof ⇒ fail closed, never "value at 0".

## 7. What each lane consumes from this brief

| Lane | Consumes |
|---|---|
| 1a (market-leaf prices) | §4 (leaf bytes, shape checks, transition constraint, wire coordination) |
| 1b (main-guest key-op crypto) | §2's envelopes (verification rules + module placement in `sybil-verifier`; allocates `expected_rp_id_hash` used by both guests) |
| 2 (escape guest, Form L) | §1 (hash + reveal), §2 (claim bytes + envelopes + weld), §3 (nullifier computation), §6 (valuation); §4's types as compile-time dep (land 1a first in-tree) |
| 3 (vault `escapeClaim`) | §1 (struct + hash, Solidity mirror + golden vector), §3 (nullifier recompute + shared map + cross-domain test) |

## 8. Stage-0 resolutions beyond the plan sketch, and OPEN items

Two deliberate deltas vs the plan's §Stage-0 sketch — both convention-grounded,
flag alongside §5(a)/(b) for the same 5-minute eyeball:

1. **`chain_id` + `vault_address` added to claim bytes** (sketch had
   `domain || genesis || R || H || A || D || X`). Grounded in the L1-bound-intent
   convention (`BridgeWithdrawalRequest` carries both) and ADR-0007's
   cross-deployment rule: two deployments from identical genesis config share
   `genesis_hash` AND early roots, so genesis+R alone does not bind a claim to
   one deployment; every deployment has a unique vault address.
2. **`genesis_hash` sourcing: private guest input, signature-bound, not
   guest-pinned.** A baked guest constant would force rebuild + repin + adapter
   redeploy on *every* re-genesis and break path-independent-build
   reproducibility; a public-input field would imply an on-chain check that
   cannot exist (settlement stores no genesis anchor). Hard deployment binding
   comes from delta 1's `chain_id||vault_address`, anchored on-chain via the
   vault's nullifier recompute (§3); `genesis_hash` stays as ADR-0007 uniform
   discipline / defense in depth.

OPEN (explicitly not pinned here — owned elsewhere):
- **`expected_rp_id_hash` value**: lane 1b allocates; the escape guest imports
  the same constant (§2). Not a shape question.
- **Wire version for 1a** (v7 vs v8): decided at land time against SYB-272's
  genesis status (§4); the layout is frozen either way.
- **Root-churn semantics at claim time** (latest-at-claim vs activation floor):
  stage 3's named decision (plan risk 5); the D8 check in §1 is frozen.
