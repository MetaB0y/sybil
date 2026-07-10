/**
 * Block cadence in ms. Mirrors the backend `SYBIL_BLOCK_INTERVAL_MS`
 * (docker-compose.yml). Single source of truth for batch countdown timers —
 * keep in sync if the backend cadence changes.
 */
export const BLOCK_INTERVAL_MS = 10_000;

/**
 * Polling fallback cadence (ms) for account/portfolio/pending queries when the
 * block WebSocket is NOT live. Normally these refresh on each block-height
 * advance driven by the WS; if the socket stalls or is reconnecting, this
 * interval keeps Holdings / open orders / fills refreshing so on-chain fills
 * still surface instead of appearing to hang. Slower than the block cadence to
 * stay light while degraded.
 */
export const ACCOUNT_POLL_MS = 5_000;
