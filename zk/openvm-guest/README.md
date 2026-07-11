# Sybil OpenVM guest

Standalone OpenVM guest program for Sybil state-transition verification, pinned
to `cargo-openvm` **v2.0.0**. It lives outside the root Cargo workspace
so normal `cargo test --workspace` does not require the OpenVM
toolchain. See the architecture note **ZK Integration Path** (`docs/architecture/`)
for how the guest fits the validium proof pipeline.

The compiled guest yields two commitment hashes — `app_exe_commit` and
`app_vm_commit` — that `contracts/src/OpenVmVerifierAdapter.sol` pins at deploy
time. This directory is **validity surface**: changing the guest source — or
any crate in its path-dependency closure (`crates/sybil-zk` →
`crates/sybil-verifier` → `crates/matching-engine`, plus
`crates/sybil-l1-protocol`, all compiled by path) —
changes those hashes and requires an on-chain redeploy.

## Three commitment records

| Record | Path | Role | Authority |
| --- | --- | --- | --- |
| Deployed pin | `OpenVmVerifierAdapter` constructor args (on-chain) | What the chain enforces | **Authoritative for validity** |
| `commit.json` | `openvm/release/sybil-openvm-guest.commit.json` (committed) | Reviewable, diff-able record of the commitment | Source of truth for the hashes in-repo |
| Lock file | `guest.commitment.lock.json` (committed) | SHA-256 fingerprint of the guest build recipe/compiler wrapper and **source tree + its path-dependency closure** (`crates/sybil-zk`, `crates/sybil-verifier`, `crates/matching-engine`, `crates/sybil-l1-protocol`), SHA-256 pins for the untracked OpenVM key material, and a copy of the commitment hashes | Staleness detector for build inputs, source, and key material |

Authority order: **deployed pin > `commit.json` > lock file.** The lock owns the
source-fingerprint role; `commit.json` owns the commitment-hash record.

Only the two small JSONs (`commit.json`, `baseline.json`) under `openvm/release/`
are committed. The large `.vmexe`, `app.pk`, `app.vk`, and `agg_prefix.pk`
binaries stay gitignored (see the repo `.gitignore`).

`scripts/zk-guest-fingerprint.sh --check` (run in CI) enforces three things:
1. the guest build recipe/compiler wrapper plus **source and its full path-dep
   closure** (`zk/openvm-guest/`, `crates/sybil-zk`,
   `crates/sybil-verifier`, `crates/matching-engine`,
   `crates/sybil-l1-protocol`) still match the lock's `source_sha256`, and
2. `openvm/app.pk`, `openvm/app.vk`, `openvm/agg_prefix.pk`, and
   `~/.openvm/internal_recursive.pk` are present and match the tracked
   `key_material` hashes for `openvm_tag: v2.0.0`, and
3. the lock's commitment hashes still equal the committed `commit.json`.

The key files themselves remain untracked and must be restored from the pinned
key-material set; they are not regenerable on the constrained build box. Do not
run OpenVM setup/keygen locally to replace them.

Hashing the whole closure (not just `zk/openvm-guest/`) closes the SYB-213 blind
spot: the SYB-196 newtype migration moved the commitment while a guest-only
fingerprint stayed green. The gate deliberately over-hashes — `#[cfg(test)]`
code in those crates affects the hash but not the built guest — because a false
"stale" (re-run `--write`) is far safer than a false "fresh".

## Rebuild status

A guest-target build break (owned arrays passed to the guest's `&[u8]`-taking
`Sha256::update` in `crates/sybil-zk/src/guest_commitments.rs` — compiles on
host via `sha2`'s `impl AsRef<[u8]>`, four `E0308`s on
`riscv32im-risc0-zkvm-elf`) entered with **SYB-170** and was caught and fixed
the same day under **SYB-208**. Host `cargo build`/`clippy`/tests and the
source-fingerprint gate never see the zkVM target, which is exactly why the
weekly `zk-rebuild` CI lane exists.

Rebuild is **deterministic and workspace-path-independent**: `just
openvm-commit` remaps the checkout root, Cargo home, and Rustup home in guest
compiler paths, so identical source reproduces from any workspace when the
untracked key files match the tracked `key_material` hashes (first measured
across workspaces 2026-07-10). The 2026-07-10 upgrade to OpenVM v2.0.0 final
moved both commitments because the final release replaces the beta proof
system with SWIRL and changes the SHA-2 VM AIR. The committed `commit.json` +
lock carry the current-source commitments. The RustCrypto SHA-2 0.11 / SHA-3
0.12 upgrade moved `app_exe_commit` to `0x004cc5ec…`; `app_vm_commit` remains
`0x00618538…`. A fresh genesis
and adapter redeploy must use these pins; older commitment compatibility is
not supported.

## Rebuild / redeploy procedure

When a rebuild *does* produce a new, correct commitment (after the guest build
is fixed and any intended source change lands):

```bash
just openvm-install     # cargo-openvm v2.0.0 (one-time)
just openvm-commit      # rebuild + print app_exe_commit / app_vm_commit
```

The weekly `zk-rebuild` CI gate uses the same path-remapped recipe and therefore
does not depend on the runner checkout path. The gate additionally requires the
four pinned key files above to be provisioned and hash-checked; a clean source
checkout alone is insufficient because those files are intentionally untracked.

Then:

1. Copy the regenerated `commit.json` + `baseline.json` into
   `openvm/release/` and commit them.
2. `scripts/zk-guest-fingerprint.sh --write` — refresh the lock's source
   fingerprint and commitment-hash copy.
3. Redeploy `OpenVmVerifierAdapter` with the new `appExeCommit` / `appVmCommit`
   (or deploy a new adapter and repoint `SybilSettlement`), and record the
   deployment.
4. Confirm `scripts/zk-guest-fingerprint.sh --check` is green before merging.

The guest is validity surface: never regenerate the artifact in the fast CI
path — only host-check that the source is unchanged.
