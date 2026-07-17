---
tags: [audit, code-quality, testing, economics, solver, sequencer]
layer: matching
status: current-audit
date: 2026-07-17
last_verified: 2026-07-17
---

# Adversarial economic-property audit

Date: 2026-07-17  
Cluster: matching, settlement, and mechanism-level property oracles  
Primary technique: generated invariants with independent arithmetic,
metamorphic relations, falsifiability checks, and solver differential coverage

## Verdict

The production one-hot economic path is better defended after this audit.
Generated tests now independently check:

- fill identity, quantity authorization, account binding, limit feasibility,
  exact uniform clearing price, and price range;
- participant surplus and reported welfare;
- conservative market-maker capital usage with integer ceiling arithmetic;
- cash plus complete-set collateral conservation;
- complementary-price coherence on every traded market;
- a two-block complete-set mint-then-burn lifecycle; and
- agreement between solver output, sequencer state, and the full verifier.

The audit found no failing economic result from the production HiGHS retained-
cash profile. It passed 2,048 generated zero-tolerance conformance cases after
the independent checks were added.

One real solver defect was found in the experimental structural retained-cash
oracle. Its convergence classifier used a fixed 32-ULP allowance based on the
possibly cancelled final score. A valid dust-scale book could therefore return
`NumericalFailure` at an exact structural breakpoint. The classifier now uses
a conservative floating-evaluation bound derived from absolute affine terms,
operation count, minting terms, and each marginal order's quantity
sensitivity. The raw unsnapped gap remains visible in diagnostics. Both the
production and structural profiles then passed 2,048 generated cases.

The most important test defect was oracle dependence. The economic replay test
called the same production settlement and minting helpers that it purported to
check. It now has separately written checked integer arithmetic. A generated
complete-set burn lifecycle, stronger fill falsifiability tests, and
independent solver MM/UCP checks close the other bounded gaps found here.

One temporal gap is deliberately deferred:
[GitHub #180](https://github.com/MetaB0y/sybil/issues/180) tracks a generated
sell-side reservation state machine across partial fill, cancel, expiry,
revalidation, and recovery. It is in Project 1 as `Todo`, Stage `Backlog`,
Priority `Medium`.

## Evidence boundary

The audited production domain is the currently admitted order language: one
binary market and one exact `+1` or `-1` payoff entry. The review covered:

- integer order and settlement primitives in `matching-engine`;
- retained-cash allocation, integer landing, and generated solver conformance
  in `matching-solver`;
- block production, MINT accounting, account settlement, and existing economic
  properties in `matching-sequencer`; and
- `sybil-verifier` as a second acceptance path, while avoiding it as the only
  oracle.

The generated solver cases include all four public directions, multiple
markets, boundary quantities including `MAX_ORDER_QTY`, several MM side/budget
profiles, zero-tolerance certificates, and verifier replay.

This is not a proof of research-only general or multi-market payoff economics.
It is also not an actor/persistence audit, an L1 collateral audit, a
performance benchmark, a long-running fuzz campaign, or a live deployment
test. Those boundaries are explicit so passing properties are not mistaken for
a whole-system proof.

## Architecture context read

The review used the root guidance and the focused guidance for
`matching-engine`, `matching-solver`, `matching-scenarios`,
`matching-sequencer`, and `sybil-verifier`, together with:

- `Payoff Vectors`
- `Binary Markets and Market Groups`
- `Nanos and Integer Arithmetic`
- `Order Types`
- `Welfare Maximization`
- `Welfare vs Volume`
- `Solver Landscape`
- `The LP Core`
- `Retained Cash Solver`
- `LP Duality and Clearing Prices`
- `MM Budget Constraint`
- `Block Lifecycle`
- `Order Admission`
- `Pending Orders and TTL`
- `Settlement`
- `Testing Strategy`
- `Block Witness`
- `Four-Layer Verification`

The key constraints carried into the test design were integer protocol truth,
exact one-hot production admission, welfare rather than volume as the
objective, UCP, collateralized complete sets, conservative MM budgets, and
independent post-landing verification.

## Research basis and method

The audit combined several techniques rather than treating a random generator
as an oracle:

- The original
  [QuickCheck paper](https://www.cis.upenn.edu/~bcpierce/courses/552-2008/resources/icfp-quickcheck.pdf)
  frames properties as executable specifications over generated inputs and
  emphasizes the value of minimal counterexamples.
- The maintained
  [proptest strategy documentation](https://docs.rs/proptest/latest/proptest/strategy/trait.Strategy.html)
  and [book](https://proptest-rs.github.io/proptest/proptest/index.html)
  informed boundary-biased strategies, deterministic campaigns, shrinking,
  and persisted regression seeds.
- Research on
  [metamorphic property-based testing](https://arxiv.org/abs/2211.12003)
  supports checking relations between executions when a full expected-output
  oracle is difficult. Existing arrival-order permutation and quantity-scaling
  relations were retained, but their preconditions and trade coverage were
  made explicit.
- Gnosis Conditional Tokens documents and implements complete-set
  [split/merge semantics](https://github.com/gnosis/conditional-tokens-contracts/blob/master/docs/glossary.rst)
  in its
  [reference contract](https://github.com/gnosis/conditional-tokens-contracts/blob/master/contracts/ConditionalTokens.sol).
  That motivated exercising both creation and destruction of a collateralized
  set instead of testing buy-side minting only.
- Higham's analysis of
  [floating-point arithmetic](https://nhigham.com/wp-content/uploads/2021/04/high21m.pdf)
  and
  [summation error](https://nhigham.com/wp-content/uploads/2023/10/high93s.pdf)
  motivated an operation-count/absolute-term `gamma_n` bound rather than an
  arbitrary tolerance on a cancellation-prone final score.

The resulting procedure was:

1. Inventory every current economic property, generator, settlement helper,
   solver conformance check, and settlement fuzz target.
2. Write the invariant in protocol integer units before inspecting the
   implementation path.
3. Identify shared helpers between the system under test and the purported
   oracle.
4. Replace critical shared calculations with narrow, separately written
   `u128`/checked-integer oracles.
5. Add generated paths missing from the inventory, especially complete-set
   burning and all MM sides.
6. Perturb valid outputs to prove the checkers reject overfills, non-UCP
   prices, limit violations, price incoherence, and cash-conservation defects.
7. Widen deterministic generated campaigns, persist minimized failures, and
   turn confirmed defects into regression tests.
8. Run full package, feature, lint, and documentation gates.

## Invariant inventory

| Invariant | Evidence before this audit | Gap found | Final evidence |
|---|---|---|---|
| Cash plus complete-set collateral conservation | Generated deposits, buys, withdrawals, and resolution | Replay reused production settlement/mint helpers; buy-side only | Independent checked arithmetic plus generated mint/burn round trip |
| Fill authorization and individual rationality | Limit check skipped zero fills | No uniqueness, positive-quantity, max-fill, account, or UCP assertion | One independent feasibility checker covers all fields |
| Uniform clearing price | Full verifier and market presence in conformance | No direct outcome-price equality in solver conformance | Exact one-hot side oracle checks every non-zero solver fill |
| Complementary prices | Minted buy markets only | Burning/traded markets were omitted | Every traded binary market is checked; lifecycle pins exact sum |
| Welfare | Sequencer-reported values | No independent participant-surplus sum for the lifecycle | Separate signed surplus oracle matches witness and analytics |
| MM solvency | Shared engine/verifier budget helpers | A shared rounding defect could bless solver and verifier together | Independent `u128` conservative-ceiling capital oracle |
| Complete-set destruction | Example production tests | No generated same-holder burn round trip | 64-case two-block mint/burn lifecycle with zero final positions |
| Metamorphic execution | Shuffle, scaling, multi-block relations | One generated crossing property did not assert that it traded | Every guaranteed-crossing case now asserts non-zero fills directly |
| Solver certificate status | Generated conformance | Fixed ULP floor failed at structural breakpoints | Higham-style bound, positive and negative guardrails, 2,048 cases/profile |
| Reservation lifecycle | Example unit/recovery tests | No independent generated temporal model | Deferred to #180 |

## Findings and disposition

| ID | Severity | Finding | Disposition |
|---|---|---|---|
| EP-1 | High | The main conservation replay reused `compute_fill_settlement` and `derive_minting`, so a shared arithmetic defect could make implementation and oracle agree. | Fixed with independent checked one-hot settlement and MINT arithmetic. |
| EP-2 | High | The fill property could accept zero fills, duplicates, overfills, account mismatches, and favorable non-UCP prices because it checked only limits and a `$1` ceiling. | Fixed; added a full feasibility oracle and falsifiability regressions. |
| EP-3 | Medium | Generated economic paths exercised complete-set creation but not destruction from live holdings. | Fixed with a two-block mint/burn property and independent welfare/value checks. |
| EP-4 | Medium | The experimental structural retained-cash oracle could report `NumericalFailure` at an exact optimum because its 32-ULP certificate floor ignored cancellation and breakpoint quantity sensitivity. | Fixed with a conservative `gamma_n` evaluation bound, a persisted seed, two unit guardrails, and widened conformance. |
| EP-5 | Medium | Solver conformance delegated UCP and MM capital truth to shared helpers/verifier logic. | Fixed with direct outcome-price and conservative-ceiling budget oracles. |
| EP-6 | Low | A global atomic “coverage” diagnostic always printed zero under the parallel Rust test runner and never failed; the generated property itself did not require a trade. | Removed the misleading order-dependent test; the property now asserts a non-zero fill per generated case. |
| EP-7 | Low | Direct-dual conic helpers compiled as dead code when their only consumer feature was disabled, preventing strict Clippy from serving as a clean gate. | Fixed with exact `conic` feature boundaries; LP, sequencer, and conic Clippy profiles pass with `-D warnings`. |
| EP-8 | Medium | Sell inventory reservation lacks an independent generated state-machine oracle across partial fill, cancellation, expiry, and recovery. | Open as #180 with acceptance criteria and Project 1 metadata. |

### EP-1 — independent settlement replay

The prior `replay_claimed_block` called the same two helpers used by production
settlement and verifier derivation. It could detect a perturbed fill price, but
not a defect shared by those helpers.

The replacement oracle is intentionally narrow:

- classify exact one-hot buy/sell sides from payoff entries;
- compute `floor(price_nanos * quantity / SHARE_SCALE)` with checked `u128`;
- apply cash and signed position deltas without production helpers;
- derive the MINT counterparty from aggregate YES/NO imbalance; and
- compute the conservation defect from deposits, withdrawal escrow, account
  cash, and one-dollar complete-set collateral.

Narrowness matters here. Reimplementing every research payoff rule would create
a second complex implementation rather than an auditable oracle.

### EP-2 — fill feasibility and falsifiability

The strengthened checker rejects:

- duplicate witness order IDs;
- unknown or duplicate fill order IDs;
- zero-quantity fills;
- `fill_qty > max_fill`;
- non-zero fill account IDs that disagree with the witness;
- prices above one dollar;
- missing market/outcome clearing prices;
- fill prices unequal to the published outcome UCP;
- buyer prices above limits and seller prices below limits; and
- any disagreement between independent payoff classification and
  `Order::is_seller`.

The honest generated block passes the checker. Deterministic perturbations prove
that it rejects a one-unit overfill and a favorable one-nano non-UCP execution,
in addition to the existing limit, incoherent-price, and conservation
perturbations. A favorable execution is not accepted merely because the trader
benefits: UCP is a market-level fairness rule.

### EP-3 — complete-set mint/burn lifecycle

The new 64-case property creates two funded accounts, generates complementary
buy limits whose sum crosses one dollar, and fully mints a complete set. It
then submits complementary sells from those live holdings with a crossing
limit sum and fully burns the set.

For both blocks it checks:

- both orders fill fully;
- the price vector sums exactly to one dollar;
- fill feasibility and UCP;
- independent participant surplus equals witness and analytics welfare;
- signed `minting_cost` is positive face value for creation and negative face
  value for destruction; and
- `verify_full` accepts the witness.

After the second block, all generated positions are zero, aggregate cash is
restored exactly, and the independent conservation defect is zero.

### EP-4 — structural certificate roundoff

The widened zero-tolerance campaign minimized a structural-oracle failure. A
previous regression had already raised the representation floor to 32 ULPs of
the final certificate scores. That was still structurally incomplete:

- current-score terms can have large cancellation;
- an exactly marginal breakpoint has almost zero hinge value;
- a few ULPs in breakpoint probability can be multiplied by a large
  `max_fill`; and
- the observed raw residual was `0.11920928955078125` nanos despite a small
  final objective scale.

The new bound sums absolute affine contributions, minting cost, and a
conservative Lipschitz scale for every order, then applies
`gamma_n = n*u/(1-n*u)` with an intentionally conservative operation count.
Only convergence classification uses this representation floor. Diagnostics
continue to report the raw unsnapped positive gap.

Two deterministic tests pin both sides:

- the minimized dust book converges with a positive raw representation gap;
  and
- a zero-iteration, materially non-optimal book remains `IterationLimit` with
  a gap of at least one dollar.

The minimized conformance seed
`74794fe1358c2f76416876a2b441d758d0ceb26ea2950f5982a7e7327d937b27`
is persisted in `solver_conformance.proptest-regressions`.

### EP-5 — solver conformance independence

Every filled generated one-hot order now maps its payoff to an outcome without
calling the production side helper and requires
`fill_price == clearing_prices[market][outcome]`.

For each MM constraint, a second oracle:

- derives risk price directly from side;
- multiplies risk price and fill quantity in `u128`;
- applies conservative ceiling division by `SHARE_SCALE`; and
- requires the independent sum not to exceed `max_capital`.

It deliberately does not call `MmSide::capital_needed`,
`MmConstraint::capital_used`, or the verifier. Existing verifier and settlement
checks remain useful as additional paths.

### EP-6 and EP-7 — diagnostic and lint integrity

Rust test order is not a global sequencing guarantee. The old
`z_print_trade_coverage` test read atomics that property cases updated in other
parallel test threads, printed zero in observed full runs, and always passed.
The generated crossing property now owns its coverage assertion, and the
misleading diagnostic was removed.

Strict Clippy also exposed methods used only by
`direct_dual_conic_solver`. Those methods and their supporting import/helpers
are now compiled under `feature = "conic"`, matching their only consumer.
This is a build-surface correction, not an algorithm change.

## Implemented changes

- Added independent one-hot side, notional, settlement, MINT, surplus, UCP, and
  MM-budget oracles.
- Strengthened fill feasibility and added overfill/non-UCP falsifiability
  tests.
- Added a generated complete-set mint/burn lifecycle.
- Required guaranteed-crossing generated cases to produce a non-zero fill.
- Removed the cross-test atomic coverage diagnostic.
- Replaced the structural certificate's fixed ULP floor with a conservative
  operation/absolute-term/quantity-sensitivity bound.
- Added structural roundoff and material-gap unit guardrails.
- Persisted the minimized conformance regression seed.
- Put direct-dual-only helpers behind the `conic` feature.
- Updated the retained-cash architecture note and structural-oracle experiment
  record.
- Opened #180 for the intentionally separate sell-reservation state machine.

No verifier/guest code, canonical protocol state, contract, deployment pin, or
live service was changed in this cluster.

## Verification

All final gates below passed on 2026-07-17:

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | Pass |
| `cargo test -p matching-sequencer` | 363 unit, 2 crossing, 12 economic, and 10 general invariant tests pass |
| Deterministic economic property binary | 12/12 pass |
| Complete-set lifecycle widened run | 128 generated cases pass |
| Full retained-cash generated conformance | Production HiGHS and structural profiles each pass 2,048 zero-tolerance cases |
| `cargo test -p matching-solver --features retained-cash` | 24 unit and 2 conformance tests pass |
| `cargo test -p matching-solver --features lp` | 39 unit and 6 conformance tests pass |
| `cargo test -p matching-solver --features conic` | 56 unit and 8 conformance tests pass |
| `cargo clippy -p matching-sequencer --all-targets -- -D warnings` | Pass |
| `cargo clippy -p matching-solver --features lp --all-targets -- -D warnings` | Pass |
| `cargo clippy -p matching-solver --features conic --all-targets -- -D warnings` | Pass |
| Strict exact-wire API Clippy profile | Pass; the prior dependency warning blocker is resolved |
| `just docs-check` | Protocol pins, doc sync, vault, links, and strict site build pass |

The generated campaigns use a fixed RNG seed for reproducibility and proptest
shrinking. The structural counterexample is additionally persisted.

## Open work and residual risk

1. Implement #180 in the actor/stateful cluster. Example tests already cover
   several reservation paths, but they do not replace an independent generated
   temporal model.
2. [GitHub #66](https://github.com/MetaB0y/sybil/issues/66) remains open for
   conserved MM budget allocation inside the experimental decomposed solver.
   Passing returned-result conformance does not prove that its intermediate
   component allocation and reported prices are coherent.
3. The `fuzz_settlement` target is primarily a no-panic harness over a broader
   research payoff domain. This cluster added semantic properties for the
   admitted one-hot domain; it did not run an unbounded fuzz campaign or claim
   semantic coverage for arbitrary payoff vectors.
4. Floating-point optimization remains behind integer landing and
   verification. The new structural error bound is conservatively tested, not
   a formal proof of every platform/library evaluation path.
5. L1 deposit custody, redemption, reorg/finality, Solidity/Rust differential
   semantics, and live collateral backing remain outside this cluster.

## Completion criteria

This cluster is complete when:

- the admitted economic invariants and their prior oracle dependencies are
  inventoried;
- critical settlement, UCP, welfare, and MM-budget claims have independent
  arithmetic or differential evidence;
- complete-set creation and destruction are both generated;
- checker falsifiability is demonstrated;
- any minimized solver defect has a persisted regression and a bounded fix;
- production and research feature profiles pass proportionate generated,
  package, and strict-lint gates; and
- broader temporal work is deduplicated and tracked in GitHub with Project 1
  metadata.

Those criteria are met. The cluster can become a dated reference after #180 is
implemented or explicitly superseded; until then it remains `current-audit`.
