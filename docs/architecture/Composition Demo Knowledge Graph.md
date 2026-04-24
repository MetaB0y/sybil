---
tags: [ontology, composition, markets, oracle, liquidity]
layer: api
status: planned
last_verified: 2026-04-24
---

# Composition Demo Knowledge Graph

The composition demo knowledge graph is the product layer that turns Sybil from a list of isolated prediction markets into a navigable prediction knowledge graph. The core idea is that a market should not start as a string title. It should start as a claim about the world:

> A measurement, observed through one or more feeds, transformed into a condition, optionally composed with other conditions into a market definition.

The current demo uses the first version of this model:

```text
DataFeed -> Measurement -> Condition -> Market Definition -> Sybil Market
```

The next version should generalize this into a proper knowledge graph:

```text
Entity -> Event/Context -> Measurement -> Observation Slot -> Condition -> Definition -> Market
```

The distinction matters. A flat title like "Will Jayson Tatum score over 29.5 points vs Knicks on 2026-04-30?" is not reusable. It is one market-shaped sentence. A graph representation can expose the same future value through multiple useful paths:

```text
Jayson Tatum
  -> player stats
  -> points
  -> Knicks at Celtics, 2026-04-30

Boston Celtics
  -> games
  -> Knicks at Celtics, 2026-04-30
  -> player box score
  -> Jayson Tatum points

NBA
  -> 2026-04-30 slate
  -> Knicks at Celtics
  -> Celtics player points
  -> Jayson Tatum
```

All of those paths point to the same observation slot: "Jayson Tatum points in Knicks at Celtics on 2026-04-30." Users should be able to arrive from any path, and market makers should recognize that every condition derived from that slot shares risk.

## Motivation

Prediction markets currently fail at three related layers:

1. **Discovery**: users search strings, not concepts. Similar markets fragment across titles and platforms.
2. **Resolution**: the oracle has to interpret prose. Ambiguous titles create disputes.
3. **Liquidity**: market makers quote isolated contracts even when contracts are logically related.

Sybil already solves a part of the liquidity problem at the exchange layer with [[Frequent Batch Auctions]], [[Payoff Vectors]], [[MM Budget Constraint|market-maker budget constraints]], and flash liquidity. The composition demo knowledge graph is the semantic layer above that engine. It tells the exchange what the contracts mean and how they relate.

The long-term product should feel less like a betting board and more like a Bloomberg terminal for uncertain facts. It should let users navigate from objects in the world to unresolved measurements, inspect the definitions built from those measurements, and trade claims with clear logical structure.

## Core Concepts

### Entity

An entity is a durable object in the world: a person, team, asset, country, office, law, company, benchmark, sports league, data series, or organization.

Examples:

- `ethereum`
- `btc`
- `jayson_tatum`
- `boston_celtics`
- `new_york_knicks`
- `2028_us_presidential_election`
- `iran`
- `federal_reserve`
- `us_unemployment_rate`

Entities are not tradable by themselves. They are graph anchors.

Entities need aliases, external identifiers, and source provenance. For example, Jayson Tatum might map to a SportsDataIO player id, Basketball Reference slug, Wikidata id, and source-market aliases. the graph does not need to trust every external graph, but it should be able to attach references.

### Context

A context scopes an observation.

Examples:

- `2026`
- `2026-Q2`
- `before 2027`
- `Knicks at Celtics, 2026-04-30`
- `2028 Democratic presidential nomination`
- `Iran escalation before 2027`

Context prevents overloading generic measurements. "Jayson Tatum points" is a statistic type. "Jayson Tatum points in Knicks at Celtics on 2026-04-30" is an observation slot.

### Measurement

A measurement is a reusable observable variable. It is not yet true or false. It is the value that will eventually be observed.

Examples:

- `ETH/USD spot price`
- `US unemployment rate`
- `Sahm rule indicator`
- `2028 Democratic nominee`
- `Iran US troop count`
- `Jayson Tatum points`

The current demo treats measurement as a single record with a subject string. That is acceptable for a demo but not enough for the product. The product version should represent measurement as a relation:

```text
MeasurementType(player_points)
  subject: Entity(jayson_tatum)
  context: Event(knicks_at_celtics_2026_04_30)
  unit: points
  feed_set: [sportsdata, official_nba_box_score]
  aggregation: final_box_score
```

This solves the user's concern about specificity: the measurement can be reached through player, game, team, slate, or stat-type navigation without duplicating the value.

### Observation Slot

An observation slot is a future value location:

```text
measurement + context + feed policy + aggregation semantics
```

Examples:

- maximum ETH/USD spot during 2026
- final Jayson Tatum points in Knicks at Celtics on 2026-04-30
- certified winner of the 2028 Democratic presidential nomination
- maximum confirmed US troop count in Iran before 2027

This is likely the most important internal object. Many user-facing conditions can derive from the same observation slot:

```text
ETH max in 2026 > 3000
ETH max in 2026 > 6000
3000 < ETH max in 2026 < 6000
NOT(ETH max in 2026 > 6000)
```

Those conditions share one eventual measured value.

### Condition

A condition is a yes/no predicate over one observation slot.

Examples:

- `ETH max in 2026 > 6000`
- `US unemployment max in 2026 > 5%`
- `Jayson Tatum points vs Knicks > 29.5`
- `2028 Democratic nominee = Gavin Newsom`

Conditions are the first layer that can be directly traded as binary Sybil markets. They should also be composable.

### Market Definition

A market definition is a formula over one or more conditions.

Examples:

```text
ETH > 3000 AND BTC > 100000
```

```text
K_OF_N(2; GDP_Q1_NEGATIVE, GDP_Q2_NEGATIVE, GDP_Q3_NEGATIVE, GDP_Q4_NEGATIVE)
```

```text
IF Newsom wins Democratic nomination THEN Democrat wins presidency
```

The demo used the word "proposition" for this. That is technically defensible but poor UI language. Product UI should call it a "market definition" or "definition."

### Market

A market is the tradable Sybil instrument attached to a condition or market definition. The market has:

- sybil-api `market_id`
- order book / batch state
- prices, volume, status
- resolution lifecycle
- oracle policy

The key distinction is:

```text
Definition != Market
```

A definition can exist before it is published. Once published, it gets a Sybil market id and can be traded.

## Relationship To Existing Knowledge Graphs

The composition demo knowledge graph should not try to rebuild the world's knowledge graph from scratch. Existing graph projects already contain useful entity identity, aliases, hierarchy, and references. Wikidata is the obvious public example. Search engines maintain private knowledge graphs. Sports and financial vendors maintain domain-specific graphs.

The important point is that Sybil's graph is not just an encyclopedia graph. It is a graph of **unresolved future observations and tradable claims**.

Existing graphs answer:

```text
Who is Jayson Tatum?
Which team does he play for?
What games exist in the NBA schedule?
What is ETH?
What is the Federal Reserve?
```

The composition demo knowledge graph answers:

```text
Which future observations about Jayson Tatum can be predicted?
Which yes/no thresholds over those observations are already traded?
Which new definitions can be composed from them?
Which markets imply or arbitrage-bound other markets?
Where can liquidity be reused?
How will the oracle resolve the claim?
```

So the graph should extend existing graphs, not compete with them. It should import references and aliases where useful, but own the prediction-specific schema:

- observation slots
- predicate conditions
- formula definitions
- implication edges
- oracle policies
- market links
- liquidity/risk relationships
- source-market aliases

## Canonical Identity

Canonical identity is what prevents market fragmentation.

A naive market platform treats these as different:

```text
Will ETH hit $6000 in 2026?
ETH above 6000 by end of 2026?
Will Ethereum trade over six thousand dollars next year?
```

The composition demo knowledge graph should treat them as the same condition if they have the same structure:

```text
ObservationSlot:
  measurement_type: spot_price
  subject: ETH
  quote: USD
  window: 2026
  aggregation: max
  feed_policy: approved_crypto_spot_feeds

Predicate:
  op: >
  threshold: 6000
  unit: USD
```

This structural key should be stable and visible in technical views, but never prominent in product UI. The user should see "same definition found" or "equivalent market exists", not a JSON canonical key.

## Implication Edges

Implication edges are logical relationships between conditions or definitions.

For thresholds:

```text
ETH > 6000 -> ETH > 3000
```

Meaning:

> If ETH is above 6000, then it is necessarily above 3000.

This creates a no-arbitrage price constraint:

```text
P(ETH > 6000) <= P(ETH > 3000)
```

For ranges:

```text
3000 < ETH < 6000 -> ETH > 3000
3000 < ETH < 6000 -> NOT(ETH > 6000)
```

The current demo only implements simple deterministic implication edges. A production the graph should generate many kinds:

- nested thresholds
- mutually exclusive outcomes
- exhaustive outcome sets
- range decomposition
- `AND` implies each child
- each child implies `OR`
- `A AND B` implies `A`
- `A` implies `A OR B`
- candidate winner outcomes are mutually exclusive within a contest
- sports game winner outcomes are mutually exclusive and exhaustive
- party-control outcomes can imply sweep definitions

Implication edges are a product feature, not only a math feature. They explain to users why related markets should have ordered prices.

## Liquidity And Market Making

The knowledge graph can materially improve market making, but there are levels.

### Level 1: UI and Quoting Guidance

The simplest version shows relationships and lets MMs choose related markets manually.

Example:

```text
ETH > 6000
ETH > 3000
3000 < ETH < 6000
```

The UI can show:

```text
P(ETH > 6000) <= P(ETH > 3000)
P(3000 < ETH < 6000) <= P(ETH > 3000)
```

This is useful immediately, even if the solver does not enforce the constraints.

### Level 2: Deterministic Quote Curves

For one observation slot with ordered thresholds, a market maker can quote an entire threshold curve.

For example, for `ETH max in 2026`:

```text
P(ETH > 3000) = 74%
P(ETH > 6000) = 31%
P(ETH > 10000) = 12%
```

The curve must be monotone decreasing as threshold increases. The MM can provide flash liquidity across all threshold markets with one risk budget.

This is highly feasible. It does not require a new solver. It requires:

- condition grouping by observation slot
- curve fitting or seeded priors
- monotonicity repair
- flash liquidity submission across all markets
- inventory/risk tracking by observation slot

This is the best near-term market-making product.

### Level 3: Formula-Derived Pricing

Definitions can be priced from their leaves:

```text
ETH > 3000 AND BTC > 100000
```

The naive independent estimate is:

```text
P(A AND B) = P(A) * P(B)
```

That is often wrong because outcomes are correlated. the graph can improve this by storing correlation groups:

- same asset / same observation slot: strong structural dependence
- crypto risk-on basket: positive correlation
- recession macro indicators: positive correlation
- sports same-game parlay legs: contextual correlation
- political primary/general chain: conditional dependence

This is feasible as a demo and as an MM heuristic. It is not a guarantee of no-arb without solver-level constraints.

### Level 4: Flash Liquidity Across The Graph

Sybil's existing architecture is well suited to graph-aware flash liquidity. MMs submit one-shot orders with `mm_budget_nanos`. The solver chooses the welfare-maximizing subset under the MM's budget constraint. This is important because graph markets can share capital.

An MM could say:

```text
I have $50,000 of risk budget for ETH 2026 threshold markets.
Here are quotes for ETH > 3000, ETH > 6000, ETH > 10000, and ETH range markets.
Let the batch solver fill the best combination without exceeding my budget.
```

This fits Sybil better than a CLOB because the MM does not need to leave stale resting orders in every related market. They can update a full graph quote every batch.

### Level 5: Solver-Enforced Implication Constraints

The hard version is to make the solver enforce all graph no-arb relationships:

```text
P(A AND B) <= P(A)
P(ETH > 6000) <= P(ETH > 3000)
P(candidate = Newsom) + P(candidate = Whitmer) + ... <= 1
```

This is possible in principle, but it needs careful design. The current matching engine treats Sybil markets as ordinary binary markets. The graph can produce payoff vectors for composed claims, but global implication constraints over many markets can create a large coupled problem.

Feasible approaches:

1. **Keep enforcement at the quote layer first**: MMs submit arbitrage-aware quotes; UI warns when prices violate edges.
2. **Use market groups for mutually exclusive/exhaustive outcomes**: candidate winner, game winner, party control.
3. **Generate synthetic arbitrage orders**: if prices violate a simple implication, submit a bundle/spread that captures the violation.
4. **Extend the solver with linear no-arb constraints** for a bounded class of implications.
5. **Use decomposition** by observation slot and market group, then coordinate MM budgets across groups.

The near-term product should not attempt full global graph clearing. It should use the graph to improve discovery and quoting first.

## Product Evaluation

### User Perspective

The idea is strong if the UI hides the ontology and exposes a simple path:

```text
What do you want to predict?
  -> Choose the thing being measured
  -> Choose the yes/no threshold
  -> Compose with other conditions
  -> Publish a market
```

The UI should not lead with:

- canonical keys
- resolver primitives
- graph source
- quality seed
- proposition
- implication edge labels

Those are internal or expert concepts. They belong in tooltips or technical details.

The user-facing vocabulary should be:

- **Measurement**: a value we will observe
- **Condition**: a yes/no rule about that value
- **Market definition**: a formula made from conditions
- **Live market**: a published tradable Sybil market
- **Related markets**: logically or economically connected markets

### Creator Perspective

The market creator wants to avoid ambiguous resolution. the graph helps by forcing a creator to specify:

- what is measured
- where it comes from
- when it is observed
- which aggregation is used
- what predicate makes it YES
- whether an equivalent market already exists
- which related markets it implies

This turns market creation from title-writing into structured claim construction.

### Trader Perspective

The trader wants to understand exactly what they are buying.

the graph should show:

- plain-English condition
- measurement source
- observation window
- related stronger/weaker markets
- current live price or model estimate
- resolution path

For a definition, the trader should see the formula visually:

```text
Newsom nominee
  IF_THEN
Democrat wins presidency
```

But the UI should explain `IF_THEN` as "This market resolves YES unless the first condition happens and the second does not."

### Market Maker Perspective

The graph is most valuable to MMs. It gives them:

- grouping by observation slot
- threshold curves
- implication constraints
- duplicate detection
- risk buckets
- correlation hints
- formula decomposition
- flash-liquidity submission targets

This can turn market making from "quote every title independently" into "quote the underlying uncertain variable."

### Oracle Perspective

The graph reduces oracle ambiguity by separating:

- source identity
- measurement type
- aggregation semantics
- observation window
- predicate
- formula

The future `sybil-oracle::ResolutionPolicy::Predicate` model should map directly onto this. A condition should eventually compile into something like:

```text
ResolutionPolicy::Predicate {
  measurement,
  window,
  aggregation,
  predicate,
  source_policy,
}
```

Definitions then compile into formula policies over predicate leaves.

### Engineering Perspective

The approach is feasible but should be staged.

The current demo schema is useful, but it is not the final graph. The main missing object is `Entity`. Without entities, measurements become overly specific strings. With entities, the same value can be reached through many paths.

Recommended next schema:

```text
Entity {
  id
  kind
  name
  aliases
  external_refs
}

Relation {
  from_entity
  relation_type
  to_entity
  valid_window
  source
}

Event {
  id
  kind
  participants
  start_time
  end_time
  source_refs
}

ObservationSlot {
  id
  measurement_type
  subject_entity
  context_entity_or_event
  unit
  feed_policy
  aggregation
  window
}

Condition {
  id
  observation_slot
  predicate
}

Definition {
  id
  formula
}

Market {
  id
  definition_or_condition
  sybil_market_id
}
```

This model can represent the Tatum example cleanly.

## Feasibility Assessment

### What Is Clearly Feasible Now

- 50-200 curated measurements
- 100-500 derived conditions
- formula-based market creation wizard
- equivalent-condition detection
- threshold implication edges
- range implication edges
- duplicate definition detection
- graph explorer
- simple MM quote curves
- flash-liquidity submission across related markets

### What Is Feasible With Moderate Work

- entity graph with aliases and external refs
- multi-path navigation to the same observation slot
- sports slate graph
- election contest graph
- macro data-series graph
- source-market alias matching
- formula pricing with correlation buckets
- no-arb UI warnings
- synthetic arbitrage order suggestions

### What Is Hard

- high-quality automatic ontology extraction from arbitrary market titles
- complete equivalence detection for all natural language markets
- global no-arb enforcement over a large arbitrary formula graph
- oracle automation for messy qualitative outcomes
- preventing spam/duplicate graph nodes in user-created markets
- explaining complex formula markets to non-expert users

### What Is Probably Not Worth Doing Early

- trying to import the entire world knowledge graph
- exposing canonical JSON keys in normal UI
- fully generalized theorem-prover-style implication logic
- full solver-level graph constraints before proving demand
- making every relation tradable

## Recommended Product Direction

The right product direction is:

1. **Build the composition demo knowledge graph as a curated prediction graph, not a general knowledge graph.**
2. **Use external graphs for entity identity and aliases, not for market semantics.**
3. **Make observation slots the central internal abstraction.**
4. **Make market creation a wizard over measurements and conditions.**
5. **Keep the first UI simple: Measurement -> Condition -> Market definition -> Live market.**
6. **Hide technical ontology fields under advanced details.**
7. **Use the graph for market-making before enforcing graph constraints in the solver.**
8. **Start with domains where the ontology is clean: crypto, macro, sports, elections.**
9. **Treat geopolitics carefully because resolution semantics are much harder.**

## Demo Implications

The current composition demo should evolve in this order:

1. Increase to roughly 50 measurements.
2. Rename propositions to market definitions everywhere.
3. Add entities and events behind the scenes.
4. Let measurements be discovered through multiple paths.
5. Group condition cards under measurements.
6. Show "related markets" instead of raw implication edges.
7. Keep canonical keys, resolver primitive, quality, and registry source in technical details only.
8. Add an MM view for threshold curves and shared risk buckets.
9. Add a source alias view that says "external markets that appear to reference this same condition."
10. Add publish flow that first checks duplicates and no-arb relationships.

## UI Direction

The current flat explorer is useful for debugging the registry, but it is probably not the right product interface. A flat list makes every object compete for attention: measurements, conditions, definitions, and markets all look like similarly ranked search results. That is why overly specific measurements such as "Jayson Tatum injury status vs Knicks 2026-04-30" feel wrong. The user is seeing an internal observation-slot label as if it were a top-level concept.

The long-term UI should be path-based and graph-aware:

```text
Domain
  -> Entity
  -> Context/Event
  -> Measurement type
  -> Observation slot
  -> Conditions
  -> Market definitions
  -> Live markets
```

For sports, the primary paths might be:

```text
NBA -> 2026-04-30 slate -> Knicks at Celtics -> Players -> Jayson Tatum -> Points
NBA -> Players -> Jayson Tatum -> Games -> Knicks at Celtics -> Points
NBA -> Teams -> Celtics -> Schedule -> Knicks at Celtics -> Box score markets
```

For crypto:

```text
Crypto -> ETH -> ETH/USD spot -> 2026 max -> Threshold curve
Crypto -> BTC -> BTC/USD spot -> 2026 max -> Threshold curve
Crypto -> Risk baskets -> ETH/BTC/SOL breakout definitions
```

For macro:

```text
Macro -> Labor market -> Unemployment rate -> 2026 thresholds
Macro -> Recession definitions -> Technical recession / Sahm / market stress
```

This can still include search, but search should jump the user into a graph location rather than return a flat table forever. A good near-term UI compromise is a three-pane layout:

1. **Navigator**: domain/entity/context tree.
2. **Workspace**: selected measurement with its condition curve and related definitions.
3. **Inspector**: selected condition or market definition, with live market/trading details.

A graph visualization may be useful for expert inspection, but it should not be the default creation flow. Most users understand trees, breadcrumbs, and grouped lists more quickly than force-directed graphs. The graph should exist in the data model; the UI should expose it as paths, groups, breadcrumbs, and related-market panels.

## Example: Tatum Points

Bad model:

```text
Measurement:
  subject: "Jayson Tatum points vs Knicks 2026-04-30"
```

Better model:

```text
Entity:
  id: jayson_tatum
  kind: player

Entity:
  id: boston_celtics
  kind: team

Entity:
  id: new_york_knicks
  kind: team

Event:
  id: nba_game_2026_04_30_nyk_bos
  kind: nba_game
  participants: [new_york_knicks, boston_celtics]
  date: 2026-04-30

ObservationSlot:
  measurement_type: player_points
  subject: jayson_tatum
  context: nba_game_2026_04_30_nyk_bos
  unit: points
  aggregation: final_box_score
  feed_policy: official_nba_or_sportsdata
```

Then conditions derive from the slot:

```text
Tatum points > 24.5
Tatum points > 29.5
Tatum points > 34.5
```

And paths can point to it:

```text
Jayson Tatum -> game log -> Knicks at Celtics -> points
Boston Celtics -> games -> Knicks at Celtics -> player stats -> Tatum points
NBA slate -> 2026-04-30 -> Knicks at Celtics -> box score -> Tatum points
```

This is the right mental model for the composition demo knowledge graph.

## First Product

The first product should not be "browse the whole graph." The graph is infrastructure. The product should be an agentic trading and market-creation workspace that uses the graph to answer user intent:

```text
User intent -> graph search -> candidate markets -> explanation -> trade or create definition
```

The four strongest initial workflows are:

1. **Hedge my exposure**

   The user starts with a position, not a market:

   ```text
   I am long ETH and worried about downside in 2026.
   ```

   The agent should ask what loss state matters, then find contracts that pay in that state. It should distinguish direct hedges from proxies:

   ```text
   Direct: ETH < 2000
   Direct basket: ETH < 2000 OR BTC < 70000
   Proxy: VIX > 40, hard landing, crypto shock
   ```

   The main product risk is basis risk. The UI must be honest when a proxy might fail even if the user's portfolio loses money.

2. **This news is underappreciated**

   The user starts with a catalyst:

   ```text
   New Iran strike reports look underappreciated.
   ```

   The agent should map the news to a causal channel:

   ```text
   air campaign -> strike thresholds
   ground escalation -> troop count/duration
   legal escalation -> AUMF/declaration
   market spillover -> VIX/oil/macro conditions
   ```

   This workflow needs careful explanation because "news is important" is not enough. The user must choose which future observable changes if the interpretation is right.

3. **Interview me to find bets**

   The user has views but not contracts:

   ```text
   I have views on macro and crypto.
   ```

   The agent should interview the user into measurable claims:

   ```text
   What would prove you right?
   When should it resolve?
   Which related outcome would falsify the thesis?
   Is this a direct forecast or a proxy?
   ```

   This is a strong consumer/product wedge because the graph can turn vague conviction into precise conditions without exposing ontology internals.

4. **I have alpha; how do I monetize it?**

   The user starts with information:

   ```text
   I have alpha about BTC ETF flows.
   ```

   The agent should rank:

   ```text
   closest direct market
   liquid proxy
   custom market definition
   no-trade if resolution does not capture the alpha
   ```

   The product must be strict here. A market can be available and still be the wrong expression of the alpha.

Graph Explorer remains useful, but it is an internal/power-user tool. The primary surface should be the copilot and workspace. Explorer should answer "what does the graph contain?" The copilot should answer "what should I trade or create?"

## Product Viability

The idea is plausible, but not easy. The strongest argument is that prediction markets have a discovery and liquidity fragmentation problem. A graph can make markets deduplicated, composable, and easier to quote. Sybil's batch/flash-liquidity design is directionally aligned with this because market makers can quote many related contracts with bounded risk.

The weakest argument is that the full knowledge graph is too large for a first product. Building a general future-facts graph across politics, sports, macro, crypto, AI, culture, and geopolitics is a large data company. If the UI exposes that scope too early, users will experience the ontology as complexity rather than leverage.

The right first wedge is narrower:

```text
agentic search/create/trade over a curated graph
```

The graph should initially contain enough domains to demonstrate compositionality, but production traction probably requires one or two verticals where:

- users already have strong opinions or exposures,
- resolution sources are credible,
- there are enough related markets for graph navigation to matter,
- market makers can quote curves or correlated baskets,
- the product can show a tradeable edge quickly.

Crypto and macro hedging are the most natural early verticals. Sports is useful for graph modeling because entity/event/stat paths are intuitive, but it has heavy data/vendor and regulatory constraints. Politics/geopolitics are good for market-definition demos but can be resolution-dispute heavy.

## Liquidity Feasibility

Liquidity is feasible only if the graph helps market makers reuse capital. It is not feasible if the product creates hundreds of bespoke markets with no quoting surface.

The most realistic path is:

```text
observation slot -> threshold curve -> batch quote -> shared MM risk budget
```

For example:

```text
ETH max in 2026 > 3000
ETH max in 2026 > 6000
ETH max in 2026 > 10000
3000 < ETH max in 2026 < 6000
```

A market maker can quote the whole curve monotonically and let Sybil's batch solver fill only the subset that fits their budget. This is a real advantage over isolated order books.

The next level is formula pricing:

```text
ETH > 3000 AND BTC > 100000
```

This can be quoted heuristically from leaves, correlations, and user flow. It should not be presented as mathematically enforced no-arb until the solver actually enforces those constraints.

Full graph-wide no-arb is possible in principle, but it should not be the first liquidity milestone. The first milestone should be:

- visible implication edges,
- duplicate/equivalent definition detection,
- threshold-curve grouping,
- simple monotone quote repair,
- MM budget reuse across related markets,
- UI warnings for violations.

## Hard VC Questions

An investor will likely ask:

- **Why now?** Existing prediction markets have struggled with liquidity and legal/regulatory ambiguity. The answer needs to be more than "better UI." The credible answer is compositional market creation plus batch/flash liquidity.
- **Who is the first user?** "Everyone who predicts things" is too broad. A sharper answer is crypto/macro users who already have exposure and want hedges or proxy trades.
- **Where does liquidity come from?** Retail flow alone is not enough. The product needs a market-maker story: graph-aware quote curves, bounded batch risk, and capital reuse across related markets.
- **What prevents market fragmentation?** The answer is canonical measurement/condition/definition identity, duplicate detection, and source-market aliases as evidence rather than roots.
- **How do you avoid oracle disputes?** The graph must force explicit measurement, source, window, aggregation, and predicate fields before publishing.
- **Why will users trust agent suggestions?** The agent must show the reasoning path: exposure/news/alpha -> measurement -> condition -> market definition -> risks.
- **Is this a data company?** Partly yes. The graph needs entity resolution, source metadata, and curated observation slots. The first product must constrain the domain enough that data quality is defensible.
- **Can users understand compositions?** Only if the UI uses plain language. Terms like proposition, canonical key, resolver primitive, quality seed, and graph source should be hidden or explained in expert views.
- **What is the moat?** Potential moats are liquidity, canonical market graph data, resolution infrastructure, market-maker tooling, and historical graph/trade data. The ontology alone is not a moat.
- **What is the killer use case?** The best candidate is "I have exposure/news/alpha; find or create the market that expresses it, then trade it with visible liquidity."

## Current Implementation Boundary

The demo should stay honest about what is implemented:

- The graph has curated feeds, entities, contexts, measurements, conditions, definitions, live-market links, and implication edges.
- The agent has deterministic workflows for hedging, news/proxy trades, interview-style discovery, alpha expression, and draft creation.
- The UI now leads with the copilot, keeps Graph Explorer as an advanced view, and exposes a tree-like navigator over domain/entity/event/measurement/condition.
- Ontology diagnostics catch missing graph anchors, broken measurement links, invalid formulas, and legacy atom rows.
- Liquidity is still demo-level: seeded fair values, simple formula estimates, and batch quote submission. It is not yet solver-enforced graph no-arb.

The next practical milestone should be graph-aware market making for threshold curves. That is the smallest liquidity feature that uses the ontology for something economically meaningful.

## Open Questions

- Should Sybil have one global entity namespace, or domain-specific namespaces linked by aliases?
- How much entity resolution should be automatic versus curated?
- How should user-created entities be moderated?
- Which graph relationships should become solver constraints versus UI warnings?
- Should market definitions be immutable after publishing?
- How should a market migrate if its underlying observation slot is later deduplicated?
- How should the graph expose uncertainty about source matching?
- Should MMs publish quote curves as first-class objects?
- Can formula pricing be learned from historical co-movement once enough markets exist?

## Bottom Line

The idea is coherent and potentially powerful. The biggest value is not "more markets." The value is that Sybil can make prediction markets compositional, deduplicated, and liquidity-aware.

The risk is product complexity. If users see graph internals, the system feels confusing. If users see a clean path from real-world object to measurable value to yes/no condition to market definition, the graph becomes intuitive.

The right framing is:

> The composition demo knowledge graph is a prediction knowledge graph. Existing knowledge graphs know what things are. The composition demo knowledge graph knows which future facts about those things are unresolved, tradable, logically related, and resolvable.
