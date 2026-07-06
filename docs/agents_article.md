# When the Agent Mistakes Stale for Cheap

We ran an LLM trading agent on prediction markets for a while before we understood the way it was going to hurt us. The failure is easy to state and unpleasant to catch: **the system doesn't flag stale numbers as stale. It flags them as opportunity.**

Here is the shape of it. An analyst model reads the news, decides "Israel-Lebanon strike, fair value 40 cents," and writes that number down. Then nothing new happens for an hour. The market, meanwhile, drifts — other traders move the price to 25 cents on ambient flow. Our agent still holds a 40-cent estimate it formed an hour ago. It looks at the 15-cent gap and does not see a stale belief. It sees *edge*. And because the position sizer bets proportionally to edge, a frozen number sitting next to a moving price doesn't just produce a bad trade — it produces a bad trade that **grows** the longer the number sits there. The agent sizes *up* into its own staleness.

We are not the only ones who found this hole. A well-known forecasting agent on Manifold — the "terminator2" bot — documents the same trap almost word for word in its public diary [VERIFY: confirm the diary is public and quote the exact phrasing before publishing]. Two independent agents, built by people who never spoke, walked into the identical failure mode. That convergence is the interesting part, and it's what this piece is about: the specific ways autonomous agents break on prediction markets, and why the *market's* structure — not just the agent's code — decides whether an agent can make money at all.

## Why prediction markets are the natural habitat for agents

Most trading is a bad fit for an LLM. Equities move on a thousand tiny continuous signals; an agent that reasons in paragraphs is too slow and too coarse. Prediction markets are different in three ways that happen to suit a language model precisely.

First, **the signal is news, and news is text.** "Will the Fed cut in March?" resolves on statements, filings, and headlines — exactly the input an LLM is built to read. Second, **the assets are binary and discrete.** A contract is a probability between 0 and 1, and the model's whole job is to output a probability. There is no basis conversion, no options greeks, no continuous hedge. Third — and this is the part people miss — **the market eventually tells you the truth.** Every contract resolves YES or NO. That resolution is a ground-truth label you can score a forecast against. In most markets "were you right?" is philosophically fraught. Here it's a database column.

That last property is what makes an agent *arena* possible rather than just an agent. You can run many strategies side by side and, when the dust settles, actually say which one forecast better — not which one got lucky.

## The split: an analyst that forecasts, a sizer that bets

The single most important design decision we made was to **cut the agent in half.**

One half — the *analyst* — reads news and produces a fair value. That's all it does. It holds no account, places no orders, and never sees a portfolio. Its output is a pure probability estimate plus a confidence, a one-line motivation, and a one-line countercase ("the strongest reason this estimate is wrong"). Three analyst personas run in parallel: a **News Trader** that chases what's genuinely new in an article, a **Contrarian** that fades overreactions, and a **Fundamentals** persona that updates slowly and refuses to move more than a few cents on a single story.

The other half — the *sizer* — takes a fair value and a market price and turns conviction into orders. It runs no LLM. It's mechanical: an edge, a Kelly fraction, position caps, exit rules. We run two sizers behind every analyst — a fractional-Kelly arm (one-third Kelly, size proportional to edge, a 30%-of-portfolio cap) and a flat arm ($20 a bet, many small positions, a hard stop-loss, deliberately terminator2-shaped).

Why bother splitting? Two reasons, both load-bearing.

**It makes the A/B test honest.** The two sizers subscribe to the *same* fair-value stream and drain the *same* update objects. When Kelly and Flat diverge in PnL, it is provably the sizing that differs, not the forecast — they were handed identical numbers, block for block. You cannot get that if each strategy runs its own LLM and quietly forms its own slightly-different opinion.

**It keeps the forecaster honest.** The analyst is portfolio-agnostic *on purpose*. It does not know what it holds, what it's up, or what it's down. This is the single guardrail we care most about, and the next section is why.

## The war story: the conviction loop, and the shrink that fixes it

The original version of this agent did something seductive and wrong: it fed the LLM its own previous fair value in the prompt, "for context." The model, being a language model, anchored on its own last answer. Each new article nudged the estimate a little further in the same direction it had already gone. Fair value marched toward 0.99 or 0.01, and because Kelly sizes proportionally to edge, an extreme fair value produced an enormous position. The agent talked itself into a corner and then bet the house on it. **A model reading its own prior is a feedback loop, and a feedback loop with a bet attached is a margin call waiting to happen.**

We rejected the most tempting escalation of that idea outright: **never feed the agent its own PnL.** An agent that can see it's losing will rationalize — double down to get back to even, or panic-close a good position. Profit-and-loss is the one input most likely to turn a forecaster into a degenerate gambler, so the analyst never sees it. It forecasts the world, not its own scoreboard.

That leaves the subtler bug from the top of this article — the frozen-number-looks-like-edge problem — which is structural rather than psychological. We shipped a two-part fix.

**Freshness decay.** A fair value is used as-is for its first ten minutes. After that, the *edge* it implies decays exponentially toward the market price with a thirty-minute half-life, and at two hours the estimate is declared dead — the sizer treats the market as having no fair value at all and exits the position. A stale belief stops being a phantom edge and starts fading into "I don't know," which is the honest thing for an hour-old opinion to become.

**Confidence-scaled shrinkage.** The Kelly bet is multiplied by `freshness × confidence`, both clamped to at most 1.0. This factor can only ever *shrink* a position, never inflate one. A low-confidence estimate on stale news gets a small bet; a fresh, high-confidence estimate gets the full Kelly size. The sizer can no longer size up into staleness, because staleness now mechanically pulls the size down. [VERIFY: exact TTL/half-life/hard-expiry defaults are 10 min / 30 min / 2 hr as configured — confirm these are the live production values, not just code defaults.]

## Calibration is the only honest scoreboard

Here is the thesis that governs the whole project: **an agent that can't beat "just quote the market price" as a forecast is negative alpha with extra steps.** The market price *is* a forecast — a crowd-sourced one — and it's free. Any agent that costs money to run has to clear that bar before anything else matters.

So we score forecasts, not vibes. Our calibration harness reads every decision the agents made, joins it to how the market actually resolved, and computes:

- **Brier score** per persona — mean squared error of the probability against the 0/1 outcome. Lower is better.
- **The market-price baseline** — the Brier score you'd get by simply parroting the market. This is the number to beat. We report the delta explicitly; a positive delta means the agent is *worse* than doing nothing.
- **Reliability curves** — bucket every "I think it's 70%" call and check whether 70% of them actually happened. This catches an agent that is confidently, consistently miscalibrated.
- **Rejection calibration** — the agent declines to trade on plenty of updates. Are the trades it *takes* better-calibrated than the ones it *skips*? If the rejected set has a better Brier than the acted set, the agent's own filter is pointed backwards.
- **A noise-trader PnL baseline** — seeded, deterministic synthetic traders that perturb prices with no view at all. Beating random flow on PnL is a floor, not an achievement, but failing to is diagnostic.

None of this rewards a good story. A persona with a compelling thesis and a Brier worse than the market baseline is, by our accounting, losing — however plausible its reasoning reads in the logs. We built the scoreboard first so we couldn't lie to ourselves later.

## The structural point: agents are the most copyable traders alive

Everything above is about making an agent *good*. This section is about whether a good agent can *survive* — and that turns out to be a property of the exchange, not the agent.

An autonomous agent is the most copyable trader that has ever existed. It's deterministic, it's always on, and it has no reflexes to disguise. A human discretionary trader leaks their edge slowly and inconsistently. An agent on a transparent continuous order book leaks it perfectly: every order it sends is a labeled training example for anyone watching. **On a fully public CLOB, a good agent doesn't build an edge — it trains its own copiers, in real time, for free.** The better and more consistent it is, the faster it gets shadowed, and the faster its alpha decays to zero.

This is the same disease we wrote about in [The Sniper's Tax](https://sybilpm.substack.com/p/the-snipers-tax), seen from the agent's side. There, transparency plus continuous time let a fast trader pick off a slow market maker's stale quote. Here, transparency plus determinism lets a copier front-run or mirror a good agent's flow. Both are information leaking through market *structure* rather than skill.

Two structural fixes change the game for agents specifically. **Batch auctions** collapse the latency race: orders in a window clear at one uniform price, so being 50ms faster than the agent buys you nothing — you can't pick off its resting intent, and it can't pick off yours. **Encrypted order flow** means a copier can't read the agent's positions to shadow them in the first place. An agent's edge is its forecast; those two mechanisms let the forecast be the thing that pays, instead of the wire to the matching engine. A well-calibrated agent is exactly the kind of participant a prediction market *wants* — it brings information — and a batch-plus-privacy market is the only structure where that information doesn't immediately become everyone else's.

The point is not that our agent is good. The point is that on the wrong market structure, it wouldn't matter if it were.

## What's next

Three things we're building toward [VERIFY: these are roadmap items, not shipped — soften or cut any that slip]:

- **Forkable personas.** The analyst prompt is data, not code. We want anyone to be able to fork a persona, change how it reads evidence, and run it in the arena against the incumbents.
- **A public agent leaderboard** — ranked by Brier-versus-market-baseline and PnL-versus-noise, not by narrative. The scoreboard we use internally, made external.
- **Per-decision auditability** — every order traces back to the article that motivated it, the fair value at the time, its confidence, and its countercase, so a bad call can be diagnosed instead of excused.

The through-line is the same as everywhere else in this project: measure the thing honestly, then fix the structure so the honest thing can win.

## Closer

The staleness bug is a small, specific error, and we fixed it with a decay curve and a multiplication. But it's a good miniature of the whole problem. An agent that can't tell "stale" from "cheap" will bet into its own confusion. An arena that can't tell "well-calibrated" from "well-narrated" will crown the confident. And a market that can't keep a good agent's flow private will let the market copy it into irrelevance. Get all three wrong and you've built a very expensive way to lose money with a straight face. **We'd rather build the kind where being right is the thing that pays.**

---

*Draft for Valery's review. Facts flagged inline with [VERIFY] need a second pair of eyes before this ships — chiefly the terminator2 diary attribution and the production freshness-config values. Not for publication as-is.*
