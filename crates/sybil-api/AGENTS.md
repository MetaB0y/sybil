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

Do not maintain a hand-written endpoint list here. The source of truth is:

- Runtime schema: `GET /openapi.json`
- Mounted route tables in `src/app.rs`: `PUBLIC_ROUTE_TABLE`, `SERVICE_ROUTE_TABLE`, and `DEV_ROUTE_TABLE`
- Handler-level request/response docs in `src/routes/*.rs`

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
`matching-engine` payoff-vector helpers remain available for research and tests,
but spreads, bundles, and custom payoff vectors are not exposed through
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
