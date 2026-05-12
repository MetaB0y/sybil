/**
 * Hook for the batches table (last N batches, newest first).
 *
 * Source of truth = `recentBlocks` in the global store. On mount, if the
 * buffer doesn't already contain `limit` blocks, trigger a WS replay by
 * seeding the singleton stream back to `latestHeight - limit` and forcing
 * a reconnect. The server streams the historical blocks over the existing
 * socket; `applyBlock` writes each one into the store via the normal path.
 *
 * No REST per-height fetches on the happy path. REST is only used as a
 * fallback when the server returns `block not found` (replay window pruned
 * past our target) — handled inside BlockStream, which transitions to
 * `failed`; the UI shows a banner and we accept a partial table.
 */

"use client";

import { useEffect, useMemo, useRef } from "react";
import { deriveBatchRow } from "./derive-batch";
import type { BatchRow } from "./types";
import {
  selectLatestHeight,
  selectRecentBlocks,
  useStore,
} from "../store";
import { getBlockStream } from "../ws/client";

export type UseBatchesResult = {
  rows: BatchRow[];
  /** True while we know the buffer is shy of `limit`. */
  isBackfilling: boolean;
};

export function useBatches(limit = 60): UseBatchesResult {
  const recentBlocks = useStore(selectRecentBlocks);
  const latestHeight = useStore(selectLatestHeight);

  // Trigger the WS replay once per mount, after hydration has populated the
  // first block. We intentionally don't re-trigger when the buffer grows past
  // the limit — that's the success signal, not a reason to reconnect.
  const triggered = useRef(false);
  useEffect(() => {
    if (triggered.current) return;
    if (latestHeight == null) return;
    if (recentBlocks.length >= limit) {
      triggered.current = true;
      return;
    }
    triggered.current = true;
    const stream = getBlockStream();
    const target = Math.max(0, latestHeight - limit);
    stream.seedLastSeenHeight(target);
    // Force re-handshake. The singleton handles the reconnect; live consumers
    // (markets, market-detail) keep receiving blocks once replay catches up.
    stream.disconnect();
    stream.connect();
  }, [latestHeight, recentBlocks.length, limit]);

  const rows = useMemo(() => {
    return recentBlocks.slice(0, limit).map(deriveBatchRow);
  }, [recentBlocks, limit]);

  return {
    rows,
    isBackfilling:
      latestHeight != null && recentBlocks.length < limit,
  };
}
