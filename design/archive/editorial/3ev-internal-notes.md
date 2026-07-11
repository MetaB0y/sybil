# 3EV Article — Internal Notes & Cut Material

> Supporting evidence, numbers, and links cut from the published article for length/pacing. Kept here for reference, fact-checking, and potential use in other content.

---

## MafiaEV — Extended Data (cut for brevity, article references Sniper's Tax instead)

- 14 of the 20 most profitable wallets on Polymarket are bots. Source: [Finance Magnates](https://www.financemagnates.com/trending/prediction-markets-are-turning-into-a-bot-playground/)
- Arbitrage traders extracted roughly $40 million from Polymarket between April 2024 and April 2025 by exploiting structural pricing inefficiencies. Source: same
- One trader turned $313 into $414,000 in a single month through latency arbitrage between Polymarket's internal pricing and external exchange spot prices. Another trader (0x8dxd) earned $515,000 in a month with 7,300+ trades at 99% win rate through temporal arbitrage. Sources: [Phemex (1)](https://phemex.com/news/article/trader-exploits-oracle-latency-for-50k-profit-in-one-week-45143), [Phemex (2)](https://phemex.com/news/article/polymarket-adjusts-rules-to-counter-temporal-arbitrage-exploits-52741)
- Markets hit hardest: geopolitical — Iran negotiations, Gaza/Lebanon strikes — $59k sniped across 70 spikes. These are also the markets with the highest information value.
- A prediction market maker holding "Will Israel strike Lebanon today?" has no correlated instrument to hedge with. They're simply exposed. (Daedalus Research's "[Toward Black-Scholes for Prediction Markets](https://arxiv.org/pdf/2510.15205)" walks through the full adverse selection framework.)

## MolochEV — Cut Subcases

### Externalized Arbitrage (full version)
A [study of 86 million Polymarket bets](https://medium.com/@navnoorbawa/negrisk-market-rebalancing-how-29m-was-extracted-from-multi-condition-prediction-markets-2f1f91644c5b) across 17,218 market conditions found $39.6 million in total arbitrage profits between April 2024 and April 2025. Of that, $29 million — 73% — came from NegRisk rebalancing in multi-outcome markets, despite these opportunities representing only 8.6% of all arbitrage instances. The pattern: retail traders anchor on one or two favorites and misprice the complementary outcomes. Professional arbitrageurs detect the inconsistency and extract the difference at 29x the capital efficiency of binary arbitrage. A matching engine that cleared orders across related markets simultaneously could capture most of this internally, pricing the entire outcome space coherently at match time.

### Subsidized Extraction (full version)
Polymarket and Kalshi spend millions on liquidity incentive programs — paying people to post orders. Retail LPs who enroll don't have millisecond infrastructure or hedging desks. They're posting orders from the standard API while news breaks on X and a bot reacts in 400ms. The rewards were never designed to cover catastrophic adverse selection. A subsidy flows from retail liquidity providers through to snipers: the retail user provides the quotes, the sniper captures the value, and the platform's incentive budget makes up the difference. The net effect is that the platform pays to maintain the conditions under which MafiaEV extraction continues.

## MonarchEV — Cut/Compressed Evidence

### Volume Inflation (full version)
In December 2025, Paradigm Research [showed](https://www.paradigm.xyz/2025/12/polymarket-volume-is-being-double-counted) that Polymarket's on-chain trade events were being double-counted by every major analytics dashboard. Each fill emits events for both the maker and taker side — the same trade, described from two perspectives — and dashboards were summing both. Reported monthly volume of ~$3.7B was likely ~$1.85B. Separately, a [Columbia University study](https://business.columbia.edu/faculty/press/polymarket-volume-inflated-artificial-activity-study-finds) found that roughly 25% of historical volume was wash trading: accounts trading against themselves to farm anticipated token airdrops, peaking at nearly 60% of weekly volume in December 2024. 14% of the platform's 1.26 million wallets were flagged as wash-trading accounts. The platform reports these figures as "Volume" without qualification. It also reports in notional terms — a share trading at $0.01 is counted as $1 of volume.

Specific wash trading networks documented by CryptoBBT:
- **Lander Network**: 109,000 accounts, $79.9M volume, 94.1% within-cluster trading, collective loss of $64,160.
- **MAY Cluster**: 200 accounts (MAY1-MAY200), wallets "MAY175" and "MAY176" traded the same 7,291 shares back and forth 90 times in 30 minutes generating $700,000 in volume.
- **TenChar Cluster**: 43,011 accounts with random 10-character names, 93.4% within-cluster trading.
- **nojkfaes network**: triangular patterns across hundreds of wallets, $78M in volume for $1,469 collective profit.
Source: [CryptoBBT](https://www.cryptobbt.com/blog/massive-wash-trading-uncovered-on-polymarket)

### Infrastructure Tiers (full version)
Polymarket's "Builder Program" offers three access levels: Unverified (default), Verified (manual approval), and Partner (enterprise). Higher tiers unlock increased rate limits, gasless trading, weekly rewards, and priority support. Upgrades require emailing builder@polymarket.com with your use case and expected volume. The platform decides who gets what.

Rate limits:
- `DELETE /order` endpoint: 3,000/10s burst, 30,000/10min sustained
- `DELETE /cancel-market-orders`: 1,000/100s burst, 1,500/10min sustained
- Polymarket uses Cloudflare-based throttling rather than rejection — requests are queued and latency spikes from ~50ms to 500ms+ before any hard failure
Source: [AgentBets Rate Limits Guide](https://agentbets.ai/guides/polymarket-rate-limits-guide/)

### WebSocket / API Issues (full version)
- **Zombie WebSocket connections**: Polymarket's WebSocket data stream has an open [bug report](https://github.com/Polymarket/real-time-data-client/issues/26) from December 2025 — connections silently stop transmitting data after 18-22 minutes while remaining nominally alive. Confirmed by multiple developers (nopsled, jchook, GorillaDaddy). As of Feb 2026 jchook suggested Polymarket "quietly discontinued support for most of this data." No official Polymarket response.
- **55-second oracle lag**: A developer [measured](https://dev.to/jonathanpetersonn/i-connected-to-a-public-websocket-feed-and-found-mispriced-tokens-on-polymarket-1931) a 55-second lag between oracle price updates and order book reflection.
- **Stale order book API**: The `get_order_book()` endpoint [returned ghost prices](https://github.com/Polymarket/py-clob-client/issues/180) of 0.01/0.99 while `get_price()` returned correct prices of ~0.40/0.41 for the same market.
- **Cancel timeout**: [GitHub Issue #244](https://github.com/Polymarket/clob-client/issues/244) — batch POST requests occasionally exceed 500-700ms, meaning orders reach the book after the market state has already changed. No official response.

### Settlement Integrity (full version)
Polymarket's CTF (Conditional Token Framework) contract attaches a nonce to each wallet, which can be incremented as a fast "cancel all orders" mechanism. But this can be exploited: if a user bumps their nonce, any block containing their previously-signed orders becomes invalid — effectively allowing cancellation of already-filled orders. The platform has reportedly started banning users who exploit this, but the vulnerability is architectural, not behavioral. Separately, users can remove USDC from their wallet before settlement completes, griefing the counterparty. These are reportedly among the reasons Polymarket is considering a move to an appchain design.

### UMA Oracle Manipulation
On March 24-25, 2025, a trader deployed 5 million UMA tokens (~25% of total voting power) across three accounts to manipulate the "Will Ukraine agree to Trump's mineral deal before April?" market from 9% to 100%, fraudulently settling it for $7 million. Source: [Orochi Network](https://orochi.network/blog/oracle-manipulation-in-polymarket-2025)

### MM Wipeout Story (from interviews)
One market maker described: WebSocket feed going through periods of random delay → not receiving position updates in time → accumulating more inventory than intended → trying to cancel excess exposure → cancel requests failing → multiple large wipeouts. Not from being wrong about the market, but from infrastructure failures compounding into unmanageable risk.

## Profitability Data
- 70% of ~1.7M Polymarket addresses have never realized a net profit
- Top 0.04% of addresses captured over 70% of $3.7B in cumulative profits
- 63.5% of profitable addresses earned between $0 and $1,000 (0.86% of all profits)
- ~149 addresses lost more than $1M each
Sources: [Coin360](https://coin360.com/news/polymarket-traders-losses-profit-concentration-2026-predictions), [Yahoo Finance](https://finance.yahoo.com/news/70-polymarket-traders-lost-money-192327162.html)

## Fee Structure — Full Details
- March 30, 2026: fees expand across nearly all categories
- Announced via [docs update](https://docs.polymarket.com/trading/fees), no blog post or tweet
- Rollout: Crypto (Jan 2026) → Sports (Feb 18) → Everything else (March 30)
- Peak rates: Crypto 1.80%, Economics 1.50%, Culture/Weather 1.25%, Politics 1.00%, Sports 0.75%, Geopolitics 0%
- Formula: `fee = C × p × feeRate × (p × (1 - p))^exponent` — peaks at 50% probability
- Maker rebates: 20-50% redistributed (Finance 50%, Politics 25%)
- Projected revenue: $800k-$1M/day based on ~$9.55B/month volume
- Referral program: 30% of fees from referrals for users trading $10k+
- Context: $2B investment from ICE, raising at ~$20B valuation, MLB partnership ~$300M
- Polymarket US exchange has "near-zero fee schedule" — suggesting international users subsidize US compliance
Sources: [Phemex](https://phemex.com/news/article/polymarket-projects-1m-daily-revenue-with-new-fee-structure-68935), [Benzinga](https://www.benzinga.com/markets/prediction-markets/26/03/51435867/polymarket-is-done-being-free-new-fee-structure-to-be-introduced-on-march-30), [ainvest](https://www.ainvest.com/news/polymarket-fee-expansion-liquidity-play-regulatory-risk-2603/)

## Taker Delay Timeline
- Originally 500ms taker delay on certain markets
- Removed in February 2026 without announcement — MMs discovered after the fact
- Later reintroduced at ~250ms
- Source: [Protos](https://protos.com/polymarket-ends-trading-loophole-for-bitcoin-quants/)

## Academic References
- Budish, Cramton, Shim (2015). "The High-Frequency Trading Arms Race: Frequent Batch Auctions as a Market Design Response." QJE 130(4), 1547–1621. [Paper](https://academic.oup.com/qje/article/130/4/1547/1916146)
- Daedalus Research. "Toward Black-Scholes for Prediction Markets." [arXiv](https://arxiv.org/abs/2510.15205)
- "Prediction Markets Are Gambling Act" — bipartisan Senate bill seeking to prohibit CFTC-registered entities from listing sports/casino-style prediction contracts

## Market Scale Context
- Prediction market volumes grew ~4X to $64B in 2025, on pace for $325B+ in 2026
- Combined Polymarket + Kalshi volume exceeded $17B in January 2026 alone
- Source: [FalconX](https://www.falconx.io/newsroom/from-opinions-to-odds-emerging-trends-in-the-prediction-market-landscape)

## Round 3 Cuts

### NegRisk — trimmed detail
Original had: "$29M from NegRisk rebalancing alone — because the matching engine prices each outcome independently rather than clearing the full probability space coherently... a handful of bots extract it at 29x the capital efficiency of simpler strategies." The 29x stat and the "handful of bots" language were cut for pacing. The $39.6M top-line and the structural point (matching engine prices outcomes independently) survive in the article. Full data: study covered 86M bets across 17,218 market conditions; NegRisk opportunities were 8.6% of all arb instances but produced 73% of profits.

### Monarch evidence — trimmed lines
These were compressed from the third Monarch paragraph into a single sentence:
- **500ms taker delay**: "later partially reinstated at ~250ms, after MMs without advance warning got hit" — trimmed to just "removed without notice"
- **Infrastructure tiers**: "Partner-level participants get capabilities unavailable through the public API" — compressed to "tiered infrastructure"
- **Volume inflation**: "before accounting for wash trading" — the [Columbia study](https://business.columbia.edu/faculty/press/polymarket-volume-inflated-artificial-activity-study-finds) link was kept but the 25% wash trading figure and the "peaking at 60% in December 2024" detail were cut. Full detail remains in the MonarchEV section above.

### Monarch framing — cut disclaimers
These lines were removed as hedges that cooled the paragraph:
- "The fee design is rational."
- "None of these requires assuming bad intent."
- "These are features of the architecture, not failures of character."
The structural framing ("A centralized exchange that sees order flow will naturally consider trading against it") now carries this load without the explicit not-accusing-them language.

### MonarchEV definition — original version
Original: "value extracted by whoever controls the execution layer. The platform sets fees, chooses rules, changes them without notice, and trades against its own users — all unverifiable."
Replaced with: "value extracted, or extractable, by whoever controls the execution layer. The operator sets fees, chooses rules, controls infrastructure access. The trust surface is broad and unverifiable."
Reason: original compressed too much accusation into a framework definition. Definitions should be general; evidence comes later.

## Round 4 Cuts

### Opening — fee sentence removed
Original opening: "Polymarket just introduced trading fees across nearly all its market categories — on top of a market structure that already leaks value in three distinct ways."
Cut because: half-hook, half-thesis, committed to neither. Fees now live only in the Monarch section where they're developed properly.

### Dead capital example — Jesus Christ replaced with 2028 election
Original: "'Will Jesus Christ return in 2025?' sat at 3% on Polymarket. To buy NO, you lock $0.97 per share, earning nothing while T-bills yield 4%+."
Replaced with 2028 presidential election (40c market, $0.60 locked per NO share until Nov 2028). Reason: the Jesus market is amusing but it's a meme market, which undercuts the "social value" argument. A presidential election makes the capital lockup problem feel consequential.

### Volume stat methodology
Volume finding ("81% zero volume 30min-2h after, 96% zero 2-4h after") tested at four baseline thresholds:
- $10 baseline: n=422, 85% / 96% zero
- $100 baseline: n=336, 82% / 96% zero
- $500 baseline: n=243, 81% / 96% zero (USED IN ARTICLE)
- $1,000 baseline: n=189, 81% / 96% zero
Finding is stable across all thresholds. Article uses $500 threshold with baseline stated explicitly ($3,059 median) for defensibility.

## Round 5 Cuts — Monarch gutted to one example

### Cut: In-house trading desk
"Polymarket runs an [in-house trading desk](https://www.coindesk.com/business/2025/12/05/polymarket-hiring-in-house-team-to-trade-against-customers-here-s-why-it-s-a-risk) that trades against its own customers."
Harry Crane (Rutgers): "takes a platform that previously felt very new and different and instead makes it look and feel just like everyone else." Kalshi faced class-action over similar arrangement.

### Cut: Taker delay removal
"It [removed a 500ms taker delay](https://protos.com/polymarket-ends-trading-loophole-for-bitcoin-quants/) without telling market makers first."
Later partially reinstated at ~250ms. One trader described old regime as "basically free money" and new one as making "latency the only moat."

### Cut: API reliability gaps
"Its public API has [documented reliability gaps](https://github.com/Polymarket/real-time-data-client/issues/26) while a separate tier gets better access."
Full details: zombie WebSocket connections (open bug since Dec 2025, no official response), 55-second oracle lag, stale REST orderbook data, Cloudflare-based cancel throttling. Builder Program tiers: Unverified/Verified/Partner with different rate limits.

### Cut: Volume doubling
"Its reported volume figures are [roughly double](https://www.paradigm.xyz/2025/12/polymarket-volume-is-being-double-counted) actual economic activity."
Paradigm Research (Dec 2025): maker+taker events double-counted. Columbia study: 25% wash trading. Full details in earlier MonarchEV section above.

### Reason for cuts
Monarch section had "case against Polymarket" energy — six specific complaints under one heading. Gutted to structural principle + fees (one concrete, timely, universal example). The other points are individually valid but collectively create prosecution feel. They're preserved here for fact-checking and potential use in a separate "What Polymarket Actually Trades" data piece.

## Round 6 — Framework precision (from deep read of sxysun's work)

### Key corrections applied to article

**Ideal distribution**: sxysun's ideal is 0% Mafia, 0% Moloch, 100% Monarch (redistributed). NOT "minimize all three." The Monarch is the coordinator — you want it powerful, you want it accountable. Article updated to reflect this.

**Mechanism mapping corrected**:
- Privacy (TEE) → MafiaEV (eliminates information asymmetry)
- Batch auctions → MolochEV (reduces coordination failure / latency race)
- ZK proofs + TEE → MonarchEV (constrains the Monarch: TEE limits what it sees, ZK limits what it does)

Previously the article conflated FBA with MafiaEV reduction. Per sxysun's FBA-FCFS comparison, FBA has "same MafiaEV" as vanilla FCFS but "less MolochEV." MafiaEV requires privacy, not batching.

**Moloch's Curse added**: sxysun's conjecture that "any mechanism eliminating property Pr in mechanism M inherits Pr itself." Applied to our design: the batch auction engine IS a Monarch. TEEs + ZK proofs are the constraint. This addresses the "too convenient / preloaded product thesis" criticism from every reviewer.

### sxysun references for fact-checking
- Original note: https://hackmd.io/@sxysun/short-note-ext
- Formalized version: https://hackmd.io/@sxysun/this-is-mev
- Devcon talk: https://archive.devcon.org/archive/watch/6/this-is-mev/?playlist=Devcon+6
- FBA-FCFS comparison: references https://research.arbitrum.io/t/transaction-ordering-policy/127/2
- Price of Anarchy as formal bound on MolochEV and MonarchEV
- "Bad MEV" three cases: (1) unsophistication tax → privacy/OFA, (2) exclusive knowledge → privacy, (3) centralized non-distributive coordinator → decentralize/redistribute
- Author handle: sxysun (not "Xinyuan Sun" in article context)
