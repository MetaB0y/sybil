---
tags: [audit, code-quality, rust, clippy, unsafe, feature-flags]
layer: cross-cutting
status: current
date: 2026-07-18
last_verified: 2026-07-18
---

# Static lint, dead-code, and unsafe-code audit — 2026-07-18

## Decision

The authored Rust repository contains no `unsafe` blocks, functions, impls, or
traits. That property was accidental rather than enforced: ordinary Clippy
covered the default and all-feature graphs but not each declared feature in
isolation, standalone workspaces used `cargo check`, and lint suppressions did
not have to explain themselves.

This cluster turns the existing property into an executable policy. Root and
standalone Clippy gates now forbid unsafe code, selected feature-bearing crates
are linted once per declared feature, and every authored `allow` outside the
commitment-fingerprinted guest closure must carry a reason. The few closure
exceptions retain adjacent explanatory comments until an already-required
guest rebuild can change their source.

## Scope and evidence boundary

Reviewed:

- every authored Rust source in the root, fuzz, and three OpenVM workspaces;
- workspace/crate lint policy and every authored `allow` attribute;
- the normal, all-target/all-feature, no-default-feature, and isolated-feature
  build graphs;
- feature declarations in the solver and simulation crates;
- compatibility aliases and feature-only benchmark helpers; and
- production `unwrap`/`panic` policy in light of the completed recovery audit.

Generated build output and third-party dependencies were excluded from the
authored-unsafe inventory. Public exports were removed only after a
repository-wide consumer search. Compiler silence is not treated as proof that
every externally consumable public API is useful.

Architecture sources included the root and affected crate `AGENTS.md` files and
the maintained model, solver, sequencer, settlement, persistence, witness, and
state-root notes.

## Findings and dispositions

| ID | Severity | Finding | Disposition |
|---|---|---|---|
| SL-1 | Medium | Zero authored unsafe code was not an enforced invariant, and standalone workspaces were not linted. | Added `-F unsafe-code` to root, feature-lattice, CI, and standalone Clippy gates. |
| SL-2 | Medium | Default plus all-feature builds hid isolated-feature failures and dead helpers. | Added `just feature-lint` and the equivalent CI matrix for the feature-bearing crates. |
| SL-3 | Low | `matching-solver/parallel` enabled Rayon without the LP/decomposed surface that consumes it. | Made `parallel` imply `lp`; isolated-feature Clippy now exercises real parallel code. |
| SL-4 | Low | The unused `BatchSequencer` compatibility alias remained exported after the `BlockSequencer` rename. | Removed the alias and its re-export after finding no authored consumer. |
| SL-5 | Low | Suppression intent was carried in nearby prose, or not recorded at all, and could silently spread. | Added attribute reasons outside the commitment-fingerprinted closure. A source gate rejects new reasonless allows and pins the exact closure exception inventory. |
| SL-6 | Low | The simulation CLI did not compile without solver features; its default, test fixture, imports, and vector construction assumed at least one solver. | Made the no-solver graph compile with `All` as an empty comparison selection and feature-gated solver-only tests/imports. |
| SL-7 | Low | A retained-cash benchmark helper compiled dead when that feature was absent. | Feature-gated the helper and its `OnceLock` import. |

No GitHub issue was opened for these findings because every accepted defect was
bounded and fixed in this cluster.

## Panic and invariant policy

The consensus-facing engine and sequencer continue to inherit workspace denies
for `unwrap_used` and direct `panic`; tests may use assertions and panic-based
oracles. `expect` remains available for reviewed local invariants with useful
failure messages. Environment, transport, persistence, actor-lifecycle, and
remote-side-effect failures remain governed by the classifications and open
work in the error/recovery and supervision reports rather than a noisy global
restriction lint.

This policy intentionally does not equate every fail-stop invariant with a
recoverable service error.

## Verification

The completion gate is:

```text
cargo fmt --all -- --check
just lint-all
just feature-lint
just standalone-check
```

The strict profiles use `-D warnings` and `-F unsafe-code` universally.
`scripts/check-rust-allow-reasons.py` parses every authored Rust source,
requires `reason = ...`, and admits only the exact nine attributes already in
the pinned matching-engine/verifier/ZK closure and two OpenVM entrypoints.
Those sites retain adjacent reasons; changing their attributes alone correctly
trips the guest source fingerprint. The isolated-feature gate found the
simulation defects above before the cluster was closed.

One dependency emits a future-incompatibility notice from
`proc-macro-error2`; it is classified in the following dependency/build
supply-chain cluster rather than hidden here.

## Residual risk

Rust's compiler cannot prove that an exported library item has no external
consumer, and reachability alone cannot establish semantic usefulness. This
cluster therefore removed only the obsolete alias with a repository-wide
consumer witness. Broader public-surface design remains an ownership review,
not an automatic deletion exercise.
