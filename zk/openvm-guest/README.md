# Sybil OpenVM guest

Standalone OpenVM guest program for Sybil state-transition verification, pinned
to `cargo-openvm` **v2.0.0-beta.2**. It lives outside the root Cargo workspace
so normal `cargo test --workspace` does not require the OpenVM prerelease
toolchain. See the architecture note **ZK Integration Path** (`docs/architecture/`)
for how the guest fits the validium proof pipeline.

The compiled guest yields two commitment hashes — `app_exe_commit` and
`app_vm_commit` — that `contracts/src/OpenVmVerifierAdapter.sol` pins at deploy
time. This directory is **consensus surface**: changing the guest source (or the
`crates/sybil-zk` it compiles by path) changes those hashes and requires an
on-chain redeploy.

## Three commitment records

| Record | Path | Role | Authority |
| --- | --- | --- | --- |
| Deployed pin | `OpenVmVerifierAdapter` constructor args (on-chain) | What the chain enforces | **Authoritative for consensus** |
| `commit.json` | `openvm/release/sybil-openvm-guest.commit.json` (committed) | Reviewable, diff-able record of the commitment | Source of truth for the hashes in-repo |
| Lock file | `guest.commitment.lock.json` (committed) | SHA-256 fingerprint of the guest **source tree** + a copy of the hashes | Staleness detector for the source |

Authority order: **deployed pin > `commit.json` > lock file.** The lock owns the
source-fingerprint role; `commit.json` owns the commitment-hash record.

Only the two small JSONs (`commit.json`, `baseline.json`) under `openvm/release/`
are committed. The large `.vmexe`, `app.pk`, `app.vk`, and `agg_prefix.pk`
binaries stay gitignored (see the repo `.gitignore`).

`scripts/zk-guest-fingerprint.sh --check` (run in CI) enforces two things:
1. the guest **source** still matches the lock's `source_sha256`, and
2. the lock's commitment hashes still equal the committed `commit.json`.

## Rebuild status and the 2026-07-03 divergence

A guest-target build break (owned arrays passed to the guest's `&[u8]`-taking
`Sha256::update` in `crates/sybil-zk/src/guest_commitments.rs` — compiles on
host via `sha2`'s `impl AsRef<[u8]>`, four `E0308`s on
`riscv32im-risc0-zkvm-elf`) entered with **SYB-170** and was caught and fixed
the same day under **SYB-208**. Host `cargo build`/`clippy`/tests and the
source-fingerprint gate never see the zkVM target, which is exactly why the
weekly `zk-rebuild` CI lane exists.

Rebuild is **deterministic**: two independent `just openvm-commit` runs
produce identical commitments (measured 2026-07-03). The committed
`commit.json` + lock now carry the **current-source** commitments
(`app_exe_commit 0x0094ea7a…`). The **deployed** OpenVmVerifierAdapter pin
still carries the **May-2026** build (`0x00796a20…`) — consensus bytes are
golden-vector-identical (SYB-170), but the artifact differs, so the next
devnet redeploy MUST update the adapter constructor args to the committed
values. Until then, on-chain verification of freshly built proofs would fail
against the old pin — expected and documented.

## Rebuild / redeploy procedure

When a rebuild *does* produce a new, correct commitment (after the guest build
is fixed and any intended source change lands):

```bash
just openvm-install     # cargo-openvm v2.0.0-beta.2 (one-time)
just openvm-commit      # rebuild + print app_exe_commit / app_vm_commit
```

Then:

1. Copy the regenerated `commit.json` + `baseline.json` into
   `openvm/release/` and commit them.
2. `scripts/zk-guest-fingerprint.sh --write` — refresh the lock's source
   fingerprint and commitment-hash copy.
3. Redeploy `OpenVmVerifierAdapter` with the new `appExeCommit` / `appVmCommit`
   (or deploy a new adapter and repoint `SybilSettlement`), and record the
   deployment.
4. Confirm `scripts/zk-guest-fingerprint.sh --check` is green before merging.

The guest is consensus surface: never regenerate the artifact in the fast CI
path — only host-check that the source is unchanged.
