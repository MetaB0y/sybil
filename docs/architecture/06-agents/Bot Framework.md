---
tags: [arena]
layer: arena
status: current
last_verified: 2026-07-18
---

The bot framework is a Python base class pattern for building trading agents. Every bot extends `BaseAgent` and implements a single method: `on_block(block: Block) -> list[OrderSpec]`. When a new block arrives via the [[WebSocket Block Stream]], the framework calls `on_block()` and submits any returned orders via the [[Python SDK]]. This event-driven design means bots are reactive — they make decisions in response to market state changes.

Reconnect replay is observational, not a new trading cadence. `BaseAgent`
updates canonical account/fill state for replayed block events but calls
`on_block()` only after the SDK's replay-complete boundary. Historical blocks
therefore cannot resubmit strategy work after a transient disconnect.

Every live strategy decision also starts from a successful canonical account
and fill refresh and a successful pending-order read. Either read failing
suppresses the decision for that block; Arena never guesses that stale cash,
positions, or reservations are safe. Only API-accepted submissions count
toward `max_blocks` and accepted-order telemetry. A failure in the
post-acceptance observation hook cannot retroactively turn that accepted write
into a rejection.

At startup, live Arena fetches `GET /v1/orders/policy` once and attaches the
typed policy to every account-holding agent. `BaseAgent` computes the same
integer ceil-price-times-quantity notional as sequencer admission and
suppresses ordinary dust locally. It deliberately does not enlarge an order:
doing so could exceed cash, remaining inventory, or a strategy exposure limit.
Suppression is visible as `below_min_notional` in durable sizer decisions,
status output, Prometheus metrics, and the operations dashboard. One-shot
flash-liquidity/MM bundles remain governed by their separate budget semantics.

Several reference bots demonstrate the pattern. `SimpleMarketMaker` quotes both sides of each market with a configurable spread. `RandomTrader` generates noise flow for testing. `InformedTrader` has a private model of true probabilities and trades when the edge (model price minus market price) exceeds a threshold. `MomentumTrader` follows price trends. All bots accept `market_ids: list[int] | None` to restrict which markets they trade on, enabling focused strategies in multi-market simulations.

The competition runner orchestrates multiple bots trading simultaneously. It starts the Sybil server, creates accounts for each bot, creates markets, and runs the bots in parallel. Each bot has its own account and sees the same block stream. The runner collects results and produces performance analytics. Adding a new bot is a four-step process: create the class extending `BaseAgent`, implement `on_block()`, export from `bots/__init__.py`, and add to the competition config.

Live Arena runs persist an explicit runtime identity and participant membership in
`arena_runs` and `arena_run_participants`. Competitors are scored; fast and noise
traders remain visible as diagnostic load but are not part of public totals. A
runtime heartbeat is refreshed with portfolio snapshots, and readers consider it
live for at most 15 minutes without a heartbeat. Historical decisions and snapshots
are retained, but score aggregates are computed only from the current live cohort;
they never infer membership from name patterns or mix prior runs into current PnL.
Decision, token-usage, and portfolio-snapshot rows carry the producing `run_id`, so
reusing a trader name cannot attach an older run's measurements to the current
cohort. Runtime replacement and participant insertion commit atomically. A replaced
writer loses its heartbeat lease; any late rows remain tagged with its old run and
cannot contaminate the successor. Roles are closed to `competitor`, `load`, and
`noise`; load/noise rows are ineligible for scoring at both the write and read
boundaries.

The Rust liquidity service has two distinct price-source policies. Mirrored
Polymarket markets quote from their external token feed and stop publishing a
reference when that feed becomes stale. Native Sybil markets have no external
price source, so their MM midpoint remains the checked-in catalog seed, inside
the configured quote range. Public block clearing prices are deliberately not
fed back into that midpoint: the block stream does not identify whether a mark
came from organic information, the MM itself, or synthetic load. Native markets
can adopt a dynamic anchor only after that provenance and its update invariant
are designed explicitly.

Live synthetic flow also stays on the ordinary client boundary. Fast and noise
actors reuse persisted `(name, strategy)` account mappings across Arena
restarts. Their total starting bankroll is fixed by
`ARENA_SYNTHETIC_TOTAL_CAPITAL` (default `$300,000`) and divided across the
configured actor count, so adding actors changes scheduling granularity rather
than minting capital. Crossing-noise choices are deterministic from
`(actor seed, block height)`, which makes a replay or restart reproduce the
same block decision. Each actor emits at most one order per selected market;
for core mutually-exclusive MarketGroups it also suppresses the final YES buy
that would complete every group outcome in one account submission.

Market discovery has two deliberately separate outputs. The analyst/sizer
cohort contains every active, unexpired mirrored market with a fresh external
reference under the configured news profile; it may be narrowed explicitly for
an experiment. Cheap synthetic flow is never narrowed by that LLM cohort. It
covers every active, unexpired native market plus every active mirrored market
with a fresh reference, and excludes untagged smoke fixtures. Fast traders
remain reference-only inside that universe, while crossing-noise traders can
participate in both native and mirrored markets. The production Compose profile
has no fixed market-count cap. `sybil_arena_selected_markets` and
`sybil_arena_synthetic_markets` expose both cardinalities independently.

## Key Properties
- `BaseAgent.on_block(block) -> list[OrderSpec]` — the core interface
- Event-driven: bots react to blocks, not poll
- Reference bots: SimpleMarketMaker, RandomTrader, InformedTrader, MomentumTrader
- `market_ids` filter for focused trading strategies
- Competition runner for multi-bot simulations
- Explicit, heartbeating live cohort separates scored competitors from load/noise
- Native MM anchors do not learn recursively from provenance-free internal clears
- Synthetic load uses durable accounts, fixed aggregate capital, and block-keyed decisions
- Canonical refresh and pending-order reads fail closed before strategy side effects
- Server-advertised admission policy suppresses dust without silently changing risk
- Adding a bot: extend BaseAgent, implement on_block, export, configure

## Where This Lives
> `arena/bots/` — bot implementations and `BaseAgent`
> `arena/examples/` — competition scripts
> `arena/scripts/` — orchestration utilities

## See Also
- [[Python SDK]] — the transport layer bots use to submit orders
- [[WebSocket Block Stream]] — delivers resumable blocks that trigger `on_block()`
- [[LLM Trader]] — AI-powered bot using LLM for decisions
