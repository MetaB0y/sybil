# AGENTS.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this directory.

## Purpose

The **arena** directory contains a Python-based AI agent trading competition framework for Sybil prediction markets. It connects to `sybil-api` (Rust server) and allows bots to trade against each other.

## Quick Start

```bash
# Terminal 1: Start the Sybil server
cargo run --release -p sybil-api -- --dev-mode --port 3001

# Terminal 2: Run a competition
cd arena
uv sync
uv run python examples/full_competition.py
```

## Architecture

```
Bots (Python)  →  sybil_client  →  HTTP/WebSocket  →  sybil-api (Rust)
                                                      ↓
                                              matching-sequencer
                                                      ↓
                                              matching-solver
```

## Directory Structure

| Directory | Purpose |
|-----------|---------|
| `sybil_client/` | Async Python SDK for sybil-api |
| `bots/` | Trading bot implementations (generic) |
| `sim/` | Generic simulation framework (clock, LLM trader, runner, results) |
| `markets/` | Per-market configuration (personas, sources, prompts) |
| `markets/iran/` | Iran strike market: config, personas, sources, fetch_data, merge_data, datasets/, phase1/, runs/ |
| `markets/iran/docs/` | Market-specific docs (llm-trader-flow decision pipeline) |
| `viz/` | Streamlit dashboards |
| `feeds/` | Data feed integrations (synthetic) |
| `scripts/` | Competition orchestration |
| `examples/` | Example competition scripts |
| `tests/` | Pytest test suite |

## Key Components

### sybil_client

Async HTTP client using `httpx`:
- `SybilClient` - main client class
- `Account`, `Market`, `Block`, `Fill` - data types
- `BuyYes`, `BuyNo`, `SellYes`, `SellNo` - order specs

```python
async with SybilClient("http://localhost:3001") as client:
    account = await client.create_account(100_000_000_000)  # $100
    await client.buy_yes(account.id, market_id=0, price=0.55, quantity=10)
    async for block in client.stream_blocks():
        print(block.clearing_prices)
```

### bots

All bots extend `BaseAgent` and implement `on_block()`:

| Bot | Strategy |
|-----|----------|
| `SimpleMarketMaker` | Quotes both sides with spread |
| `RandomTrader` | Random orders for noise |
| `InformedTrader` | Trades on model vs market edge |
| `MomentumTrader` | Follows price trends |

```python
class MyBot(BaseAgent):
    async def on_block(self, block: Block) -> list[OrderSpec]:
        for market_id, (yes_nanos, no_nanos) in self.filter_markets(block).items():
            return [BuyYes.at_price(market_id, price=0.55, quantity=5)]
        return []
```

All bots accept `market_ids: list[int] | None` to restrict trading to specific markets.

### sim/ — Generic Simulation Framework

| Module | Purpose |
|--------|---------|
| `sim/clock.py` | `SimulatedClock` — time-compressed clock with ref-counted pause |
| `sim/llm_trader.py` | `LlmTrader` — LLM makes full trading decisions (analysis + orders) |
| `sim/headline_filter.py` | Phase 1 headline relevance filter |
| `sim/runner.py` | `SimulationConfig` + `run_simulation()` orchestration |
| `sim/results.py` | `build_block_records()` + `save_and_print_results()` |

### markets/ — Per-Market Configuration

Each market provides a `get_config() -> MarketConfig` with question, prompts, personas, sources, and paths.

```python
from markets.iran import get_config
config = get_config()
# config.question, config.personas, config.build_persona, config.datasets_dir, ...
```

To add a new market: create `markets/mymarket/` with `__init__.py` (get_config), `config.py`, `personas.py`, `sources.py`.

### viz/ — Dashboards

```bash
cd arena && uv run streamlit run viz/news_explorer.py -- --market iran
```

### feeds

| Feed | Source |
|------|--------|
| `SyntheticFeed` | Random events for testing |

## Simulation Commands

```bash
# Run simulation
cd arena && uv run python -m sim.runner --market iran --compression 300 --dates 20260101

# Run phase1 filter
cd arena && uv run python -m sim.headline_filter --market iran --bot american_believer --date 2026-01-02

# Launch dashboard
cd arena && uv run streamlit run viz/news_explorer.py -- --market iran
```

## Order Matching Rules

- **BuyYes + BuyNo** can match via minting (costs $1 total)
- **SellYes/SellNo** requires owning the position first
- Orders rest until canceled or their explicit time-in-force expires
- Prices are in nanos: 1 dollar = 1,000,000,000 nanos

## Testing

```bash
uv run pytest tests/ -v
```

## Common Issues

1. **Port 3000 in use**: Use `--port 3001` for sybil-api
2. **SCIP library error**: The `milp` feature was removed from matching-sequencer to avoid this
3. **Orders not filling**: Need both buy and sell sides, or BuyYes + BuyNo to mint

## Adding a New Bot

1. Create `bots/my_bot.py`
2. Extend `BaseAgent`
3. Implement `on_block(self, block: Block) -> list[OrderSpec]`
4. Export from `bots/__init__.py`
5. Add to competition config in examples
