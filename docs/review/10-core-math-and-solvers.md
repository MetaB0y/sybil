# Core Math and Solvers

**Crates:** `matching-engine`, `matching-solver`, `matching-scenarios`, `matching-sim`, `fuzz`

## Verdict

The single-market LP core is the best-engineered part of the exchange: well-factored, deterministic-enough, with a genuinely good shared plumbing layer. It is wrapped in a thick layer of aspirational generality the code does not implement, which produces two value-leak bugs and ~2,500 lines of dead or fossil code. This subsystem embodies [Theme 1](02-cross-cutting-themes.md) more than any other.

## Architecture as built

**`matching-engine` (~2.7k LOC)** is the pure-data core. `types.rs` defines `Nanos = u64` and `Qty = u64` as bare aliases (not newtypes), `MarketId(u32)` with a `u32::MAX` sentinel, and f64↔nanos helpers. `order.rs` is the center: `Order { markets: [MarketId;5], payoffs: [i8;32], num_markets, num_states, limit_price, max_fill, condition: Option<PriceCondition>, expires_at_block }` — a payoff vector over 2^N atomic states, with the counterintuitive convention **outcome 0 = YES**. `order_builder.rs` holds the only payoff-vector constructors (simple, spread, bundle, butterfly, ratio_spread, conditional). `settlement.rs` is a pure, side-effect-free module (`compute_fill_settlement`, `derive_minting`) **shared verbatim with `sybil-verifier`** — a genuinely good boundary. `mm_constraint.rs` models MM budgets.

**`matching-solver` (~6.2k LOC)** implements the `Solver` trait behind feature flags. The production reality: `matching-sequencer` depends with `features = ["lp"]` and hardcodes `LpSolver::new()`. The other five solvers (EG, Conic, IterLP, Decomposed, MILP) are used only by `matching-sim` and benches. The LP core builds a linear program (variables `q_i ∈ [0, max_fill]`, per-market mint columns, group-mint columns), extracts prices as `|dual|` rounded and renormalized to sum to $1, and shades MM budgets with one SLP re-solve. The shared plumbing (`SolverContext`, `build_and_solve_lp`, `finalize_result`) de-duplicates the LP family well. EG/Conic/IterLP each end with the same "projection LP" scaffold, triplicated.

**The lab:** `matching-scenarios` (seeded ChaCha8 generators — but emits *only* single-market orders), `matching-sim` (2,183-line hand-rolled CLI), `fuzz` (2 targets: API parse and settle no-panic).

**Divergence from docs:** the vault says "the matching engine is EG/Fisher-market," but production runs plain linear-welfare LP with 1-round SLP shading. Docs describe entropy smoothing that does not exist and "no floating point anywhere" while every solver is f64-based.

## Strengths

- The LP-family refactor (`SolverContext` / `build_and_solve_lp` / `finalize_result`) is real de-duplication done well.
- `settlement.rs` as a pure module shared by sequencer and verifier is exactly the right boundary; it uses i128 intermediates and documents its truncation.
- The conic solver's numerical engineering (1/NANOS objective scaling, the `t' = α_k·t` substitution for interior-point conditioning) is thoughtful and well-commented.
- `iterative_lp_solver.rs`'s tests *construct* the LP-SLP failure mode with concrete scenarios — engineering-science, not hand-waving.
- Deterministic seeded scenario generation with reproducibility tests; the `OrderDirection` byte encoding is pinned by a test that explains its ZK consequence.

## Findings

| ID | Kind | Sev | Summary |
|----|------|-----|---------|
| [C1](01-critical-bugs.md) | bug | **critical** | Multi-market orders accepted but mis-modeled in release (single-market assumption is a `debug_assert`, compiled out) |
| [C2](01-critical-bugs.md) | bug | **critical** | Custom payoff magnitudes (`[2,0]`) break fill-price semantics → value leak invisible to verifier |
| CM-1 | bug | high | i64 overflow in `validation.rs:39` (`limit_price as i64 * max_fill as i64`), no quantity cap, no release overflow-checks — see [D4](01-critical-bugs.md) |
| CM-2 | doc-drift | high | Docs describe entropy smoothing that doesn't exist; deterministic tie-breaking is an unimplemented documented invariant |
| CM-3 | bloat | high | `matching-solver/src/verifier.rs` (554 LOC) is a stale, weaker duplicate of `sybil-verifier::verify_match`; only consumer is `matching-sim`, which already calls the real one two lines earlier |
| CM-4 | bloat | medium | Conditional-order machinery is a five-layer dead feature (struct field, builder, canonical encoding, ZK check) with no API path and no solver support |
| CM-5 | bloat | medium | `PipelineResult` is a fossil of a removed multi-phase pipeline; `combine_stats`/`ucp_stats`/`iteration_stats`/`phase_snapshots` are never populated |
| CM-6 | bloat | medium | `book.rs` (558 LOC `LiquidityBook`/`LiquidityPool`) is dead outside one uncalled viz helper; `marginal_payoffs_*` (~75 LOC) and `Problem::validate` are dead |
| CM-7 | bug | medium | Clearing prices from rounded/renormalized f64 duals can exceed a filled order's limit by a few nanos; nothing clamps, and the verifier's limit check is exact |
| CM-8 | bug | medium | `DecomposedSolver` and `EgSolver` outputs are non-deterministic across runs (HashMap/HashSet iteration order in component numbering and MM-group summation) |
| CM-9 | inconsistency | medium | Welfare/minting-cost semantics differ across solvers: LP zeroes `minting_cost`; MILP reports net-of-minting; the `finalize` gate means different things per solver |
| CM-10 | design | medium | `MmSide` duplicates information derivable from the payoff vector and is never cross-validated → wrong capital formula possible |
| CM-11 | test-gap | medium | `proptest` is a dead dev-dependency; no invariant tests for solver output; four ~300-line copy-pasted test suites (~1,200 LOC) |
| CM-12 | inconsistency | low | State-index convention (0 = YES) is confusing and self-contradictorily documented (the `order.rs` docstring argues with itself; the `Payoff Vectors.md` worked example flips bit order) |
| CM-13 | bloat | low | Dead deps: `rand`/`rand_chacha`/`proptest` in `matching-solver` src; `clap` unused in `matching-sim`; `state.rs` mixed-radix machinery for a binary-only system; MILP `DualAnalysis` returns `Default` |
| CM-14 | bug | low | EG Frank-Wolfe fallback step moves against the objective (`γ = 2/(t+2)` even when `φ'(0) ≤ 0`) and keeps the last iterate, not the best |

## Ambitious ideas

1. **Collapse the solver zoo to two.** Keep `LpSolver` (production) and `ConicSolver` QuasiFisher (the theoretically canonical EG per `paper.typ` in `~/github/prediction-markets-are-fisher-markets/` and the benchmark winner). Delete `EgSolver` and `IterLpSolver` (redundant EG approximations — keep their killer test scenarios in the conformance suite), and move `DecomposedSolver` + MILP into a research crate outside the sequencer's dependency graph. Halves `matching-solver` and makes "what runs in production" obvious from the crate.
2. **Resolve the payoff-vector schism decisively** (the C1/C2 root). *Option A (honest minimalism):* solver input becomes `BinaryOrder { market, outcome, side, limit, qty }`, converted at the sequencer boundary with hard rejection of anything else. *Option B (honest generality):* implement per-**state** balance constraints in the LP so bundles/spreads price coherently out of joint EG, and charge the state-price inner product in settlement. Either eliminates the entire critical-bug class; the current halfway state is the worst of both.
3. **Make `sybil-verifier` the single verification brain.** Delete `matching-solver/verifier.rs`; run `verify_match(strict)` in the sequencer pre-commit; define solver correctness as "always produces a witness the ZK layer accepts" and drive one parameterized conformance suite (over `&dyn Solver` + proptest generators) through it.
4. **Shrink `PipelineResult` to `SolveResult { fills, prices, welfare, solve_time }`** and delete the pipeline fossils, the dead half of `viz.rs`, and all of `book.rs` — ~2,000 LOC of pure deletion with zero behavior change.
5. **Introduce integer-safe money types** (`Nanos(u64)`, `Qty(u64)` with u128-backed `Mul`) and `overflow-checks = true` in release. Turns the whole silent-wrap class into compile errors and makes the all-integer convention machine-enforced. (See [Theme 3](02-cross-cutting-themes.md).)
6. **Fix determinism explicitly:** a canonical post-solve normalization pass (sort fills by order id, clamp prices into the limit half-space, stated rounding rule) so solver output is a pure function of the `Problem` regardless of HiGHS internals — then document *that* in place of the fictional entropy smoothing, with a determinism proptest.
