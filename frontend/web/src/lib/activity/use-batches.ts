/**
 * Hook backing the paginated batches table.
 *
 * A page is a half-open height window `[lo, hi]` ending just below `head`.
 * `head` is the newest batch the table is willing to show: the chain tip when
 * the tail is live, or the height the user froze at. Page 0 is the newest
 * `pageSize` batches at or below `head`, page 1 the `pageSize` below that, and
 * so on — the window is derived from `head` rather than accumulated, so a page
 * never shifts under the reader while they're on it.
 *
 * Two sources, one shape:
 *
 *   1. `recentBlocks` in the global store (fed by the WS tail + the one-shot
 *      REST backfill below). Covers the newest ~80 heights, so the first pages
 *      resolve without a request and page 0 keeps tailing live.
 *   2. `GET /v1/blocks?limit&before_height` for anything older. Blocks are
 *      sealed and immutable, so those pages cache forever.
 *
 * Deep pages deliberately do NOT `applyBlocks` their results: the store is a
 * bounded newest-first ring, and pushing 2000-block-old history through it
 * would evict the live tail every other page turn.
 *
 * Why REST for the backfill and not a WS replay handshake: the replay path is
 * all-or-nothing — asking for `latest - N` when the server retains less makes
 * it close with `block not found` and we get zero history. REST clamps to what
 * exists. `applyBlocks` dedupes + sorts by height, so the REST seed and live
 * blocks merge cleanly. We never touch the stream here — the old code forced a
 * disconnect/reconnect on the shared singleton, interrupting every other page.
 */

"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { deriveBatchRow } from "./derive-batch";
import type { BatchRow, Block } from "./types";
import { selectRecentBlocks, useStore } from "../store";
import { api } from "../api/client";

/** First block the chain ever produces. There is no height 0. */
const GENESIS_HEIGHT = 1;

/** Seeds the store so the first pages — and row expansion — need no request. */
const BACKFILL_LIMIT = 60;

export type UseBatchPageArgs = {
  /** Newest height to show: the tip when live, the frozen height otherwise. */
  head: number | null;
  /** 0-based, counting backwards from `head`. */
  page: number;
  pageSize: number;
};

export type UseBatchPageResult = {
  /** Newest-first, up to `pageSize` long. */
  rows: BatchRow[];
  /** Nothing to render yet and something is in flight. */
  isLoading: boolean;
  /** There are batches below this page. */
  hasOlder: boolean;
};

export function useBatchPage({
  head,
  page,
  pageSize,
}: UseBatchPageArgs): UseBatchPageResult {
  const recentBlocks = useStore(selectRecentBlocks);
  const [backfilling, setBackfilling] = useState(true);

  // One-shot per mount. The server clamps `limit` to what it retains, so
  // over-asking is safe and future-proofs the table if retention grows.
  const backfilled = useRef(false);
  useEffect(() => {
    if (backfilled.current) return;
    backfilled.current = true;
    let cancelled = false;
    void (async () => {
      try {
        const { data, error } = await api.GET("/v1/blocks", {
          params: { query: { limit: BACKFILL_LIMIT } },
        });
        if (cancelled || error || !data || data.length === 0) return;
        useStore.getState().applyBlocks(data);
      } finally {
        if (!cancelled) setBackfilling(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Exclusive upper bound, i.e. what `before_height` means to the API.
  const beforeHeight = head == null ? null : head + 1 - page * pageSize;
  const hi = beforeHeight == null ? null : beforeHeight - 1;
  const lo =
    beforeHeight == null
      ? null
      : Math.max(GENESIS_HEIGHT, beforeHeight - pageSize);
  const expected = hi == null || lo == null ? 0 : Math.max(0, hi - lo + 1);

  const fromStore = useMemo(() => {
    if (hi == null || lo == null) return [];
    return recentBlocks.filter((b) => b.height >= lo && b.height <= hi);
  }, [recentBlocks, hi, lo]);

  // The store holds every height in the window, so skip the network entirely.
  const complete = expected > 0 && fromStore.length === expected;

  const pageQ = useQuery({
    queryKey: ["blocks", "page", beforeHeight, pageSize],
    queryFn: () => fetchBlockPage(beforeHeight as number, pageSize),
    enabled: beforeHeight != null && expected > 0 && !complete && !backfilling,
    staleTime: Infinity, // sealed blocks never change
  });

  const rows = useMemo(
    () => (complete ? fromStore : (pageQ.data ?? [])).map(deriveBatchRow),
    [complete, fromStore, pageQ.data],
  );

  const oldest = rows.length > 0 ? rows[rows.length - 1]?.height ?? null : null;

  return {
    rows,
    isLoading:
      rows.length === 0 &&
      (head == null ||
        backfilling ||
        (expected > 0 && !complete && pageQ.isFetching)),
    hasOlder: oldest != null && oldest > GENESIS_HEIGHT,
  };
}

async function fetchBlockPage(
  beforeHeight: number,
  limit: number,
): Promise<Block[]> {
  const { data, error } = await api.GET("/v1/blocks", {
    params: { query: { limit, before_height: beforeHeight } },
  });
  if (error || !data) throw new Error("/v1/blocks page failed");
  return data;
}
