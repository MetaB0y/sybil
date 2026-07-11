# Oracle, L1 Protocol, Indexer, and Contracts

**Crates:** `sybil-oracle`, `sybil-l1-protocol`, `sybil-l1-indexer`; `contracts/`

## Verdict

The oracle is genuinely elegant — one primitive (a signed attestation from a registered feed evaluated by a market's `ResolutionPolicy`), with a TEE-correct inside/outside split and only the shipped `Immediate` arm actually wired. The L1/bridge/contract stack is well-structured at the boundaries and Foundry-tested, but its **trust model is materially weaker than the docs claim**: the ZK guest never verifies deposit inclusion, the contract deposit checkpoint is bypassable, emergency escape is decorative, and the dev indexer applies unconfirmed tip logs as irreversible credits. These are the load-bearing safety gaps before any L1 custody goes live.

## Architecture as built

**Oracle** (`sybil-oracle`) is a pure decision layer — no I/O, no settlement, no bond escrow. `ResolutionPolicy` is a single enum with one live arm (`Immediate { feed_id }`). Resolution flows: `POST /v1/markets/{id}/resolve` reconstructs a `SignedAttestation` from the path id + body (so the signature binds `market_id` at the HTTP layer) → the actor verifies the P256 signature over canonical borsh bytes → `MarketLifecycle::resolve_from_attestation` maps the signer to a registered feed, calls `evaluate_immediate`, and returns `SettleNow` → the sequencer settles and records `MarketResolved`. Feeds/templates are bootstrapped at startup (admin always; polymarket_mirror if configured).

**L1 protocol** (`sybil-l1-protocol`, 339 LOC) is a neutral ABI/hash seam: strict `DepositReceived` log parsing (topic/data-length checks, high-byte rejection to fit u64) and keccak hash domains (deposit leaves with 0x00/0x01 domain-separated nodes, withdrawal nullifier). The cleanest crate in the tree.

**Indexer** (`sybil-l1-indexer`, 455 LOC) is a single-file poll loop: `eth_blockNumber` + `eth_getLogs` from an in-memory cursor to `latest` (no confirmation depth), resolves account keys via the dev API, and submits sequential deposits to the dev-only bridge endpoint.

**Sequencer bridge** (`bridge.rs`) tracks `{deposit_cursor, deposit_root, next_withdrawal_id, withdrawals}`. `ingest_l1_deposit` validates the account/sequence/key, converts units→nanos, credits balance, and **stores the event's `deposit_root` verbatim** (never recomputes the tree).

**Contracts** (`contracts/src`): `SybilVault` (custody, depth-32 incremental Merkle deposit log, `depositRootByCount` checkpoints, withdrawal queue with delay, escape-mode flag, roles); `SybilSettlement` (accepted-root chain, monotonic height, deposit-root binding, verifier-adapter dispatch); `OpenVmVerifierAdapter` (pins app-exe/vm commitments, checks the 32-word public values) + `UnsafeAcceptAllVerifierAdapter` for devnet. The state-transition public-input hash is mirrored field-for-field in `sybil-zk`.

**Doc drift:** `Oracle System.md`/`Oracle Lifecycle.md`/`Market Resolution.md` are accurate to the shipped oracle; `L1 Settlement and Vault.md` (`status: planned`) over-claims deposit-inclusion proving and escape-cash behavior relative to the code.

## Strengths

- The oracle is a real one-primitive win: adding Quorum/Optimistic/Predicate policies is a new enum arm, not a new trait; the inside-TEE (verify + state machine) vs outside-TEE (all I/O in untrusted signers) split is clean and enforced by `verify_signed_attestation` being the only channel in.
- The L1 ABI seam isolates log parsing and hash domains so the sequencer and indexer convert typed structs; the parser is strict against silent u64 truncation.
- Contracts are disciplined for pre-production: custom errors, granular pause roles, verifier versioning, domain-separated deposit tree, a nullifier that excludes `stateRoot` to prevent cross-root replay, and an adapter that pins app commitments.
- `state_transition_public_input_hash` is mirrored field-for-field between Rust and Solidity, reducing host/guest/contract hash divergence risk.

## Findings

| ID | Kind | Sev | Summary |
|----|------|-----|---------|
| [H5](01-critical-bugs.md) | bug | high | ZK guest never verifies deposit-leaf inclusion; deposit Merkle machinery is dead code; an operator can credit unbacked deposits and still prove |
| [H6](01-critical-bugs.md) | bug | high | `SybilSettlement` deposit-root checkpoint is bypassable at any not-yet-reached `depositCount` (zero default root passes) |
| [H14](01-critical-bugs.md) | bug | high | Emergency escape mode is a decorative flag — no escape-claim path exists |
| [H12](01-critical-bugs.md) | bug | high | Indexer applies unconfirmed L1 tip logs as irreversible credits (no confirmation depth, no reorg handling) |
| OL-1 | bloat | medium | Duplicated ~70-line `SettleNow` arm between `resolve_market` and `resolve_market_attested` (also [SEQ-6](11-sequencer.md)) |
| OL-2 | bug | medium | `resolve_from_attestation`/`evaluate_immediate` never assert `attestation.market_id == market_id` — safety rests entirely on the HTTP route reconstructing it; any other caller could settle the wrong market with a valid signature |
| OL-3 | design | medium | `requestWithdrawal` hashes `claimKind` into the public input but never checks it equals `CLAIM_KIND_NORMAL` — a future escape proof would be accepted through the normal queue |
| OL-4 | test-gap | medium | No cross-language golden-vector test for `deposit_leaf` or the public-input hashes — each side only tests itself, so an `abi.encode` layout drift wouldn't be caught until a live proof fails |
| OL-5 | debt | low | Large reserved-but-unbuilt oracle surface (Propose/Challenge/`check_finalization`/`AutomatedL0`) — real unexercised code, documented as intentional headroom — see [Theme 1](02-cross-cutting-themes.md) |
| OL-6 | inconsistency | low | Attestation `nonce` is signed but never stored or validated; replay is prevented only by the `AlreadyResolved` check, so the field is vestigial and its "timestamp_ms" contract is unenforced |
| OL-7 | doc-drift | medium | `L1 Settlement and Vault.md` is `status: planned` yet the contracts are implemented and tested, and it describes deposit-inclusion + escape-cash as present when neither is |

## Ambitious ideas

1. **Make deposit inclusion a first-class guest obligation:** feed the `L1Deposit` witness events through `sybil-l1-protocol`'s tree primitives inside `sybil-zk` to reconstruct `deposit_root` and assert each credited `(amount, account, id)` hashes into it. This gives the dead Merkle code its intended caller and closes the unbacked-deposit gap (H5) in one move.
2. **Collapse the legacy `Oracle` trait + `AdminOracle` facade:** route the unsigned admin path through the same `evaluate_immediate` (the admin key is already a registered feed), so `market_lifecycle` has exactly one resolution codepath — removing the trait indirection and the `resolve_market` vs `resolve_market_attested` duplication (OL-1).
3. **Add a `claimKind`-dispatched withdrawal entrypoint and a real escape-cash claim now** (even against the mock verifier) so the vault's safety story is code, not prose (H14, OL-3).
4. **Promote the hash domains into one shared source of truth** with generated Solidity + Rust constants and a checked-in golden-vector test, eliminating the three hand-maintained `abi.encode` implementations (sybil-zk, sybil-l1-protocol, Solidity) that must agree byte-for-byte (OL-4; see [Theme 6](02-cross-cutting-themes.md)).
5. **Give the dev indexer a persistent cursor + confirmation depth** and evolve it toward a signed indexer authority that attests `(deposit_id, leaf, root, confirmations)`, so deposit ingestion stops depending on a dev-only endpoint and gains reorg safety (H12).
