/**
 * Hook for the batches table (last N batches, newest first).
 *
 * Source of truth = `recentBlocks` in the global store. On mount we do a
 * one-shot REST backfill: `GET /v1/blocks?limit=N` returns the server's
 * in-memory block ring (newest-first), clamped to whatever it actually holds.
 * We feed those into the store via `applyBlocks`; the table then shows however
 * many blocks exist instead of nothing.
 *
 * Why REST and not a WS replay handshake: the replay path is all-or-nothing —
 * asking for `latest - N` when the server's ring is shallower than N makes the
 * server close with `block not found` and we get zero history. REST clamps to
 * the ring depth and always returns what's available. The live tail (new
 * blocks) is owned by RealtimeProvider's singleton socket; `applyBlocks` /
 * `applyBlock` dedupe + sort by height, so the REST seed and live blocks merge
 * cleanly. We intentionally do NOT touch the stream here — the old code forced
 * a disconnect/reconnect on the shared singleton, which interrupted live data
 * for every other page too.
 */

"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { deriveBatchRow } from "./derive-batch";
import type { BatchRow } from "./types";
import { selectRecentBlocks, useStore } from "../store";
import { api } from "../api/client";

export type UseBatchesResult = {
  rows: BatchRow[];
  /** True only while the initial REST backfill is in flight and the table is
   *  still empty. */
  isBackfilling: boolean;
};

export function useBatches(limit = 60): UseBatchesResult {
  const recentBlocks = useStore(selectRecentBlocks);
  const [loading, setLoading] = useState(true);

  // One-shot backfill per mount. The server clamps `limit` to its ring depth,
  // so over-asking is safe and future-proofs the table if the ring grows.
  const backfilled = useRef(false);
  useEffect(() => {
    if (backfilled.current) return;
    backfilled.current = true;
    let cancelled = false;
    (async () => {
      try {
        const { data, error } = await api.GET("/v1/blocks", {
          params: { query: { limit } },
        });
        if (cancelled || error || !data || data.length === 0) return;
        useStore.getState().applyBlocks(data);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [limit]);

  const rows = useMemo(
    () => recentBlocks.slice(0, limit).map(deriveBatchRow),
    [recentBlocks, limit]
  );

  return {
    rows,
    isBackfilling: loading && recentBlocks.length === 0,
  };
}
