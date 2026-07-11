# Sybil

*Private bets. Fair prices. Your alpha stays yours.*

---

## What Is Sybil?

Sybil is a prediction market with a different market structure.

Instead of a continuous orderbook where speed wins, Sybil runs **private batched auctions**. Orders accumulate privately, then clear at a single fair price. No frontrunning. No snipers. No information leakage.

Your collateral earns yield while locked in positions. Markets resolve quickly via automated oracles. Everything is ZK-proven — you verify, not trust.

---

## The Best Forecaster Problem

Imagine you're the best prediction market trader in the world.

You did the work. You read the primary sources. You hired local pollsters. You flew to Guatemala and talked to people on the ground. You built a model that synthesizes everything. The market says 66%. Your model says 90%.

You have $1M in conviction. You start buying.

After your first $100k, the price moves to 68%. Normal. But then it keeps moving — 72%, 78%, 85% — in seconds. You haven't bought anything else yet.

What happened?

You're #1 on the leaderboard. Everyone watches your wallet. The moment you touched the market, bots copied you. They front-ran your own thesis. Your $1M position now costs $1.4M, or you get a fraction of the size you wanted.

**The edge you built with months of work got eaten in milliseconds by people who did nothing.**

So what do you do? You create throwaway accounts. Anonymous wallets. You hide.

And it works, sort of. Your alpha stays yours. But now:

- You can't build public reputation
- You can't prove your track record ("I'm top 10 on Polymarket" → "prove it" → you can't, your real account has no volume)
- You can't attract capital from others who'd back your judgment
- You can't use your forecasting skill as a credential for anything else

The best forecasters are invisible by necessity. The leaderboards are dominated by snipers running copy-trading bots — people who built fast infrastructure, not people who actually know things.

**The signal is polluted. The reputation system is broken. The people who should be building track records can't.**

---

## The 3EV Lens

There's a useful framework from MEV research that clarifies what's broken and how to fix it: **3EV** (Mafia, Moloch, Monarch Extractable Value).

Every market has three types of value extraction:

**MafiaEV** — Value extracted through asymmetric information. Sophisticated actors exploiting others via private knowledge: frontrunning, copy-trading, sandwiching. In prediction markets, this is the bots that watch whale wallets and copy trades, the snipers that catch news 400ms faster than market makers.

**MolochEV** — Value lost to uncoordination. The "price of anarchy." Market makers widening spreads defensively because they expect to get sniped. Capital sitting at 0% yield because there's no coordination. Latency races that waste resources and produce nothing.

**MonarchEV** — Value extracted by whoever controls ordering and execution. The platform, the sequencer, the coordinator. Can they frontrun you? Censor you? Manipulate the match?

The thesis: **Total extractable value = Mafia + Moloch + Monarch.** The ideal is 0% Mafia, minimal Moloch, and a Monarch that's constrained and transparent.

Current prediction markets are terrible on all three:
- **MafiaEV is rampant** — copy-trading and frontrunning dominate
- **MolochEV is high** — defensive spreads, dead capital, wasted latency competition  
- **MonarchEV is opaque** — trust the platform not to exploit you

---

## How Sybil Fixes This

### MafiaEV → Zero

Private orders. Nobody sees your position until the batch clears. Nobody can copy you. Nobody can frontrun you.

Batch auctions eliminate speed advantage. Everyone in a batch gets the same price. The bots that dominate current leaderboards — copy-traders, news snipers, frontrunners — have no edge.

**The leaderboard reflects forecasting skill, not infrastructure sophistication.**

### MolochEV → Minimized

Batch auctions are coordinated clearing. Instead of continuous adversarial trading, everyone submits orders, then they clear at a single fair price. MMs don't need to widen spreads defensively because there's no speed disadvantage.

Yield-bearing collateral eliminates the opportunity cost of long-dated markets. Capital isn't dead — it's working. The coordination problem of "why lock up money at 0%" goes away.

### MonarchEV → Constrained and Verifiable

The TEE sees orders (necessary for matching). But it can't cheat on execution — that's ZK-proven. Every batch settlement generates a proof that the matching was fair and the price was optimal.

The only thing the Monarch (TEE) can do is censor orders. And censorship is detectable — if your order consistently doesn't appear, you can prove it.

**Narrow trust assumption. Verifiable execution. Constrained Monarch.**

---

## Selective Reputation via ZK Proofs

Privacy is the default. But you choose what to reveal.

Want to prove you're top 10 by PnL without revealing which trades were yours? ZK proof.

Want to prove your Sharpe ratio over 100 markets without doxxing your positions? ZK proof.

Want to reveal your full track record because you're raising a fund? You can do that too.

You control your reputation. Prove exactly what you want, nothing more. Build credentials without leaking alpha.

**You can be known as the best without being front-run for being the best.**

---

## The Capital Problem

"Will Jesus Christ return in 2025?" sat at 3% on Polymarket for months. To buy NO, you lock up $0.97 per share. For a year. Earning nothing.

This is insane.

Long-dated markets should be where prediction markets shine. Elections. Technological milestones. "Will we have AGI by 2027?" But the opportunity cost kills them. Why lock up capital at 0% when you could be getting ~4% in T-bills or more in DeFi?

Sybil wraps collateral in yield-bearing assets. Your money works while it waits. The Jesus market becomes rational to participate in. Long-dated macro bets become viable.

---

## The Market Maker Problem

You're a market maker. You post tight spreads, providing liquidity like a good citizen. Then someone's Twitter bot catches news 400ms before you do. They snipe your stale quotes. You're rekt.

So what do you do? You widen spreads. You post less liquidity. The market gets worse for everyone.

Batch auctions fix this. All orders in a batch execute at the same price at the same time. No speed advantage. MMs can quote tighter because adverse selection drops. More liquidity, better prices, everyone wins.

---

## How The Market Works

Sybil serves two sides:

**Sophisticated traders and market makers** get:
- Private order flow — your edge stays yours
- Batch execution — no adverse selection from speed disadvantage
- Yield on collateral — capital efficiency on long-dated positions
- ZK-proven settlement — verify everything, trust nothing
- Selective reputation — prove your track record without leaking positions

**Retail participants** get:
- Simple UX — bet on outcomes you care about
- Fair pricing — same clearing price as the whales, no one gets picked off
- Instant option — don't want to wait? "Buy Now" fills immediately at a premium

**Instant Execution (Buy Now):** For users who want speed over privacy, Sybil offers an RFQ (Request for Quote) interface. Market makers provide instant quotes at a premium. This doesn't fragment liquidity — MMs hedge these positions into the next batch. The batch auction remains the central liquidity engine; "Buy Now" is just a premium access point for the impatient.

---

## Resolution

The biggest failure mode in prediction markets isn't corruption — it's **ambiguity**. "Did they *agree* to a deal?" "Does this count as *announcing*?" Vague resolution criteria lead to disputes, delays, and eroded trust.

Sybil addresses this at the source: **resolution criteria are defined before the market opens**, not argued about after.

When a market is created, it includes explicit conditions for YES and NO outcomes. What sources count. What wording qualifies. What edge cases resolve to which outcome. This removes subjectivity at market creation rather than resolution.

For the **95% of markets that are straightforward**, automated resolution (including LLM evaluation against the pre-defined criteria) settles outcomes in hours, not days.

For the **5% of genuinely ambiguous cases** — situations the criteria didn't anticipate — there's an escalation path to human arbitration. This is where current platforms struggle, and the problem isn't fully solved anywhere. Sybil won't pretend otherwise.

The goal: make most markets unambiguous by design, handle the rest honestly.

---

## How It Works

```
User → Encrypted Order → TEE → Batch Settlement → ZK Proof → On-chain
                                      ↓
                              Validium (off-chain DA)
```

**Private until settlement:** Orders are encrypted and processed inside a TEE. Nobody sees them until the batch clears.

**Proven correct:** Every batch settlement generates a ZK proof. The matching was fair. The price was optimal. Verify it yourself.

**Narrow trust assumption:** The TEE could theoretically censor orders (not include them). It cannot cheat on execution — that's ZK-proven. Censorship is detectable and attributable.

**Validium economics:** Posting 10k individual orders to Ethereum would be prohibitively expensive. Posting a single ZK proof covering 10k orders costs almost nothing. Data lives off-chain, proofs live on-chain. This is what makes frequent batch intervals economically viable.

**Already benchmarked:** ZK proof generation for 10k trades per batch runs on a laptop in seconds. 100k+ is feasible with proper infrastructure.

---

## Why Now?

This wasn't buildable a year ago:

- **ZK proving costs dropped 10-100x** — real-time proving of batch settlements is now feasible on commodity hardware
- **TEE tooling matured** — easier to deploy, easier to verify, more trusted
- **Validium economics work** — data off-chain, proofs on-chain makes frequent batches viable
- **Legal clarity** — Polymarket proved the model, Kalshi won its lawsuit, prediction markets are "mostly legal"
- **LLMs can evaluate criteria** — automated resolution against pre-defined rubrics actually works now

---

## Bootstrapping

**Market coverage:** Launch with mirrors of high-volume Polymarket markets. Same questions, better execution (privacy + yield). Users don't have to learn new things to bet on.

**Liquidity migration:** Bring your Polymarket positions, get matched exposure on Sybil plus tokens. Reward people for moving liquidity over.

**Token launch:** Clear economics from day one while Polymarket dangles vague airdrop hints. Airdrop farmers are captive — give them something concrete.

---

*Sybil is in early development. This document will evolve.*

