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
- `POST /v1/accounts` — create account (service route; token skipped in dev mode)
- `GET /v1/accounts/{id}` — balance + positions
- `POST /v1/accounts/{id}/fund` — add funds (service route; token skipped in dev mode)
- `POST /v1/accounts/{id}/keys` — register P256 public key

### Markets
- `GET /v1/markets` — list all with current prices
- `POST /v1/markets` — create binary market (service route; token skipped in dev mode)
- `GET /v1/markets/{id}` — market details + status
- `GET /v1/markets/prices` — all clearing prices
- `GET /v1/markets/groups` — list market groups
- `POST /v1/markets/groups` — create group (service route; token skipped in dev mode)
- `POST /v1/markets/{id}/resolve` — resolve with payout or signed attestation (service route; token skipped in dev mode)
- `POST /v1/markets/prices/reference` — update external reference prices (service route)
- `POST /v1/markets/{id}/metadata` — update off-block mirror metadata (service route)
- `PUT /v1/events/{event_id}/raw` — update raw mirror event snapshot (service route)

### Orders
- `POST /v1/orders` — submit unsigned orders
- `POST /v1/orders/signed` — submit P256-signed order
- `GET /v1/orders/pending` — diagnostic pending-order listing (dev mode only; not mounted in prod)
- `GET /v1/markets/{id}/orderbook` — diagnostic market orderbook listing (dev mode only; not mounted in prod)

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
}
```

Public API admission only accepts single-market binary one-hot orders. The core
`matching-engine` payoff-vector helpers for spreads, bundles, and custom payoff
vectors remain available for research and tests, but are not exposed through
`OrderSpec`.

## Service and Dev Mode

Production operator/service endpoints are mounted in all modes but require:

```text
Authorization: Bearer $SYBIL_SERVICE_TOKEN
```

when `dev_mode=false`. If `SYBIL_SERVICE_TOKEN` is unset in production, these
routes fail closed. In `dev_mode=true`, the bearer check is skipped so local
workflows and plain `docker compose up` remain convenient.

`dev_mode` is deliberately narrow:
- Permissive CORS
- Simulation pause/resume
- Diagnostic pending-order and market orderbook listings

Production deployments disable dev routes entirely and use restrictive,
env-configured CORS (`SYBIL_CORS_ORIGINS`, comma-separated; empty means
same-origin only).

## Architecture

```
ApiConfig (port, dev_mode, service_token, cors_origins, block_interval_ms)
    ↓
AppState (SequencerHandle + dev_mode + service_token + cors_origins)
    ↓
Axum Router (public/service/dev route tiers, CORS, tracing)
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
