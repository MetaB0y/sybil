# AGENTS.md

This file provides guidance to Claude Code when working with code in this crate.

## Purpose

The **sybil-polymarket** crate mirrors Polymarket markets onto a Sybil instance and runs a reference-price market maker. It treats sybil-api as a blackbox — pure HTTP client, no sybil workspace crate imports.

## Architecture

Four tokio actors communicate via channels:

```
SyncActor ──→ FeedActor:  mpsc (new token subscriptions)
SyncActor ──→ MmActor:    mpsc (new market notifications)
FeedActor ──→ MmActor:    watch (real-time price snapshots)
ResolutionActor polls Gamma and resolves Sybil markets via service API calls
```

### SyncActor (`sync.rs`)
Polls Polymarket Gamma API for new events, creates corresponding markets/groups on Sybil, maintains the mapping store.

### FeedActor (`feed.rs`)
WebSocket connection to Polymarket CLOB for real-time prices. Falls back to REST polling. Publishes `PriceSnapshot` via watch channel.

### MmActor (`mm.rs`)
Listens to Sybil SSE block stream. Each block: reads latest Polymarket reference price, submits BuyYes + BuyNo as flash liquidity (mm_budget_nanos).

### ResolutionActor (`resolution.rs`)
Polls mirrored Polymarket markets for resolution status and submits signed/authorized Sybil resolutions when outcomes settle.

## Module Map

| Module | Purpose |
|--------|---------|
| `config.rs` | CLI + env configuration (clap) |
| `error.rs` | Crate-level error types |
| `mapping.rs` | Bidirectional Polymarket <-> Sybil ID mapping |
| `polymarket/types.rs` | Gamma event/market types, WS message types |
| `polymarket/gamma.rs` | Gamma REST client, CLOB midpoint client |
| `polymarket/ws.rs` | CLOB WebSocket price feed |
| `sybil/client.rs` | Sybil HTTP client + SSE streaming using `sybil-api-types` DTOs |
| `sync.rs` | SyncActor |
| `feed.rs` | FeedActor |
| `mm.rs` | MmActor |
| `resolution.rs` | ResolutionActor |
| `main.rs` | Orchestration + shutdown |

## Running

```bash
# Start sybil-api first
cargo run --release -p sybil-api -- --dev-mode --port 3000

# In another terminal
cargo run --release -p sybil-polymarket -- --sybil-url http://localhost:3000 --max-events 10
```

## Key Design Decisions

- **No sybil crate imports**: This crate is a standalone HTTP client. Sybil DTOs are duplicated in `sybil/types.rs`.
- **Watch channel for prices**: Single-writer (FeedActor), multiple-reader pattern. Always has the latest snapshot.
- **Flash liquidity**: MM submits all orders with `mm_budget_nanos` — the solver picks the welfare-optimal subset.
- **WebSocket reconnect**: Proactive reconnect every 15 minutes to preempt Polymarket's known 18-22 minute zombie connection bug.
- **Mapping persistence**: Optional JSON file. In-memory by default for dev.

## Polymarket API Notes

- Gamma API: `gamma-api.polymarket.com` — no auth needed, 4000 req/10s
- CLOB WebSocket: `wss://ws-subscriptions-clob.polymarket.com/ws/market` — no auth, needs PING every 10s
- CLOB REST: `clob.polymarket.com` — no auth, 9000 req/10s
- `outcomes`, `outcomePrices`, `clobTokenIds` fields are JSON strings inside JSON (double-parse)
- Token IDs are 77+ digit integers stored as strings
- NegRisk events = multi-outcome → map to Sybil MarketGroups

## Testing

```bash
cargo test -p sybil-polymarket
cargo clippy -p sybil-polymarket
```
