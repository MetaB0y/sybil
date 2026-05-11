/**
 * Global app store (Zustand). One singleton; every component subscribes to
 * slices via selectors. Mutations are restricted to the dispatchers exported
 * below — the RealtimeProvider is the only thing that drives them.
 *
 * The store has no React/WS coupling here; the WS client lives in
 * `lib/ws/client.ts` and is piped into this store by `lib/ws/realtime-provider.tsx`.
 */

import { create } from "zustand";
import type { Block } from "../ws/types";
import type { ConnectionState } from "../ws/types";

const RECENT_BLOCKS_CAP = 20;

export type ConnectionSnapshot = {
  state: ConnectionState;
  lastSeenHeight: number | null;
};

export type StoreState = {
  /** WebSocket connection status. */
  connection: ConnectionSnapshot;

  /** Most recent committed block. */
  latestBlock: Block | null;

  /** Ring buffer of recent blocks (newest first), capped. */
  recentBlocks: Block[];

  /**
   * Latest clearing prices per market_id, as raw nanos arrays (strings).
   * For binary markets: [yes, no]. For multi-outcome: longer. Parse to
   * bigint at display time via lib/format/nanos.
   */
  pricesByMarketId: Record<number, string[]>;

  // ── Dispatchers ─────────────────────────────────────────────────────
  setConnection: (snapshot: ConnectionSnapshot) => void;
  applyBlock: (block: Block) => void;
  resetForFreshSnapshot: () => void;
};

export const useStore = create<StoreState>((set) => ({
  connection: { state: "idle", lastSeenHeight: null },
  latestBlock: null,
  recentBlocks: [],
  pricesByMarketId: {},

  setConnection: (snapshot) => set({ connection: snapshot }),

  applyBlock: (block) =>
    set((s) => {
      const recent = [block, ...s.recentBlocks].slice(0, RECENT_BLOCKS_CAP);
      const prices = { ...s.pricesByMarketId };
      if (block.clearing_prices_nanos) {
        for (const [key, vec] of Object.entries(block.clearing_prices_nanos)) {
          const id = Number(key);
          if (!Number.isFinite(id)) continue;
          prices[id] = vec;
        }
      }
      return {
        latestBlock: block,
        recentBlocks: recent,
        pricesByMarketId: prices,
      };
    }),

  // Called when a "block not found" reconnect happens — local view is stale.
  // For now we just clear the live caches; REST hydration (Milestone C) will
  // re-seed from snapshots before the WS resubscribes.
  resetForFreshSnapshot: () =>
    set({
      latestBlock: null,
      recentBlocks: [],
      pricesByMarketId: {},
    }),
}));

// ── Convenience selectors ───────────────────────────────────────────────
// These are not required (components can pass selectors inline), but they
// make common reads readable and consistent.

export const selectConnection = (s: StoreState) => s.connection;
export const selectLatestBlock = (s: StoreState) => s.latestBlock;
export const selectLatestHeight = (s: StoreState) =>
  s.latestBlock?.height ?? null;
export const selectRecentBlocks = (s: StoreState) => s.recentBlocks;
export const selectPricesByMarketId = (s: StoreState) => s.pricesByMarketId;
export const selectMarketCount = (s: StoreState) =>
  Object.keys(s.pricesByMarketId).length;
