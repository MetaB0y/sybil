## Verdict

Build **Path 2 first**: an **agent/MM-native structured event-risk venue**, then expose it through a simple “ask/trade the future” UI.

The first real wedge is not consumer probability search, not arbitrary compositions, and not sponsored alpha markets. It is:

> **A high-density, priceable event-risk surface where one market maker can quote hundreds of related outcomes with one shared batch-local risk budget, under private frequent batch clearing.**

That is the thing Polymarket/Kalshi cannot easily copy without changing market structure.

Polymarket/Kalshi can copy LLM search, thematic pages, scalar binaries, and better UI. They also have regulatory momentum, liquidity, and capital: Polymarket US is listed by the CFTC as a designated contract market, and ICE announced another $600m investment after a prior $1bn investment arrangement; Kalshi has been reported at more than $1bn weekly volume, with sports now central to the business. ([Commodity Futures Trading Commission][1])

So Sybil’s wedge must be **economic**, not cosmetic.

---

## The strongest product direction

The strongest direction is:

> **Structured event-risk surfaces for agents and market makers, with human-facing probability search as the access layer.**

Not:

> “Ask anything, get a market.”

But:

> “Ask anything; Sybil maps it to the closest canonical, live, quoteable event-risk surface. If it is not quoteable, Sybil says so and gives proxies.”

The strategic object should be a **quoteability graph**, not a market graph.

A measurement is valuable only if it anchors one of these:

1. a reusable resolution source;
2. a threshold/bin curve;
3. a maker risk bucket;
4. a cluster of related markets with shared pricing logic;
5. a path from free-form user intent to firm quotes.

If a measurement does not help one of those, it is ontology drag.

---

## What is actually hard to copy

The copy-resistant product experiences are these.

### 1. Event option chains for non-option domains

A user sees:

> “AI compute bottleneck surface”
> “Ethereum infra-risk surface”
> “Fed inflation distribution”
> “Model benchmark progress surface”

Underneath, there are many linked binaries/scalars:

```text
Measurement → thresholds/bins → curves → risk bucket → shared MM budget
```

Polymarket/Kalshi can list 100 related binaries. The hard part is making them **coherently quoteable** without fragmenting maker capital across 100 isolated books.

### 2. Flash-liquidity quote packs

The MM-facing primitive should not be “quote market X.”

It should be:

```text
Quote this measurement family / event cluster.
Here are your curves.
Here is your batch risk budget.
Solver decides feasible fills.
```

That lets Sybil create the user-facing illusion of broad liquidity without requiring full per-market collateralization.

### 3. Private batch RFQ for structured event risk

The good RFQ version is narrow:

```text
User asks for exposure.
System canonicalizes it.
If it maps to a known bucket, makers quote.
Accepted RFQ becomes a reusable canonical instrument.
```

The bad version is “send arbitrary prose to makers.”

RFQ is useful as an **edge creation path**, not as the whole product.

### 4. Verifiable dark execution for event markets

Private order flow is useful only if paired with verifiable clearing. Otherwise it reads as “trust us.”

The FBA literature gives you a credible macrostructure story: discrete-time uniform-price batch auctions reduce the value of tiny speed advantages and transform speed competition into price competition; Sybil’s differentiated move is applying that to private event-risk markets and MM budget constraints. ([OUP Academic][2])

### 5. Sponsored/private markets — but not the current framing

Opportunity markets are genuinely differentiated, but the phrasing “sponsor gets signal first and trades external markets” is now strategically dangerous.

The CFTC just charged a service member with insider trading in event contracts using classified nonpublic information, explicitly calling it the first CFTC insider-trading charge involving event contracts. ([Commodity Futures Trading Commission][3])

So opportunity markets should be reframed as:

> **private research / decision-support auctions with compliance controls**

not:

> “pay for a private probability so you can front-run public markets.”

Keep the delayed-reveal mechanism. Kill the external-alpha trading story.

---

## Answers to the main strategic questions

### 1. Are measurements useful, or just elegant ontology?

Useful, but only as **backend discipline**.

The user should almost never see “measurement,” “condition,” or “composition.” The system should use measurements to canonicalize, deduplicate, price, resolve, and route liquidity.

The right internal object is not:

```text
measurement ontology
```

It is:

```text
measurement-backed risk bucket
```

A measurement is worth creating when it supports multiple related quoteable claims:

```text
ETH blob demand in Q3 2026
→ >X, >Y, >Z thresholds
→ bins
→ composed infra-risk claims
→ shared maker budget
→ clean resolution source
```

A one-off measurement with one bespoke condition is usually just disguised market spam.

### 2. Does flash liquidity change economics, or is pricing still binding?

Pricing is still binding.

Flash liquidity does not produce beliefs. It only changes the economics once a maker already has a model.

It helps in three places:

```text
capital efficiency
stale-quote protection
cross-market risk expression
```

The right claim is not:

> “We can make markets on everything.”

The right claim is:

> “For domains where makers can model a risk bucket, Sybil lets them quote much more surface per dollar and with less speed-based adverse selection.”

That is commercially meaningful.

### 3. Which product direction best uses the architecture?

Ranked:

1. **Agent/MM infrastructure for structured event-risk surfaces**
2. **High-resolution event clouds in one narrow domain**
3. **Human probability search over those live surfaces**
4. **Scalar/distribution markets where the measurement is naturally scalar**
5. **RFQ as a creation/onboarding mode**
6. **Opportunity markets, compliance-reframed**
7. **Arbitrary compositions / market for everything — kill as primary direction**

Probability search is the best **front door**, not the first proof.

The first proof should be MM economics.

### 4. What minimal demo would feel obviously compelling?

Build **Demo E + a thin Demo A**.

Not a beautiful consumer app. A two-layer artifact:

#### Layer 1: maker/economic proof

Show:

```text
Same maker.
Same capital.
Same event domain.
Two market structures.

CLOB-style isolated quoting:
  quotes 12 markets

Sybil flash-liquidity batch quoting:
  quotes 180 related markets

Metrics:
  active markets quoted
  average spread
  quoted depth
  budget utilization
  maker PnL under news shocks
  rejected fills due to budget constraint
  stale-quote losses
```

#### Layer 2: human-facing wrapper

User types:

```text
I’m exposed to Ethereum infra risk around blob congestion and L2 demand.
```

Sybil returns:

```text
Best expression:
ETH data-availability stress Q3 2026

Pays if:
blob base fee > X for Y days OR L2 sequencer fees > Z

Status:
Live / quoted this batch

Available:
$N liquidity from shared MM budget

Basis risk:
does not cover validator/client bugs
```

The viewer should not need to understand convex clearing. They should see:

> “This venue can make obscure but structured event risk tradeable.”

### 5. What should be killed early?

Kill these as first products:

```text
visible ontology browser
formula builder
arbitrary user-created markets
generic “market for everything”
unpriced hardcoded market catalog
geopolitical war-cloud launch
sports launch
macro-only scalar launch
opportunity markets framed as private tradable alpha
LLM probability answers not backed by live markets
```

The formula builder is especially dangerous. It will make the product look powerful in demos and incoherent in production.

---

## Recommended first domain

Use **crypto infrastructure** for the first validation artifact.

AI infrastructure/progress is better for narrative. Crypto infrastructure is better for proving the mechanism.

Why crypto infra first:

```text
cleaner measurements
onchain data
native users
native market makers
fewer oracle ambiguities
clearer hedging motives
less direct competition with sports/options
easier simulation and replay
```

Example measurements:

```text
ETH blob base fee
L2 daily active addresses
L2 sequencer revenue
stablecoin supply on Ethereum
validator exit queue length
restaking TVL
bridge volume
major exploit count
gas fee percentiles
MEV/proposer-builder concentration
ETF net inflows
protocol revenue
```

From these, derive:

```text
threshold curves
bins
stress clusters
infra-growth clusters
composed but templated event-risk baskets
```

Then later build the public-facing version in **AI infrastructure/progress**, because that is culturally legible and less crypto-insular.

The sequence should be:

```text
crypto infra = prove quoteability
AI infra = prove market appeal
```

---

## Opportunity markets: plausible but not first

Opportunity markets are real, but they are a second product line.

The private-market mechanism is valuable. The “sponsor gets the price first” mechanism is valuable. But the product must avoid looking like a marketplace for monetizing nonpublic information.

Current legal momentum is mixed and active: the Third Circuit recently treated Kalshi sports event contracts on a CFTC-registered DCM as likely under exclusive CFTC jurisdiction, but the decision was a preliminary-injunction posture and other circuit/state disputes remain live. ([Holland & Knight][4])

So the safe version is:

```text
sponsor asks a permitted question
participants certify allowed information sources
market clears privately
sponsor receives aggregate research signal
delayed public reveal or archived audit trail
no positioning story in correlated external markets
surveillance and exclusion rules from day one
```

Do not put “macro fund acts in copper futures before public reveal” in the pitch deck.

That makes the differentiator look like regulatory arbitrage.

---

## How to say “not priceable”

Use four statuses:

```text
Live
Firm quotes available this batch.

Indicative
Mapped to a canonical measurement, but no firm maker quote.

RFQ-able
Structured enough to request quotes.

Not quoteable
No reliable measurement, resolution source, or pricing model.
```

For “not quoteable,” always return substitutes:

```text
No live market for “Will AI labs run out of power in 2027?”

Closest tradeable proxies:
1. US data-center power interconnection delays > X
2. Hyperscaler capex > $Y
3. NVIDIA data-center revenue < $Z
4. AI benchmark progress basket
```

This converts disappointment into discovery.

---

## What should be public in private FBA markets?

Default public surface:

```text
last clearing price
batch interval / next batch time
indicative spread
available depth bucket
maker count bucket
volume bucket
resolution source
market status
delayed aggregate tape
proof / audit status
```

Do not reveal:

```text
individual orders
positions
exact maker budgets
maker identities by default
unfilled order imbalance
sponsor identity for private markets unless disclosed
```

A private market still needs a public trust substitute. That substitute is:

```text
aggregate price + liquidity status + clearing proof + resolution clarity
```

Not an order book.

---

## Is “search engine for future probabilities” real?

Yes, but only as a distribution layer.

It is not enough to build a nice interface that answers future questions. If the answers are LLM priors, users will eventually treat it as vibes.

The durable version is:

```text
question → canonical market/risk bucket → live quote/probability → trade/proxy/RFQ
```

The answer engine becomes powerful once Sybil has enough live surfaces. Before that, it should be a demo wrapper around one domain.

Do not make probability search carry the cold start.

---

## Route to liquidity without retail gambling flow

Start with **sponsored liquidity + maker infrastructure + agent/API users**.

Initial liquidity sources:

```text
reference MM operated by Sybil
domain-specific market makers
liquidity incentives per risk bucket
sponsors funding specific surfaces
funds/treasuries hedging event exposure
agent integrations routing structured queries
data subscribers paying for probability surfaces
```

Retail can arrive later through the answer UI.

The first liquidity goal is not “many users.” It is:

> one or two makers can quote a surface that would be uneconomic on a public CLOB.

---

## The one-sentence thesis I would use

> **Sybil is a private batch-auction venue for structured event risk, where flash liquidity lets market makers quote dense real-world outcome surfaces that public order-book prediction markets cannot economically support.**

Consumer translation:

> **Ask about the future. If the question is priceable, Sybil gives you a market-backed probability and a trade.**

The phrase “if priceable” is essential. It is the difference between a serious exchange and an infinite market-content generator.

---

## Concrete next build

Build this:

```text
Domain:
crypto infrastructure risk

Surface:
100–200 canonical markets from 20–40 measurements

MM:
one reference market maker quoting curves/clusters

Clearing:
batch solver with shared MM budget

Simulation:
CLOB isolated collateral vs Sybil flash budget

UI:
one search box + event-surface dashboard

Output:
firm quote cards, not ontology
```

Success criteria:

```text
10x+ more quoted markets per unit of maker capital
coherent monotone curves
no nonsensical generated markets
clear budget-binding behavior
maker PnL survives news-shock simulation
users understand best trade/proxy in under 30 seconds
```

The demo should make one point:

> **This surface is only liquid because the market maker can quote the whole bucket with one budget inside a private batch auction.**

Everything else is secondary.

[1]: https://www.cftc.gov/IndustryOversight/IndustryFilings/TradingOrganizations?utm_source=chatgpt.com "Industry Filings: Designated Contract Markets (DCM) | CFTC"
[2]: https://academic.oup.com/qje/article/130/4/1547/1916146?utm_source=chatgpt.com "The High-Frequency Trading Arms Race: Frequent Batch ..."
[3]: https://www.cftc.gov/PressRoom/PressReleases/9217-26 "CFTC Charges U.S. Service Member with Insider Trading in Nicolás Maduro-Related Event Contracts | CFTC"
[4]: https://www.hklaw.com/en/insights/publications/2026/04/federal-appeals-court-cftc-jurisdiction-over-sports-event-contracts "Federal Appeals Court: CFTC Jurisdiction Over Sports Event Contracts Likely Exclusive | Insights | Holland & Knight"

