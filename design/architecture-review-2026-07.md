# Architecture Review — Simplification Proposals (2026-07)

*A strategic-programming review in the spirit of "A Philosophy of Software Design": complexity is anything that makes the system harder to understand or modify, and it accumulates as dependencies (what you must coordinate) and obscurity (what you must discover). The goal of every proposal here is the same: fewer parallel implementations of one idea, deeper modules behind smaller interfaces, and boundaries that match the system's real fault lines. Companion document: `docs/SPEC.md` (what the system is); this one is about what it should become.*

Evidence base: full-code sweep of all 15 crates, arena, frontend, contracts, and deploy config; git churn over the last 300 commits; the architecture vault.

---

## 1. What is already right (and should be defended)

Before the critique, the load-bearing good decisions — these are the boundaries proposals must *not* blur:

- **`matching-engine` as a pure leaf.** Domain types with zero solver/IO logic, depended on by everything. Correct.
- **Float search, integer truth.** Solvers may use f64 internally; only integer fills/prices are trusted, and the verifier judges only those. This is the cheapest possible containment of numerical mess.
- **Settlement written once.** `compute_fill_settlement` / `derive_minting` are shared by the sequencer and verifier Layer 2 — checked twice, written once. Exactly how duplication should be avoided in a verify-the-computation architecture.
- **The redb fence.** One commit point, honest about the absence of cross-DB atomicity, recovery reads the fence and never guesses. A model of "define errors out of existence."
- **The actor model around a synchronous core.** `BlockSequencer` is deterministic and single-threaded; concurrency lives entirely in the mailbox. Right shape, even if the surface has bloated (P4).
- **The guest-safety split.** `sybil-zk` (no commonware, no sequencer) vs native verification is a real constraint boundary, not accidental structure.
- **The Polymarket mirror as an untrusted signer.** All external I/O outside the trust boundary, entering only through signed attestations.
- **Monitoring depth.** ~40 grounded alert rules at this stage of a project is rare and valuable.

## 2. The shape of the problem

Rust workspace ≈ 66k lines; the essential core is much smaller than the mass suggests:

| Layer | LOC | Notes |
|---|---|---|
| matching-sequencer | 25.5k | **~45% is `#[cfg(test)]`**; prod ≈ 14k; hottest file in the repo (`sequencer.rs`: 119 touches in the last 300 commits) |
| matching-solver | 7.5k | six solvers; heavy intra-crate duplication; vestigial interfaces from a removed pipeline |
| sybil-verifier | 5.7k | the reference "circuit" + all canonical byte schemas |
| sybil-api (+types) | 7.7k | thin transport, but with repeated assembly blocks and a stale spec |
| ZK host stack (zk/witgen/witgen-cli/prover) | 5.2k | five compilation units for one pipeline |
| arena | 22k | ~7k of it legacy/one-off |
| frontend | 34k | plus ~200KB of historical planning markdown and a duplicated dev console |

Churn concentrates exactly where the review finds mixed concerns: `matching-sequencer` (629 file-touches/300 commits), `sybil-api` (299), `matching-solver` (294). The complexity tax is being paid on every change, today.

Four recurring complexity taxes:

1. **Parallel implementations of one idea** — two simulation stacks, two solver-output verifiers, three hand-maintained API clients, two dev consoles, three copies of `hash_header`, two snapshot byte-encoders, duplicated qMDB proof types.
2. **Vestigial interfaces** — types and fields that survived the design they served (`PipelineResult`'s empty fields, `viz.rs` rendering data no solver produces, `book.rs` with no callers, `DualAnalysis` stub).
3. **Shallow layers and boilerplate** — the actor's 50-variant message enum with ~90 hand-written RPC wrappers; 200 lines of analytics pass-through getters on `BlockSequencer`; four near-identical market-response assembly blocks.
4. **Documentation that lies** — `CLEARING.md` (removed architecture), `DEPLOY.md` (never-landed Kamal topology), sequencer/api/polymarket AGENTS.md files that describe modules and behavior that no longer exist. False docs are worse than no docs: they cost every reader a verification pass.

---

## 3. Proposals, ranked by leverage

### P1 — Make block production a pure state-transition function; evict everything else from the sequencer core

**Problem.** `produce_block_in_place` (`sequencer.rs:2227`, ~820 lines) braids ten concerns: system-event digesting, order-book maintenance, submission processing, STP, solving, settlement, minting, five near-duplicate `HistoryEvent` construction blocks, liquidity/equity/welfare tracker updates, witness assembly, and an inline `verify_full` call. `BlockSequencer` additionally carries ~200 lines of analytics getters that its own `analytics.rs` header disclaims ("not the sequencing core"). This is the hottest file in the repo; every feature pays the toll of understanding all ten concerns.

**Proposal.** Split along the line the system *already* draws elsewhere — the witness. Define the kernel as literally the function the ZK guest verifies:

```
fn apply_block(state: &ChainState, inputs: BlockInputs) -> (ChainState, Block, BlockWitness)
```

where `BlockInputs` = drained submissions + system events + timestamp. Everything in the witness is kernel; everything not in the witness (price trackers, equity series, fill history, candles, account event feed, liquidity scores) is a **derived view** that subscribes to the sealed-block/witness stream — the same position SSE/WS consumers and the persistence "Tier 3" tables already occupy. The persistence design (Tier 1 core vs Tier 3 derived) has already discovered this boundary; the code just doesn't honor it yet.

Concretely: (a) move tracker updates out of `produce_block_in_place` into an `AnalyticsState::observe_block(&SealedBlock, &BlockWitness)` call driven by the actor after commit; (b) extract a `record_history` helper to collapse the duplicated `HistoryEvent` blocks; (c) move the analytics getters off `BlockSequencer` behind a single `analytics()` accessor; (d) drop the inline `verify_full` to a config-gated debug path (its production home is the prover — the TODO at `sequencer.rs:3020` already says so).

**Payoff.** The consensus-critical surface becomes small enough to read in one sitting and *is by construction* the thing the proof attests. Analytics changes (frequent, low-risk) stop touching the block path (rare, high-risk). The 820-line function decomposes into the phase functions that already exist around it.

**Risk.** Some views need intra-block information not in the witness — the witness is deliberately complete, so this should be rare; anything genuinely missing is a signal it belongs *in* the witness.

### P2 — One verifier; one welfare definition

**Problem.** `matching-solver/src/verifier.rs` (554 lines, 9 violation kinds) re-implements the fill-validity checks of `sybil-verifier` Layer 1 (~38 kinds). matching-sim runs both. Worse, they paper over a real spec bug: LP-family solvers report `total_welfare` gross (`minting_cost = 0`) while MILP reports net — so the shared `WelfareMismatch` check only passes because each solver gets to define its own arithmetic, and cross-solver "gap analysis" in matching-sim compares apples to oranges by exactly the minting cost.

**Proposal.** Delete the solver-crate verifier; make `sybil-verifier` the single reference (matching-sim already constructs witnesses for it). Decide the welfare convention once — recommendation: **net of minting** everywhere, since that's the LP objective and what MILP already reports — and encode it in `MatchingResult` so a solver *cannot* fill the fields inconsistently (e.g., store `gross_welfare` + `minting_cost` and derive `total`). While here: delete the never-populated `PipelineResult` fields (`contributions`, `combine_stats`, `iteration_stats`, `ucp_stats`, `iterations`) and the `viz.rs` machinery that renders them — they are the ghost of a removed pipeline architecture.

**Payoff.** −~1,200 lines, one source of truth for "is this clearing valid," and solver benchmarks that measure the same number.

### P3 — One simulation stack

**Problem.** Two parallel harnesses: `matching-sim` + `matching-scenarios` (drives the solver directly, 2.9k lines) and the sequencer's embedded sim (`simulation.rs`, `scenario.rs`, `agent/`, `metrics.rs`, `bin/sybil_sim.rs` — ~1.7k lines compiled into the production library that `sybil-api` links). Two scenario formats, two CLIs, two metric sets.

**Proposal.** Extract the sequencer's agent sim into a dev-only crate (or fold it into `matching-sim` behind a `--sequencer` mode, which is the more honest home: one CLI, two depths — solver-only for benchmarks, full-sequencer for lifecycle realism). Production `matching-sequencer` stops shipping agents. `matching-sim`'s own 2,181-line `main.rs` should split its four concerns (dispatch, witness-building, reporting, JSON export) — its two witness builders are ~90% identical.

**Payoff.** −~1k lines from the production library; one place to add a scenario; the "which sim do I run?" question disappears.

### P4 — Shrink the actor surface mechanically

**Problem.** `actor.rs` (4.2k lines) is dominated by a triple written by hand three times: the ~50-variant `SequencerMsg` enum, the ~60-arm handler match, and ~90 near-identical `SequencerHandle` RPC wrappers. Every new query = three edits of boilerplate. Separately, `store.rs` runs blocking redb transactions inline on the tokio executor (only qMDB got dedicated threads) — every block commit and every durable admit stalls the runtime thread.

**Proposal.** (a) A small macro (or a generic `Query<T>`/`Command<T>` envelope pair) to declare an RPC once. (b) Route redb writes through `spawn_blocking` or give `Store` the same dedicated-thread treatment qMDB already has. (c) The read-heavy getters largely disappear if P1's derived views are served from their own state outside the actor, turning many RPCs into plain reads of an `Arc` snapshot.

**Payoff.** Hundreds of lines of pure ceremony gone; the latency cliff under write load removed; adding an endpoint becomes a one-place change.

### P5 — One home for canonical bytes and hashes

**Problem.** Byte-truth is scattered: `hash_header` exists three times (sequencer, `sybil-verifier/block.rs:298` — with a comment saying "must match" — and `sybil-zk/lib.rs:229`); `state_schema.rs` and `witness_schema.rs` contain ~200 lines of parallel `append_*` encoders over the same snapshot types; qMDB proof types are declared twice (`sybil-zk` and `matching-sequencer::qmdb_state`) with three `convert_*` bridge functions; and the crate *named* `sybil-canonical` is a different, unrelated serialization system (client signing bytes). In a ZK system, hand-synchronized encoders are the highest-consequence duplication there is: a silent divergence is a soundness bug.

**Proposal.** `sybil-verifier::commitments` is already the de-facto owner — finish the job: move `hash_header` there (sequencer and zk import it); merge the two snapshot encoders into one visitor with two entry points; let `sybil-zk` own the qMDB proof types and have the sequencer produce them directly. Rename for honesty: `sybil-canonical` → `sybil-signing` (or fold it into `sybil-api-types`), so "canonical" means exactly one thing in this codebase.

**Payoff.** The set of code that must be byte-identical for soundness becomes one module with golden tests, not five files with comments pointing at each other.

### P6 — One client story for the API

**Problem.** The wire contract exists in four hand-synchronized forms: `sybil-api-types` (truth), the generated TS `schema.d.ts` (good — generated), the hand-written Python SDK (subset, drifts), and the hand-written Rust client inside `sybil-polymarket` (whose own comment says "mirrors the Python SybilClient"; `sybil-admin` embeds a third client with a TODO admitting it). Meanwhile the OpenAPI spec under-reports the real surface (candles, pause/resume, raw events, bots endpoints missing), so the one generated consumer is generated from an incomplete source.

**Proposal.** (a) Make `/openapi.json` complete — it's the contract; treat a route not in it as a CI failure (utoipa can enumerate routes vs paths in a test). (b) Generate the Python SDK from it (openapi-python-client), keeping a thin hand-written ergonomic layer (`buy_yes`, `stream_blocks`) on top. (c) Extract one Rust client crate used by polymarket + admin. (d) Pick **one** realtime transport for first-party clients — WS with height-resume is strictly more capable than SSE; keep SSE only if third-party simplicity is a goal, and say so.

**Payoff.** Adding an endpoint becomes: implement + annotate, regenerate clients. Drift class eliminated rather than managed.

### P7 — Collapse the ZK host tooling to two units

**Problem.** One pipeline, five compilation units: `sybil-zk`, `sybil-witgen`, `sybil-witgen-cli`, `sybil-prover`, `zk/openvm-tools`. The witgen-cli/prover split exists only because one links `matching-sequencer` and the other must not; `openvm-tools` is a 174-line encoder in its own workspace.

**Proposal.** Keep exactly two boundaries, because exactly two constraints exist: **`sybil-zk`** (guest-safe, no commonware/sequencer — shared with the guest) and **`sybil-prover`** (everything host-side: job export behind a `sequencer-store` feature absorbing witgen + witgen-cli, prepare/DA/worker/serve/submit as subcommands). `openvm-tools` stays a separate workspace only if the OpenVM version pin forces it; otherwise it's a `prover encode-input` subcommand. Also: move `mock-live` out of the production binary (dev feature or scripts/), and split the 2,310-line `main.rs` into modules (job, artifacts, da, abi, serve).

**Payoff.** The pipeline's mental model drops from "which of five crates does X" to "guest-safe vs host." −2 workspace members, −1 standalone workspace.

### P8 — Make the dev/prod boundary real

**Problem.** The deployed devnet runs `SYBIL_DEV_MODE=true` (base compose, not overridden in prod) with permissive CORS — so account funding, market creation/resolution, reference-price pushes, and pause/resume are publicly reachable, and the docs claim otherwise. The Polymarket mirror *depends* on dev endpoints, which is why the flag can't simply be flipped.

**Proposal.** Split "dev conveniences" from "operator surface." The mirror and admin CLI are operators: give them an authenticated service identity (they already hold P256 keys for attestations — extend signed requests to market-creation/metadata/reference-price writes, or bind an API token checked by middleware). Then `dev_mode` can mean only "free money and pause buttons," off in prod, and the documented invariant becomes true. Tighten CORS in the prod overlay at the same time.

**Payoff.** The security boundary matches the docs; flipping one env var no longer requires re-architecting the mirror mid-incident.

### P9 — Delete the dead weight

Nothing below is load-bearing; all of it costs orientation time (and some of it misleads):

| Target | ~Size | Action |
|---|---|---|
| `arena/nba/` | 6,200 lines | delete (jj history preserves it); it's the only Anthropic-SDK dependency left |
| `arena/sim/news_trader_legacy.py` | 600 | delete (superseded by `llm_trader.py`) |
| `arena/mm_backtest.py`, `mm_pnl_analysis.py`, `mm_param_sweep.py` | 1,740 | move to a branch or `arena/analysis/` with a README; they're one-off studies |
| `matching-engine/src/book.rs` | 558 | delete — no solver/sequencer caller; only a never-called viz helper references it |
| `matching-solver/CLEARING.md` | 663 | delete (describes removed modules: pipeline/negrisk/dual_master) |
| `matching-solver` vestigial result fields + dependent `viz.rs` machinery | ~600 | delete with P2 |
| `MilpSolver::solve_with_duals` / `DualAnalysis` stub | ~80 | delete |
| `DEPLOY.md` | — | rewrite to match the justfile reality (Kamal/Traefik/Tempo never landed); delete `deploy/tempo.yml` |
| `frontend/archive/` + `handoff/` (except `tokens/`) + 200KB planning MDs | large | archive branch; keep `tokens/colors_and_type.css` |
| Duplicated dev console (`sybil-api/static` vs frontend `/dev/*`) | — | pick one (recommendation: keep the Rust static console as the zero-dependency fallback, freeze it; or delete it and commit to `/dev/*` — either, but not both maintained by hand) |
| Stale AGENTS.md sections (sequencer mempool/TTL, api endpoints, polymarket actors, "five solvers", "entropy smoothing") | — | fix in place; where a section duplicates the vault, replace with a pointer — docs that can drift independently, will |

Total: roughly **10–12k lines** removable with zero behavior change, plus the drift-class docs.

### P10 — Tactical cleanups (do opportunistically)

- Four near-identical market-response assembly blocks in `routes/markets.rs` (list/summary/get/search) → one builder.
- `app.rs` should not contain an arena-SQLite metrics reader duplicated with `routes/bots.rs`; longer-term, the Rust API reading a Python-owned SQLite file is a boundary inversion — the arena should push its metrics (or serve them) itself.
- `now_ms` reimplemented in ~8 places → one util.
- `metric_path_label`'s 60-line hand-maintained route match → derive from axum's `MatchedPath`.
- `get_equity` filters a full series in memory despite range params → push down to the store.
- Solver crate: hoist the copy-pasted MM setup into `SolverContext`; share the projection-LP epilogue; extract common test fixtures (~800 lines); pick one RNG stack in `matching-scenarios`.
- Consider whether `LpSolver`'s single-pass SLP should remain the production configuration versus `IterLpSolver` (which exists specifically because one pass linearizes poorly under tight budgets); if LP stays, say why where the config is defined.
- Fuzz the canonical decoders (`witness_schema`, `event_schema`) — the fuzz workspace exists; the byte schemas are the highest-value target in the repo.

---

## 4. What *not* to do

- **Don't merge `sybil-api-types` into `matching-engine`.** The wire/domain split is intentional; the cost (a `convert.rs`) buys the freedom to evolve the wire format without touching consensus types.
- **Don't unify `sybil-canonical`'s signing bytes with commitment bytes.** Clients sign a stable, minimal payload; commitments encode full state. Rename it (P5), don't merge it.
- **Don't collapse the native/guest commitment implementations.** The hand-rolled guest verifier exists so the guest doesn't link commonware; the golden tests pinning them equal are the right mechanism. (Do add fuzzing, per P10.)
- **Don't chase crate-count for its own sake elsewhere.** engine/solver/scenarios/sequencer/api are real boundaries with distinct change cadences; the ZK host tools (P7) are the only place the crate graph outruns the concept graph.
- **Don't formalize the Rust↔Lean gap yet.** The Lean development proves the continuous theory; bridging to integer code is research, not cleanup. Keep citing theorems from comments.

## 5. Sequencing

1. **P9 deletions + doc fixes** — a day of work, immediately lowers the noise floor for everything else.
2. **P2 (one verifier, one welfare)** — small, sharpens the spec before the big refactor.
3. **P1 (kernel/views split)** — the centerpiece; do it while P2's single verifier makes "did I break clearing?" a one-command question.
4. **P4 + P10** fall out naturally during P1 (the actor shrinks as views move out).
5. **P5 (canonical bytes)** before the next ZK milestone — it reduces the soundness-critical surface ahead of real proving.
6. **P6–P8** as the external surface stabilizes toward testnet.

The through-line: this codebase's ideas are strong and its hardest problems (integer determinism, witness completeness, fenced persistence) are already solved well. The debt is almost entirely *residue* — parallel structures left standing after the design moved on. Removing residue is cheap, safe, and compounds: every proposal above makes the next one smaller.
