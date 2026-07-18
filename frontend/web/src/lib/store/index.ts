/**
 * Global app store (Zustand). One singleton; every component subscribes to
 * slices via selectors. Mutations are restricted to the dispatchers exported
 * below — the RealtimeProvider is the only thing that drives them.
 *
 * Data flow:
 *   REST hydration (/v1/blocks/latest + /v1/markets/prices) seeds the head,
 *   while one global /v1/blocks history bootstrap fills the bounded recent
 *   ring → BlockStream connects with from_block=H₀+1 → server replays any
 *   missed blocks → live blocks keep prices fresh.
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
/** State of the independent bounded recent-block history bootstrap. */
export type RecentHistoryState = "idle" | "loading" | "ready" | "error";

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
  /** Recent-block bootstrap state. This is independent from head hydration:
   *  the live stream may be useful even when historical reads are unavailable. */
  recentHistory: RecentHistoryState;

  /** Most recent committed block. */
  latestBlock: Block | null;
  /** `performance.now()` captured when `latestBlock` was received — a monotonic
   *  anchor for the batch countdown. Living in the store (not a component ref)
   *  means the countdown survives remounts, so switching pages/outcomes no
   *  longer restarts the timer mid-batch. Null until the first block arrives. */
  latestBlockAnchorPerf: number | null;
  /** Ring buffer of recent blocks (newest first), capped. */
  recentBlocks: Block[];
  /** Current YES / NO clearing price per market_id, in nanos as bigint. */
  pricesByMarketId: Record<number, MarketPrice>;

  // ── Dispatchers ─────────────────────────────────────────────────────
  setConnection: (snapshot: ConnectionSnapshot) => void;
  setHydration: (state: HydrationState, height?: number | null) => void;
  setRecentHistory: (state: RecentHistoryState) => void;
  /** Seed prices from /v1/markets/prices. Does not overwrite existing entries
   *  that were applied from a more recent block. */
  applyRestPrices: (
    prices: Record<string, { yes_price_nanos: string; no_price_nanos: string }>,
  ) => void;
  /** Apply a committed block: update latest, ring buffer, and any prices it
   *  carried in clearing_prices_nanos. */
  applyBlock: (block: Block) => void;
  /** Seed many blocks at once (the global REST recent-history bootstrap) in a
   *  single atomic update. Dedupes against the ring by height; only the newest
   *  incoming block may advance latest/prices/anchor, so history never
   *  regresses the live tip. */
  applyBlocks: (blocks: Block[]) => void;
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
  recentHistory: "idle",

  latestBlock: null,
  latestBlockAnchorPerf: null,
  recentBlocks: [],
  pricesByMarketId: {},

  setConnection: (snapshot) => set({ connection: snapshot }),

  setHydration: (state, height) =>
    set((s) => ({
      hydration: state,
      hydratedAtHeight: height ?? s.hydratedAtHeight,
    })),
  setRecentHistory: (recentHistory) => set({ recentHistory }),

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
      // Dedupe the ring buffer by height — a block can arrive twice if a
      // replay handshake re-streams something we already saw live. Keep the
      // most recent payload (the new one) and sort desc so the table never
      // has to reorder.
      const recent = [
        block,
        ...s.recentBlocks.filter((b) => b.height !== block.height),
      ]
        .sort((a, b) => b.height - a.height)
        .slice(0, RECENT_BLOCKS_CAP);

      // latestBlock and prices are monotonic: a replay block is older than
      // the live tip, so it must not regress these. The buffer above still
      // carries the replay block for the Activity table, but the rest of the
      // app sees a stable "newest" view.
      const isNewest =
        s.latestBlock == null || block.height >= s.latestBlock.height;
      const latestBlock = isNewest ? block : s.latestBlock;

      // Re-anchor the batch countdown only on a strictly newer height. A
      // replayed/duplicate block at the same height refreshes prices but must
      // NOT restart the timer.
      const isNewHeight =
        s.latestBlock == null || block.height > s.latestBlock.height;
      const latestBlockAnchorPerf = isNewHeight
        ? performance.now()
        : s.latestBlockAnchorPerf;

      let prices = s.pricesByMarketId;
      if (isNewest && block.clearing_prices_nanos) {
        prices = { ...prices };
        for (const [key, vec] of Object.entries(block.clearing_prices_nanos)) {
          const id = Number(key);
          if (!Number.isFinite(id)) continue;
          const parsed = parseClearingPair(vec);
          if (parsed) prices[id] = parsed;
        }
      }

      return {
        latestBlock,
        latestBlockAnchorPerf,
        recentBlocks: recent,
        pricesByMarketId: prices,
      };
    }),

  applyBlocks: (blocks) =>
    set((s) => {
      if (blocks.length === 0) return {};

      // Merge into the ring in one pass: dedupe by height (incoming wins),
      // newest-first, capped. Equivalent to calling applyBlock per block but
      // produces a single store snapshot / render instead of N intermediate ones.
      const byHeight = new Map<number, Block>();
      for (const b of s.recentBlocks) byHeight.set(b.height, b);
      for (const b of blocks) byHeight.set(b.height, b);
      const recent = [...byHeight.values()]
        .sort((a, b) => b.height - a.height)
        .slice(0, RECENT_BLOCKS_CAP);

      // Only the newest incoming block may move latest/prices/anchor — these
      // are usually historical backfill blocks older than the live tip.
      const newest = blocks.reduce((m, b) => (b.height > m.height ? b : m));
      const isNewest =
        s.latestBlock == null || newest.height >= s.latestBlock.height;
      const latestBlock = isNewest ? newest : s.latestBlock;
      const isNewHeight =
        s.latestBlock == null || newest.height > s.latestBlock.height;
      const latestBlockAnchorPerf = isNewHeight
        ? performance.now()
        : s.latestBlockAnchorPerf;

      let prices = s.pricesByMarketId;
      if (isNewest && newest.clearing_prices_nanos) {
        prices = { ...prices };
        for (const [key, vec] of Object.entries(newest.clearing_prices_nanos)) {
          const id = Number(key);
          if (!Number.isFinite(id)) continue;
          const parsed = parseClearingPair(vec);
          if (parsed) prices[id] = parsed;
        }
      }

      return {
        latestBlock,
        latestBlockAnchorPerf,
        recentBlocks: recent,
        pricesByMarketId: prices,
      };
    }),

  resetForFreshSnapshot: () =>
    set({
      latestBlock: null,
      latestBlockAnchorPerf: null,
      recentBlocks: [],
      pricesByMarketId: {},
      hydration: "idle",
      hydratedAtHeight: null,
      recentHistory: "idle",
    }),
}));

// ── Selectors ───────────────────────────────────────────────────────────

export const selectConnection = (s: StoreState) => s.connection;
/** True while the block stream is caught up (or catching up) — i.e. block
 *  invalidation is reliably driving per-batch refreshes. When false (idle /
 *  connecting / reconnecting / failed) consumers should fall back to polling. */
export const selectWsLive = (s: StoreState) =>
  s.connection.state === "live" || s.connection.state === "replaying";
export const selectHydration = (s: StoreState) => s.hydration;
export const selectHydratedAtHeight = (s: StoreState) => s.hydratedAtHeight;
export const selectRecentHistory = (s: StoreState) => s.recentHistory;
export const selectLatestBlock = (s: StoreState) => s.latestBlock;
export const selectLatestBlockAnchor = (s: StoreState) =>
  s.latestBlockAnchorPerf;
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
