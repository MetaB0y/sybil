---
tags: [audit, code-quality, dependencies, supply-chain, reproducibility, security]
layer: cross-cutting
status: current
date: 2026-07-18
last_verified: 2026-07-18
---

# Dependency and build supply-chain audit — 2026-07-18

## Decision

The repository had strong lockfile and generated-artifact foundations but no
single current-advisory gate. As a result, reproducible stale graphs contained
known vulnerabilities across Rust, Node, Arena, and the legacy visualization
environment.

All actionable advisories were removed by reviewed lock refreshes. `just
audit-dependencies` now queries RustSec, npm, and PyPA for every maintained
lockfile. Three unreachable or non-product-runtime Rust graphs require upstream
changes or the next scheduled guest rebuild and are explicit, executable
exceptions tracked by GitHub
[#194](https://github.com/MetaB0y/sybil/issues/194).

## Scope and evidence boundary

Reviewed:

- the root, fuzz, and three OpenVM Cargo lockfiles;
- Arena and visualization uv locks;
- the frontend pnpm lock, age policy, build allowlist, and registry integrity;
- Cargo feature unification and duplicate/source inventories;
- generated OpenAPI client ownership and its pinned generator;
- Rust, Node, Python, Foundry, Docker-build, and GitHub Actions tool roots;
- external-package license metadata; and
- existing issues for generated artifacts and immutable deployed images.

Current advisory results are date-dependent evidence, not deterministic build
evidence. The audit command is therefore separate from `just check-all`.

## Initial findings

| Ecosystem | Initial result | Disposition |
|---|---|---|
| Root Cargo lock | 2 vulnerabilities, 1 unsound warning, 5 other warnings | Refreshed the compatible graph and Alloy macros. SCIP build-only and Ark R1CS tracing exceptions remain; the unsound dependency disappeared. |
| Fuzz Cargo lock | 1 vulnerability, 1 unsound warning, 2 other warnings | Aligned with the exact production Commonware graph; the shared Ark tracing exception remains and the unsound dependency disappeared. |
| OpenVM locks | `lru 0.12.5` unsound warning | Pinned OpenVM v2 host proc-macro graph does not call the affected `IterMut`; tracked in #194. |
| Frontend pnpm lock | 1 high and 1 moderate Vite advisory | Forced Vite 8.0.16 with an exact, version-specific exception to the seven-day package-age policy. |
| Arena uv lock | 40 advisory records across 11 packages | Refreshed the resolved graph; audit and 318 tests pass. |
| Visualization uv lock | 36 advisory records across 9 packages | Refreshed the resolved graph; audit passes. |

## Changes

### Current, reviewable dependency graphs

- Refreshed the root and fuzz Cargo graphs, including Alloy macros
  `1.6.0 → 1.6.1`, while keeping every Commonware crate at 2026.5.
- Kept the fingerprinted validity manifests unchanged and aligned the root and
  fuzz lockfiles on Commonware 2026.5. The advisory gate asserts that parity so
  an isolated lock refresh cannot silently select a source-incompatible graph.
- Removed the future-incompatible, unmaintained `proc-macro-error2` graph from
  fuzz. Root and fuzz deliberately retain the Ark R1CS tracing package
  described below.
- Refreshed Arena and visualization uv graphs to patched releases.
- Updated Vitest and overrode its Vite graph to 8.0.16. The exception is
  narrowly listed under `minimumReleaseAgeExclude`; future Vite releases still
  wait seven days.

### Repeatable audit and build tools

- Added `scripts/check-dependency-advisories.sh` and
  `just audit-dependencies`. The gate:
  - scans all five Cargo locks and denies unsound advisories by default;
  - audits the pnpm graph at moderate severity or above; and
  - exports both uv graphs with hashes and audits them with pinned
    `pip-audit 2.10.1`.
- Pinned Docker's `cargo-chef` installation to 0.1.77 and the Arena uv image to
  0.11.28 instead of `latest`.
- Corrected local frontend guidance to pnpm 11, matching `package.json` and CI.

### Existing boundaries retained

- The only non-registry Rust source is the OpenVM v2 tag resolved to commit
  `15a7ab6b…` in every Cargo lock.
- The pnpm graph carries registry integrity hashes, blocks unapproved install
  scripts, rejects transitive exotic sources by default, and enforces a
  seven-day minimum release age.
- The generated Python SDK uses a pinned `openapi-python-client 0.29.0` and a
  deterministic full OpenAPI document. Whether to move the generated tree out
  of ordinary source review remains the explicit packaging trade-off in
  [#118](https://github.com/MetaB0y/sybil/issues/118).
- All 748 external packages in root Cargo metadata publish license metadata.
  The 27 packages without license metadata are the private workspace packages;
  this agrees with the README's deliberate all-rights-reserved state.

## Explicit exceptions

`RUSTSEC-2020-0071` is present only because bundled `scip-sys 0.1.28` declares
an unused direct `zip 0.5` build dependency. Its extraction code uses
`zip-extract`/`zip 0.6` and does not call the vulnerable local-time functions.
This graph exists only when compiling the feature-gated research MILP solver.

`RUSTSEC-2025-0055` is present through `ark-relations` in the pinned Commonware
2026.5 validity graph. The advisory concerns ANSI field formatting in
`tracing-subscriber` 0.2; Sybil neither initializes Ark's R1CS trace layer nor
formats its fields. A Commonware 2026.7 trial passed vectors but required source
edits inside the fingerprinted guest closure, so this dependency-only cluster
rejected that trial instead of refreshing a proof commitment. The upgrade is
deferred to the next scheduled guest rebuild under #194.

`RUSTSEC-2026-0002` is present only through `num-prime` in OpenVM v2
host-side algebra proc-macro expansion. The affected API is `lru::IterMut`; the
Sybil/OpenVM graph does not invoke it.

The audit script names all three exceptions and still fails for every other
vulnerability or unsound advisory. #194 owns their upstream removal; vendoring
forks into Sybil would be a worse boundary for these presently unreachable
APIs.

## Deferred, deduplicated work

- [#194](https://github.com/MetaB0y/sybil/issues/194): eliminate all three RustSec
  exceptions through upstream SCIP/OpenVM graphs.
- [#195](https://github.com/MetaB0y/sybil/issues/195): pin GitHub Actions to
  immutable revisions and automate advisory/lock refresh.
- [#65](https://github.com/MetaB0y/sybil/issues/65): deploy immutable image
  digests with rollback; mutable deployment image tags are not duplicated here.
- [#118](https://github.com/MetaB0y/sybil/issues/118): choose the generated SDK
  packaging boundary.

All new issues are in Project 1 as Todo / Backlog / Medium.

## Verification

Successful evidence includes:

```text
just audit-dependencies
cargo check --workspace --all-targets --all-features
cd arena && uv run ruff check . && uv run pytest -q
cd frontend/web && pnpm install --frozen-lockfile && pnpm audit
```

Golden-vector, guest-fingerprint, and validity-pin checks ensure the dependency
refresh did not move the consensus boundary. No guest proof was generated.

The legacy `viz` tree has no test suite and its pre-existing Ruff baseline has
53 style/unused-import findings. That is not concealed as dependency-update
evidence; its source ownership is assessed later in the documentation and
simplicity closeout.
