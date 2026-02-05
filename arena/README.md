# Sybil Arena

AI Agent Trading Competition for Sybil Prediction Markets.

## Quick Start

### 1. Start Sybil API Server

```bash
# From repo root
cargo run --release -p sybil-api -- --dev-mode --port 3000
```

### 2. Install Python Dependencies

```bash
cd arena
uv sync  # or: pip install -e .
```

### 3. Run a Test Competition

```bash
python examples/simple_test.py
```

## Architecture

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Bot 1     │     │   Bot 2     │     │   Bot N     │
│  (Python)   │     │  (Python)   │     │  (Python)   │
└──────┬──────┘     └──────┬──────┘     └──────┬──────┘
       │                   │                   │
       └───────────────────┼───────────────────┘
                           │
                  ┌────────▼────────┐
                  │  sybil-client   │
                  │  (Python SDK)   │
                  └────────┬────────┘
                           │ HTTP/SSE
                  ┌────────▼────────┐
                  │   sybil-api     │
                  │  (Rust server)  │
                  └─────────────────┘
```

## Components

- **sybil_client/**: Python SDK for sybil-api
- **bots/**: Trading bot implementations
- **feeds/**: Data feed integrations (sports, crypto)
- **scripts/**: Competition orchestration
- **examples/**: Example competitions

## Creating a Bot

```python
from sybil_client import SybilClient
from bots.base import BaseAgent

class MyBot(BaseAgent):
    async def on_block(self, block):
        # Your trading logic here
        orders = []
        for market_id, prices in block.clearing_prices.items():
            if self.should_trade(market_id, prices):
                orders.append(self.make_order(market_id, prices))
        return orders
```

## Running a Competition

```python
from scripts.run_competition import run_competition

standings = await run_competition(
    bots=[bot1, bot2, bot3],
    duration_seconds=300,
    resolve_callback=resolve_markets
)
```

See [PLAN.md](PLAN.md) for detailed implementation plan.
