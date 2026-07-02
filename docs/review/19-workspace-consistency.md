# Rust Workspace Consistency

**Scope:** the 17-crate Cargo workspace — boundaries, dependencies, error/log/config idioms, naming

## Verdict

The workspace is in decent shape on the axes that matter most (no `anyhow`, almost no `unsafe`, typed errors, uniform metrics namespace, strong test counts, CI with `-Dwarnings`). It has one god-crate, a broken feature graph that silently forces bundled SCIP into every build, consensus byte-layouts duplicated across crates held together only by comments, and a long tail of cross-cutting inconsistencies: three actor idioms, three logging styles, two dependency-version spellings, and a crate-naming split (`matching-*` vs `sybil-*`) that no longer encodes a real boundary.

## Architecture as built

17 crates (`resolver = 3`, edition 2021, all v0.1.0, pinned toolchain 1.94.1 matching CI), plus two deliberately-separate workspaces (`fuzz/`, `zk/openvm-guest`). The dependency DAG is genuinely clean at the top: `matching-engine` is the foundation with only a `serde` dep; middle-tier crates depend on it but not each other; `matching-sequencer` composes solver + oracle + verifier; `sybil-api` wraps the sequencer. The ZK boundary is carefully layered (`sybil-verifier`'s commonware deps are feature-gated; `sybil-zk` consumes it `default-features = false` for guest safety; `sybil-witgen`'s sequencer dep is feature-gated so the prover never links the node).

The runtime topology of the `sybil-api` process is worth noting: **three tokio runtimes** — the main axum runtime, plus two dedicated OS threads each running a commonware-tokio runtime (qMDB accounts, qMDB state) bridged by channels — with redb `fsync` happening inline in the actor's async context.

## Strengths

- **Zero `anyhow`** in the workspace; libraries use `thiserror` consistently with well-designed error enums.
- Only **3 `unsafe` sites**, all justified and localized; only 22 `allow()` attributes, mostly in test helpers.
- Uniform `sybil_*` Prometheus namespace; CI runs fmt-check, clippy `--all-features`, and tests under `-Dwarnings` with the pinned toolchain.
- Wire types centralized once in `sybil-api-types` and reused by three downstream crates — no REST DTO duplication.
- Careful guest-safety feature-gating so the prover never links the node.
- Strong test discipline (matching-sequencer 260 test fns, sybil-verifier 82, matching-engine 55, plus integration + insta snapshots).
- `sybil-l1-indexer` is a model service crate: 455 lines, typed errors, dot-namespaced structured logs, saturating arithmetic.

## Findings

| ID | Kind | Sev | Summary |
|----|------|-----|---------|
| [D8](01-critical-bugs.md) | bug | high | `matching-solver` feature graph broken: `milp` without `lp` fails to compile (`milp.rs` imports gated `lp_solver`); CI never catches it because `--all-features` unions `lp` in |
| WK-1 | bloat | high | `matching-sim` hard-enables `matching-solver/milp`, forcing bundled SCIP (large C build) into every `just build`/`just test`, and the documented `--features milp` command errors |
| WK-2 | design | high | `matching-sequencer` is a god-crate (production node + agent simulator + two storage engines + actors + crypto in 23k LOC); sim-only deps compile into every prod build — see [11-sequencer](11-sequencer.md) |
| WK-3 | debt | medium | Consensus byte-layouts/hashes duplicated across crates with divergence already visible (`hash_header` ×3, digest encoders, `bridge_account_key`, two disagreeing reservation encoders) — see [Theme 6](02-cross-cutting-themes.md) |
| WK-4 | bug | medium | `u64→i64` wrap in dev-mode account create/fund allows negative balances on the public devnet — see [12-api](12-api.md) |
| WK-5 | bug | medium | f64 in order-book reservation rescaling violates the all-integer convention and loses precision > 2^53 nanos — see [SEQ-2](11-sequencer.md) |
| WK-6 | design | medium | Three actor idioms + three tokio runtimes in one process; redb `fsync` runs inline in the actor's async context |
| WK-7 | inconsistency | medium | Workspace dependency management barely used: 3 workspace deps, `serde` spelled two ways ×15, `tokio` 9× with 5 feature sets, an RC crypto crate (`p256 = 0.14.0-rc.7`) copy-pasted into 4 manifests, no `[workspace.package]` or `[workspace.lints]` |
| WK-8 | inconsistency | medium | Error-handling fractures at the binary boundary: `Result<_, String>` in `sybil-api/main.rs`, stringly-typed `SequencerError` variants erasing typed sub-errors, `StoreError::Qmdb(String)` |
| WK-9 | inconsistency | medium | Logging inconsistent across services: `sybil-prover` has no tracing at all (println!/eprintln!); env-filter defaults differ (info vs crate-scoped vs ERROR-by-default); message conventions split (dot-namespaced vs free text) |
| WK-10 | inconsistency | medium | Crate naming boundary `matching-*` vs `sybil-*` is unprincipled (matching-sequencer depends on 4 `sybil-*` crates); binary names collide (`sybil-witgen-cli` builds a bin named `sybil-witgen`; two different simulators named `sybil-sim`/`matching-sim`) |
| WK-11 | inconsistency | low | Env/config divergence: unprefixed collision-prone vars in polymarket, `SYBIL_URL` vs `SYBIL_API_URL`, config read outside the config struct, a hand-duplicated `impl Default` mirroring clap defaults, empty-String sentinels instead of `Option<PathBuf>` |
| WK-12 | bloat | low | 20 hand-rolled UNIX-epoch-millis computations across 13 files |
| WK-13 | design | low | Single-impl trait abstractions (`Oracle`, `AccountStateStore`) add indirection without a second implementation |
| WK-14 | test-gap | low | `matching-sim` (2,183 lines) and `sybil-prover` (2,493 lines) are untested single-file binaries containing consensus-adjacent encoding logic |

## Ambitious ideas

1. **Re-layer the workspace by dependency truth, not prefix.** `matching-{engine,solver,scenarios,sim}` = pure market math (no `sybil` deps, no tokio); `sybil-canonical` absorbs **all** consensus byte-layouts/hashes/domains (deleting every "must match" comment); `sybil-sequencer` (renamed, node-only after evicting the sim harness); `sybil-sim` (the agents/scenario/sim harness + both sim bins); `sybil-proof` = merge `sybil-witgen` + `sybil-witgen-cli` + `sybil-prover` into one crate with lib + bins. Net: 17 → ~12 crates, and the crate graph becomes the architecture diagram.
2. **Adopt workspace-wide manifest inheritance in one sweep:** `[workspace.package]` (version/edition — jump to edition 2024 while the toolchain is 1.94), `[workspace.dependencies]` for all ~20 shared deps, and `[workspace.lints]` with `clippy::unwrap_used = "deny"` for lib crates plus a `clippy.toml` `disallowed-types` banning `f64`/`f32` in the core modules. Turns the two strongest conventions (all-integer, no-panic libs) from prose into compile errors.
3. **Introduce real money/quantity newtypes** (`Nanos(u64)`, `SignedNanos(i64)`, `Qty(u64)` with checked ops and `ceil_mul_ratio`) replacing the bare `type Nanos = u64` alias. The order-book f64 bug becomes unwritable; the `u64→i64` API wraps become `TryFrom` sites by construction.
4. **Unify on a single actor substrate:** replace ractor with the ryhl pattern (the polymarket crate already proves the house style scales) plus a ~100-line supervisor util; move redb commits and qMDB onto one dedicated storage actor thread so the process runs exactly one tokio runtime with explicit blocking boundaries — three runtimes → one.
5. **Make CI prove the feature lattice and the docs:** add `cargo hack --each-feature`, a no-default-features build of every crate, and a doc-test that executes the command blocks in AGENTS.md (the broken `--features milp` command would have failed the day it drifted — D8, WK-1).
