"use client";

import { useQueryClient } from "@tanstack/react-query";
import { useEffect, type ReactNode } from "react";
import { api } from "../api/client";
import { useStore } from "../store";
import { getBlockStream } from "./client";

/**
 * Min gap between REST stat-chip refreshes (vol / 24h / liq / traders). Blocks
 * can arrive in replay bursts; refreshing every event would hammer the API, so
 * we coalesce to at most one refetch per this window.
 */
const STAT_REFRESH_MS = 5000;

/**
 * Timestamp (perf clock) of the last stat-chip refresh, module-scoped so the
 * throttle holds across any number of listener registrations — React StrictMode
 * double-invokes effects in dev, and HMR can briefly stack them. The block
 * stream is a singleton too, so one shared gate is the right granularity.
 */
let lastStatRefresh = 0;

/**
 * Owns the singleton block-stream connection for the whole app.
 *
 * Lifecycle on mount:
 *   1. HYDRATE — fetch /v1/blocks/latest + /v1/markets/prices in parallel.
 *      The latest block gives us H₀ and any fresh clearing prices; the
 *      prices endpoint fills in the markets that didn't update last block.
 *   2. SEED — push hydration data into the store, seed the stream with H₀.
 *   3. CONNECT — open the WebSocket with `?from_block=H₀+1`. Server replays
 *      any blocks committed during hydration, then transitions to live.
 *
 * On unmount: detach event listeners and close the socket.
 */
export function RealtimeProvider({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient();

  useEffect(() => {
    const stream = getBlockStream();
    const { setConnection, setHydration, applyBlock, applyRestPrices } =
      useStore.getState();

    const offConnection = stream.on("connection", (event) => {
      setConnection({
        state: event.state,
        lastSeenHeight: stream.getLastSeenHeight(),
      });
    });

    const offBlock = stream.on("block", (event) => {
      applyBlock(event.block);
      // The stat chips (vol / 24h / liq / traders) live only in the REST market
      // queries, which the store never writes to — so without this they freeze
      // at first paint while the odds tick live. Nudge the mounted market
      // queries to refetch so every number tracks the batches instead. Only
      // ACTIVE observers refetch, so it's a no-op on pages not showing markets;
      // react-query also dedupes an in-flight refetch (e.g. a reconnect replay
      // burst). Coalesce to STAT_REFRESH_MS: these stats don't need sub-block
      // freshness, so we spare the API — especially
      // the full /v1/markets list on the home page — while staying "live". The
      // odds themselves still update every block via the store, unthrottled.
      const now = performance.now();
      if (now - lastStatRefresh >= STAT_REFRESH_MS) {
        lastStatRefresh = now;
        void queryClient.invalidateQueries({
          predicate: (q) => {
            const k = q.queryKey;
            // Home-page markets list: ["markets", "all"] — carries the stat
            // fields (vol / liq / traders / 24h) for every card.
            if (k[0] === "markets") return true;
            // Market-detail stat object: exactly ["market", id]. Deliberately
            // NOT its chart sub-queries (["market", id, "prices", …] /
            // [..., "candles", …] / [..., "history", "24h"]) — those already
            // move live off the WS store, so re-pulling them every few seconds
            // would only add API load and chart flicker.
            return k[0] === "market" && k.length === 2;
          },
        });
      }
    });

    let cancelled = false;

    (async () => {
      setHydration("hydrating");
      try {
        const [latestRes, pricesRes] = await Promise.all([
          api.GET("/v1/blocks/latest"),
          api.GET("/v1/markets/prices"),
        ]);
        if (cancelled) return;

        if (latestRes.error || !latestRes.data) {
          throw new Error("hydrate: /v1/blocks/latest failed");
        }
        if (pricesRes.error || !pricesRes.data) {
          throw new Error("hydrate: /v1/markets/prices failed");
        }

        // Order matters: prices first (fills the full set), then the latest
        // block overrides for whatever moved this batch.
        applyRestPrices(pricesRes.data.prices);
        applyBlock(latestRes.data);

        const h0 = latestRes.data.height;
        setHydration("hydrated", h0);
        stream.seedLastSeenHeight(h0);
        stream.connect();
      } catch (err) {
        if (cancelled) return;
        console.error("[realtime] hydration failed:", err);
        setHydration("error");
        // Best-effort: connect anyway with no from_block. We'll get live
        // blocks; missed-snapshot recovery is a future concern.
        stream.connect();
      }
    })();

    return () => {
      cancelled = true;
      offConnection();
      offBlock();
      stream.disconnect();
    };
    // `queryClient` is created once in <Providers> (useState) so its identity is
    // stable — listing it here satisfies exhaustive-deps without re-running the
    // effect and re-opening the socket.
  }, [queryClient]);

  return <>{children}</>;
}
