# Sybil market-structure evidence report

Protocol: `sybil-market-structure-heldout-2026-07-21-v1`  
Issue: [#198](https://github.com/MetaB0y/sybil/issues/198)  
Evidence implementation: `29c4651c661cba312f6a1419d06ef9b747e56cc5`

## Executive answer

Sybil does not have an unconditional frequent-batch-auction advantage over a
competent continuous limit order book. The held-out result is conditional:

- Current FBA protects a stale maker better when the CLOB maker's cancellation
  reaches the venue after informed flow. Across the 324 jump configurations,
  FBA had significantly lower stale loss in 135, higher stale loss in 105, and
  tied in 84. There is no preregistered materiality threshold for total stale
  loss, so these are paired 95% interval classifications, not materiality
  labels.
- At 500 ms, with a 60-cent jump, 1-cent quote, and a CLOB maker reacting at
  500 ms versus a 25 ms informed taker, FBA improved maker markout by $1.705 per
  episode (95% CI $1.452 to $1.959) and maker PnL by 32.885 cents per filled
  share (27.566 to 37.721 cents). Its fill-rate difference was -4.688 points,
  below the preregistered 5-point threshold, and its 219.9 ms delay difference
  was below the 250 ms threshold.
- The same 60-cent/1-cent setup reverses when maker cancellation ties the 25 ms
  taker and wins the declared tie-break. The CLOB avoids the trade; current
  non-cancellable FBA loses $1.245 per episode (95% CI $0.991 to $1.498), which
  becomes informed-trader surplus.
- One, two, and eight informed traders produced identical configuration-level
  advantage/disadvantage counts. This experiment does not support a claim that
  more same-batch informed traders strengthen current Sybil price protection.
- Quiet flow favors different stakeholders. The CLOB maker earns positive
  pre-incentive spread PnL and executes immediately. FBA's natural traders
  match one another, giving the maker zero PnL; it fills more at wide quotes but
  waits for the batch.
- Atomic shared-budget allocation is real, but it is not a generic batching
  advantage. Against a shared-risk CLOB, catalog coverage is identical. FBA
  materially improves fills only at the 10% budget boundary; at 25% the gain is
  below the declared threshold, and at 50% it is zero.

No single cell wins for makers, ordinary traders, coverage, price accuracy,
and delay simultaneously. The evidence supports conditional product language,
not a universal market-structure claim.

## Evidence contract

The controlled experiment uses 128 held-out paired seeds, 10000 through 10127.
Every mechanism receives the same fundamental path, shock phase, information
and venue-arrival times, valuations, quantities, budget, inventory, zero fees,
and zero incentives. FBA uses the production `ProductionSolver` and verifier.
The CLOB has price-time priority, immediate execution, cancellation/replacement
latency, cancellation-before-taker priority on an exact timestamp tie, IOC
informed orders, and natural orders resting through the episode.

All 133,632 engine rows completed or completed with zero fill. There were no
solver failures, panics, timeouts, or verifier-invalid attempts. Conditional
metrics remain undefined when either paired engine has no defined value; fill
and coverage metrics retain every zero-fill episode.

The experiment identifies consequences of this declared model. It does not
estimate production traffic, customer demand, or equilibrium spreads.

## News jumps and stale quotes

The timing boundary, not batching alone, determines the stale-loss result.

| CLOB timing relative to informed taker | Current FBA result |
|---|---|
| Maker slower: 500 ms vs 25 ms | Lower stale loss in 15 of 18 cells at each cadence; the three ties are the 10-cent jump at a 10-cent half-spread. |
| Maker slower: 2,000 ms vs 25 ms | Same 15-of-18 lower-loss pattern at every cadence. |
| Maker slower: 2,000 ms vs 500 ms | Lower stale loss in 15 of 18 cells at every cadence. |
| Exact timing tie | The declared CLOB cancellation wins; FBA is worse once informed flow remains in the eligible batch. |
| Maker faster: 25 ms vs 500 ms | CLOB avoids stale fills; FBA is worse at 1 s and 10 s and tied at 500 ms when the taker generally lands in a fresh batch. |

For the representative 60-cent jump, 1-cent quote, and one informed 5-share
order:

| Case | CLOB maker markout | Current FBA maker markout | Fill rate, CLOB / FBA | FBA delay |
|---|---:|---:|---:|---:|
| 500 ms batch; maker 25 ms, taker 25 ms | $0.000 | -$1.245 | 0% / 95.31% | 219.9 ms |
| 500 ms batch; maker 500 ms, taker 25 ms | -$2.950 | -$1.245 | 100% / 95.31% | 219.9 ms |
| 10 s batch; maker 2,000 ms, taker 500 ms | -$2.950 | -$1.245 | 100% / 95.31% | 4,396.6 ms |

The 10-second case preserves the average maker protection but adds a material
4,396.6 ms delay relative to CLOB (95% CI 3,931.0 to 4,882.1 ms). Longer
batches are therefore not a free way to obtain protection.

The fixed maker quote is 10 shares and each informed order is 5 shares. Two
traders exhaust the maker quote; eight lowers the aggregate trader fill rate
but does not change the maker-side clearing classifications. A claim about
competition moving price would require richer two-sided informed demand,
multiple makers, or an equilibrium response—not merely more identical IOC
orders on one side.

## Current versus cancellable FBA

The live Sybil MM bundle is deferred and one-shot. It cannot use the
resting-order cancellation path after acknowledgement and before its eligible
batch. A separately labeled future-design sensitivity allowed atomic
cancel/replace before cutoff.

That sensitivity lowered stale loss with a positive paired interval in 165 of
324 jump configurations and tied in 159. It also caused a material fill-rate
reduction in the same 165 configurations. Per-filled-share maker PnL and price
error were not materially better. Cancellation mainly removes toxic fills; it
does not create a free execution-quality improvement.

This sensitivity is not current Sybil behavior.

## Quiet long-tail flow

Four natural 5-share limit orders arrive during each quiet episode. FBA allows
them to cross at one batch price. The CLOB executes continuously against the
maker or against earlier natural orders.

| Half-spread | Fill rate, CLOB / FBA | Maker PnL, CLOB / FBA | Price error, CLOB / FBA |
|---:|---:|---:|---:|
| 1 cent | 100% / 100% | $0.200 / $0.000 | 1.00 / 4.47 cents |
| 5 cents | 91.21% / 100% | $0.447 / $0.000 | 4.14 / 4.47 cents |
| 10 cents | 93.95% / 100% | $0.207 / $0.000 | 5.33 / 4.47 cents |

The 5- and 10-cent FBA fill gains are material, but the maker captures no
spread because natural traders match each other. At a 1-cent spread the CLOB
fills the same amount, earns spread, executes immediately, and has materially
lower price error. FBA mean delay is 248.9 ms at 500 ms, 498.1 ms at 1 s, and
4,984.5 ms at 10 s; the latter two are material disadvantages.

Thus maker profitability before incentives is not stronger in quiet flow, even
where natural-trader matching improves fill coverage.

## Shared budget and catalog coverage

The firm-reserve CLOB reserves worst-case two-sided capital for every quoted
market. It is a useful lower bound, but the shared-risk CLOB is the fairer
coverage comparator: it displays the whole catalog under one risk limit,
executes serially, consumes capital, and cancels later fills when exhausted.

Against shared risk, FBA and CLOB have identical displayed, single-market
executable, and simultaneous-worst-case coverage in all 18 portfolio
configurations. FBA's material fill advantage appears only in the six 10%
budget configurations:

| Markets | Flow | FBA minus shared-risk CLOB fill rate at 10% budget (95% CI) |
|---:|---|---:|
| 8 | head-heavy | +15.51 points (+13.69, +17.37) |
| 8 | shuffled | +16.31 (+14.53, +18.09) |
| 8 | uniform | +18.12 (+16.35, +19.83) |
| 72 | head-heavy | +18.08 (+17.34, +18.83) |
| 72 | shuffled | +17.65 (+16.88, +18.42) |
| 72 | uniform | +20.80 (+20.13, +21.45) |

At 25% budget, differences range from 0 to 3.17 points and do not meet the
5-point threshold. At 50%, every fill-rate difference is zero.

For 72 uniform markets:

| Budget | Simultaneous coverage, both | Fill, CLOB / FBA | Capital consumed, CLOB / FBA | Natural surplus, CLOB / FBA | Price error, CLOB / FBA |
|---:|---:|---:|---:|---:|---:|
| 10% | 9.72% | 40.05% / 60.85% | $69.12 / $65.73 | $11.55 / $5.42 | 2.00 / 8.69 cents |
| 25% | 25.00% | 97.70% / 99.61% | $169.37 / $156.77 | $28.01 / $12.89 | 2.00 / 6.38 cents |
| 50% | 50.00% | 100% / 100% | $173.57 / $158.92 | $28.65 / $14.00 | 2.00 / 6.07 cents |

FBA maker PnL per filled share is materially higher in all 18 shared-risk
comparisons, while FBA price error and delay are materially worse in all 18.
At 50% budget, joint maker-plus-natural surplus is identical ($35.851 per
episode): higher maker PnL is a transfer from traders, not extra gains from
trade. At 10%, joint surplus is higher because FBA actually fills more under
the scarce budget.

The defensible advantage is therefore atomic allocation under severe capital
scarcity, not broader displayed coverage and not a universal capital-efficiency
claim.

## Public Polymarket case study

The identity-free capture contains all 31 markets in the preregistered January
2026 Israel/Gaza event family and 21,190 public taker transactions. Resting-side
rows reconcile exactly for 21,189 transactions. One 1.0752-share transaction is
short by 0.0052 share in the public counterpart projection and remains labeled
unreconciled.

The four previously cited sweeps all reconcile exactly. Their resting-side
gross settlement markouts total -$2,505.26 before fees, rebates, hedges, and
inventory history. Their quantity ranks are 130, 137, 142, and 217 among
21,189 exact transactions; their loss ranks are 33, 42, 130, and 258. They are
large adverse-selection examples, but not the largest transactions in the
family and not an unbiased sample.

Across the complete family, exact resting-side settlement markout sums to
+$66,337.67, and 11,158 transactions have negative markout. That aggregate is
descriptive only: resting counterparties are anonymous executions, not
identified professional makers, and public trade data does not reveal canceled
quotes, reaction races, fees, hedges, or an FBA counterfactual.

## External validity

[Budish, Cramton, and Shim](https://doi.org/10.1093/qje/qjv027) show how
discrete time and a uniform-price auction can turn competition on speed into
competition on price. Their proposed mechanism also permits orders to be
canceled or modified before processing. Current Sybil's acknowledged deferred
MM bundle lacks that boundary, so the classic result cannot be imported
without qualification.

The empirical literature is mixed in the same direction as this study:

- [Zhang and Ibikunle](https://doi.org/10.1016/j.irfa.2023.102737) associate
  periodic auctions with lower adverse-selection costs but weaker liquidity
  and informational efficiency.
- [Lee, Riccó, and Wang](https://doi.org/10.1016/j.finmar.2026.101082) exploit
  Taiwan's move from roughly five-second auctions to continuous trading and
  report improved liquidity and efficiency for mid- and small-cap stocks,
  alongside gains to fast investors and losses to individuals.
- [Kyle and Lee](https://doi.org/10.1093/oxrep/grx042) develop continuous scaled
  limit orders inspired by Fischer Black's flow-trading vision. That is a
  distinct mechanism for controlling temporary impact, not evidence that
  Sybil's current batch rule protects a large one-shot sweep.

Equity venues are not prediction markets, so these are mechanism checks, not
transferable effect sizes.

## Decision

Safe founder-facing statements are:

1. In the held-out controlled model, 500 ms FBA mitigates stale-quote losses
   when informed flow reaches the venue before the CLOB maker can cancel. In
   the representative 60-cent-jump, 1-cent-quote, 500 ms-maker/25 ms-taker case,
   that protection has no preregistered material fill or delay penalty; this
   joint tradeoff does not hold in every jump cell.
2. Atomic shared-budget clearing improves realized fills relative to a strong
   shared-risk CLOB when capital is extremely scarce (10% of full two-sided
   reserve), while catalog and worst-case coverage are unchanged.
3. These gains trade against delay, trader surplus, or maker exposure in other
   regimes. Quiet flow and fast cancellation can favor the CLOB.

Do not claim universal maker protection, universally tighter sustainable
spreads, better price discovery, broader catalog coverage than any competent
CLOB, or historical proof that FBA would have prevented a Polymarket sweep.
Simulated fills, accounts, volume, and PnL are not traction.

## Limitations and next evidence

- One fixed-size maker quote and exogenous spreads do not identify competitive
  equilibrium profitability or sustainable quote tightness.
- One-sided informed IOC flow does not model several informed price schedules,
  multiple makers, hedging, inventory, or adaptive strategies.
- The retained-cash solver can transfer surplus between makers and traders;
  this study does not choose the desired economic objective.
- Portfolio episodes use synthetic valuations and arrival order. They identify
  model mechanics, not production market demand.
- Arena and existing replay corpora remain development tools. Their generated
  activity and anonymous snapshots are not calibrated customer flow or a
  cancellation tape.
- The historical capture has settlement markouts only. Timestamped quote,
  cancel, and order-lifecycle data is required for short-horizon causal replay.

Follow-up work is tracked in the issues linked from `CLAIMS.md`.
