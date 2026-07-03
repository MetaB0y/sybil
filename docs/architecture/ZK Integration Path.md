---
tags: [zk]
layer: verification
status: planned
last_verified: 2026-07-03
---

Sybil is designed for a Validium architecture: off-chain data, on-chain proofs. The exchange runs off-chain for performance, but every batch's correctness is attested by an OpenVM proof posted to Ethereum L1. The [[L1 Settlement and Vault|on-chain contracts]] store accepted [[State Root and Parent Hash|state roots]], custody collateral, and process proof-backed withdrawals — they never see individual orders, fills, or account balances.

The path from current architecture to ZK proofs is deliberately incremental. The [[Four-Layer Verification|4-layer verification logic]] already exists and runs on every batch in tests. This same logic — match validity, settlement correctness, block integrity, order validation — is exactly what the ZK circuit will enforce. The [[Block Witness]] is designed as the circuit's input: a self-contained package of everything needed to verify a state transition. OpenVM is the chosen proving stack: the guest program is Rust, and the L1 contracts verify proofs through an OpenVM Solidity adapter. The current OpenVM integration is pinned to the 2.0 prerelease line, currently `v2.0.0-beta.2`.

Several architectural choices were made specifically for ZK-friendliness. [[Nanos and Integer Arithmetic|All-integer arithmetic]] maps directly to finite field operations (no floating-point emulation needed). The [[State Root Schema|state commitment]] uses a SHA-256 qMDB root so membership/exclusion checks can be wrapped in settlement and withdrawal proofs without forcing Solidity to understand qMDB directly. The [[Payoff Vectors|payoff vector]] representation keeps orders as small fixed-size arrays rather than variable-length structures, simplifying circuit layout. The verification layers are independent, allowing the circuit to be decomposed and parallelized. The OpenVM guest boundary now exists; prover orchestration, proof service operations, and DA semantics are still future work. The rollout is planned in four phases:

1. **Phase 1:** 4-layer verification logic runs in Rust, exercised in tests and `matching-sim`.
2. **Phase 2 (started):** Compile the verification logic into an OpenVM guest program. The current guest verifies public input binding, post-state qMDB proofs, event-root recomputation, witness-root binding, and match/settlement/order logic. Local app-proof generation works for the smoke proof job; generated verifier-contract integration remains follow-up work.
3. **Phase 3:** Prover service that takes a `BlockWitness` and produces an OpenVM proof per batch. Runs alongside the sequencer.
4. **Phase 4:** [[L1 Settlement and Vault|L1 settlement and vault contracts]] on Ethereum. Store accepted state roots, verify proofs on-chain, custody deposits, and process conservative proof-backed exits; full operator disappearance recovery depends on the DA/operator replacement design.

## Current OpenVM Boundary

The first guest boundary is intentionally narrow:

- `crates/sybil-zk/` owns the public input binding shared by host tests and
  the guest. Its `guest_commitments` module contains the OpenVM-safe
  qMDB/event-root verifier subset.
- `crates/sybil-prover/` owns host-side prover input construction. Its core
  API is a serializable `StateTransitionProofJob`: a committed
  `BlockWitness`, job identity metadata, and ordered post-state qMDB proofs.
  The optional `sequencer-store` feature adds the `witgen` subcommands that
  collect this job from sequencer storage; the default job-to-guest conversion
  has no dependency on `matching-sequencer`.
- `crates/sybil-verifier::commitments` owns the canonical state, event, and
  witness byte schemas used by native verification, witgen, and the guest.
- `crates/sybil-prover/` is the proof-job CLI/service boundary. It consumes
  serialized `StateTransitionProofJob` values, validates them, runs the native
  `sybil-zk` transition verifier before emitting serialized
  `StateTransitionGuestInput` artifacts, and reports the public input hash. It
  also encodes `submitStateRoot` calldata for the L1 settlement contract and
  can emit a file-based `eth_sendTransaction` request for large proof calldata.
  When given an OpenVM EVM proof JSON file, it
  converts OpenVM's proof fields into the ABI payload expected by
  `OpenVmVerifierAdapter`. Its local worker mode scans a proof-job directory,
  writes per-block guest input, DA manifest/payload, public-input hash, and
  `status.json` artifacts. Its local API mode serves those durable artifacts
  through `GET /proofs/{height}`. Real OpenVM proof orchestration is still
  follow-up service work.
- `zk/openvm-guest/` is a standalone OpenVM package pinned to
  `v2.0.0-beta.2`. It is outside the root Cargo workspace so normal Rust
  checks do not require the OpenVM prerelease CLI or generated artifacts.
- `zk/openvm-tools/` is a standalone host-tool package pinned to the same
  OpenVM tag. It converts prepared `StateTransitionGuestInput` MessagePack into
  the JSON/hex input format expected by `cargo openvm run` and
  `cargo openvm prove`. It stays outside the root workspace because the encoder
  must call `openvm::serde` from the pinned OpenVM git tag to match the guest
  byte-for-byte, and folding it into `sybil-prover` would pull that prerelease
  dependency graph into normal checks and production builds.
- The OpenVM CLI input is a raw byte stream containing little-endian
  `openvm::serde` words. The guest reconstructs those words and decodes
  `StateTransitionGuestInput` with `openvm::serde::from_slice`, then derives
  the canonical typed state leaves from the block witness, verifies
  ordered-current-qMDB
  key/value proofs for those leaves against the public `new_state_root`,
  verifies that each qMDB `next_key` pointer forms the exact sorted key ring,
  recomputes the keyless-qMDB `events_root` from canonical event leaf bytes,
  recomputes `witness_root = BLAKE3("sybil/witness" || witness_bytes)`,
  then verifies the match, settlement, and order-validation layers through
  `sybil-verifier` with qMDB block-runtime features disabled. The guest uses
  small local SHA-256/MMR verifiers for the qMDB proof/root shapes so OpenVM
  does not need to link commonware storage or its native cryptography
  dependencies.
- The guest reveals
  `keccak256(abi.encode("sybil/openvm/state-transition/v1", ...))` as the
  public value expected by `SybilSettlement`.
- `contracts/src/OpenVmVerifierAdapter.sol` wraps the generated OpenVM Halo2
  verifier contract. It pins `appExeCommit` and `appVmCommit`, checks the first
  OpenVM user public value equals the settlement public-input hash, requires
  the remaining default public-value words to be zero, then calls
  `IOpenVmHalo2Verifier.verify`.
- The current state-root proof is a post-state exact-keyspace proof: every
  witness leaf must be in qMDB, and hidden extra leaves are rejected because
  they alter the verified `next_key` ring.
- `da_commitment` is a concrete [[Data Availability]] envelope over the
  canonical witness payload, block height, state root, witness root, payload
  length, and provider-reference hash. The smoke path binds a deterministic
  file provider reference; production DA networks can use the same hash slot
  once their reference encoding is defined. This file/witness path is prover
  scaffolding, not the final encrypted recovery DA design. `SybilSettlement`
  stores the proven commitment but does not judge provider availability
  on-chain.

## Guest Commitment Records

The guest's compiled artifact yields two commitment hashes — `app_exe_commit`
and `app_vm_commit` — that `contracts/src/OpenVmVerifierAdapter.sol` pins at
deploy time. Because the compiled artifacts under `zk/openvm-guest/openvm/` are
large binaries, there are **three** records of these hashes, with distinct
roles and authority (SYB-208):

1. **Deployed pin (authoritative for consensus).** The `appExeCommit` /
   `appVmCommit` constructor args baked into the deployed
   `OpenVmVerifierAdapter`. This is what the chain actually enforces. It is the
   ground truth; everything else is a record *of* it. The current pin is the
   May-2026 build.
2. **Committed `commit.json` (reviewable source of truth).**
   `zk/openvm-guest/openvm/release/sybil-openvm-guest.commit.json` (plus
   `…baseline.json`) is now committed — the small, human-reviewable JSON that
   records which commitment the source *should* produce. Committing it gives
   the repo a persistent, diff-able record instead of relying solely on the
   deployed constructor args. The large `.vmexe`/`.pk`/`.vk` binaries stay
   gitignored.
3. **`guest.commitment.lock.json` (source fingerprint).** A SHA-256 fingerprint
   of the guest *source tree* plus a copy of the commitment hashes. Its job is
   staleness detection: `scripts/zk-guest-fingerprint.sh --check` fails if the
   guest source changes without regenerating the pin, and now also cross-checks
   that its copied hashes still equal `commit.json`.

**Authority order:** deployed pin > `commit.json` > lock file. The lock owns the
source-fingerprint role; `commit.json` owns the commitment-hash record.

### Rebuild status: deterministic; deployed pin diverges until next redeploy

The fingerprint gate only covers `zk/openvm-guest`'s own source tree — it does
**not** cover the transitively-compiled `crates/sybil-zk` (the guest compiles it
by path). So the gate can be green while the produced commitment has drifted.
[[Canonical Serialization|SYB-170]] consolidated the canonical byte/hash code
into `sybil-zk` and flagged exactly this.

Measured under SYB-208 (2026-07-03): SYB-170 had additionally broken the
guest-target build itself — `guest_commitments.rs` passed owned `[u8; N]`
arrays to `Sha256::update`, which compiles on the host (`sha2` takes
`impl AsRef<[u8]>`) but fails on `riscv32im-risc0-zkvm-elf`, where the guest's
`Sha256::update` takes `&[u8]`. Host builds, workspace tests, and the
fingerprint gate never see the zkVM target, so everything stayed green — the
blind spot the `zk-rebuild` lane exists to close. Fixed the same day (borrowed
args, same commit as the SYB-208 landing).

With the fix, rebuilds are **deterministic** (two independent
`just openvm-commit` runs → identical commitments), so the `zk-rebuild` CI
lane (below) is a **hard gate**: regenerated commits must equal the committed
`commit.json`. The committed `commit.json` + lock now carry the
**current-source** commitment (`app_exe_commit 0x0094ea7a…`); the **deployed**
adapter still pins the **May-2026** build (`0x00796a20…`). Consensus bytes are
golden-vector-identical, but the artifact differs — the next devnet redeploy
must update the adapter constructor args to the committed values (freshly
built proofs will not verify against the old pin until then).

### Redeploy procedure (when the commitment legitimately changes)

When a guest rebuild *does* produce a new, correct commitment (after the guest
build is fixed and any intended source change lands):

1. `just openvm-commit` → regenerate the artifacts and read the new
   `app_exe_commit` / `app_vm_commit`.
2. Copy the new `commit.json` + `baseline.json` into
   `zk/openvm-guest/openvm/release/` and commit them.
3. `scripts/zk-guest-fingerprint.sh --write` → refresh the lock's source
   fingerprint and commitment-hash copy.
4. Redeploy `OpenVmVerifierAdapter` with the new pin (or deploy a new adapter
   and repoint `SybilSettlement`), then record the deployment.
5. Confirm `scripts/zk-guest-fingerprint.sh --check` is green (source ↔ lock ↔
   commit.json all agree) before merging.

See `zk/openvm-guest/README.md` for the same procedure alongside the build
commands.

Commands:

```bash
just openvm-install
just openvm-guest-check
just openvm-guest-build
just openvm-keygen-app
just openvm-keygen
just openvm-setup
just openvm-setup-evm-download
just openvm-commit
just zk-smoke
just zk-smoke true
just witgen-smoke-job /tmp/sybil-smoke.redb /tmp/job.msgpack
just witgen-export-latest data/sybil.redb /tmp/job.msgpack
just prover-inspect /tmp/job.msgpack
just prover-prepare /tmp/job.msgpack
just prover-prepare-file-da /tmp/job.msgpack /tmp/sybil-guest-input.msgpack /tmp/sybil-da /tmp/sybil-da-manifest.json /tmp/sybil-public-input-hash.hex
just prover-publish-da /tmp/sybil-guest-input.msgpack /tmp/sybil-da-witness.bin /tmp/sybil-da-manifest.json
just prover-worker-once /tmp/sybil-prover-jobs /tmp/sybil-prover-artifacts
just prover-worker /tmp/sybil-prover-jobs /tmp/sybil-prover-artifacts
just prover-serve /tmp/sybil-prover-artifacts /tmp/sybil-prover-jobs 127.0.0.1:3002
just openvm-input /tmp/sybil-guest-input.msgpack /tmp/sybil-openvm-input.json
just openvm-run /tmp/sybil-openvm-input.json
just openvm-prove-app /tmp/sybil-openvm-input.json /tmp/sybil-openvm.app.proof
just openvm-verify-app /tmp/sybil-openvm.app.proof
just openvm-prove-evm /tmp/sybil-openvm-input.json /tmp/sybil-openvm.evm.proof
just openvm-verify-evm /tmp/sybil-openvm.evm.proof
just prover-submit-state-root 0xYourSettlement /tmp/sybil-guest-input.msgpack /tmp/sybil-openvm.app.proof
just prover-submit-state-root-rpc 0xYourSettlement 0xYourSender
just prover-submit-state-root-evm-rpc 0xYourSettlement 0xYourSender
```

`just zk-smoke` is the normal local integration smoke. It creates a one-block
sequencer fixture, exports a portable proof job, prepares `StateTransitionGuestInput`,
writes the file-backed DA payload and manifest, encodes OpenVM input JSON,
builds/transpiles the guest, and runs the guest in OpenVM. `just zk-smoke true`
additionally runs app keygen, app proof generation, and app proof verification.
It deliberately never runs EVM setup/proving.

`just prover-worker-once` is the first standalone prover-node boundary for
SYB-29. It treats `jobs_dir/*.msgpack` as an inbox of exported
`StateTransitionProofJob` values and writes deterministic per-block artifact
directories under `artifacts_dir`. Each directory contains prepared guest
input, proof-bound file-DA artifacts, public-input hash, and `status.json`
with `proof_status: "not_started"`. The worker is intentionally file-based so
sequencer export, prover preparation, DA publication, and future proof
generation can evolve independently. `just prover-serve` exposes the current
artifact store over HTTP; `GET /proofs/{height}` returns the corresponding
`status.json` once the worker has prepared that height. It also exposes
`/metrics`, derived from the durable artifact store and optional job inbox, so
Grafana/vmalert can track latest prepared height, queue depth, artifact age,
and failed proof statuses without depending on worker process memory.

Anvil/devnet contract plumbing can use
`contracts/src/dev/UnsafeAcceptAllVerifierAdapter.sol`. That adapter accepts
every proof but keeps the final `IOpenVmVerifierAdapter.verify(proof,
publicInputHash)` call boundary intact. This lets local devnets submit real
OpenVM app-proof bytes or dummy bytes while EVM verifier generation remains
heavy. It must not be used for production or public testnet deployments.

## Key Properties
- Validium: off-chain data, on-chain proofs
- OpenVM: Rust guest program with on-chain verification through the Solidity SDK
- State roots on Ethereum L1 — proofs attest each state transition
- Escape hatch: conservative exits plus DA-backed recovery design
- All architectural choices (integer arithmetic, typed state roots, fixed-size arrays) are ZK-motivated
- Status: partial implementation; local guest execution, app proof generation,
  unsafe/mock-verifier settlement submission, and prover observability work,
  while production proof orchestration and generated on-chain verifier
  integration remain open.

## Where This Lives
> `crates/sybil-verifier/` — verification logic that will become the ZK circuit
> `crates/sybil-prover/` — portable proof jobs, OpenVM guest input construction, optional sequencer-store export, local worker/API, settlement calldata encoder, and future proof orchestrator
> `crates/sybil-zk/` — public input hash and guest-safe transition verifier
> `zk/openvm-guest/` — OpenVM 2.0 beta guest entrypoint
> `zk/openvm-tools/` — OpenVM CLI input encoder for prepared guest inputs
> `contracts/src/OpenVmVerifierAdapter.sol` — L1 adapter from Sybil public-input hash to OpenVM Halo2 verifier calls
> `crates/matching-sequencer/src/qmdb_state.rs` — persisted typed-state qMDB roots and proofs used by witgen

## See Also
- [[Proof Architecture]] — authenticated data layer for arbitrary account-level proofs
- [[Four-Layer Verification]] — the checks that become the circuit
- [[Block Witness]] — the circuit's input
- [[State Root and Parent Hash]] — anchors the on-chain proof chain
- [[L1 Settlement and Vault]] — contract boundary for accepted roots and bridge custody
- [[Canonical Serialization]] — byte layout the circuit consumes
- [[Nanos and Integer Arithmetic]] — ZK-friendly arithmetic
