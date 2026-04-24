---
tags: [strategy, product, market-structure, liquidity, privacy, agents]
layer: product
status: planned
last_verified: 2026-04-24
---

# Market Structure Product Directions

This is a self-contained strategic problem statement for Sybil. It is written for a very strong external reasoning model or reviewer. The reader can be assumed to understand prediction markets, market microstructure, derivatives, RFQ, options, market making, privacy, and agentic UX. What the reader cannot know is our internal context: what Sybil has built, what strategic fork we are facing, what intuitions we are testing, and where we are uncertain.

The goal is not to explain basic concepts. The goal is to put all relevant assumptions on the page so the reader can critique the strategic direction without guessing what we mean.

Sybil is a prediction-market exchange project built around three core technical claims:

1. **Private frequent batch auctions should improve prediction-market microstructure.** Public continuous order books create stale-quote sniping, copy-trading, and toxic flow around discrete information events. Batch clearing plus private order flow should reduce those losses and make informed participation less self-defeating.
2. **Budget-constrained batch clearing enables flash liquidity.** Market makers should be able to quote many markets in the same batch against one shared risk budget, with the clearing engine deciding which fills are jointly feasible.
3. **Agent-native and privacy-aware UX may support market types that public CLOB prediction markets cannot.** In particular: delayed-reveal sponsored markets, high-resolution event surfaces, scalar distributions, and long-tail structured event risk.

The strategic question is:

> Given Sybil's actual technical advantages — frequent batch auctions, privacy, verifiable clearing, budget-constrained market-maker clearing, and flash liquidity — what product experiences become possible that Polymarket, Kalshi, and sportsbook-style prediction markets cannot easily copy?

The attractive but dangerous answer is "markets for everything." The sharper question is:

> Can Sybil support a much larger surface of real-world event exposure without degenerating into a graveyard of bespoke illiquid markets?

This report lays out the terrain.

## 0. What We Want The Reader To Help With

The central uncertainty is whether these technical advantages point to a real product wedge or whether they tempt us into an incoherent "market for everything" direction.

We want critique on:

1. Whether measurements/compositions are actually a useful constraint on long-tail market creation, or whether they are elegant ontology work with weak commercial force.
2. Whether flash liquidity plus FBA materially changes the economics of quoting many related markets, or whether pricing remains the binding bottleneck.
3. Which product direction best uses Sybil's architecture:
   - market-backed search/answers for future questions;
   - thematic long-tail markets;
   - scalar/distribution markets;
   - high-resolution event clouds;
   - private opportunity markets;
   - RFQ for event risk;
   - agent/MM infrastructure.
4. What minimal demo would make the direction feel obviously compelling.
5. What should be killed early because it is technically interesting but strategically weak.

The reader should assume we are not emotionally attached to the current composition/measurement framing. We are trying to decide whether it is the right abstraction, a useful backend-only primitive, or a dead end.

## 0.1 Project-Specific Terms

Only the project-specific terms are defined here.

**Flash liquidity** means market makers quote many markets in a batch while declaring one shared batch-local risk budget. The exchange's clearing problem chooses which quoted orders fill under that budget. This is meant to let the same maker capital cover much more surface than isolated per-market collateralization.

**Measurement** means a reusable observable variable that can anchor many markets: ETH/USD max price in 2026, CPI YoY in a given month, a benchmark score, a troop count, a company's quarterly revenue, a player's season goal count. The point is not ontology for its own sake; the point is to create shared risk buckets and reusable resolution sources.

**Condition** means a predicate over a measurement: ETH/USD max in 2026 > $6,000; CPI YoY > 4%; FrontierMath best score > 50%.

**Composition** means a formula over conditions: CPI > 4% AND Fed funds > 5%; ETH > 6000 AND BTC > 150k; K_OF_N(2, [FrontierMath > 50%, SWE-bench > 80%, ARC-AGI > 85%]).

**Opportunity market** means a sponsored private market where the sponsor gets the clearing price before the public. The sponsor pays for crowdsourced alpha and receives a delayed-reveal window.

## 1. Current Market Context

Prediction markets have crossed from weird internet toy into mainstream financial/cultural object. Polymarket and Kalshi proved that real users want event markets. They also proved the easiest demand pockets:

- sports;
- elections;
- major geopolitical headlines;
- crypto/finance price moves;
- very short-duration up/down markets;
- culturally viral yes/no questions.

That success creates a strategic problem for a new entrant. Competing directly against Polymarket/Kalshi as "another consumer prediction market with better UX" is probably not viable. The category leaders have brand, liquidity, regulatory momentum, distribution, and the cash-cow markets. Even if their infrastructure is flawed, they can continue to monetize sports, politics, and price-adjacent contracts.

The obvious incumbent model is casino-like:

```text
retail flow -> public market -> market maker / house edge
```

The casino-style loop is attractive because:

- users understand it;
- sports and price props are easy to market;
- liquidity concentrates in high-volume events;
- market makers can price familiar risk;
- the product can look like a betting app.

But that lane is not where Sybil is differentiated. Polymarket, Kalshi, sportsbooks, and exchange entrants can all fight there. The likely outcome is an acquisition and liquidity-spend race, not a technology-led wedge.

Sybil's thesis should therefore not be:

> We will outspend Polymarket/Kalshi for retail attention.

It should be:

> Their architecture optimizes for public, continuous, sportsbook-like event markets. Sybil's architecture can support market types and agent workflows that are difficult or impossible on that architecture.

## 2. What Sybil Actually Has

Sybil is not just a prediction-market frontend. The core stack has several unusual properties.

### 2.1 Frequent Batch Auctions

Sybil clears markets in batches rather than continuously. Orders accumulate over a short interval, then the engine computes a welfare-maximizing clearing result. Everyone in a batch trades at the same clearing price.

This matters because many prediction-market price moves are information shocks:

- a news report drops;
- a data release prints;
- an injury is announced;
- a war headline hits;
- an election call appears;
- a model benchmark leaks.

In a continuous order book, the fastest actor can pick off stale quotes before market makers update. Market makers respond by widening spreads, pulling liquidity, or quoting only the safest high-volume markets. The "sniping tax" becomes a structural liquidity cost.

FBA changes the game:

- speed within a batch does not determine execution priority;
- stale quotes clear at the batch's aggregate price, not the pre-news stale price;
- market makers can quote with less fear of millisecond toxic flow;
- the clearing problem becomes a well-defined optimization over a finite set of orders.

This is not just a UX tweak. It changes market-maker economics.

### 2.2 Configurable Privacy

Sybil's design can hide orders and positions by default, with configurable reveal policies. A market can reveal:

- aggregate clearing prices immediately;
- aggregate prices after a delay;
- prices only to selected parties for a period;
- coarse activity signals but not orders;
- individual user data only via selective disclosure.

This matters because public prediction markets leak alpha. If an informed trader buys publicly, copy-traders, scrapers, and competitors can infer the thesis. If a sponsor pays for a market to discover information, a public order book gives the discovered information to everyone.

Privacy is not only a user protection feature. It enables market designs where information itself is the product.

### 2.3 Verifiable Clearing

The stronger the coordinator, the more it must be constrained. A private batch auction engine sees orders and computes matches. That creates a powerful "monarch" role. Sybil's answer is verifiability:

- deterministic integer accounting;
- block witnesses;
- settlement verification;
- future ZK proof path;
- narrow trust assumption around order inclusion/censorship rather than arbitrary execution correctness.

This matters because private markets without verifiability become opaque dark pools. The value proposition is not "trust us with private orders." It is:

> The order flow can be private, but execution correctness should be provable.

### 2.4 Budget-Constrained Clearing

Sybil's research contribution is solving the batch clearing problem when market makers have budget constraints. Market makers should be able to quote across many markets with one risk budget. The engine should decide which fills are jointly feasible.

This is difficult because prices and fill quantities interact. The key claim from the Fisher-market formulation is that market-maker budgets can be absorbed into the clearing objective through a reduced-form utility, giving a clean convex/tractable structure for the relevant cases.

This is the technical basis for flash liquidity.

### 2.5 Flash Liquidity

Flash liquidity means a maker does not lock capital into every market continuously. Instead, a maker can quote many markets in a batch and declare a shared batch-local budget.

Example:

```text
MM has $100 of risk budget this batch.
MM quotes 200 related markets.
The engine fills only a feasible subset under the shared budget.
```

This can multiply effective market coverage. It does not mean the same $100 can settle unlimited losses. It means the maker can express willingness to make many mutually unlikely or partially offsetting markets without fully collateralizing every quote in isolation.

This is central for long-tail coverage. If every new market requires dedicated resting capital, "market for everything" dies immediately. If a maker can cover hundreds of related markets with one budget, the surface becomes more plausible.

### 2.6 Agent-Native Surface

FBA, privacy, composed markets, delayed reveal, and flash liquidity are not simple retail primitives. They are powerful but abstract. For humans, they can feel like friction:

- hidden order book;
- delayed public information;
- batch timing;
- many possible instruments;
- complex related-market structure.

For agents and market makers, these are features:

- APIs can search the universe;
- agents can price and quote many markets;
- agents can route user intent to instruments;
- agents can run RFQ-like workflows;
- agents can maintain belief models;
- agents can exploit the batch structure without needing a continuous UI.

This suggests the primary Sybil surface may eventually be agent-native, with a human UI that looks more like search/chat than a trading terminal.

## 3. The Core Strategic Tension

The dream:

> Bet on everything.

The failure mode:

> RFQ for arbitrary unknowable things with no counterparty, no pricing model, and no liquidity.

If every user creates a unique bespoke market, no two users meet. Market makers cannot price everything. The exchange becomes an infinite catalog of dead contracts.

So the viable version must be narrower:

> Many possible markets, but only a structured subset is priceable and quoteable.

The product must distinguish four layers:

1. **Latent universe**

   The set of questions that could be represented.

2. **Structured universe**

   Questions mapped to measurements, predicates, data sources, windows, and resolution policies.

3. **Priceable universe**

   Structured questions for which a maker/model can produce a defensible probability or quote curve.

4. **Quoteable/live universe**

   Priceable markets where makers are willing to provide batch-local liquidity and users can trade.

Most product confusion comes from collapsing these layers. "We can represent it" is not the same as "we can price it." "We can price it" is not the same as "we should show it as live."

## 4. Measurements As Structure

The measurement idea is an attempt to avoid arbitrary market spam.

Instead of every market being a prose title with its own oracle, markets are derived from reusable observation slots:

```text
Entity/context -> measurement -> condition -> composition -> market
```

Examples:

```text
ETH/USD spot price during 2026
US CPI YoY monthly releases during 2026
FrontierMath best public benchmark score in 2026
US troop count in Iran before 2027
Jayson Tatum regular-season points in 2026-27
```

From one measurement, many conditions can be generated:

```text
ETH max 2026 > 3000
ETH max 2026 > 6000
ETH max 2026 > 10000
3000 < ETH max 2026 < 6000
```

This does three important things:

### 4.1 Deduplication

Many titles collapse to one canonical claim:

```text
Will ETH hit $6000 in 2026?
ETH over six thousand dollars next year?
Ethereum above 6000 by end of 2026?
```

If the measurement, window, aggregation, source policy, and predicate match, these are the same condition.

### 4.2 Connected Pricing

Thresholds on the same measurement form curves:

```text
P(ETH > 10000) <= P(ETH > 6000) <= P(ETH > 3000)
```

Market makers can quote the curve rather than isolated one-off contracts. This is much more priceable than arbitrary prose markets.

### 4.3 Resolution Precision

The oracle no longer interprets a vague title. It resolves:

- measurement source;
- observation window;
- aggregation;
- predicate;
- formula if composed.

This reduces ambiguity and makes automated resolution more plausible.

The measurement layer is therefore not the product. It is a discipline:

> Only create markets that can be grounded in reusable, observable variables.

## 5. Compositions: Useful But Dangerous

Compositions are formulas over conditions:

```text
CPI > 4% AND Fed funds > 5%
ETH > 6000 AND BTC > 150k
Iran troops > 1000 OR Iran strikes > 50
K_OF_N(2, [FrontierMath > 50%, SWE-bench > 80%, ARC-AGI > 85%])
```

They are compelling because they let users express nuanced views. They are dangerous because pricing arbitrary formulas is hard and demand fragments.

Compositions should be treated as a power tool, not as the main consumer primitive. They become valuable when:

- leaves are already priceable;
- correlations are understandable;
- the formula maps to a real thesis;
- enough users care about the same thesis;
- market makers can quote it with bounded risk.

They become bad when:

- the formula is bespoke to one user;
- the leaves are weakly related;
- correlation dominates the price;
- no maker has a model;
- the user cannot understand resolution.

The product should not celebrate arbitrary creation. It should route users back to canonical shared markets whenever possible.

## 6. Pricing: The Hard Center

Pricing is the make-or-break issue.

FBA and flash liquidity help execution and capital efficiency, but they do not magically produce beliefs. Someone still needs a model:

```text
fair probability -> quoted price/spread/size -> batch order
```

Sybil's architecture can make market making cheaper and safer. It cannot make unpriceable things priceable by force.

### 6.1 What Is Priceable?

Most priceable:

- financial price thresholds;
- macro data releases;
- election outcome sets;
- sports lines where sportsbook/model data exists;
- AI benchmark thresholds with reference forecasts;
- conflict event counts with structured feeds;
- company metric thresholds;
- scalar/binned outcomes around known distributions.

These can use:

- options/futures/implied volatility;
- historical distributions;
- polling and statistical models;
- sportsbook lines;
- source market prices;
- expert priors;
- LLM-assisted research;
- market-maker private models.

Least priceable:

- arbitrary one-off narratives;
- cross-domain conjunctions with unknown correlation;
- poorly specified geopolitical claims;
- markets where the resolution source is contested;
- personal/local/private events;
- combinations built because they are syntactically possible rather than economically meaningful.

### 6.2 Flash Liquidity Helps Pricing Indirectly

Flash liquidity does not answer "what is the fair probability?" It helps in three other ways:

1. **Capital reuse**

   A maker can quote many related markets without locking full collateral into every one.

2. **Stale quote protection**

   FBA reduces the speed-based pickoff risk that makes makers reluctant to quote news-sensitive markets.

3. **Graph-wide risk expression**

   A maker can submit quotes across a local measurement/condition graph and let the engine decide feasible fills under a shared budget.

This can expand the quoteable universe, but only around domains where makers can maintain belief models.

### 6.3 The Right Liquidity Unit Is A Risk Bucket

The product should not think in isolated markets. It should think in risk buckets:

```text
ETH 2026 price curve
CPI 2026 inflation curve
Fed path 2026 curve
Iran escalation event cluster
AI benchmark progress cluster
NBA season player-stat cluster
```

Market makers quote buckets. Users see individual expressions. The engine clears across the bucket.

This is where measurements matter. A measurement is not just an oracle field; it is a risk bucket anchor.

## 7. Is This Just Options?

Partly, yes.

Threshold markets over continuous financial variables are digital options:

```text
ETH > 6000
SPX < 5000
VIX > 40
```

If Sybil only lists financial thresholds, it competes with options/derivatives markets and probably loses.

The differentiated surface is where existing options do not naturally reach:

- real-world event predicates;
- non-financial scalar outcomes;
- cross-domain proxy trades;
- structured event clusters;
- sponsored private information markets;
- agent-created but canonicalized long-tail contracts;
- conditional/composed claims over event measurements.

So "are we reinventing options?" is a valid kill-test. The answer must be:

> We use option-like machinery where appropriate, but the product is structured event exposure for things that do not have natural option chains.

## 8. Candidate Product Directions

The following directions are not mutually exclusive. They are different ways to exploit Sybil's stack.

## 8.1 Intent-To-Trade / Future Search

User starts with a free-form question or goal:

```text
I'm long ETH and worried about downside.
What does the market think about AI capex?
This Iran news seems underpriced; how do I trade it?
Will GPT-6 launch in 2026?
```

The system returns:

- one answer or probability;
- one best trade expression;
- 2-3 alternatives;
- why each maps to the intent;
- basis risk;
- liquidity/priceability status;
- create-if-missing path.

This is the "search engine for the future" framing. Betting is not the front door; querying probabilities is.

Why Sybil is differentiated:

- privacy lets informed contributors trade without leaking;
- FBA improves price quality by reducing sniping;
- large latent market space makes more questions answerable;
- agent-native UX maps questions to canonical instruments.

Validation demo:

- one input box;
- curated canonical answer pages;
- agent maps 30-50 representative queries into trade cards;
- show when no good market exists;
- show provenance path and liquidity status;
- no graph explorer, no wizard, no ontology terms.

Pre-validation question:

> Do users find it valuable to ask forward-looking questions and receive market-backed answers, even before they trade?

Risks:

- answering arbitrary questions may require too much LLM/research work;
- if probabilities are weak or fake-looking, trust dies;
- hard to bootstrap coverage.

## 8.2 Thematic Long-Tail Markets

Instead of trying to cover everything, Sybil could choose themes where current platforms have poor coverage:

- AI progress and compute supply;
- crypto infrastructure and DeFi activity;
- non-US politics/elections;
- regional conflicts and supply-chain risk;
- macro subcomponents beyond headline CPI/Fed;
- niche public-company operational metrics;
- climate/weather/energy infrastructure.

The product becomes:

> high-resolution market coverage for domains current PMs ignore.

Why Sybil is differentiated:

- flash liquidity lets makers cover more markets per dollar;
- measurements make many markets derived from a smaller number of data sources;
- FBA protects makers in news-sensitive niches;
- agent UX helps users discover obscure but relevant exposures.

Validation demo:

- pick one theme, e.g. AI infrastructure;
- seed 50-100 measurements;
- derive 200-500 conditions;
- expose only answer/search cards;
- build a reference MM that quotes curves/clusters;
- show "coverage depth" versus Polymarket/Kalshi.

Pre-validation question:

> Can a narrow domain feel meaningfully richer than existing platforms without becoming incoherent?

Risks:

- curation burden;
- too little user demand;
- pricing models weak outside finance/sports;
- hard to acquire domain experts.

## 8.3 Scalar Markets

Many real questions are not naturally yes/no:

```text
What will CPI be?
How many troops?
How many goals this season?
What benchmark score?
How many seats?
What revenue?
```

Scalar markets can be implemented as bins/threshold curves:

```text
CPI < 2
2 <= CPI < 3
3 <= CPI < 4
CPI >= 4
```

or as continuous payout functions.

Why Sybil is differentiated:

- batch clearing can clear the whole distribution coherently;
- flash liquidity lets makers quote all bins with one budget;
- measurements naturally define scalar variables;
- agents can summarize the distribution as an answer.

Potential specialist collaboration:

If an external team specializes in scalar/continuous prediction interfaces, a collaboration could be useful. Sybil would supply market structure and clearing; the specialist would supply UX, modeling, or distribution-market primitives.

Validation demo:

- one scalar domain, e.g. CPI/Fed/VIX or AI benchmark scores;
- show distribution curve, not market list;
- let user trade "over", "under", and bins;
- MM quotes full distribution with one budget.

Pre-validation question:

> Do users understand and trust distributions more than a list of binary thresholds?

Risks:

- scalar UX is harder than yes/no;
- resolution/payout math must be very clear;
- may converge back to existing options/futures for financial variables.

## 8.4 High-Resolution Event Clouds

The idea:

> A major evolving event is broken into hundreds of sub-markets, producing a probability cloud.

Example: "week 2 of the Iran conflict":

- troop count thresholds;
- strike count thresholds;
- locations affected;
- legal actions;
- oil/Hormuz disruption;
- casualty ranges;
- diplomatic talks;
- escalation/de-escalation milestones;
- time-to-event bins.

This resembles sportsbook prop coverage for a match. The difference is applying high-resolution prop-market coverage to real-world events beyond sports.

Why Sybil is differentiated:

- existing PMs list a handful of headline markets;
- bookmakers do high-resolution coverage only where they have mature sports models;
- measurements let the sub-markets share sources and risk buckets;
- flash liquidity lets makers cover the cloud with shared capital;
- FBA protects makers against headline shocks.

Validation demo:

- choose one scenario: Iran escalation, Taiwan Strait, AI benchmark release, Fed week;
- generate 100-300 structured markets from a small set of measurements;
- show an "event cloud" interface: not a list, but clusters and probability heatmaps;
- run a reference MM that quotes clusters;
- simulate news updates moving the cloud.

Pre-validation question:

> Does a probability cloud feel like a new information product, or just a confusing wall of markets?

Risks:

- hard to price correlations;
- noisy event definitions;
- UX can become overwhelming;
- could look insensitive for war/violence markets.

## 8.5 Opportunity Markets

Opportunity markets are sponsored private markets:

1. Sponsor creates a question.
2. Forecasters submit blind orders.
3. Batch clears privately.
4. Sponsor receives the clearing price first.
5. Public reveal happens later.

The sponsor buys crowdsourced alpha and gets a window to act before the signal becomes public.

Example:

```text
Will Escondida copper mine strike by Friday?
```

A macro fund sponsors the market. Local/informed forecasters trade blind. Sybil clears the batch and privately routes the probability to the sponsor for 48 hours. The sponsor acts in copper futures before public reveal.

Why Sybil is differentiated:

- public CLOBs leak the signal immediately;
- blind orders prevent herding and copy-trading;
- batch clearing produces one aggregate signal;
- configurable reveal gives the sponsor exclusive access;
- verifiable clearing makes private execution auditable.

Validation demo:

- simulate sponsor dashboard;
- create a private market;
- show forecaster submissions are hidden;
- clear batch;
- sponsor sees signal;
- public sees delayed reveal;
- compare to public-orderbook leakage.

Pre-validation question:

> Would a sponsor pay for private, crowdsourced probability discovery with a delayed reveal?

Risks:

- legal/regulatory sensitivity;
- forecaster incentives;
- sponsor trust;
- preventing external leakage by participants;
- narrow B2B sales motion.

## 8.6 RFQ / Ultimate Pricing Engine

There is a tempting interpretation:

> User asks for any exposure; market makers return quotes.

This is basically RFQ for event risk. It may be closer to the truth for bespoke claims.

The good version:

- user requests a structured exposure;
- system canonicalizes it;
- a small set of makers quote if they can price it;
- quote can be accepted instantly or routed into next batch;
- non-priceable requests are rejected honestly.

The bad version:

- any arbitrary query is sent to makers;
- no structure;
- no reuse;
- no shared liquidity;
- no reason for future users to trade the same thing.

RFQ should be treated as a product mode, not the whole exchange. It can help bootstrap long-tail markets, but only if accepted RFQs turn into canonical reusable instruments.

Why Sybil is differentiated:

- RFQ quotes can hedge into batch clearing;
- privacy hides the user's intent;
- makers can quote related markets with flash budgets;
- accepted structures can become public/latent markets later.

Validation demo:

- user asks for a niche but structured exposure;
- system returns "quoteable / not quoteable";
- if quoteable, show 2-3 maker quotes;
- if accepted, create canonical market and settle via batch.

Pre-validation question:

> Is there demand for event-risk RFQs, and can makers respond fast enough on structured requests?

Risks:

- becomes a services business;
- no network effects if requests do not repeat;
- difficult maker onboarding;
- regulatory complexity.

## 8.7 Agent-Native Market Making Platform

Instead of focusing on retail first, Sybil could be the best venue for agentic market makers:

- APIs expose measurements, markets, curves, and activity signals;
- MMs quote huge surfaces with flash budgets;
- agents maintain belief models;
- the UI is mostly monitoring, not consumer entertainment.

Why Sybil is differentiated:

- FBA and privacy are infrastructure advantages;
- budget-constrained clearing is directly useful to MMs;
- public CLOBs are hostile to broad, automated quoting in news-sensitive markets.

Validation demo:

- one reference MM;
- one synthetic event domain;
- many markets;
- show same capital quoting more markets than CLOB-style collateralization;
- compare fill quality, spread, and maker PnL under news shocks.

Pre-validation question:

> Can a maker quote meaningfully more surface on Sybil with the same capital and survive adverse selection better?

Risks:

- technical buyer only;
- hard to get real maker feedback without volume;
- requires robust simulation and clear economics.

## 9. What Needs To Be Validated

The most important questions are not UI questions.

### 9.1 Can Measurements Create Priceable Risk Buckets?

Hypothesis:

> Measurements turn long-tail markets into related curves/clusters that MMs can price and quote.

Test:

- choose one domain;
- create 50-100 measurements;
- derive 200+ conditions;
- build threshold/scalar curves;
- implement a reference pricing model;
- show coherent quotes over many markets with one MM budget.

Success:

- many markets can be quoted from fewer models;
- no obvious nonsensical rows;
- quote curves are monotone/coherent;
- maker budget utilization is meaningfully better than isolated collateral.

Failure:

- every market still needs bespoke analysis;
- correlations dominate;
- curves look fake;
- users cannot understand the output.

### 9.2 Can Flash Liquidity Expand Coverage?

Hypothesis:

> A maker can quote far more markets per dollar using batch-local budgets than in an isolated CLOB.

Test:

- simulate same maker capital under two regimes:
  - isolated quotes/collateral per market;
  - Sybil flash budget across related markets.
- run informed and uninformed trader flow;
- measure quoted market count, spread, fill quality, PnL volatility, budget utilization.

Success:

- coverage expands materially;
- maker PnL does not explode;
- takers get better expected prices;
- solver clears reliably.

Failure:

- maker risk blows up;
- solver bottlenecks;
- budget sharing reduces volume too much;
- only trivial/uncorrelated markets benefit.

### 9.3 Can FBA Reduce Sniping Enough To Matter?

Hypothesis:

> FBA reduces stale-quote losses, allowing tighter liquidity on news-sensitive markets.

Test:

- replay historical news-shock markets;
- compare continuous fill model vs batch clearing;
- model a maker with stale quotes and cancel/update delays;
- measure sniping loss, spreads required for break-even, and maker participation.

Success:

- reduced adverse selection is large enough to change quoting behavior;
- user prices improve;
- effect survives realistic batch intervals and privacy constraints.

Failure:

- makers still price adverse selection similarly;
- delays hurt users more than they help makers;
- gains are only theoretical.

### 9.4 Can Privacy Enable A New Product?

Hypothesis:

> Configurable privacy enables opportunity markets and informed-trader participation that public CLOBs cannot support.

Test:

- build a sponsor-market demo;
- simulate hidden submissions and delayed reveal;
- interview potential sponsors;
- quantify what they would pay for exclusive signal windows.

Success:

- sponsors understand and value delayed reveal;
- forecasters understand incentives;
- privacy is seen as core, not suspicious.

Failure:

- too legally fraught;
- sponsors do not trust crowdsourced alpha;
- forecasters do not participate without public prices;
- external leakage kills value.

### 9.5 Can Agent UX Make The Complexity Disappear?

Hypothesis:

> Users can express beliefs/risks in natural language and receive precise, understandable, tradeable expressions.

Test:

- one input box;
- no mode buttons;
- no visible ontology;
- response cards with price/liquidity/risk;
- test on 50 representative user queries;
- measure whether users understand the recommended expression.

Success:

- users get from intent to trade in under 30 seconds;
- most queries route to existing canonical markets;
- users trust "not quoteable" answers;
- creation flow feels like a fallback, not a product burden.

Failure:

- recommendations feel arbitrary;
- too many clarification questions;
- users want browsing/feed instead;
- every query creates a bespoke market.

## 10. Validation Artifacts That Could Feel Compelling

The validation artifact should pick one strong claim and show it clearly. It should not expose all machinery at once. The viewer should quickly understand what market structure advantage is being demonstrated.

### Demo A: One Box, Many Trade Expressions

User types:

```text
I am long ETH and worried about downside.
```

Output:

```text
Best hedge:
Crypto downside shock
Pays if ETH < 2000 OR BTC < 70000
Quoted: YES 22-25
Why: direct downside hedge for broad crypto exposure
Basis risk: does not hedge SOL/alts perfectly
Liquidity: quoted by MM budget #4; $8k available this batch
```

This validates agentic intent-to-market routing.

### Demo B: High-Resolution Event Cloud

Select "Iran escalation week 2."

Show clusters:

- troop presence;
- strike count;
- legal authorization;
- Hormuz/oil;
- diplomacy;
- casualty thresholds.

Each cluster has curves and a few composed definitions. A news event updates the probability cloud. The maker quotes the entire cloud with one budget.

This validates measurements + flash liquidity + FBA.

### Demo C: Scalar Distribution Market

Ask:

```text
Where will CPI YoY print by December 2026?
```

Show distribution bins, not market cards:

```text
<2%, 2-3%, 3-4%, >4%
```

User can trade any bin/over/under. MM quotes the whole distribution. Batch clearing keeps the outcome vector coherent.

This validates scalar markets and answer-not-bet UX.

### Demo D: Opportunity Market

Sponsor creates:

```text
Will Company X announce supply disruption by Friday?
```

Forecasters submit blind orders. Sponsor sees 81% after clearing. Public reveal delayed 48 hours.

This validates privacy as product, not just protection.

### Demo E: Maker Coverage Simulation

Show two panels:

```text
CLOB-style capital: $100 quotes 8 markets
Sybil flash liquidity: $100 quotes 160 related markets
```

Run synthetic flow. Show fills, PnL, spread, budget utilization.

This validates the technical/economic core even without consumer polish.

## 11. What Not To Build First

Avoid:

- a visible ontology browser as primary UI;
- a formula builder as primary UI;
- huge market lists;
- arbitrary user-created markets with no quoteability;
- graph diagrams as consumer product;
- mode buttons that ask users to classify their intent;
- prototypes with many hardcoded but unpriced markets;
- "bet on everything" without saying what is priceable.

These create the impression that Sybil is a confusing market generator rather than a better market structure.

## 12. The Most Important Strategic Fork

There are two possible company identities:

### Path 1: Consumer Probability Search

Sybil becomes the place people and agents ask:

```text
What is the probability of X?
```

Trading is the mechanism underneath. The answer is the product.

Pros:

- broad vision;
- strong cultural surface;
- LLM/API/embed distribution;
- turns privacy into a feature;
- can grow beyond traders.

Cons:

- needs huge coverage;
- hard answer quality;
- hard bootstrapping;
- may become an expensive content/data product.

### Path 2: Market-Maker Infrastructure For Event Risk

Sybil becomes the best venue for MMs/agents to price and trade structured event-risk surfaces.

Pros:

- directly uses FBA/flash liquidity;
- technically differentiated;
- clearer buyer/user;
- easier to validate with simulations and maker interviews.

Cons:

- less consumer excitement;
- cold-start volume problem;
- depends on professional maker adoption;
- may look like infrastructure, not a network.

These paths can converge, but the first validation artifact should not try to prove both. The safest sequence may be:

1. Prove maker/economic advantage in one domain.
2. Wrap it in a simple intent/search UI.
3. Only then expand toward probability search.

## 13. Recommended Near-Term Focus

The next build should not be a generic composition/product demo. It should be a tightly scoped experiment:

> Pick one domain where measurement curves are real, build a high-density priceable surface, and show that flash-liquidity MMs can quote it better than isolated markets.

Candidate domains:

### AI infrastructure / AI progress

Pros:

- differentiated from sportsbook/casino markets;
- high public interest;
- measurable variables exist: benchmarks, model releases, capex, GPU shipments;
- options markets do not directly cover many claims;
- agents/users care about forward-looking answers.

Cons:

- data quality and resolution can be hard;
- pricing benchmarks may be speculative;
- public interest may be episodic.

### Macro release curves

Pros:

- priceable;
- strong data sources;
- scalar/distribution markets natural;
- institutional use case clear.

Cons:

- closer to existing derivatives;
- hard to compete with rates/options markets;
- retail excitement lower.

### Geopolitical event clouds

Pros:

- current PMs have shallow coverage;
- high value for sponsors/analysts;
- opportunity-market angle strong;
- FBA protects news-sensitive quoting.

Cons:

- sensitive;
- resolution disputes;
- pricing hard;
- regulatory/reputation risk.

### Crypto infrastructure

Pros:

- Sybil-native audience;
- measurable onchain data;
- options do not cover many protocol metrics;
- natural agent/MM participants.

Cons:

- niche;
- data can be gamed;
- may look too crypto-insular.

My current recommendation:

> Use AI infrastructure/progress or crypto infrastructure for the next validation artifact, not sports.

Sports is too occupied by bookmakers and legally fraught. It is useful for ontology testing, but not an obvious wedge.

## 14. The Cleanest One-Sentence Thesis

Possible thesis:

> Sybil is a private batch-auction exchange for structured event risk, where reusable measurements and flash liquidity let market makers quote far more real-world outcomes with the same capital.

Consumer translation:

> Ask anything about the future. If it is priceable, Sybil gives you a market-backed probability and a trade.

Important caveat:

> "If it is priceable" must be explicit. That is what prevents the "market for everything" fantasy from becoming RFQ chaos.

## 15. Open Questions For GPT Pro / External Review

1. Which product direction best exploits FBA + privacy + flash liquidity: probability search, opportunity markets, scalar markets, high-resolution event clouds, or maker infrastructure?
2. Is the measurement abstraction strong enough to create priceable risk buckets, or does it mostly produce ontology complexity?
3. Which domain gives the best first proof: AI, crypto infrastructure, macro, geopolitics, or something else?
4. Can flash liquidity be made legible in a demo without requiring users to understand batch clearing?
5. What is the minimal experiment that proves market makers can cover more surface with the same capital?
6. Are opportunity markets legally/product-wise plausible, or too niche/sensitive?
7. Is "search engine for future probabilities" a real consumer product, or only a narrative for API/LLM distribution?
8. How should the product say "not priceable" without disappointing users?
9. What should be public in a private FBA market: prices, spreads, activity buckets, volume, maker count, confidence scores?
10. Is there a route to liquidity that does not rely on retail gambling flow?

## 16. Bottom Line

The broad "measurement + composition = market for everything" direction is too vague by itself. It becomes compelling only when tied to Sybil's market-structure advantages:

- FBA reduces speed-based adverse selection;
- privacy protects informed flow and enables delayed-reveal markets;
- budget-constrained clearing enables flash liquidity;
- flash liquidity lets makers quote more surface per dollar;
- measurements turn arbitrary questions into connected risk buckets;
- agents hide the complexity from users.

The correct product is not a graph, a wizard, or an infinite list of markets.

The correct product is likely one of:

1. a market-backed answer engine for forward-looking questions;
2. a high-resolution event-risk surface for domains incumbents do not cover;
3. a private opportunity-market system for sponsored alpha discovery;
4. an agent/MM infrastructure layer for quoting structured event risk.

The next validation artifact should prove one of these in a narrow domain with real pricing discipline. The best version will make a viewer think:

> This could not exist on a public continuous order book.

If it does not make that clear, it is probably not using Sybil's actual advantage.
