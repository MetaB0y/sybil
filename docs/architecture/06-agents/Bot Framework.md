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

The production noise flow is deliberately stricter than the generic bot
framework. `live.noise_coordinator.NoiseCoordinator` owns exactly fifteen
durable role-bound accounts. After each live block every actor independently
samples about 1.9% of the committed universe and submits a sparse IOC epoch for
`height + 1`; aggregate touched-market coverage is about 25%. Random lanes
include principal, generation, height, market, and purpose, so selection,
direction, group holes, size, and aggressiveness differ by actor but replay
deterministically. Every order prices from the previous committed Sybil mark:
clearing when traded, otherwise the committed book midpoint or carried mark.
Sixty percent are independently aggressive and 40% passive, with randomized
distance bounded by the frontend Lite-tax curve. The coordinator never reads
the MM's upcoming quote; natural MM crosses are measured only after submission.
Inventory-marked direction bias increases real YES/NO selling as holdings
accumulate. Categorical groups retain an actor-specific uncovered hole, native
prices stay inside complementary actor guardrails, and anti-starvation raises
selection probability gradually after eight untouched blocks.

The Rust mirror process is the production MM. It quotes every effective mirror
and native market in one actor epoch, using `SellYes` as the ask and `SellNo` as
the economic YES bid from pre-collateralized complete sets. It replenishes a
neutral YES+NO inventory floor before quoting. Fresh mirror anchors quote
normally, soft-stale anchors quote smaller and wider, and hard-stale/extreme
anchors carry typed skips. Native anchors start from coherent catalog seeds,
accept only qualifying organic fills, use a capped weighted median and bounded
EWMA, mean-revert when quiet, project categorical and threshold cohorts, and
stay in the two-sided quoteable interior.
Inventory, volatility, exposure, stale feeds, extreme prices, and insufficient
cash can widen, reduce, or explicitly skip quotes; the operational coverage
target is 100%, with at least 98% two-sided in healthy steady state.

## Key Properties
- `BaseAgent.on_block(block) -> list[OrderSpec]` — the core interface
- Event-driven: bots react to blocks, not poll
- Reference bots: SimpleMarketMaker, RandomTrader, InformedTrader, MomentumTrader
- `market_ids` filter for focused trading strategies
- Competition runner for multi-bot simulations
- Explicit, heartbeating live cohort separates scored competitors from load/noise
- Production sparse noise coordinator: exactly 15 durable IOC actors, ~25% aggregate market coverage
- Production MM: complete-set-funded, all-market actor epochs with native guardrails
- Adding a bot: extend BaseAgent, implement on_block, export, configure

## Where This Lives
> `arena/bots/` — bot implementations and `BaseAgent`
> `arena/examples/` — competition scripts
> `arena/scripts/` — orchestration utilities

## See Also
- [[Python SDK]] — the transport layer bots use to submit orders
- [[SSE Block Stream]] — delivers blocks that trigger `on_block()`
- [[LLM Trader]] — AI-powered bot using LLM for decisions
