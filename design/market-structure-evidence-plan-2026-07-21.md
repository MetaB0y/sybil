---
tags: [research, matching, economics]
status: proposed
last_verified: 2026-07-21
---

# Market-structure evidence plan

Issue: [#198](https://github.com/MetaB0y/sybil/issues/198)

## Decision to resolve

Sybil needs to know where its market design creates an economically meaningful
advantage over a competent continuous limit order book, and where the cost of
waiting for a batch or the absence of within-batch spread capture dominates.
The answer is allowed to be conditional, mixed, or negative.

This study will not estimate customer demand. It will not infer production
maker profitability from Arena bots, synthetic devnet flow, or solver welfare.

## What the repository already establishes

- The production mechanism is a frequent batch auction cleared by the actual
  retained-cash `ProductionSolver`, with integer fills and prices checked at the
  protocol boundary.
- The solver benchmark suite has good provenance and public Polymarket depth
  and taker-flow projections, but those cases compare solver algorithms. Their
  anonymous depth cannot recover maker capital, cancellations, information
  arrival, or a CLOB-versus-FBA counterfactual.
- Arena is suitable for offline strategy experiments but its generated and
  LLM activity is not calibrated customer flow.
- The shared MM constraint can allocate one execution budget across many
  markets in a batch. A comparison must separate this atomic portfolio feature
  from batching itself.

The new package therefore reuses the production solver and existing integer
types while keeping the dynamic CLOB and experiment orchestration outside
production semantics.

## Identification strategy

No single dataset identifies the requested answer. The evidence package uses
three non-interchangeable layers.

### 1. Public historical case study

The Polymarket Data API exposes public trade projections with transaction,
price, size, side, outcome, and timestamp. Gamma exposes the complete January
2026 Israel/Gaza daily event family and resolution outcomes. The capture will
project away participant identities, retain all available rows for all 31
markets, and preserve source hashes.

This can answer:

- whether large, rapid book sweeps occurred;
- their effective YES price range and volume after converting complementary NO
  fills;
- gross short- and long-horizon markouts of resting-side executions; and
- whether the four anecdotes are typical or tail observations within that
  event family.

It cannot answer whether counterparties were designated makers, whether their
net strategy lost money, whether a cancel was racing the taker, or what an FBA
would have cleared at. Those are explicit non-claims.

As a provenance check, the known 3 January transaction currently projects to a
2,094.927975-share YES buy at a 0.5728120553 weighted price. Complementary
resting-side rows span effective YES prices from 0.11 to 0.84. With YES
resolution, gross settlement markout on those fills is about $894.93 before
fees, rebates, hedges, and prior inventory. The committed capture, not this
planning observation, will own any final number.

### 2. Paired mechanism experiments

Each independent episode generates one exogenous tape: fundamentals, public
shock time, participant observation and network latency, valuations,
quantities, budget, and initial inventory. Both mechanisms receive that tape.

The CLOB baseline has price-time priority, immediate execution, cancel/replace,
and explicit venue-arrival ordering. It is evaluated with two capital policies:

- firm reservation, where every displayed maker quote reserves worst-case
  capital; and
- shared account risk, where broad quotes are executable serially until the
  common risk limit is consumed and remaining quotes are canceled.

The second is deliberately strong. It distinguishes Sybil's atomic
simultaneous budget allocation from the weaker claim that a CLOB simply cannot
implement account-level controls. Neither mechanism gets to call every broadly
submitted quote simultaneously firm when one shared budget cannot honor every
cross. The harness reports displayed-market coverage, single-market
executability, simultaneous worst-case coverage, and realized fill coverage
separately.

The FBA path uses the repository's production solver. Ordinary resting orders
can use the cancellation path, but the live MM's acknowledged multi-order,
MM-constrained quote is held as a deferred one-shot bundle. The current cancel
path only removes orders already in the resting book, so that bundle cannot be
canceled or replaced before its eligible batch. The primary model preserves
that exposure. A separately labeled counterfactual may measure the value of a
future atomic deferred-bundle cancel boundary, but it is not current Sybil
behavior. Shock phase, CLOB maker reaction time, batch cadence, and number of
independently informed price competitors are therefore first-class axes.

### 3. External-validity synthesis

The report will compare the controlled mechanisms with primary literature,
including:

- Budish, Cramton, and Shim's FBA mechanism and stale-quote argument:
  <https://doi.org/10.1093/qje/qjv027>
- their implementation discussion and batch-interval tradeoff:
  <https://doi.org/10.1257/aer.104.5.418>
- Lee, Ricco, and Wang's 2026 Taiwan switch from five-second auctions to
  continuous trading, which reports overall liquidity and efficiency gains for
  continuous trading in mid- and small-cap equities:
  <https://doi.org/10.1016/j.finmar.2026.101082>
- Zhang and Ibikunle's periodic-auction evidence, which reports lower adverse
  selection alongside weaker liquidity and informational efficiency:
  <https://www.pure.ed.ac.uk/ws/portalfiles/portal/365839645/ZhangZIbikunleG2023IRFATheMarketQualityEffects.pdf>

These studies prevent a one-sided interpretation. Equity venues differ from
binary, jump-sensitive prediction markets, so the report will treat them as
external evidence about mechanisms and tradeoffs rather than transferable
effect sizes.

Official Polymarket endpoint semantics are documented at:

- <https://docs.polymarket.com/api-reference/core/get-trades-for-a-user-or-markets>
- <https://docs.polymarket.com/market-data/websocket/overview>

## Most decision-relevant uncertainties

1. **Price competition condition.** Uniform pricing protects a stale quote
   only if enough same-batch information moves the clearing price. The live
   deferred MM bundle cannot currently replace its quote before the cutoff, and
   one informed taker against one stale seller need not yield a well-identified
   fair price.
2. **Cadence tradeoff.** Longer batches increase the chance of aggregating
   informed competition and cancellations, but impose delay and can worsen
   price discovery for ordinary flow.
3. **Equilibrium spread.** A fixed-spread comparison measures transfer, not the
   long-run liquidity response. Profit curves across a preregistered spread
   grid are needed before discussing sustainable tightness.
4. **Quiet long tail.** When jumps are rare and cancellation is fast, a CLOB can
   earn spread and execute immediately; FBA may offer little or negative
   benefit.
5. **Shared-budget attribution.** Breadth from atomic portfolio allocation is a
   separate Sybil feature. It must not be marketed as a generic consequence of
   batching.
6. **Public-data selection.** A known sweep is a mechanism case study, not an
   estimate of average adverse selection. The complete prespecified event
   family and explicit missingness are required.

## Smallest credible experiment set

The development matrix is in
`benchmarks/market-structure/protocol-development.json`. After mechanics and
runtime are validated, the frozen protocol will retain only axes needed to
resolve the six uncertainties above, record an untouched seed range, and bind
an immutable pushed implementation revision.

The publishable run must include:

- quiet and jump episodes at the binary default 500 ms cadence and the current
  Compose 10 s cadence, plus only the intermediate cadences justified during
  development;
- at least one, two, and several informed same-batch traders;
- CLOB maker reaction both faster and slower than taker reaction, with uniform
  shock phase across the batch; current non-cancellable Sybil bundles remain
  exposed until their eligible clearing;
- a spread grid rather than one hand-picked quote width;
- the firm-reserve and shared-risk CLOB capital policies;
- a many-market shared-budget episode with uniform and concentrated flow;
- every zero-fill and solver-failure row; and
- paired uncertainty intervals over held-out seeds.

## Stop conditions

Implementation may improve clearly mechanical research support, capture
validation, and reporting. It stops before choosing production cadence, fees,
maker incentives, cancellation policy, or a new economic objective. Those are
product/protocol decisions and require separate issues if evidence makes them
valuable.

Research changes must not be deployed to the devnet.
