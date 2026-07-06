---
tags: [review, simplification, tech-debt]
layer: core
status: planned
last_verified: 2026-07-06
---

# Sybil Exchange — Simplification Plan

Read-only survey of `crates/` (main, 2026-07-06). No code changed. This plan
goes to human review before any deletion.

## Executive summary

**The "78k Rust lines" is real but misleading.** `tokei` reports ~78.6k code
lines (94.5k raw). Breakdown of raw `.rs`:

| Bucket | Raw lines | Share |
|---|---:|---:|
| Inline `#[cfg(test)]` modules | ~33,000 | 35% |
| Integration tests (`crates/*/tests/`) | 7,864 | 8% |
| Benches | 128 | <1% |
| Sim/scenario crates (`matching-sim`, `sequencer-sim`, `matching-scenarios`) | 4,735 | 5% |
| **Actual product code** | **~48,900 raw / ~40k tokei-code** | **~52%** |

**~48% of the Rust is tests/sim/benches.** For what this is — a ZK-verified
frequent-batch-auction prediction market with its own authenticated state DB
(QMDB), a from-scratch verifier + prover, an L1 bridge, a full API, and a live
Polymarket integration — the product code is **appropriately sized, not
bloated.** Wins are real but modest, and mostly about cognitive load, not
deletion. Do not chase the 78k by cutting tests; nearly half is a healthy suite
doing real work (86 `#[cfg(test)]` modules, proptest, a solver conformance
harness differential-testing against the verifier).

### Top 3 wins
1. **Feature-gate the research solver zoo** — ~1,731 code lines compiled into
   the *production* sequencer but never instantiated. Zero behavior change.
2. **Delete the empty `crates/sybil-witgen-cli/`** — dead scaffold, not a
   workspace member.
3. **Split the three sequencer god-modules** (`sequencer.rs` 7,164, `store.rs`
   6,683, `actor.rs` 6,006). No lines saved; biggest readability win.

## Ranked table

| # | Item | Est. lines | Verdict | Risk | Consensus/guest? |
|---|---|---:|---|---|---|
| 1 | Gate `eg_solver`+`iterative_lp_solver`+`viz` behind a `research` feature | ~883 out of prod compile | FEATURE-GATE | low | No |
| 2 | Gate `decomposed.rs` behind `research` (deferred roadmap) | ~848 out of prod compile | FEATURE-GATE / KEEP-DEFERRED | low | No |
| 3 | Delete empty `crates/sybil-witgen-cli/` | 0 (clutter) | REMOVE | low | No |
| 4 | Keep `conic`+`milp` behind existing features (already prod-excluded) | ~1,900 already gated | KEEP-GATED | low | No |
| 5 | Split `sequencer.rs`/`store.rs`/`actor.rs` god-modules | 0 saved | SPLIT | med | Adjacent |
| 6 | Order-type generality (payoff vectors/bundles/multi-outcome/conditionals) | ~300–500 helper-only | KEEP-DEFERRED | **HIGH** | **YES — Order is in the guest commitment** |
| 7 | `#[allow(dead_code)]` (12) + TODOs (9) | negligible | KEEP | low | No |

**Net honest saving: ~1.7k lines gated out of the sequencer compile (items
1–2). No correctness-preserving hard deletions of product code beyond one empty
crate.**

## 1 — The solver zoo (biggest win)

Production path is unambiguous: `matching-sequencer/Cargo.toml:9` pulls
`matching-solver` with **only `features=["lp"]`**, and instantiates
**`LpSolver::new()` and nothing else** (`sequencer.rs:914`, `:921`). Every other
solver appears only in `matching-sim`, `tests/solver_conformance.rs`, and
`benches/solver_bench.rs`.

| Solver | File | Code | Prod? | In prod binary? | Verdict |
|---|---|---:|---|---|---|
| LP (HiGHS + SLP) | `lp_solver.rs` | ~887 | **YES** | yes | **PRODUCTION — KEEP** |
| EG (Eisenberg-Gale/Frank-Wolfe) | `eg_solver.rs` | ~389 | no | yes (under `lp`) | FEATURE-GATE |
| IterLP | `iterative_lp_solver.rs` | ~231 | no | yes (under `lp`) | FEATURE-GATE |
| Decomposed | `decomposed.rs` | ~848 | no | yes (under `lp`) | FEATURE-GATE / KEEP-DEFERRED |
| Conic (Clarabel) | `conic_solver.rs` | ~481 | no | no (`conic` off) | KEEP-GATED (oracle) |
| MILP (SCIP) | `milp.rs` | ~943 | no | no (`milp` off) | KEEP-GATED (oracle) |
| viz | `viz.rs` | ~263 | no | yes (unconditional) | FEATURE-GATE |

EG is the *theoretical* core (`design/lmsr-proof.typ`, "Prediction Markets Are
Fisher Markets"), but the production clearer is the **LP** solver; EG/IterLP/
Conic all build on LP utilities and serve as **differential-testing oracles** in
the conformance harness (which checks solver output against `sybil-verifier`).
Genuine correctness value — **gate, do not delete.**

Steps: add `research = ["lp"]` to `matching-solver/Cargo.toml`; switch
`eg_solver`/`iterative_lp_solver`/`decomposed`/`viz` from
`#[cfg(feature="lp")]`/unconditional to `#[cfg(feature="research")]`; give
`matching-sim`/conformance/bench the `research` feature; sequencer keeps
`["lp"]`. Removes ~1,731 lines from the prod compile with zero behavior change.
`decomposed.rs` is KEEP-DEFERRED (implements `design/decomposition.typ`,
combinatorial-markets roadmap).

## 2 — Unused order generality

`Order` (`matching-engine/src/order.rs:54`) is a general payoff-vector
instrument: `payoffs:[i8;32]` over up to 5 markets plus optional
`PriceCondition`. **None is reachable today** — `validate_binary_one_hot()`
(`order.rs:184`) is enforced at API ingress (`convert.rs:497`), sequencer
(`validation.rs:13`), the **consensus verifier** (`sybil-verifier/orders.rs:45`,
`:266`), and solver ingress (`solver.rs:42`).

**KEEP-DEFERRED, do not remove**, for two hard reasons: (1) multi-outcome/
bundle/conditional is the stated combinatorial-markets direction
(`design/bundle-clearing.typ`, `decomposition.typ`); (2) `Order` is serialized
into the block witness and re-derived by `sybil-verifier`/`sybil-zk` —
`marginal_payoffs_i64` (`order.rs:227`) matches verifier semantics and
`OrderDirection::to_byte` has a stability test (`order.rs:493`). Changing the
struct **moves the guest commitment** and invalidates historical proofs.
Removable surface ~300–500 lines at HIGH risk for ~0 gain. At most, add an
`// EXPERIMENTAL: combinatorial roadmap` banner.

## 3 — Largest modules & duplication

Giants: `sequencer.rs` 7,164, `store.rs` 6,683, `actor.rs` 6,006. **SPLIT** for
readability (0 lines saved, medium care — one hop from the state root).

**Duplication: low.** The redb `store.rs` and the QMDB path (`qmdb_state.rs`,
`qmdb_accounts.rs`, `account_storage.rs`) are **not** redundant: QMDB produces
the authenticated **state root** proven in `actor.rs:2133`. Load-bearing, DO NOT
TOUCH. No removable copy-paste scaffolding found; canonical-bytes logic is
centralized in the verifier schemas.

## 4 — Dead/unused surface

- `crates/sybil-witgen-cli/` — empty, untracked, **not a workspace member**.
  REMOVE the dir.
- `#[allow(dead_code)]` — 12 sites, all but one in test helpers. Clean.
- TODO/FIXME/XXX/HACK — only 9 repo-wide. No graveyard.
- Feature flags — `lp` (prod-on); `conic`/`milp`/`parallel`/`viz` (prod-off).
  None always-dead. Recommendation is to *add* a `research` umbrella.
- No unused workspace crates; all 17 members referenced.

## 5 — Test vs product ratio

~41k of ~94.5k raw Rust (~44%) are tests (33k inline + 7.9k integration), plus
~4.7k sim/scenario. Real product ≈ 40k tokei-code / ~49k raw.

## DO NOT TOUCH

- `LpSolver` — the production clearer.
- `matching-engine` consensus types (`Order`, `Fill`, `Problem`,
  `settlement.rs`, `marginal_payoffs_i64`, `OrderDirection` bytes) — serialized
  into the witness / re-derived by the guest; layout changes move the
  commitment.
- `sybil-verifier`, `sybil-zk`, `sybil-prover` — the guest closure.
- QMDB state path — produces the proven state root; not redundant with
  `store.rs`.
- MILP + Conic — differential-testing oracles for LP. Keep gated.
- `decomposed.rs` and Order payoff-vector generality — combinatorial-markets
  roadmap.

## Bottom line

Appropriately sized for a ZK exchange, not bloated. One genuinely clean win:
feature-gate the research solver zoo out of the production compile (~1.7k lines,
low risk, zero behavior change). Plus cosmetics (delete empty crate; split three
god-modules). The order-type generality looks deletable but is deferred-roadmap
inside the guest commitment — HIGH risk, near-zero savings, leave it.
