/**
 * Block cadence in ms. Mirrors the backend `SYBIL_BLOCK_INTERVAL_MS`
 * (docker-compose.yml). Single source of truth for batch countdown timers —
 * keep in sync if the backend cadence changes.
 */
export const BLOCK_INTERVAL_MS = 10_000;
