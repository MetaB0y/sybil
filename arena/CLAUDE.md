# CLAUDE.md

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
Bots (Python)  →  sybil_client  →  HTTP/SSE  →  sybil-api (Rust)
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
| `feeds/` | Data feed integrations (synthetic) |
| `scripts/` | Competition orchestration |
| `examples/` | Example competition scripts |
| `tests/` | Pytest test suite |
| `iran/` | Iran strike market simulation (news data, bot logic, runner) |
| `nba/` | Legacy NBA/sports code (preserved for reference) |

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
        # Use filter_markets() to only trade on allowed markets
        for market_id, (yes_nanos, no_nanos) in self.filter_markets(block).items():
            return [BuyYes.at_price(market_id, price=0.55, quantity=5)]
        return []
```

All bots accept `market_ids: list[int] | None` to restrict trading to specific markets. The competition runner automatically passes the competition market IDs to all bots.

For `InformedTrader`, use `use_market_index=True` when the probability model uses indices (0, 1, 2...) rather than absolute market IDs.

### feeds

| Feed | Source |
|------|--------|
| `SyntheticFeed` | Random events for testing |

### scripts

`run_competition.py` provides:
- `setup_competition()` - create accounts and markets
- `run_competition()` - execute with live standings
- `print_leaderboard()` - display results

## Order Matching Rules

- **BuyYes + BuyNo** can match via minting (costs $1 total)
- **SellYes/SellNo** requires owning the position first
- Orders persist for 3 blocks if unfilled (TTL)
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
