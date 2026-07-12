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

import { useCallback, useEffect, useMemo, useState } from "react";
import { deriveBatchRow } from "./derive-batch";
import type { BatchRow } from "./types";
import { selectRecentBlocks, useStore } from "../store";
import { api } from "../api/client";

export type UseBatchesResult = {
  rows: BatchRow[];
  /** True only while the initial REST backfill is in flight and the table is
   *  still empty. */
  isBackfilling: boolean;
  isFetching: boolean;
  error: Error | null;
  retry: () => void;
};

export function useBatches(limit = 60): UseBatchesResult {
  const recentBlocks = useStore(selectRecentBlocks);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);
  const [attempt, setAttempt] = useState(0);

  // One initial backfill plus explicit retries. The server clamps `limit` to
  // its ring depth, so over-asking is safe and future-proofs the table.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const data = await fetchBatchBackfill(limit);
        if (cancelled) return;
        setError(null);
        if (data.length === 0) return;
        useStore.getState().applyBlocks(data);
      } catch (cause) {
        if (!cancelled) {
          setError(
            cause instanceof Error
              ? cause
              : new Error("/v1/blocks backfill failed"),
          );
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [limit, attempt]);

  const retry = useCallback(() => {
    setLoading(true);
    setAttempt((value) => value + 1);
  }, []);

  const rows = useMemo(
    () => recentBlocks.slice(0, limit).map(deriveBatchRow),
    [recentBlocks, limit]
  );

  return {
    rows,
    isBackfilling: loading && recentBlocks.length === 0,
    isFetching: loading,
    error,
    retry,
  };
}

export async function fetchBatchBackfill(limit: number) {
  const { data, error } = await api.GET("/v1/blocks", {
    params: { query: { limit } },
  });
  if (error || !data) throw new Error("/v1/blocks backfill failed");
  return data;
}
