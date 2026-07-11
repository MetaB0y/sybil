---
tags: [arena]
layer: arena
status: current
last_verified: 2026-07-11
---

The bot framework is a Python base class pattern for building trading agents. Every bot extends `BaseAgent` and implements a single method: `on_block(block: Block) -> list[OrderSpec]`. When a new block arrives via the [[SSE Block Stream]], the framework calls `on_block()` and submits any returned orders via the [[Python SDK]]. This event-driven design means bots are reactive — they make decisions in response to market state changes.

Several reference bots demonstrate the pattern. `SimpleMarketMaker` quotes both sides of each market with a configurable spread. `RandomTrader` generates noise flow for testing. `InformedTrader` has a private model of true probabilities and trades when the edge (model price minus market price) exceeds a threshold. `MomentumTrader` follows price trends. All bots accept `market_ids: list[int] | None` to restrict which markets they trade on, enabling focused strategies in multi-market simulations.

The competition runner orchestrates multiple bots trading simultaneously. It starts the Sybil server, creates accounts for each bot, creates markets, and runs the bots in parallel. Each bot has its own account and sees the same block stream. The runner collects results and produces performance analytics. Adding a new bot is a four-step process: create the class extending `BaseAgent`, implement `on_block()`, export from `bots/__init__.py`, and add to the competition config.

## Key Properties
- `BaseAgent.on_block(block) -> list[OrderSpec]` — the core interface
- Event-driven: bots react to blocks, not poll
- Reference bots: SimpleMarketMaker, RandomTrader, InformedTrader, MomentumTrader
- `market_ids` filter for focused trading strategies
- Competition runner for multi-bot simulations
- Adding a bot: extend BaseAgent, implement on_block, export, configure

## Where This Lives
> `arena/bots/` — bot implementations and `BaseAgent`
> `arena/examples/` — competition scripts
> `arena/scripts/` — orchestration utilities

## See Also
- [[Python SDK]] — the transport layer bots use to submit orders
- [[SSE Block Stream]] — delivers blocks that trigger `on_block()`
- [[LLM Trader]] — AI-powered bot using LLM for decisions
