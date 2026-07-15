---
tags: [arena]
layer: arena
status: current
last_verified: 2026-07-15
---

The bot framework is a Python base class pattern for building trading agents. Every bot extends `BaseAgent` and implements a single method: `on_block(block: Block) -> list[OrderSpec]`. When a new block arrives via the [[SSE Block Stream]], the framework calls `on_block()` and submits any returned orders via the [[Python SDK]]. This event-driven design means bots are reactive — they make decisions in response to market state changes.

Several reference bots demonstrate the pattern. `SimpleMarketMaker` quotes both sides of each market with a configurable spread. `RandomTrader` generates noise flow for testing. `InformedTrader` has a private model of true probabilities and trades when the edge (model price minus market price) exceeds a threshold. `MomentumTrader` follows price trends. All bots accept `market_ids: list[int] | None` to restrict which markets they trade on, enabling focused strategies in multi-market simulations.

The competition runner orchestrates multiple bots trading simultaneously. It starts the Sybil server, creates accounts for each bot, creates markets, and runs the bots in parallel. Each bot has its own account and sees the same block stream. The runner collects results and produces performance analytics. Adding a new bot is a four-step process: create the class extending `BaseAgent`, implement `on_block()`, export from `bots/__init__.py`, and add to the competition config.

Live Arena runs persist an explicit runtime identity and participant membership in
`arena_runs` and `arena_run_participants`. Competitors are scored; fast and noise
traders remain visible as diagnostic load but are not part of public totals. A
runtime heartbeat is refreshed with portfolio snapshots, and readers consider it
live for at most 15 minutes without a heartbeat. Historical decisions and snapshots
are retained, but score aggregates are computed only from the current live cohort;
they never infer membership from name patterns or mix prior runs into current PnL.

## Key Properties
- `BaseAgent.on_block(block) -> list[OrderSpec]` — the core interface
- Event-driven: bots react to blocks, not poll
- Reference bots: SimpleMarketMaker, RandomTrader, InformedTrader, MomentumTrader
- `market_ids` filter for focused trading strategies
- Competition runner for multi-bot simulations
- Explicit, heartbeating live cohort separates scored competitors from load/noise
- Adding a bot: extend BaseAgent, implement on_block, export, configure

## Where This Lives
> `arena/bots/` — bot implementations and `BaseAgent`
> `arena/examples/` — competition scripts
> `arena/scripts/` — orchestration utilities

## See Also
- [[Python SDK]] — the transport layer bots use to submit orders
- [[SSE Block Stream]] — delivers blocks that trigger `on_block()`
- [[LLM Trader]] — AI-powered bot using LLM for decisions
