import type { Block } from "../activity/types";
import { api } from "../api/client";

/**
 * Keep this below the store's 80-block cap so live blocks can arrive while the
 * request is in flight without immediately evicting the oldest bootstrap rows.
 */
export const RECENT_BLOCK_HISTORY_LIMIT = 60;

/** Fetch the newest sealed public blocks for all global recent-data surfaces. */
export async function fetchRecentBlockHistory(): Promise<Block[]> {
  const { data, error } = await api.GET("/v1/blocks", {
    params: { query: { limit: RECENT_BLOCK_HISTORY_LIMIT } },
  });
  if (error || !data) throw new Error("/v1/blocks recent history failed");
  return data;
}
