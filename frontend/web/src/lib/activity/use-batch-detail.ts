/**
 * Hook for the expanded batch detail panel.
 *
 * Inputs:
 *   - `height` — the batch we're showing, or `null` when nothing is expanded
 *
 * Resolution order for the block:
 *   1. `recentBlocks` in the store (typical case — table rows come from there)
 *   2. React Query fallback against `GET /v1/blocks/{height}` (for heights
 *      pushed out of the buffer; also used for the previous-height delta when
 *      `current - 1` isn't in the buffer)
 *
 * Market titles + categories come from `/v1/markets`. We cache the lookup
 * map across renders so the deriver gets a stable callback.
 */

"use client";

import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "../api/client";
import { selectRecentBlocks, useStore } from "../store";
import { deriveBatchMarketRows } from "./derive-batch";
import type { BatchMarketRow, Block } from "./types";

export type UseBatchDetailResult = {
  block: Block | null;
  prev: Block | null;
  rows: BatchMarketRow[];
  isPending: boolean;
};

export function useBatchDetail(height: number | null): UseBatchDetailResult {
  const recentBlocks = useStore(selectRecentBlocks);

  const inStoreCurrent = useMemo(
    () =>
      height == null
        ? null
        : recentBlocks.find((b) => b.height === height) ?? null,
    [recentBlocks, height]
  );
  const inStorePrev = useMemo(
    () =>
      height == null
        ? null
        : recentBlocks.find((b) => b.height === height - 1) ?? null,
    [recentBlocks, height]
  );

  // Fallback for the current block: rare, only when the user expands a row
  // for a height that's already been evicted from the 80-slot buffer.
  const currentQ = useQuery({
    queryKey: ["block", height],
    queryFn: () => fetchBlock(height as number),
    enabled: height != null && !inStoreCurrent,
    staleTime: Infinity, // heights are immutable
  });

  const prevQ = useQuery({
    queryKey: ["block", height == null ? null : height - 1],
    queryFn: () => fetchBlock((height as number) - 1),
    enabled: height != null && height > 0 && !inStorePrev && !!inStoreCurrent,
    staleTime: Infinity,
  });

  const marketsQ = useQuery({
    queryKey: ["markets", "all"],
    queryFn: async () => {
      const { data, error } = await api.GET("/v1/markets");
      if (error || !data) throw new Error("/v1/markets failed");
      return data;
    },
    staleTime: 60_000,
  });

  const marketLookup = useMemo(() => {
    const map = new Map<number, { title: string; category: string | null }>();
    for (const m of marketsQ.data ?? []) {
      map.set(m.market_id, {
        // Per-market title (e.g. "Will X win…"), not the shared event title —
        // sibling markets in one event would otherwise read identically.
        title: m.name ?? m.event_title ?? `Market #${m.market_id}`,
        category:
          (m.categories && m.categories[0]) ??
          m.category ??
          null,
      });
    }
    return map;
  }, [marketsQ.data]);

  const block = inStoreCurrent ?? currentQ.data ?? null;
  const prev = inStorePrev ?? prevQ.data ?? null;

  const rows = useMemo(() => {
    if (!block) return [];
    return deriveBatchMarketRows(block, prev, (id) =>
      marketLookup.get(id) ?? { title: `Market #${id}`, category: null }
    );
  }, [block, prev, marketLookup]);

  return {
    block,
    prev,
    rows,
    // Only "pending" when we actually lack the data to render: no block yet, or
    // the markets lookup hasn't loaded. Note React Query reports `isPending` for
    // *disabled* queries too — `currentQ`/`prevQ` are disabled in the common
    // case (block already in the store), so we must not gate on them, or the
    // panel would show "loading…" forever. The prev block only affects the Δ
    // column, so it never blocks the table.
    isPending: !block || (marketsQ.isPending && !marketsQ.data),
  };
}

async function fetchBlock(height: number): Promise<Block> {
  const { data, error } = await api.GET("/v1/blocks/{height}", {
    params: { path: { height } },
  });
  if (error || !data) throw new Error(`/v1/blocks/${height} failed`);
  return data;
}
