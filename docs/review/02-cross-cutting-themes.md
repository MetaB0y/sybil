# Cross-Cutting Themes

The individual findings across thirteen subsystems are not independent. They are symptoms of ten recurring patterns. Fixing the patterns — not just the instances — is what turns Sybil "crispy clear." Each theme below names the pattern, lists where it shows up, and states the structural fix.

---

## Theme 1 — The aspirational/actual schism

**The pattern.** Code, types, docs, and the API consistently advertise capabilities the runtime does not implement. The advertised surface is accepted at every edge and silently mishandled at the core. This is the single most damaging pattern in the repo, and it is the root of the two money-leak bugs.

**Where it shows up:**
- **Payoff vectors / multi-market / conditional / multi-outcome orders.** `Order` carries `markets: [MarketId;5]`, `payoffs: [i8;32]`, `condition: Option<PriceCondition>`; the API exposes Spread/Bundle/Custom; docs say "the solver just sees vectors, adding an order type only needs a conversion function." Every solver assumes single-market binary one-hot orders, guarded by `debug_assert`. Conditional orders have a builder, canonical encoding, and a ZK activation check — but no API path creates one and no solver honors them. → [C1, C2](01-critical-bugs.md); [10-core-math](10-core-math-and-solvers.md).
- **Oracle resolution policies.** `ResolutionPolicy` has one live arm (`Immediate`); `Propose`/`Challenge`/`check_finalization`/`AutomatedL0`, `MarketStatus::{Proposed,Challenged,Voided}` are reserved but unexercised, and `resolve_market` converts a `Propose` into an error. → [14-oracle-l1](14-oracle-l1-contracts.md).
- **Vault escape mode.** A flag and an event, no claim path. → [H14](01-critical-bugs.md).
- **ZK deposit inclusion.** Merkle primitives exist; nothing calls them; the guest asserts equality of a self-reported root. → [H5](01-critical-bugs.md).
- **Sidecar-transition verification.** The sidecar is committed into the state root but no layer derives or checks its transition. → [H4](01-critical-bugs.md).
- **Public docs.** A Mintlify site documents fees, an SDK, an auth scheme, and endpoints that do not exist. → [20-documentation-estate](20-documentation-estate.md).

**Why it's dangerous.** The half-built state is strictly worse than either finishing or deleting: it passes types and compiles, so reviewers and agents assume it works; it reaches settlement/commit, so the failure is a silent value leak or an attestable-wrong proof rather than a clean rejection.

**The fix (one rule).** For each schism, decide in **one** direction and make the edge match the core:
- *Honest minimalism:* delete the aspirational surface to the edge. Reject `num_markets != 1` and non-one-hot payoffs at admission; delete Spread/Bundle/Custom, the conditional-order machinery, the reserved oracle arms, and the escape scaffolding until a real design exists.
- *Honest generality:* implement it. Per-state balance constraints in the LP (bundles/spreads price coherently out of joint EG, as `design/lmsr-proof.typ` intends); a real escape-claim path; deposit-inclusion in the guest.

The project's stated rule — *elegance over backward compatibility, early dev* — is precisely the license to do this. Pick minimalism where no near-term design exists (conditional orders, escape), generality where the thesis demands it (payoff vectors).

---

## Theme 2 — Verify, don't log (make the safety net load-bearing)

**The pattern.** Real safety checks exist and then don't gate anything.

**Where:**
- The sequencer's post-settlement balance-conservation and position-balance checks `error!`-log and commit anyway; `verify_full` runs every block and only logs on failure. → [H1](01-critical-bugs.md).
- The verifier's Layer 3 is *circular*: the sequencer writes the header by calling `sybil_verifier::block::compute_state_root_with_sidecar`, and the verifier "checks" it by calling the identical function — it can only fail on transport bugs, never on root-computation bugs. Layer 2 shares the same `matching_engine` settlement functions the sequencer uses. Genuine independence exists only in the guest path, which runs only in smoke tests. → [15-verification-zk](15-verification-zk.md).

**The fix.** Two moves:
1. Make the invariant checks and `verify_full` a **precondition of block commit** (fail-closed, retain pre-block state). The prepare/commit split already supports this.
2. Collapse the four partially-overlapping layers toward **one deterministic `apply_block(pre_state, pre_sidecar, inputs) -> (post_state, post_sidecar)`** that the sequencer runs, the verifier re-runs, and the guest proves. This closes the sidecar gap (H4), the fill-binding gap (H2), and the digest gap in one move, matches the project's own "coherence from one joint program" philosophy, and lets the layers survive only as a diagnostic decomposition of the STF's failure output. For the native root check, consume the sequencer's persisted qMDB proofs via the guest verifier path so the production check is the independent one.

---

## Theme 3 — Conventions as prose, not constraints

**The pattern.** The two strongest conventions in AGENTS.md are stated and then violated, because nothing enforces them.

**"All-integer, no floating point":**
- Every solver is f64-based (prices from rounded f64 duals). This is *acceptable* because fills are witness data the verifier re-checks — but it is undocumented, contradicts the "no float anywhere" claim, and produces a real edge bug: rounded/renormalized duals can put a fill price a few nanos over an order's limit, which the verifier's exact `>` check rejects. → [10-core-math](10-core-math-and-solvers.md).
- **Worse:** `order_book.rs` reservation release uses `ratio = remaining as f64 / max_fill as f64; new_cost = (old_cost as f64 * ratio).ceil() as i64` on a path whose values are committed into the **state root** (verified at `order_book.rs:575-592`). Above 2^53 nanos (~$9M reserved) f64 loses integer precision, and any independent re-execution with exact integer math derives a different root → the block fails verification. → [11-sequencer](11-sequencer.md).
- Unchecked `i64` multiplications (`limit_price as i64 * max_fill as i64`) across `validation.rs`, `order.rs`, `orders.rs`, `settlement.rs`, with **no `[profile.release] overflow-checks`**, so release builds wrap silently. → [C1/H3](01-critical-bugs.md).

**"Prefer actors over mutexes":** three actor idioms coexist (ractor in the sequencer, hand-rolled ryhl in polymarket, dedicated OS-thread commonware runtimes for qMDB); the polymarket mirror shares an `Arc<RwLock<MappingStore>>` and wraps a pointless `Mutex` inside a single-task actor. → [13](13-polymarket-mirror.md), [19](19-workspace-consistency.md).

**The fix.** Make both conventions mechanical:
- Introduce `Nanos(u64)` / `SignedNanos(i64)` / `Qty(u64)` newtypes with checked/saturating i128-backed multiplication and an explicit `ceil_mul_ratio(remaining, max_fill)` helper. The order-book f64 bug becomes unwritable; the `u64→i64` API wraps become `TryFrom` sites.
- Add `[profile.release] overflow-checks = true` to the workspace.
- Add `clippy.toml` `disallowed-types`/`disallowed-methods` banning `f32`/`f64` in `matching-engine` and the `matching-sequencer` core modules (exempt metrics/admission), and `[workspace.lints]` with `clippy::unwrap_used = "deny"` for library crates.

This converts an entire class of findings into compile errors.

---

## Theme 4 — Half-finished migrations leave two of everything

**The pattern.** Good refactors that were started and not completed, leaving a transitional seam that carries cost and drift risk.

**Where:**
- **Hot/cold state split.** `Hot State and Read Models.md` (`status: planned`) is partially built: the `ReadModelStore` trait exists and three cold routes (fills/events/equity) use it — but they still round-trip through the actor mailbox, ~25 read RPCs remain on the actor, `list_markets` fires a 10-way `try_join` of actor RPCs per dashboard poll, and `get_block_by_height` only searches the 100-entry in-memory ring (returns 404 for persisted older blocks). → [11](11-sequencer.md), [12](12-api.md).
- **Admission decoupling.** The new single `ADMISSIONS` stream coexists with legacy `PENDING_BUNDLES`/`ADMIT_LOG` tables still created, cleared, and merged on load "until old devnet rows are absorbed"; `try_admit_direct`/`AdmitOutcome` survive only for tests. → [11](11-sequencer.md).
- **Canonical encodings.** See Theme 6.

**The fix.** Finish each migration and delete the old path — a devnet reset is acceptable (early dev). Route all cold reads directly to `ReadModelStore` in `sybil-api` and delete the actor's read RPCs; drop the legacy admission tables; make the actor enum fit on one screen (submit, cancel, bridge, resolve, latest-block, account, pause).

---

## Theme 5 — God-files and a god-crate

**The pattern.** A few files and one crate hold a disproportionate share of the system, mixing concerns that want separate homes.

**Where:**
- `matching-sequencer` is **23.6k lines — 37% of the workspace** — mixing the production node + a full agent simulator + two storage engines + actors + crypto. Inside it: `sequencer.rs` 5.3k, `store.rs` 3.2k, `actor.rs` 3.1k. Its `Cargo.toml` description still says "Agent-based multi-batch simulation."
- `sybil-prover/main.rs` 2.5k (eight concerns incl. a mock producer sharing the real status interface), `matching-sim/main.rs` 2.2k (zero tests, hand-rolled argv despite a `clap` dep), `arena/viz/news_explorer.py` 1.8k (re-implements a matching engine by parsing display strings), `arena/nba/` 6.6k (dead), `routes/markets.rs` 0.9k.

**The fix.** Split along real seams:
- `matching-sequencer` → node core (BlockSequencer, order_book, settlement, admission, canonical_state, block) + `sequencer-runtime` (actor/supervisor/handle/mailbox) + `sequencer-store` (store/account_storage/qmdb_*) + `sequencer-analytics` + a new `sybil-sim` for the agent harness. Rename the node crate `sybil-sequencer` (it depends on `sybil-*` crates; the `matching-` prefix is a lie).
- Break the god-files by phase (e.g. `produce_block_in_place` → system-event application, batch assembly, solve/settle/minting, witness assembly — each under 100 lines of orchestration and independently testable).

---

## Theme 6 — Consensus-critical encodings duplicated by hand

**The pattern.** The byte layouts that must agree exactly across the sequencer, verifier, and ZK guest are copy-pasted and held together only by comments and snapshot tests — and are **already diverging.**

**Where:**
- `hash_header` exists three times (sequencer `block.rs:338`, verifier `block.rs:298` with a "Must match matching-sequencer" comment, `sybil-zk lib.rs:663`).
- Account digest event encoders are byte-for-byte duplicated between `matching-sequencer/digest.rs` and `sybil-zk/lib.rs` — and the zk copy **omits the fill (0x01) and mint (0x05) tags**, which is exactly why fill-driven digest updates are unverified (a structural gap, not an accident).
- `bridge_account_key` implemented twice with the same domain string; the canonical `Order`/`BridgeWithdrawal`/`Attestation` mirrored in `sybil-signing` "without importing" the source; two `append_position_reservations` encoders that **already disagree** (one sorts by `(market,outcome)` and drops zeros, the other sorts by `(market,outcome,qty)` and keeps zeros).
- Three `Order` representations (`matching_engine::Order`, `sybil_signing::Order`, the API DTOs) hand-mapped in `crypto.rs::to_canonical_order`; a new `Order` field is silently excluded from the signature unless someone updates the mapping. Only `expires_at_block` has a guarding test — and the deployed `trade.html` [already drifted on exactly this field](01-critical-bugs.md#h11).

**The fix.** Extract one dependency-light, `no_std`-able **`sybil-commitments`** crate that owns *every* consensus byte layout: signing bytes, header hashing, event/state/witness leaf prefixes, bridge/DA domain constants, account-key derivation, and checked-nanos arithmetic. `matching-sequencer`, `sybil-verifier`, and `sybil-zk` all consume it. Drift becomes a compile error instead of a comment. Unify the two reservation encoders deliberately while doing it.

---

## Theme 7 — Duplication of user-facing surfaces

**The pattern.** Multiple independent implementations of the same thing, kept in sync by hand, where the reachable one is often the worst.

**Where:**
- **Frontends (four+).** A polished, undeployed 26k-line Next.js app; a 3k-line Alpine console compiled into the API binary and deployed at `:3000`; a broken `/trade` page whose signatures can never verify; a Streamlit arena dashboard; plus a local Streamlit solver visualizer. The Next.js app's "Dev Zone" hand-mirrors the Alpine console (~3,900 lines re-implementing 1,727, self-documented as "re-sync by hand"). → [17](17-frontend.md).
- **Block stream (two transports).** SSE and WebSocket both sit on the same broadcast channel; SSE silently drops lagged blocks and has no versioning/replay while WS has all three. The vault documents only SSE. → [12](12-api.md).
- **Arena LLM trader (two copies).** `sim/llm_trader.py` and `live/trader.py` duplicate the FV-parsing, prompt-building, and client-factory logic with drift. → [16](16-arena.md).

**The fix.** Pick one of each and delete the rest. Deploy the Next.js app and delete `static/` + the Dev Zone duplication; keep WebSocket and delete SSE; unify sim/live traders on one core with injected Clock/ArticleSource/PriceSource boundaries. Make `sybil-api-types` (via `/openapi.json`) the one type oracle that generates the frontend types and the Python SDK, so drift becomes structurally impossible.

---

## Theme 8 — The documentation estate is five layers of sediment

**The pattern.** Overlapping docs at different truth levels, with the load-bearing numeric facts drifting in triplicate.

**Where:** an excellent Obsidian vault (validated links, honest `status: planned` markers, hot-path notes co-maintained with code) **plus** a fictional Mintlify site (fees, SDK, endpoints that don't exist; describes a deleted solver generation) **plus** a dead Kamal `DEPLOY.md` **plus** a stale README pointing at superseded design docs **plus** a committed live API key. Facts that appear in multiple places contradict: **36 vs 37 vs 38** verification checks; **500ms vs 1s vs 10s** block cadence; **5 vs 6** solvers; a **5-block TTL** claim vs an effectively-GTC default. → [20](20-documentation-estate.md).

**The fix.** Three estates with hard rules: (1) the vault is the only prose spec; (2) generated reference — README repo-map from `cargo metadata`, API reference from `/openapi.json`, the solver table from one source; (3) `docs/ops/` runbooks. Ban hardcoded volatile facts (counts, cadences, benchmarks) in `status: current` notes. Run `check-vault.sh` in CI and extend it to validate backticked paths and `crate:` frontmatter. Delete the Mintlify site (or move to a marketing repo), `DEPLOY.md`, and the API key.

---

## Theme 9 — Tests and CI are absent exactly where the bugs live

**The pattern.** Coverage is strong in the mature core and missing in precisely the seams that harbor the defects.

**Where:**
- `proptest` is a declared dev-dependency of `matching-solver` with **zero uses**; there is no solver conformance suite, and the scenario generators only ever emit single-market orders, so nothing the API can produce (spread/bundle/custom) is exercised against a solver.
- The three sequencer bugs (H7, H8, open-batch placers) all sit in untested seams — no store test sets `checkpoint_interval > 1`.
- `zk/openvm-tools` **does not compile** (missing `pre_state_sidecar` in a struct literal) and both `zk/` packages are outside the workspace with zero CI.
- No CI job for the Python arena (despite `ruff`/`pytest` configured and a currently-failing date-dependent test), no `docs-check`, the frontend's 114-test suite runs only on dev machines, and `just check-all` omits arena while AGENTS.md calls it "the CI equivalent."
- Deploy depends on one laptop; no backups; the ZK guest's committed artifacts predate witness-format changes.

**The fix.** Land the missing CI: a parameterized solver conformance suite over `&dyn Solver` + proptest generators asserting `verify_match(strict)` passes and positions balance; arena `ruff`+`pytest`; `docs-check`; frontend `pnpm test`; a `cargo hack --each-feature` check (would have caught D8); a `cargo check --tests` job for `zk/`. Make CI literally run `just check-all` and put every check in `check-all` so the two cannot drift.

---

## Theme 10 — Deployment is a demo with the doors open

**The pattern.** A thoughtful observability stack undermined by a security and resilience posture that assumes a private network it doesn't have.

**Where:** `SYBIL_DEV_MODE=true` in prod exposes unauthenticated account-minting and arbitrary market resolution to the internet (the mirror pipeline structurally depends on dev-mode endpoints, which is *why* it's on); every internal port is published on `0.0.0.0` on the public host (unauthenticated VictoriaMetrics writes, Grafana `admin/admin`, raw prover); the entire alert stack runs on the monitored host with `-notifier.blackhole` by default and no external heartbeat, so the documented "zombie host" failure is undetectable; a live API key is committed and also passed via argv; no backups; single disk at 73%; container memory limits sum to ~4.3GB on a 1.9GB host. → [18](18-ops-deployment.md).

**The fix.** Introduce a privileged-service auth tier (the mirror already owns a P256 identity — have it sign) so `dev_mode` can be off in prod; rebind all non-Caddy ports to `127.0.0.1`; move secrets to `/opt/sybil/.env`; add a dead-man's-switch alert + external uptime probe + a persistence-failure alert + a disk alert; add a backup recipe; move to CI-built images on a registry so deploys stop depending on one machine.

---

## How the themes map to the roadmap

| Phase | Themes addressed |
|-------|-----------------|
| 0 — Stop the bleeding | 1 (reject unsupported orders), 2 (fail-closed), 3 (overflow-checks), 10 (dev-mode, ports, key) |
| 1 — Delete the sediment | 5, 7, 8 (deletions) |
| 2 — Resolve the schisms | 1, 2, 4, 6 |
| 3 — Enforce & harden | 3 (newtypes/lints), 9 (CI), 10 (registry/backups) |

See [30-roadmap.md](30-roadmap.md) for the sequenced plan.
