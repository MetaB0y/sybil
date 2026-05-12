/**
 * Global app store (Zustand). One singleton; every component subscribes to
 * slices via selectors. Mutations are restricted to the dispatchers exported
 * below — the RealtimeProvider is the only thing that drives them.
 *
 * Data flow:
 *   REST hydration (/v1/blocks/latest + /v1/markets/prices) seeds the store
 *   on mount → BlockStream connects with from_block=H₀+1 → server replays
 *   any missed blocks → live blocks keep prices fresh.
 */

import { create } from "zustand";
import { parseNanos } from "../format/nanos";
import type { Block, ConnectionState } from "../ws/types";

// Cap sized for the Activity page (60-row batches table + 20 headroom for
// live-prepend churn). Memory cost: ~80 × ~50–200 KB per block on busy
// networks → 30–80 MB steady-state. Acceptable on laptop; watch on phone.
const RECENT_BLOCKS_CAP = 80;

/** Hydration phases for the initial REST snapshot. */
export type HydrationState = "idle" | "hydrating" | "hydrated" | "error";

export type MarketPrice = {
  yes: bigint;
  no: bigint;
};

export type ConnectionSnapshot = {
  state: ConnectionState;
  lastSeenHeight: number | null;
};

export type StoreState = {
  /** WebSocket connection status. */
  connection: ConnectionSnapshot;
  /** REST hydration status (gates the WS handshake). */
  hydration: HydrationState;
  /** H₀ — the height captured from REST before subscribing to WS. */
  hydratedAtHeight: number | null;

  /** Most recent committed block. */
  latestBlock: Block | null;
  /** Ring buffer of recent blocks (newest first), capped. */
  recentBlocks: Block[];
  /** Current YES / NO clearing price per market_id, in nanos as bigint. */
  pricesByMarketId: Record<number, MarketPrice>;

  // ── Dispatchers ─────────────────────────────────────────────────────
  setConnection: (snapshot: ConnectionSnapshot) => void;
  setHydration: (state: HydrationState, height?: number | null) => void;
  /** Seed prices from /v1/markets/prices. Does not overwrite existing entries
   *  that were applied from a more recent block. */
  applyRestPrices: (
    prices: Record<
      string,
      { yes_price_nanos: string; no_price_nanos: string }
    >
  ) => void;
  /** Apply a committed block: update latest, ring buffer, and any prices it
   *  carried in clearing_prices_nanos. */
  applyBlock: (block: Block) => void;
  /** Wipe live caches; used when the WS reports "block not found" and we
   *  need to re-hydrate from REST. */
  resetForFreshSnapshot: () => void;
};

function parseClearingPair(arr: string[] | undefined): MarketPrice | null {
  if (!arr) return null;
  const yesStr = arr[0];
  const noStr = arr[1];
  if (yesStr == null || noStr == null) return null;
  return { yes: parseNanos(yesStr), no: parseNanos(noStr) };
}

export const useStore = create<StoreState>((set) => ({
  connection: { state: "idle", lastSeenHeight: null },
  hydration: "idle",
  hydratedAtHeight: null,

  latestBlock: null,
  recentBlocks: [],
  pricesByMarketId: {},

  setConnection: (snapshot) => set({ connection: snapshot }),

  setHydration: (state, height) =>
    set((s) => ({
      hydration: state,
      hydratedAtHeight: height ?? s.hydratedAtHeight,
    })),

  applyRestPrices: (rest) =>
    set((s) => {
      const next: Record<number, MarketPrice> = { ...s.pricesByMarketId };
      for (const [key, value] of Object.entries(rest)) {
        const id = Number(key);
        if (!Number.isFinite(id)) continue;
        // Don't clobber a fresher price already set from a block.
        if (next[id]) continue;
        next[id] = {
          yes: parseNanos(value.yes_price_nanos),
          no: parseNanos(value.no_price_nanos),
        };
      }
      return { pricesByMarketId: next };
    }),

  applyBlock: (block) =>
    set((s) => {
      const recent = [block, ...s.recentBlocks].slice(0, RECENT_BLOCKS_CAP);
      const prices = { ...s.pricesByMarketId };
      if (block.clearing_prices_nanos) {
        for (const [key, vec] of Object.entries(block.clearing_prices_nanos)) {
          const id = Number(key);
          if (!Number.isFinite(id)) continue;
          const parsed = parseClearingPair(vec);
          if (parsed) prices[id] = parsed;
        }
      }
      return {
        latestBlock: block,
        recentBlocks: recent,
        pricesByMarketId: prices,
      };
    }),

  resetForFreshSnapshot: () =>
    set({
      latestBlock: null,
      recentBlocks: [],
      pricesByMarketId: {},
      hydration: "idle",
      hydratedAtHeight: null,
    }),
}));

// ── Selectors ───────────────────────────────────────────────────────────

export const selectConnection = (s: StoreState) => s.connection;
export const selectHydration = (s: StoreState) => s.hydration;
export const selectHydratedAtHeight = (s: StoreState) => s.hydratedAtHeight;
export const selectLatestBlock = (s: StoreState) => s.latestBlock;
export const selectLatestHeight = (s: StoreState) =>
  s.latestBlock?.height ?? null;
export const selectRecentBlocks = (s: StoreState) => s.recentBlocks;
export const selectPricesByMarketId = (s: StoreState) => s.pricesByMarketId;
export const selectMarketCount = (s: StoreState) =>
  Object.keys(s.pricesByMarketId).length;
export const selectMarketPrice =
  (marketId: number) =>
  (s: StoreState): MarketPrice | undefined =>
    s.pricesByMarketId[marketId];
