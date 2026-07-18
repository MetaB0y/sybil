"use client";

import { useQueryClient } from "@tanstack/react-query";
import { useEffect, type ReactNode } from "react";
import { api } from "../api/client";
import { useStore } from "../store";
import { getBlockStream } from "./client";
import { fetchRecentBlockHistory } from "./recent-block-history";

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
 *   1. HYDRATE — fetch /v1/blocks/latest + /v1/markets/prices in parallel,
 *      while independently bootstrapping the bounded recent-block ring.
 *      The latest block gives us H₀ and any fresh clearing prices; the prices
 *      endpoint fills in markets that didn't update last block. Recent history
 *      serves global trade surfaces and Activity without owning the live head.
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
    const {
      setConnection,
      setHydration,
      setRecentHistory,
      applyBlock,
      applyBlocks,
      applyRestPrices,
      resetForFreshSnapshot,
    } = useStore.getState();

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
    let recoveryInFlight = false;
    let snapshotGeneration = 0;

    const hydrateSnapshot = async (recovering: boolean) => {
      if (recovering) {
        if (recoveryInFlight) return;
        recoveryInFlight = true;
        resetForFreshSnapshot();
      }
      const generation = ++snapshotGeneration;
      setHydration("hydrating");
      setRecentHistory("loading");

      // Recent history is useful but not part of the critical WS handshake.
      // Fetch it independently so a history outage cannot prevent the live
      // head from connecting. The generation guard prevents an old request
      // from repopulating state after retention-gap recovery reset it.
      void fetchRecentBlockHistory()
        .then((blocks) => {
          if (cancelled || generation !== snapshotGeneration) return;
          applyBlocks(blocks);
          setRecentHistory("ready");
        })
        .catch((err: unknown) => {
          if (cancelled || generation !== snapshotGeneration) return;
          console.error("[realtime] recent history failed:", err);
          setRecentHistory("error");
        });

      try {
        const [latestRes, pricesRes] = await Promise.all([
          api.GET("/v1/blocks/latest"),
          api.GET("/v1/markets/prices"),
        ]);
        if (cancelled || generation !== snapshotGeneration) return;

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
        if (recovering) {
          stream.recoverFromSnapshot(h0);
          void queryClient.invalidateQueries();
        } else {
          stream.seedLastSeenHeight(h0);
          stream.connect();
        }
      } catch (err) {
        if (cancelled || generation !== snapshotGeneration) return;
        console.error("[realtime] hydration failed:", err);
        setHydration("error");
        if (!recovering) {
          // There is no stale local snapshot on initial mount, so a live-only
          // connection is still useful while REST is unavailable.
          stream.connect();
        }
      } finally {
        if (generation === snapshotGeneration) recoveryInFlight = false;
      }
    };

    const offRetentionGap = stream.on("retention-gap", () => {
      void hydrateSnapshot(true);
    });

    void hydrateSnapshot(false);

    return () => {
      cancelled = true;
      offConnection();
      offBlock();
      offRetentionGap();
      stream.disconnect();
    };
    // `queryClient` is created once in <Providers> (useState) so its identity is
    // stable — listing it here satisfies exhaustive-deps without re-running the
    // effect and re-opening the socket.
  }, [queryClient]);

  return <>{children}</>;
}
