"use client";

import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { api } from "../api/client";
import { useStore } from "../store";
import type { DevBlock } from "./types";

/**
 * Merge backfilled history with live blocks. `live` is applied last so a
 * live block overrides a backfilled one at the same height. Returns
 * ascending, deduped, capped to `window`.
 */
export function mergeBlocks(
  backfill: DevBlock[],
  live: DevBlock[],
  window: number
): DevBlock[] {
  const map = new Map<number, DevBlock>();
  for (const blk of backfill) if (blk && blk.height != null) map.set(blk.height, blk);
  for (const blk of live) if (blk && blk.height != null) map.set(blk.height, blk);
  return Array.from(map.values())
    .sort((a, z) => a.height - z.height)
    .slice(-window);
}

async function backfillBlocks(window: number): Promise<DevBlock[]> {
  const latestRes = await api.GET("/v1/blocks/latest");
  const latest = latestRes.data as DevBlock | undefined;
  if (!latest || latest.height == null) return [];
  const start = Math.max(0, latest.height - (window - 1));
  const heights: number[] = [];
  for (let h = start; h < latest.height; h++) heights.push(h);
  const rows = await Promise.all(
    heights.map((h) =>
      api
        .GET("/v1/blocks/{height}", { params: { path: { height: h } } })
        .then((r) => (r.data as DevBlock | undefined) ?? null)
        .catch(() => null)
    )
  );
  return [...rows.filter((r): r is DevBlock => r != null), latest];
}

export interface RecentBlocks {
  blocks: DevBlock[];
  latestBlock: DevBlock | null;
  isBackfilling: boolean;
}

/** Recent block window: one-time REST backfill + live store blocks. */
export function useDevRecentBlocks(window = 80): RecentBlocks {
  const backfillQ = useQuery({
    queryKey: ["dev", "blocks-backfill", window],
    queryFn: () => backfillBlocks(window),
    staleTime: Infinity,
  });

  // Store keeps recentBlocks newest-first; copy before handing to mergeBlocks.
  const liveBlocks = useStore((s) => s.recentBlocks) as unknown as DevBlock[];

  const blocks = useMemo(
    () => mergeBlocks(backfillQ.data ?? [], liveBlocks ?? [], window),
    [backfillQ.data, liveBlocks, window]
  );

  return {
    blocks,
    latestBlock: blocks[blocks.length - 1] ?? null,
    isBackfilling: backfillQ.isPending,
  };
}
