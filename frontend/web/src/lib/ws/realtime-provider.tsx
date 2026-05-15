"use client";

import { useEffect, type ReactNode } from "react";
import { api } from "../api/client";
import { useStore } from "../store";
import { getBlockStream } from "./client";

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
  useEffect(() => {
    const stream = getBlockStream();
    const {
      setConnection,
      setHydration,
      applyBlock,
      applyRestPrices,
    } = useStore.getState();

    const offConnection = stream.on("connection", (event) => {
      setConnection({
        state: event.state,
        lastSeenHeight: stream.getLastSeenHeight(),
      });
    });

    const offBlock = stream.on("block", (event) => {
      applyBlock(event.block);
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
  }, []);

  return <>{children}</>;
}
