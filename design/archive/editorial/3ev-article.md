# 3EV: A Framework for Value Extraction in Prediction Markets

Prediction markets systematically leak value to speed, to coordination failure, and to operator discretion. A [blockchain analysis](https://coin360.com/news/polymarket-traders-losses-profit-concentration-2026-predictions) of ~1.7 million Polymarket addresses found that 70% have never turned a net profit, while the top 0.04% captured over 70% of $3.7B in cumulative gains. That level of concentration reflects more than differences in skill.

There's a framework from MEV research — developed by [sxysun](https://hackmd.io/@sxysun/short-note-ext) at Flashbots, first presented at [Devcon Bogota in 2022](https://archive.devcon.org/archive/watch/6/this-is-mev/?playlist=Devcon+6) — that makes this precise. It's called **3EV**: three types of extractable value that together account for how markets lose value.

**MafiaEV** — value extracted through information asymmetry. One participant sees something before another can react: stale-quote sniping, copy-trading, latency arbitrage.

**MolochEV** — value destroyed by coordination failure. No one captures it. Spreads widen defensively, capital sits at zero yield, cross-market inefficiencies leak to external bots. The price of anarchy.

**MonarchEV** — value extracted, or extractable, by whoever controls the execution layer. The operator sets fees, chooses rules, controls infrastructure access. The trust surface is broad and unverifiable.

Total leakage = Mafia + Moloch + Monarch. In sxysun's formulation, the ideal is 0% Mafia, 0% Moloch, and a Monarch whose profits are constrained and redistributed. The three types compound.

## Mafia

We covered this in detail in [The Sniper's Tax](https://sybilpm.substack.com/p/the-snipers-tax). The compressed version:

In February 2026, across Polymarket's non-sport, non-price markets, we identified 473 price spikes and traced the money. $311k extracted from market makers in one month. Loss-to-volume ratio of 1.38%. 1,874 unique snipers, 269 repeat offenders. Median spike: 3 minutes 16 seconds.

The mechanism: news breaks, a sniper buys at the pre-news price, the market reprices, and the MM who had a resting order absorbs the full move. In equities this costs basis points. In prediction markets, where a contract goes from 10 cents to 95 cents on a headline, it costs the position.

But sniping is the visible, measurable part. The other two leakages are larger and less discussed.

## Moloch

MolochEV shows up as absence. Trades that didn't happen, liquidity that wasn't posted, markets that couldn't sustain participation. Sniping feeds into this, but Moloch also has causes that are purely structural.

Locked capital is the clearest example. A 2028 presidential election market trading at 40 cents requires locking $0.60 per NO share until November 2028, earning nothing while T-bills yield 4%+. At those economics, sophisticated liquidity stays away. What remains is a thin order book on the question where accurate pricing matters most. No one stole anything. The liquidity just never arrived.

Pricing fragmentation works the same way. [$39.6 million in arbitrage profits](https://medium.com/@navnoorbawa/negrisk-market-rebalancing-how-29m-was-extracted-from-multi-condition-prediction-markets-2f1f91644c5b) drained from Polymarket's multi-outcome markets in a single year because the matching engine prices each outcome independently. A retail trader buys one candidate without realizing they've mispriced the rest of the field. A bot detects the inconsistency and extracts the difference. These multi-outcome opportunities were only 8.6% of arbitrage instances but generated 73% of profits. The retail trader doesn't even know they lost.

## Monarch

Any centralized prediction market with a continuous order book creates a broad trust surface for the operator: visibility into all order flow, control over matching, rule-setting authority, discretion over infrastructure access. Users have no mechanism to verify any of it.

Polymarket's [new fee structure](https://docs.polymarket.com/trading/fees) makes this concrete. Taker fees peak at 1.80% for crypto, 1.00% for politics, 0.75% for sports ([projected at $800k-$1M/day](https://phemex.com/news/article/polymarket-projects-1m-daily-revenue-with-new-fee-structure-68935)). The fees are probability-based, peaking at 50% — maximum cost where uncertainty is highest. They're taker-only, so the platform's revenue scales with all trading activity, including toxic flow. A portion (20-50%) is recycled as maker rebates; the rest the operator retains. Fee rates, category exemptions, and redistribution percentages are set unilaterally and can change at any time — as can any other parameter of the system. Sophisticated participants respond accordingly: less capital, wider quotes, more caution.

## How They Compound

The three types form feedback loops.

**Mafia creates Moloch.** We can measure this directly. Across 154 spikes on markets that didn't resolve, the median bid-ask spread widened from 4.1 cents to 10.0 cents, and order book depth within 10 cents of mid dropped 44.8%. In 64% of spikes, spreads widened; in 49%, depth was gutted by more than half. After the US-Iran nuclear talks spike, spreads went from 1 cent to 20 cents. After the Israel-Lebanon strike markets, the order book was emptied entirely.

What happens to trading activity is starker. During the spike and the first 30 minutes after, volume runs at 17-126x the pre-spike hourly rate — this is the extraction event itself and its immediate aftermath. After that, the market goes quiet. Among the 243 spikes where trading volume exceeded $500 in the 4 hours before (median baseline: $3,059), 81% had zero trading volume in the 30-minute to 2-hour window afterward. 96% had zero from 2 to 4 hours out. (The result is stable across baseline filters from $10 to $1,000 pre-spike volume.) The spike doesn't degrade the market. It kills it.

**Monarch enables Mafia.** Public order flow lets anyone see positions and replicate strategies. The operator controls infrastructure that participants depend on, and when it fails, the cost falls on them. One market maker we spoke with described this: the data feed went through periods of random delay, they didn't receive position updates, accumulated excess inventory, tried to cancel, and the cancel requests failed. Multiple large wipeouts from infrastructure failures, not from being wrong about the market.

**Moloch insulates the Monarch.** When spreads are wide and order books are thin, users attribute the problem to "prediction markets are hard" rather than tracing it to specific design decisions. Coordination failure becomes ambient. The platform points to market conditions rather than its own architecture.

The fee structure shows all three loops at once. The monarch introduces taker fees (MonarchEV). These compound with sniping losses: MMs now absorb adverse selection and pay 2-3.6% round-trip costs. MMs widen spreads further or exit (MolochEV). A fraction of fee revenue is recycled as maker rebates — partially offsetting the damage the fees helped create. The platform monetizes the adversarial dynamics it hasn't solved. The rebate program is a partial refund on a problem the architecture generates. Liquidity incentives subsidize MMs, but the subsidies flow through to snipers. Faster cancellation helps MMs, but it's an arms race. Each fix displaces the problem rather than solving it.

## What the Design Space Looks Like

The question is not how to remove the coordinator. It's how to concentrate coordination in one place while constraining it.

Encrypted order flow eliminates MafiaEV — no participant can exploit another's information if they can't see it. Batch auctions reduce MolochEV — simultaneous clearing replaces the latency race, and cross-market matching internalizes arbitrage that would otherwise leak to bots. Verifiable execution constrains MonarchEV — trusted execution environments isolate order data from the operator, and cryptographic proofs ensure the clearing was correct.

But solving Mafia and Moloch increases the coordinator's power. A clearing engine that sees private orders and matches across markets is a more capable Monarch than a continuous order book. sxysun calls this Moloch's Curse: any mechanism that eliminates a coordination failure inherits the coordination role. The batch auction engine IS a Monarch. TEEs and ZK proofs are how you keep it honest — TEEs constrain what the Monarch can see, proofs constrain what it can do. Without both, you've just built a more powerful version of the problem you started with.

Consider a breaking-news event under current architecture: a strike is confirmed, a sniper buys YES at the pre-news price, market makers' resting orders fill at stale prices, they eat the loss and pull remaining quotes, spreads blow out, liquidity vanishes for hours, and the platform collects taker fees on every fill — including the sniper's.

Under a batch auction with private orders: the strike is confirmed, orders accumulate during the batch window, the clearing engine computes a single price reflecting all submitted information, and every participant trades at that price. The MM's resting order fills at the fair price, or not at all. The sniper's speed advantage is gone because everyone in the batch is simultaneous. No stale quotes to pick off means no sniping loss, which means no defensive spread widening, which means the flywheel can actually run. And a cryptographic proof of the settlement means you verify the clearing was correct rather than trusting the operator.

Privacy without batch auctions still leaves MMs exposed to the latency race. Batch auctions without privacy still allow information extraction. Verifiable execution without the other two just proves that an unfair market was operated correctly.

## Why It Matters

This is not about trader PnL. Prediction markets are supposed to aggregate information — a mechanism for society to know what's likely before it happens. The leakages described here prevent these markets from sustaining liquidity on the events where accurate prices matter most: geopolitics, policy, breaking crises. 81% of markets going silent after a spike is a design failure in infrastructure that was meant to be public.

A prediction market that can't hold liquidity during a news event has failed at its core function. The three types of value extraction are not a tax on participants. They're a tax on the market's ability to do what it exists to do.

---

*Data from our analysis of February 2026 Polymarket trading. Full dataset and methodology at [sybil.exchange/spike-analysis](https://sybil.exchange/spike-analysis). For the detailed sniping mechanics, see [The Sniper's Tax](https://sybilpm.substack.com/p/the-snipers-tax).*

---

## TODO: Before publishing

> Remove this section before publishing.

### Done
- Spread widening data: computed from 214 orderbook snapshot pairs (154 unresolved markets)
- Depth destruction data: same source
- Volume direction: increases post-spike (narrative adjusted)

### Data inserted
- Compounding section: "median spread 4.1c → 10.0c, depth dropped 44.8%, 64% widened, 49% gutted >50%"

### Still possible (not critical)
- **Spread recovery time**: would need time-series orderbook data (not just pre/post snapshots). Current data has one post-spike snapshot per event, not a recovery curve.
- **Fee-on-sniping overlay by category**: would need market category labels mapped to the spike data. Could cross-reference with Polymarket's category taxonomy.
- **Dollar multiplier**: "for every $1 sniped, $N in market quality destroyed" — hard to compute rigorously from snapshot data. The spread widening + depth destruction numbers are more honest than a fabricated dollar multiplier.
