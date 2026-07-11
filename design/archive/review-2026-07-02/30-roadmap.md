# Improvement Roadmap

A sequenced plan from the current state to the north star: **one state-transition function, proven end-to-end, over integer money, with one canonical encoding, served by one API to one frontend, deployed from CI.**

The ordering is deliberate: close the exploitable and value-leaking holes first (small, mostly deletion), then delete the sediment so later work happens in a smaller codebase, then resolve the architectural schisms, then make the conventions mechanical so regressions can't recur. Effort estimates assume one experienced engineer and are rough — S = hours, M = days, L = 1–2 weeks, XL = multi-week.

Each item links to its detailed treatment. Bug IDs (C1, H1…) are in [01-critical-bugs.md](01-critical-bugs.md); theme numbers are in [02-cross-cutting-themes.md](02-cross-cutting-themes.md).

---

## Phase 0 — Stop the bleeding (this week)

Small, high-urgency, mostly deletion or config. Nothing here needs design discussion.

| # | Action | Effort | Refs |
|---|--------|--------|------|
| 0.1 | Turn `SYBIL_DEV_MODE` off in prod, or gate account-mint + market-resolve behind an admin token; rebind all non-Caddy ports to `127.0.0.1` | S | [D5](01-critical-bugs.md), [18](18-ops-deployment.md) |
| 0.2 | Delete `docs/api-keys.md`, rotate the key, move it to `/opt/sybil/.env`, drop the `--api-key` argv | S | [D6](01-critical-bugs.md) |
| 0.3 | Reject `num_markets != 1` and non-one-hot payoffs at admission **and** in `convert.rs`; delete the Spread/Bundle/Custom `OrderSpec` variants | M | [C1](01-critical-bugs.md), [C2](01-critical-bugs.md) |
| 0.4 | Add a `MAX_ORDER_QTY`/notional cap at admission; switch `validation.rs` money muls to checked i128 | S | [D4](01-critical-bugs.md) |
| 0.5 | Add `[profile.release] overflow-checks = true` to the workspace `Cargo.toml` | S | [Theme 3](02-cross-cutting-themes.md) |
| 0.6 | Make conservation checks + `verify_full` a **precondition of block commit** (fail-closed, retain pre-block state) | M | [H1](01-critical-bugs.md) |
| 0.7 | Replace the `.expect`s in bridge/deposit WAL replay with drop-with-metric (stop the crash loop) | S | [H7](01-critical-bugs.md) |
| 0.8 | Only clear pending read-model deltas when persistence happened (or force persist when deltas non-empty) | S | [H8](01-critical-bugs.md) |
| 0.9 | Guard the arena FV parse (try/except + strip trailing dot); catch `on_block` exceptions in `BaseAgent.run` | S | [H9](01-critical-bugs.md) |
| 0.10 | Delete `crates/sybil-api/static/trade.html` + the console Trade tab (stop serving a page that can't sign) | S | [H11](01-critical-bugs.md) |
| 0.11 | Add alerts for `sybil_persistence_failures`, disk-space, and `up{job="sybil-arena"}`; make `deploy-monitoring` fail loudly without Telegram creds | S | [OPS-1,3,4](18-ops-deployment.md) |

**Exit criteria:** no internet-reachable mint/resolve; no order type reaches a solver it isn't modeled for; a mis-solved block cannot seal; the node cannot crash-loop on restart; the arena survives one bad LLM response.

---

## Phase 1 — Delete the sediment (1–2 weeks)

Pure deletions and consolidations. Zero behavior change on the live paths, large reduction in surface area. Do this before Phase 2 so the schism work happens in a smaller repo. Target: **~15k lines removed.**

| # | Action | Effort | Refs |
|---|--------|--------|------|
| 1.1 | Delete `arena/nba/` (6.6k), `sim/news_trader_legacy.py` + test, root `mm_*.py` (1.7k), `arena/live/composition_demo/`, `arena/feeds/`, 4 unused MM classes; fix/remove the `arena-demo` justfile targets; set `pytest testpaths=["tests"]` | M | [16](16-arena.md) |
| 1.2 | Delete `matching-solver/verifier.rs`, `book.rs`, the `PipelineResult` fossils + dead `viz.rs` halves, `marginal_payoffs_*`, dead deps (~2.5k) | M | [10](10-core-math-and-solvers.md) |
| 1.3 | Delete the conditional-order machinery end-to-end (struct field, builder, canonical encoding, ZK check) until a real design exists | M | [Theme 1](02-cross-cutting-themes.md), [10](10-core-math-and-solvers.md) |
| 1.4 | Pick one block-stream transport (keep WebSocket, delete SSE + `sse.rs`); update the vault note | S | [Theme 7](02-cross-cutting-themes.md), [12](12-api.md) |
| 1.5 | Delete the fictional Mintlify pages (or move to a marketing repo), `DEPLOY.md`, `mint.json`, `CLEARING.md`, `docs/superpowers/`, marketing articles; rewrite the README from the vault MOC | M | [20](20-documentation-estate.md) |
| 1.6 | Delete `apps/` (empty), move `frontend/handoff` → `design/`, `frontend/archive` + `BACKEND_*_PLAN.md` → deleted | S | [17](17-frontend.md) |
| 1.7 | Drop the legacy `PENDING_BUNDLES`/`ADMIT_LOG` tables + `try_admit_direct`/`AdmitOutcome`/`reinsert_for_replay` (one devnet reset) | M | [SEQ-11](11-sequencer.md) |
| 1.8 | Fix the `matching-solver` feature graph (`milp = ["dep:russcip", "lp"]`) and stop `matching-sim` force-enabling SCIP | S | [D8](01-critical-bugs.md), [WK-1](19-workspace-consistency.md) |

**Exit criteria:** "which X is real?" (frontend, solver, trader, doc, deploy path) has one answer each; bare `pytest` and every documented command run; the repo is materially smaller.

---

## Phase 2 — Resolve the schisms (2–4 weeks)

The architectural core. Each item picks a direction for a half-built capability and commits. These have design content; sequence them so the shared primitives (the commitments crate, the STF) land before their consumers.

| # | Action | Effort | Refs |
|---|--------|--------|------|
| 2.1 | Extract a `no_std` **`sybil-commitments`** crate owning every consensus byte-layout (signing bytes, `hash_header`, event/state/witness leaf prefixes, digest encoders, bridge/DA domains, checked nanos arithmetic); make sequencer/verifier/zk consume it; unify the two divergent reservation encoders | L | [Theme 6](02-cross-cutting-themes.md), [15](15-verification-zk.md) |
| 2.2 | Decide the payoff-vector direction and implement it: **minimalism** (`BinaryOrder` solver input) or **generality** (per-state balance constraints in the LP + state-price-inner-product settlement) | L–XL | [Theme 1](02-cross-cutting-themes.md), [10](10-core-math-and-solvers.md) |
| 2.3 | Collapse the four verification layers toward one `apply_block(pre_state, pre_sidecar, inputs) -> (post_state, post_sidecar)` STF the sequencer runs, the verifier re-runs, and the guest proves — closing the fill-binding (H2), sidecar (H4), and digest (ZK-1) gaps together | XL | [Theme 2](02-cross-cutting-themes.md), [15](15-verification-zk.md) |
| 2.4 | Bind the ZK proof to what it commits even before the full STF lands: fill→account check (H2), verifier arithmetic range/overflow guards (H3), deposit-inclusion in the guest (H5) | L | [H2,H3,H5](01-critical-bugs.md) |
| 2.5 | Fix the contract safety gaps: deposit-checkpoint bound (H6), a real `claimKind`-dispatched escape-cash claim (H14), indexer confirmation depth + reorg reconciliation (H12) | L | [14](14-oracle-l1-contracts.md) |
| 2.6 | Finish the hot/cold state split: route all cold reads to `ReadModelStore` in `sybil-api`, delete ~25 actor read-RPCs, persist blocks/price-history | L | [Theme 4](02-cross-cutting-themes.md), [11](11-sequencer.md), [12](12-api.md) |
| 2.7 | Split `matching-sequencer` along its seams (node core / runtime / store / analytics / `sybil-sim`); rename it `sybil-sequencer`; break the god-files by phase | L | [11](11-sequencer.md), [WK-2](19-workspace-consistency.md) |
| 2.8 | Fix the arena's purpose: split analysis from trading (one FV stream both strategies subscribe to), unify sim/live on one trader core, make bot identity durable with reconnect | L | [16](16-arena.md) |
| 2.9 | Deploy `frontend/web` behind Caddy; delete `static/` + the Dev Zone; fix u64→JSON-string serialization in `sybil-api-types` once (delete `patch-bigints.mjs`) | L | [17](17-frontend.md) |

**Exit criteria:** every capability the edges advertise, the core implements (or the edge no longer advertises it); the ZK proof attests what it commits; there is one console, one read path, one Order representation, one canonical-encodings crate.

---

## Phase 3 — Enforce and harden (ongoing, start in parallel)

Make the conventions mechanical so none of the above regresses, and fix the deployment posture. Several of these can start during Phase 1.

| # | Action | Effort | Refs |
|---|--------|--------|------|
| 3.1 | Introduce `Nanos`/`SignedNanos`/`Qty` newtypes with checked i128-backed ops; replace bare aliases and raw `as i64` money muls (kills the f64-reservation and overflow classes at the type level) | L | [Theme 3](02-cross-cutting-themes.md), [WK-3](19-workspace-consistency.md) |
| 3.2 | Add `[workspace.package]`, `[workspace.dependencies]` for all shared deps, `[workspace.lints]` (`clippy::unwrap_used = deny` for libs), and a `clippy.toml` banning `f64` in core modules | M | [WK-7](19-workspace-consistency.md) |
| 3.3 | Land the missing CI: solver conformance suite (`&dyn Solver` + proptest → `verify_match(strict)`); arena `ruff`+`pytest`; `docs-check`; frontend `pnpm test`; `cargo hack --each-feature`; `cargo check --tests` for `zk/`; a golden cross-language hash test | L | [Theme 9](02-cross-cutting-themes.md) |
| 3.4 | Make CI run `just check-all` and put every check in `check-all` (align `-Dwarnings`) so the two can't drift | S | [OPS-6](18-ops-deployment.md) |
| 3.5 | Move to CI-built images on a registry; deploy via `docker compose pull && up -d`; add a backup recipe + external heartbeat + auto-reboot script | L | [Theme 10](02-cross-cutting-themes.md), [18](18-ops-deployment.md) |
| 3.6 | Move block production off the actor task (async solve stage + tick coalescing); adopt the `BlockDelta` model to kill the O(total-state) clone and full qMDB rewrite | L | [D1,D2](01-critical-bugs.md), [11](11-sequencer.md) |
| 3.7 | Unify on one actor substrate + one tokio runtime; move redb `fsync` and qMDB behind a dedicated storage actor | L | [WK-6](19-workspace-consistency.md) |
| 3.8 | Vault as contract: ban hardcoded volatile facts, CI-enforce `check-vault.sh`, replace per-crate AGENTS.md with one-line stanzas, write the four missing notes | M | [20](20-documentation-estate.md) |
| 3.9 | Add signed-write replay protection (nonce/expiry + bounded per-account replay set) before any signed endpoint goes public | M | [D3](01-critical-bugs.md) |

**Exit criteria:** the all-integer and no-panic conventions are compile errors, not prose; every subsystem has CI; deploys don't depend on one laptop; block production scales with activity, not total state.

---

## Dependency notes and sequencing cautions

- **2.1 (`sybil-commitments`) should land before 2.3/2.4** — the STF and the guest bindings both want the shared encoders, and doing them in the other order means encoding the same bytes twice.
- **2.2 (payoff-vector decision) gates the solver conformance suite (3.3)** — the suite's generators and invariants differ depending on whether the input type is `BinaryOrder` or a general payoff vector.
- **1.7 (drop legacy WALs) needs a devnet reset** — batch it with any other reset-requiring change (e.g. 2.6's persistence changes) to spend the reset once.
- **3.1 (newtypes) is easiest right after 0.5** (overflow-checks already on) and before 2.x adds more money-handling code; but it touches many files, so land it when the tree is otherwise quiet.
- **Phase 3 items can interleave with Phase 1/2** — 3.2, 3.3, 3.4, 3.8 are independent of the schism work and reduce regression risk while it happens.

## What "done" looks like

When the roadmap is complete, the one-paragraph description of Sybil becomes accurate as written: a prediction-market exchange whose matching coherence comes from one joint program, whose state transition is a single function proven end-to-end over compiler-enforced integer money, whose consensus bytes live in one crate, served by one API that is the type oracle for one frontend and the Python SDK, deployed from CI with backups and an external heartbeat — and documented by one vault that CI keeps honest. That is "amazing, crispy clear and nice," and every step above is a concrete move toward it.
