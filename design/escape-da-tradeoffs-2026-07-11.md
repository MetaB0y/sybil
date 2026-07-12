---
tags: [escape, validium, data-availability, privacy, custody, syb-32, syb-120]
status: trade-off analysis
date: 2026-07-11
---

# Escape model, DA shape, and encryption: Option A versus Option B

## Decision summary

**Recommendation: keep Option A — a frozen newest accepted root with mark-to-market valuation — and support it with one encrypted, independently retained full snapshot per accepted settlement epoch. Do not publish a full snapshot per 10-second block, and do not introduce deltas for escape.** Deltas/openings should be evaluated separately as a way to reduce the transition guest's private input and proving cost.

The deciding factor is product truth: **an exchange emergency exit should preserve the ownership transfers that trading created.** Option B has the cleanest availability story, but it makes a valid winning account's emergency property right stop at deposited cost basis. That is not a small edge case for a prediction exchange; it changes what users own precisely when custody is under stress.

This recommendation is not based on sunk cost. Even from a greenfield design, I would choose A once the DA requirement is stated correctly. A does not require putting approximately 257 MB on Ethereum/Celestia every block. Under the chosen hourly monolithic settlement model it requires making the final accepted state retrievable: one replace-in-place snapshot, approximately once per hour, redundantly stored and encrypted. At the quoted stress scale that is operationally material but ordinary object-storage traffic, not DA-layer traffic.

Option B remains the correct alternative if the team is unwilling to operate independent retention and threshold key release. It must not be described as a minor simplification of the current code, however: the current account leaf lacks a withdrawal counter, current `total_deposited` includes unbacked development funding, and the vault lacks account-scoped cross-root normal-withdrawal accounting.

## Scope and established soundness boundary

Sybil is a private single-sequencer validium. Ethereum verifies state transitions but does not receive the data needed to reconstruct an account opening. This analysis covers only an individual user's escape claim. Full-state reconstruction for operator handoff (SYB-116) is deliberately excluded.

The soundness boundary supplied for this analysis is:

- **Unsound:** any accepted root plus mark-to-market. Different users can select non-contemporaneous personal peaks, so aggregate payouts need not correspond to any solvent state.
- **Option A:** one common, frozen newest accepted root plus mark-to-market. Trading P&L is preserved, but claim openings for that unpredictable final root must remain available.
- **Option B:** any accepted root plus an external-capital refund ceiling. Trading P&L is not paid; the proof needs only the claimant's account opening. Cross-root and normal-withdrawal consumption must be account-scoped, not root-scoped.

An older fallback in `design/archive/implemented/escape-hatch-reconstruction.md` §3 says to walk back to the newest retrievable accepted root. That is safe for audit/reconstruction, but **not for an Option-A payout**. For escape payout it is superseded by the common-root soundness result: if the frozen root is unavailable, A is unavailable; the vault cannot silently accept an older root.

## Current code baseline

The current implementation is already strongly shaped toward A:

- `crates/sybil-escape-claim/src/lib.rs:50-62` defines a Form-L input containing the account opening, an `acct_resv/` inclusion or exclusion proof, every relevant `market/` opening, active keys, and authorization.
- `verify_escape_claim` proves every opening against one `state_root`, verifies the committed key set and P256/WebAuthn authorization, computes the exact amount, checks a deployment/account/root-bound nullifier, and reveals the L1 hash (`lib.rs:106-150`).
- The valuation is cash plus positions at committed `last_clearing_prices`, less reserved cash, floored at zero and converted to token units (`lib.rs:215-279`). Market openings must be complete, unique, exactly relevant, and price-shaped (`lib.rs:221-244,281-291`).
- `MarketSnapshot.last_clearing_prices` is in the canonical `market/` leaf (`crates/sybil-verifier/src/types.rs:455-466`; `snapshot_schema.rs:57-67`). Stage 1a therefore landed, rather than remaining a plan.
- The canonical transition witness is version 9 (`crates/sybil-verifier/src/witness_schema.rs:28`). The account leaf contains `balance`, `total_deposited`, positions, event digest, and key digest, but **not `total_withdrawn`** (`types.rs:432-444`; `snapshot_schema.rs:28-35`).
- The escape guest is a separate OpenVM program that deserializes `EscapeClaimGuestInput`, calls the library, and reveals the 32-byte hash (`zk/openvm-escape-guest/src/main.rs:1-26`). Its executable/VM commitments are independent of the transition guest.
- `SybilVault.activateEscapeMode` stores a nonzero `escapeStateRoot` and `escapeHeight`; `escapeClaim` requires that frozen head, recomputes the root-bound nullifier, consumes the shared `nullifierUsed` entry, verifies the escape proof, and pays immediately while bypassing pause.
- `SybilSettlement.submitStateRoot` can continue advancing after activation without changing the vault's frozen escape head. Deposits and new normal withdrawal requests are closed once escape activates. Requests queued beforehand cannot be cancelled and may finalize even while paused.
- Normal `WithdrawalPublicInputs` do not reveal `accountId` (`contracts/src/SybilTypes.sol:32-40`), so the vault cannot currently maintain a per-account ledger spanning normal withdrawals and escape.
- `sybil-custody` already has a versioned own-opening bundle containing account, reservation, market proofs, and active keys (`crates/sybil-custody/src/format.rs:5-21`), and it rejects a snapshot that is not the settlement head when assembling an A claim (`crates/sybil-custody/src/claim.rs:44-71`).
- Current DA is the plaintext canonical witness. `payload_root` hashes the domain, length, and plaintext bytes; `da_commitment` additionally binds height, state root, witness root, payload length, and provider-reference hash (`crates/sybil-zk/src/lib.rs:439-484`). The file publisher writes those plaintext bytes and a version-1 manifest (`crates/sybil-prover/src/da.rs:124-168,255-284`).

## Option A ledger — frozen newest root plus mark-to-market

### Soundness and contract work

A pays all claimants from one contemporaneous state, so it avoids personal-peak aggregation. The contract change should make “last root” literal:

1. On `activateEscapeMode`, store immutable-for-escape `escapeStateRoot` and `escapeHeight` from the settlement head.
2. Reject activation before a nonzero accepted root exists, or define a separate pre-first-root deposit refund. A mark-to-market proof cannot target the zero root.
3. In `escapeClaim`, compare to the stored escape root/height, not the settlement's moving `latest*` values.
4. Keep the existing nullifier domain `keccak256(domain, chain_id, vault, account_id, state_root)`. Once the root is frozen, it is one nullifier per account for the only payable root. The shared `nullifierUsed` map remains sufficient because normal and escape domains are distinct.
5. Reject deposits and new `requestWithdrawal` calls after escape activation. Already queued normal withdrawals must remain finalizable even while paused, and cannot be cancelled, because their withdrawal leaf was created only after the sequencer debited the account. Allowing cancellation would strand that debit outside the frozen payable state; allowing a new historical-root request after the escape root freezes would reopen stale-leaf overlap.
6. Add tests for a root accepted after activation, repeated account claims, pending/finalized normal withdrawal overlap, post-activation deposits and normal requests, cancellation closure, pause bypass, and zero-root activation.

Freezing in the vault is preferable to assuming the settlement is paused. It makes the payout state explicit and prevents a claimant who was paid at root N from acquiring a fresh root-bound nullifier at N+1. An additional `escapeClaimed[accountId]` mapping is harmless defense in depth but is not required if the stored root can never change.

No all-roots registry scan or per-root payout sum is needed for A. Existing normal withdrawals are already debited from account state when their leaf is created (`crates/matching-sequencer/src/sequencer/bridge_ops.rs:328-381`), which is why an already queued transfer can coexist with the frozen-root valuation. Closing deposits, cancellations, and new normal requests at activation while preserving queued finalization keeps later vault actions from creating value absent from, or stranding value already debited in, the frozen state.

### Guest, state schema, and wire work

The current escape statement is A. No valuation redesign is required:

- Retain `MarketLeafWitness`, reservation inclusion/exclusion, market completeness checks, last-clearing-price validation, checked signed notionals, key binding, and authorization.
- Retain the v1 escape public-input and nullifier domains. Freezing the root is a vault rule and does not change the revealed statement.
- No main witness-format bump is needed; the market price field is already in witness v9 and the state root.
- No escape guest-input change or guest repin is needed solely for the root freeze.
- The custody JSON can remain v1 for the claim opening. It should additionally record whether its root is the frozen escape root and the encrypted-snapshot manifest/key-release metadata.

If encryption metadata is added to the DA manifest, version the manifest/provider-reference schema. Provider-reference bytes are already opaque to the guest and hash-bound, so this need not change the transition verifier's logic; the final provider references must be fixed before proving.

### What a user must retain or synchronize

There are two valid A delivery shapes:

**Retained full snapshot (recommended):** the user keeps their signing/recovery key. After escape activation they download and decrypt the frozen epoch snapshot, verify it against the accepted root, and locally generate/extract the account, reservation, and market openings. The user may keep a small custody bundle as defense in depth, but continuous per-root local synchronization is not mandatory while the independent snapshot service meets its SLO.

**Continuously delivered per-user slices:** at each accepted root, produce the user's account proof, reservation proof, relevant market proofs, and active key set; encrypt the bundle to the user and place it outside the operator. This avoids retaining unrelated order/account plaintext and can be much smaller than a full snapshot, but it adds per-account fan-out, proof generation, encryption-key registration/rotation, delivery receipts, retry queues, and monitoring.

The failure UX is categorical. An A proof from root N is worthless after root N+1 becomes the frozen root. If the user loses their N+1 slice and the independent full snapshot cannot be retrieved or decrypted, the user cannot claim; accepting N would reintroduce the peak-picking insolvency. Product copy must say this plainly.

### DA infrastructure and ongoing burden

A needs availability, not L1 data publication:

- Before submitting an hourly epoch root, produce its final full-state snapshot (or retain the epoch's final full witness), verify that it recomputes the proposed root, encrypt it, and upload it to at least two independently administered/object-store locations.
- Bind the plaintext and final provider references before the validity proof/public input is finalized. Keep the existing property that `da_commitment` binds plaintext.
- Advance a small “latest accepted escape snapshot” pointer only after L1 acceptance. Retain the previous snapshot through an overlap/grace interval so a race or bad upload does not destroy the only copy.
- Monitor root-to-object lag, replica integrity, decryptability/key-share availability, and restore drills. Alerting only on HTTP presence is insufficient.
- Retain the frozen snapshot for the entire escape-claim lifetime. Once escape is activated, never garbage-collect it while the vault can pay claims.

This is an ongoing operational subsystem. Object storage alone is not a self-custody guarantee; replica and key custodians must be able to act when the sequencer/operator is absent.

### Existing work retained or made unnecessary

A uses essentially all built SYB-32 work: market-leaf prices and transition constraints (Stage 1a), mark-to-market valuation and market proof completeness (Stage 2), the separate escape guest, vault verifier, custody opening format, proving/ABI path, and frozen-root check.

That is not a reason by itself to choose A. The independently retained DA publisher, encryption/key release, monitoring, and drills remain new work. Full-state reconstruction/import for operator handoff remains descoped and should not be used to inflate A's benefit.

For individual escape, Form P (making the escape guest itself ingest the full DA payload) is unnecessary. The user can authenticate/decrypt the snapshot outside the guest, derive Form-L openings, and prove the existing small statement. This keeps the claim prover cost proportional to one account rather than the exchange snapshot.

## Option B ledger — any root plus deposit-bounded refund

### Soundness rule and a code-grounded prerequisite

B must pay only external collateral attributable to the claimant, not market value. A precise target is:

```text
leaf_refund_cap(R) = max(0, total_deposited(R) - total_withdrawn(R))
payout             = min(claimable(R), leaf_refund_cap(R))
```

Here `claimable(R)` is the account-local amount the refund statement permits; positions, market leaves, and reservations do not contribute. A custody client normally requests the full cap.

The account opening proves both cumulative counters at the chosen accepted root. A historical leaf by itself is nevertheless not enough: a claimant could choose a root before a later normal withdrawal, when `total_withdrawn(R)` was lower. The L1 vault must therefore also supply the current account-scoped external-exit debit, including normal withdrawals already paid or reserved, and enforce the stronger current check:

```text
payout + exitDebitedByAccount <= proven total_deposited(R)
```

This does not add another Merkle opening; the ZK witness remains the user's own account leaf. It is the contract-side cross-root consumption rule that prevents an old proof from erasing a later L1 exit.

Under those constraints, every claimant's chosen historical `total_deposited(R)` is no larger than that account's latest cumulative real deposits, and the vault subtracts all account-scoped exits. At the current state, the sum of those real net-deposit entitlements is the vault's external collateral; selecting older roots cannot increase the per-account cumulative deposit ceiling. Therefore aggregate refunds cannot exceed collateral. Trading transfers do not change the ceiling.

Today's `total_deposited` is **not yet that sound ceiling**:

- Account creation sets `total_deposited = initial_balance` (`crates/sybil-verifier/src/system.rs:73-95`).
- The public API currently allows unbacked caller-selected development balances, as documented in `design/dos-audit-2026-07-11.md` finding 2.
- Generic `Deposit` and L1 deposit events both increment it (`crates/sybil-verifier/src/system.rs:97-115`).
- No `total_withdrawn` exists in `AccountSnapshot`.
- Withdrawal creation debits balance but no cumulative external-exit account field (`crates/sybil-verifier/src/system.rs:117-143`; `crates/matching-sequencer/src/sequencer/bridge_ops.rs:363-380`).

Consequently B implemented naively as `amount <= current total_deposited` would make the current vault insolvent. B first requires either (preferred) distinct validity-constrained `real_l1_deposited`/external-exit accounting, or removal of every unbacked funding path plus a fresh genesis that establishes `total_deposited` as collateral-only.

### Contract work and cross-root double-spend accounting

B replaces A's simple common-root gate with more accounting:

1. Accept any proven root only if `settlement.rootAt(inputs.height).stateRoot == inputs.stateRoot`. `isAcceptedRoot(root)` alone does not bind the supplied height.
2. Make escape consumption root-independent: use `keccak256("sybil/escape-refund-nullifier/v1", chain_id, vault, account_id)` or a direct `escapeClaimed[accountId]`. The current root-bound nullifier permits the same account to claim at multiple roots.
3. Add `accountId` to normal withdrawal public inputs and their proof hash. The current Solidity struct lacks it, so the vault cannot attribute normal exits to an account.
4. Maintain `exitDebitedByAccount`. Reserve it when a normal withdrawal is queued, keep it on finalization, and release it only on a valid cancellation. Once escape activates, either forbid new normal requests/finalizations or keep applying the same ledger before every transfer.
5. Reveal the proof's real-deposit ceiling in the escape public inputs and require `exitDebitedByAccount + escapeAmount <= provenDepositCeiling`. Consume the account escape marker before the external call, as today.
6. Define pending withdrawal, cancellation/refund, pre-first-root deposits, quarantined deposits, token rounding, zero claims, and post-activation deposits explicitly. In particular, an old root must never make a later completed withdrawal disappear.

The account-scoped exit ledger is the important subtlety. A root-independent escape nullifier prevents two escape claims, but does not by itself prevent “normal withdrawal at a later root, then refund from an earlier root.”

### Guest, state schema, and wire work

B's escape guest is smaller, but the system migration is not:

- Remove `AccountReservationLeafWitness`, `MarketLeafWitness`, market proof-set completeness, last-price validation, and position valuation from the escape statement.
- Retain the account qMDB inclusion proof, active-key/`keys_digest` weld, P256/WebAuthn authorization, deployment binding, checked token conversion, and exact public-input reveal.
- Add validity-constrained collateral counters to the account state. If represented in `AccountSnapshot`, update sequencer account state, `snapshot_schema` account leaf bytes, `state_schema`, system-event application, settlement comparisons, canonical witness encoder/decoder, import/recovery, golden vectors, fixtures, and every constructor.
- Bump `WITNESS_FORMAT_VERSION` from 9 because account snapshot bytes change. Repin the main transition guest and take the required fresh-genesis/state migration decision.
- Change `EscapeClaimGuestInput`; repin the escape guest. Add a new escape public-input/nullifier domain (v2/refund) if the public struct reveals a deposit ceiling or changes nullifier semantics.
- Bump `CUSTODY_SNAPSHOT_VERSION`; remove reservation/market fields for B and retain the account opening, active keys, accepted `RootRecord`, authorization inputs, and collateral counters.
- Change `WithdrawalPublicInputs` and its Solidity/Rust hash twin to carry `accountId`; version that domain and update golden vectors. This affects the unfinished normal-withdrawal proof path as well as escape.

The cleanest early-development implementation is a fresh genesis with dedicated `real_l1_deposited` semantics rather than overloading the P&L-oriented `total_deposited` field and trying to grandfather demo balances.

### What a user must retain or synchronize

B eliminates the need to predict the last root. It does **not** let a key invert a Merkle root. A distrustful user still stores:

- one accepted account leaf plus qMDB inclusion proof;
- the active key list needed to reproduce `keys_digest`;
- the L1 `RootRecord`/height reference; and
- their signing/recovery key.

This bundle is a few kilobytes, can be copied to ordinary personal backups, and needs refreshing only when the user wants later real deposits/exits reflected—not after every trade or accepted root. If the latest copy is lost, an older accepted copy remains usable but may prove a lower deposit ceiling. If every copy is lost and nobody retained historical proof material, B cannot reconstruct it; “no DA” means no system-wide DA dependency, not no user-held evidence.

The UX on a successful claim is the principal cost: winners forfeit gains above their external-capital ceiling; open positions and reservations are irrelevant; Stage-2 fair valuation is intentionally bypassed. The UI must call this a deposit-bounded emergency refund, not a portfolio withdrawal.

### DA, encryption, ongoing burden, and sunk work

B needs no exchange snapshot publisher, replica SLO, DA manifest, disclosure committee, or system encryption scheme for escape. Users may encrypt their small local custody file with their normal backup tooling. SYB-120 is moot for this escape model.

The ongoing operations burden is correspondingly low: preserve the L1 accounting invariant, keep custody clients compatible with the account-proof format, and drill claims. The one-time protocol migration and audit burden is meaningful, especially the interaction with normal withdrawals.

For escape purposes B strands the Stage-1a market price commitment and the Stage-2 reservation/market valuation work. Those fields may remain useful for portfolio display and other proofs, but they are no longer required by emergency custody. The reusable work is the account/key proof, authorization, OpenVM guest wrapper, verifier adapter, vault transfer path, custody prover/ABI tooling, and qMDB proof API. The fact that more A code is already built should not decide the policy.

## The DA-shape question: witness shape is not retention shape

### Crisp answer

**Yes: one latest-root full snapshot is still a full snapshot. No: Option A does not need a full snapshot on a public DA layer every block, and it does not need deltas for escape.**

Two independent artifacts have been conflated:

1. **Transition proving witness:** private input the main guest ingests to prove state changes. Whether this is a full-state witness or changed-leaf/openings witness controls input bytes, witness construction, and proving cost.
2. **Escape retention artifact:** data that must be retrievable after operator loss to derive Form-L openings for the one frozen accepted root. Because Sybil is a validium and A pays only that root, this can be one full post-state snapshot per accepted settlement epoch, with only the latest (plus overlap) retained. It need not be every block's transition witness history.

The current v9 witness happens to serve both roles because every block carries complete account phases and full state sidecars. That coupling should not become an architectural requirement. If the proving witness later becomes deltas/openings, add a separate full escape snapshot at epoch close.

### Cost comparison at the audited stress scale

The audit measured approximately 405 recurring canonical witness bytes per account per block and 217 bytes per resting order per block. At approximately one million orders, the full canonical witness is approximately **257 MB per block**. This is a witness estimate, not a measured compact typed-state snapshot: a purpose-built post-state snapshot can omit pre-state/event duplication and may be smaller. Until measured, hundreds of MB is the honest stress-order estimate.

At a 10-second block cadence:

| Shape | Transfer at 257 MB stress size | Retained bytes | Proving implication |
|---|---:|---:|---|
| Current full witness every block | 25.7 MB/s average (~206 Mbit/s); ~2.22 TB/day | ~2.22 TB per retained day | Guest ingests full state every block; an hourly 360-block execution sees ~92.5 GB of raw witness input |
| One full snapshot per hourly accepted epoch | 257 MB/hour; ~6.17 GB/day transferred (~0.57 Mbit/s average) | 257 MB for latest only; ~514 MB with one-root overlap | No direct proving cost if it is a separate host-produced snapshot; authenticate by recomputing the accepted state root and/or binding its plaintext hash in the epoch DA commitment |
| Changed-leaf/openings proving witnesses + hourly full escape snapshot | Workload-dependent deltas each block, plus 257 MB/hour snapshot | Same latest escape snapshot; optional bounded proving artifacts | Reduces guest input/proving work; does not change A's escape requirement |
| Per-user encrypted Form-L slices each epoch | Roughly accounts × proof/opening size; can avoid one million unrelated order bodies | Latest slice per account plus overlap | Small escape guest unchanged; adds per-user proof/encryption fan-out outside proving |

The approximately 92.5 GB/hour figure is `360 × 257 MB`; it explains why deltas/openings remain worth studying for proving even if OpenVM proving is cheap. OpenVM 2.0/SWIRL and the team-reported approximately 2× OpenVM 2.1 improvement reduce compute time, but they do not make tens of gigabytes of decoding, hashing, memory traffic, and witness generation free. The chosen monolithic hourly-epoch model in `design/settlement-aggregation-swirl.md` changes proof aggregation/L1 cadence; it does not by itself shrink per-block private input.

Conversely, deltas make escape retention more fragile: a claimant would need a base plus every delta through the frozen root. Since only one root matters, replacing one independently verifiable snapshot each epoch is simpler than retaining and replaying a chain. Deltas are optional transfer optimization, not an escape requirement.

Celestia's few-MB target and Ethereum's roughly 768 KB maximum blob capacity are roughly 10^2–10^3 below a 257 MB full witness. They are not plausible destinations for this stress-size snapshot. Sybil is explicitly a validium, so A should use redundant off-chain object storage/archival providers with a committed plaintext hash, not pretend to be a rollup. The trade is an operational availability assumption, and it must be named as such.

### Required publication order for A

For each hourly epoch:

1. Produce and locally verify the final post-state snapshot.
2. Encrypt and upload it to independent providers; verify reads and ciphertext hashes.
3. Fix the manifest/provider-reference bytes and plaintext snapshot commitment.
4. Prove and submit the epoch root with those commitments bound.
5. After L1 acceptance, atomically advance the advertised latest pointer; retain the predecessor for overlap.
6. At escape activation, freeze that root and retention/key-release policy indefinitely for claims.

Availability cannot be repaired after the sequencer disappears. “Upload after L1 acceptance” is therefore the wrong ordering.

## Encryption for Option A (SYB-120)

### Privacy requirement

Publishing a plaintext full snapshot would expose balances, positions, orders, and market activity and would defeat the validium's confidentiality. A retained snapshot therefore needs encryption unless the product explicitly accepts public state.

The existing binding is well-shaped for this. As reserved in `design/archive/implemented/escape-hatch-reconstruction.md` §6, `payload_root` and `da_commitment` should continue to bind **plaintext**. Encryption is a storage envelope:

```text
plaintext snapshot
  -> plaintext payload_root / accepted state_root binding
  -> AEAD ciphertext stored by providers
  -> ciphertext hash + algorithm + key-release metadata in the manifest/provider ref
  -> decrypt on escape
  -> verify plaintext payload_root, da_commitment, and recomputed state_root
```

This avoids changing leaf proofs or the escape guest. Decryption happens before custody tooling derives the existing Form-L input.

### Recommended concrete scheme: chunked AEAD plus threshold-released epoch key

For each accepted epoch:

1. Generate a random 256-bit data-encryption key (DEK).
2. Split the snapshot into fixed chunks (for example 4 MiB) and encrypt each with XChaCha20-Poly1305 or AES-256-GCM. Use a unique nonce per chunk and associated data containing `genesis_hash`, chain/vault, height, state root, plaintext length, and chunk index/count.
3. Hash the complete ciphertext/chunk manifest. Put algorithm/version, chunk size, nonces, ciphertext hash, plaintext `payload_root`, and key-custody metadata in a versioned manifest. Include the final encrypted-provider reference bytes in the existing `provider_refs_hash` before proving.
4. Shamir-split the DEK across independent recovery custodians (for example 3-of-5), or encrypt it to an equivalent threshold service. Publish a commitment to the DEK/shares and signed custody receipts.
5. When `escapeModeActive` is observable on L1 for the frozen root, custodians publish shares. Anyone reconstructs the DEK, downloads any replica, decrypts, and runs the normal plaintext/root verification chain.

A key held only by the operator and “disclosed on escape” is not a credible design: operator death/refusal is the event being handled. The operator may hold a share, but enough non-operator shares must already exist before root submission.

With 4 MiB chunks, a 257 MB object has roughly 62 chunks. A 16-byte tag plus nonce per chunk is only a few kilobytes; ciphertext is effectively the same size as plaintext. AEAD and one ciphertext hash pass are negligible beside snapshot construction and ZK proving. The complexity is not cryptographic compute—it is custody membership, rotation, receipts, release authorization, drills, and handling a custodian that disappears.

Threshold release preserves privacy during normal operation but makes the whole snapshot public in an emergency. That is a reasonable simple failure policy if documented. If privacy must survive escape too, use per-user encrypted slices instead:

- Register a dedicated HPKE/X25519 recovery encryption key per user; WebAuthn signing credentials generally cannot be assumed to perform arbitrary decryption.
- Encrypt each Form-L bundle to that key and retain it at independent providers.
- Handle encryption-key rotation, multiple devices/recipients, lost keys, account-key-set changes, fan-out, and delivery monitoring.

Per-user slices give stronger post-failure confidentiality and smaller data when orders dominate, but materially increase protocol/client operations. For the team's simplicity priority, the threshold-released full snapshot is the better A implementation.

For Option B, system DA encryption is moot. Only the user's small local account-proof bundle exists, and the user can encrypt/back it up using ordinary device recovery.

## Trade-off table

| Dimension | Option A — frozen newest root + mark-to-market | Option B — any root + deposit-bounded refund |
|---|---|---|
| Soundness | Sound because all claims value one frozen contemporaneous state; vault must freeze root/height at activation | Sound only with real-collateral counters, root-independent account consumption, and account-scoped normal-withdrawal/escape accounting across roots |
| Fairness / winner UX | Preserves cash and trading gains at committed last prices; open exposure receives a common oracle-free mark | Caps emergency property at external cost basis; winners lose gains and open-position value above the cap |
| Self-custody | Conditional on latest-root snapshot/slice availability and decryptability; stale local proof is unusable | Strong: one small accepted account proof can be stored personally and remains usable; newer copies only increase the provable deposit ceiling |
| DA infrastructure | Yes: latest accepted epoch snapshot (or per-user slices), independent replicas, retention SLO, monitoring, restore drills | None system-wide for escape |
| Contract complexity | Low incremental change: freeze root/height, close new normal requests on activation, test queued-withdrawal overlap; existing root-bound nullifier then suffices | Moderate/high: any-root height binding, root-independent escape consumption, normal withdrawal `accountId`, per-account exit ledger, proven deposit ceiling, pending/cancel semantics |
| Guest / wire change | None for core A; current Form-L guest and witness v9 already implement it. Manifest/provider-ref version for encryption | Main state/account schema and witness version bump; main guest repin/fresh-genesis decision; smaller escape input and repin; public-input/nullifier v2; custody format bump; normal-withdrawal hash change |
| Encryption | Required to preserve validium privacy; recommended chunked AEAD + non-operator threshold key release | Moot at system level; user encrypts their own small backup if desired |
| User storage / sync | Key only if independent snapshot works; otherwise must continuously receive the exact latest-root slice. Losing both latest slice and DA loses escape | Key + one few-KB account proof/key-set/RootRecord bundle; refresh after deposits/exits. Losing newest may reduce refund; losing every copy loses unilateral proof |
| Built work not used | None of Stage 1a/2; A uses both. Form P/full-payload-in-guest remains unnecessary | Market price commitment and Stage-2 reservation/market valuation become unnecessary for escape; account/key/prover/vault plumbing remains useful |
| Ongoing operations | High relative to B: publication ordering, replicas, encryption shares, SLO alerts, key rotation, periodic recovery drills | Low: contract invariant monitoring, client-format support, and claim drills; most complexity is one-time validity/accounting code |
| Failure mode | DA or key-release failure blocks an otherwise valid winner; older-root fallback is forbidden | Claim works from an old personal proof but deliberately pays no trading gains above the external-capital ceiling |

## Final recommendation and implementation boundary

Adopt A with the smallest availability shape that actually satisfies it:

1. Freeze the settlement head in `SybilVault` when escape activates.
2. Close deposits, withdrawal cancellation, and new normal-withdrawal requests on activation while allowing already debited/queued withdrawals to finalize despite pause.
3. Retain one encrypted final-state snapshot per accepted hourly epoch at independent providers; keep one predecessor during overlap and the frozen snapshot for the full claim lifetime. While v9 witnesses remain full, retain the epoch's final canonical witness so the existing plaintext `da_commitment` already binds it; if proving later moves to deltas, add and bind a separate epoch snapshot artifact.
4. Bind plaintext as today and bind encrypted provider metadata before proving.
5. Use non-operator threshold release of the epoch key; do not rely on an operator-only key.
6. Keep the existing Form-L guest. Decrypt/reconstruct outside it and prove only the claimant's openings.
7. Study delta/openings witnesses solely as a transition-proving optimization. Do not make escape depend on a delta chain.

This is a recommendation for A despite, not because of, the work already landed. The retained-snapshot cadence makes A operationally feasible; the common-root mark preserves the exchange's actual allocation of value. If the team later decides it will not fund and drill the retention/key-release subsystem, it should choose B explicitly and market it as a deposit-refund safety valve—not claim that winners' exchange balances are self-custodied.
