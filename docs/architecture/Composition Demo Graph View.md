---
tags: [ontology, composition, graph, ui, agents]
layer: api
status: planned
last_verified: 2026-04-24
---

# Composition Demo Graph View

The graph view is a product and developer surface over the composition demo knowledge graph. It should show the actual ontology relationships, but it should not make the UI depend on raw registry records. The right architecture is a graph projection layer:

```text
Registry objects -> Graph projection -> UI graph view / agent search / diagnostics
```

The projection is deliberately smaller and more regular than the full object model.

## Projection Model

A graph projection has nodes and edges:

```text
GraphNode:
  id
  kind: entity | context | measurement | condition | definition | market
  label
  domain
  summary
  score
  object_id
  object_kind

GraphEdge:
  from
  to
  type: entity_measurement | context_measurement | measurement_condition | condition_definition | implication | live_market
  label
  strength
```

This is the contract every consumer should use. The React graph view should render this projection. Agents should search and traverse this projection. Diagnostics should validate this projection. The UI should not infer graph edges from prose labels.

## Direction

Edges should follow the product explanation path:

```text
Entity/Context -> Measurement -> Condition -> Definition -> Market
```

Implication edges are special because they express logic between conditions:

```text
Condition -> stronger/weaker related Condition
```

For example:

```text
ETH > 6000 -> ETH > 3000
```

This means if the first resolves YES, the second must resolve YES. The graph view can render implication edges with a distinct style, but the canonical direction remains implication direction.

## Search And Neighborhoods

Agents need easy search. The projection should expose:

- text search over label, summary, domain, kind, path, predicate, formula leaves, and aliases;
- filters by kind and domain;
- a focus id;
- neighborhood depth;
- a limit.

The intended query shape:

```text
POST /graph
  query?: string
  domain?: string
  kind?: string
  focus_id?: string
  depth?: 1 | 2 | 3
  limit?: number
```

The response should include:

```text
nodes
edges
focus_id
matched_ids
facets
```

This gives agents a compact graph to reason over without loading every registry detail. A future agent can ask:

```text
Search "ETH downside hedge"
Expand depth 2 around ETH < 2000
Return connected definitions and live markets
```

## UI Layout

The first graph view should be a layered graph, not a force-directed blob. The ontology has a natural left-to-right structure:

```text
Entity/Event | Measurement | Condition | Definition | Live market
```

Layered layout is better for debugging because it makes wrong edges obvious. If a sports player condition appears under macro, or a definition points directly to an entity without a condition, the mistake is visible.

The view should support:

- click node to select it in the main inspector;
- hover node to highlight direct neighbors;
- search/filter before rendering;
- focus on selected node;
- depth toggle;
- kind legend;
- edge labels in tooltip or compact text;
- stable deterministic layout so data changes are reviewable.

Force-directed layout can be added later for exploration, but it should not be the default. A stable graph is easier to compare across seed changes and easier for agents to reference.

## Data Quality Role

The graph view should make sloppy ontology data uncomfortable. Good data should look like paths:

```text
NBA -> Knicks at Celtics -> Jayson Tatum points -> Tatum points > 29.5 -> Celtics SGP
Crypto -> ETH/USD spot -> ETH > 6000 -> Crypto supercycle
Macro -> VIX -> VIX > 40 -> Hard landing
```

Bad data will show up as:

- orphan measurements with no entity or context;
- condition nodes detached from measurements;
- definitions with strange cross-domain leaves;
- too many single-use measurements that should share an observation slot;
- generic subjects like "event happens" with no entity anchor;
- markets attached to definitions that do not explain resolution.

The graph projection therefore becomes both UI infrastructure and ontology QA.

## Population Strategy

To stress-test the ontology, the demo needs more measurements, but not random title spam. The right expansion pattern is:

```text
domain templates + curated entities + contexts + reusable measurement types
```

For each domain:

- add entities first;
- add contexts/events/windows second;
- add measurement types third;
- derive several conditions from each measurement;
- compose a smaller number of definitions from high-quality conditions.

This creates density and reuse. It avoids the anti-pattern:

```text
one market title -> one bespoke measurement -> one bespoke condition
```

The seed target for the next demo stress test should be roughly:

- 80-120 measurements;
- 180-260 conditions;
- 40-70 definitions;
- 10-25 threshold curves;
- no legacy atom rows;
- zero ontology diagnostic errors.

Quality is more important than count. If a row makes someone ask "why is this here?", it should be removed or grounded with better entities/context.
