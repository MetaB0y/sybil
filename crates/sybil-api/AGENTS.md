# AGENTS.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this crate.

## Purpose

The **sybil-api** crate is an HTTP API server exposing the prediction market matching engine to external clients. Built with Axum and OpenAPI documentation.

## Architecture Notes

Before modifying this crate, read these vault notes (`docs/architecture/`):
- [[REST API]] — endpoint design, request/response contracts
- [[SSE Block Stream]] — server-sent events for real-time block updates
- [[P256 Authentication]] — signed order submission flow

## Endpoints

### System
- `GET /v1/health` — server health + latest block height
- `GET /v1/state-root` — current state root hash

### Accounts
- `POST /v1/accounts` — create account (dev mode)
- `GET /v1/accounts/{id}` — balance + positions
- `POST /v1/accounts/{id}/fund` — add funds (dev mode)
- `POST /v1/accounts/{id}/keys` — register P256 public key

### Markets
- `GET /v1/markets` — list all with current prices
- `POST /v1/markets` — create binary market (dev mode)
- `GET /v1/markets/{id}` — market details + status
- `GET /v1/markets/prices` — all clearing prices
- `GET /v1/markets/groups` — list market groups
- `POST /v1/markets/groups` — create group (dev mode)
- `POST /v1/markets/{id}/resolve` — resolve with payout (dev mode)

### Orders
- `POST /v1/orders` — submit unsigned orders
- `POST /v1/orders/signed` — submit P256-signed order

### Blocks
- `GET /v1/blocks/latest` — latest block with fills, prices, rejections
- `GET /v1/blocks/{height}` — block at specific height
- `GET /v1/blocks/stream` — SSE stream of new blocks

## Order Types (OrderSpec)

```rust
enum OrderSpec {
    BuyYes { market_id, limit_price_nanos, qty },
    BuyNo { market_id, limit_price_nanos, qty },
    SellYes { market_id, limit_price_nanos, qty },
    SellNo { market_id, limit_price_nanos, qty },
    Spread { buy_market, sell_market, limit_price_nanos, qty },
    BundleYes { market_ids, limit_price_nanos, qty },
    BundleSell { market_ids, limit_price_nanos, qty },
    Custom { market_ids, payoffs, limit_price_nanos, max_fill },
}
```

## Dev Mode

Many endpoints require `--dev-mode` flag:
- Account creation/funding
- Market creation/resolution
- Market group creation

Production deployments disable these for security.

## Architecture

```
ApiConfig (port, dev_mode, block_interval_ms)
    ↓
AppState (SequencerHandle + dev_mode)
    ↓
Axum Router (routes, CORS, tracing)
    ↓
SequencerActor (message passing)
```

## Module Map

| Module | Purpose |
|--------|---------|
| `app.rs` | Router creation, OpenAPI schema |
| `config.rs` | ApiConfig parsing |
| `state.rs` | AppState with sequencer handle |
| `convert.rs` | API ↔ engine type conversions |
| `sse.rs` | Server-sent events streaming |
| `routes/*.rs` | Endpoint handlers |
| `types/*.rs` | Request/response DTOs |

## Units

All prices and balances in nanos (u64):
- 1 dollar = 1,000,000,000 nanos
- Binary market: YES + NO prices = $1

## Signed Orders

P256 ECDSA signatures for authenticated order submission:
1. Register public key via `POST /accounts/{id}/keys`
2. Submit signed order via `POST /orders/signed`
