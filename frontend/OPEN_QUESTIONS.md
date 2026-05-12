# Open Questions

Running list of frontend-related questions to discuss. Keep entries short.

1. **Card `liq` metric — store pre-clearing book state in backend?**
   Idea: avg depth on both sides within ±5¢ of mid over last 5 batches.
   Today: backend persists clearing prices + fills per block, but no resting orderbook snapshot per batch. Live `/orderbook` is dev-mode only and snapshot-only.
   Question: can we add per-block resting depth (price levels + sizes) to the backend so the frontend can compute this?

2. **Card `traders` metric — expose unique trader count?**
   Data exists: every fill carries `account_id`, so distinct-count per market is derivable.
   Missing: no `trader_count` field on `MarketResponse`, no `/v1/markets/{id}/fills` endpoint, no aggregate.
   Question: add a maintained `trader_count` (HashSet of account_ids per market, updated on fills) to `MarketResponse`?
