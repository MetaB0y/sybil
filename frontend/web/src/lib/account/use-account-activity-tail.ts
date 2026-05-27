"use client";

/**
 * Live per-account activity tail derived from the WebSocket block stream.
 *
 * Pure `useMemo` over `selectRecentBlocks` — no setState-in-effect, no
 * second WS subscription. The store's `recentBlocks` ring buffer is
 * already populated by the singleton RealtimeProvider.
 *
 * Each emitted event row carries:
 *   - source block height + timestamp
 *   - kind ("fill" | "rejection" | "system")
 *   - the raw payload, narrowed enough that the dev page can render it
 */

import { useMemo } from "react";
import type { components } from "@/lib/api/schema";
import { selectRecentBlocks, useStore } from "@/lib/store";

export type ActivityKind = "fill" | "rejection" | "system";

export interface ActivityEvent {
  blockHeight: number;
  timestampMs: number;
  kind: ActivityKind;
  payload: AnyEventPayload;
}

type AnyEventPayload =
  | components["schemas"]["FillResponse"]
  | components["schemas"]["RejectionResponse"]
  | components["schemas"]["SystemEventResponse"];

/**
 * Filter the in-store recent blocks for events touching `accountId`. Newest
 * first. Returns at most `limit` rows (default 50).
 */
export function useAccountActivityTail(
  accountId: number | null,
  limit = 50,
): ActivityEvent[] {
  const blocks = useStore(selectRecentBlocks);

  return useMemo(() => {
    if (accountId === null) return [];
    const out: ActivityEvent[] = [];

    // recentBlocks is appended in arrival order; iterate newest → oldest.
    for (let i = blocks.length - 1; i >= 0 && out.length < limit; i--) {
      const b = blocks[i];
      if (!b) continue;
      const blockHeight = b.height;
      const timestampMs = b.timestamp_ms;

      for (const f of b.fills ?? []) {
        if (f.account_id !== accountId) continue;
        out.push({ blockHeight, timestampMs, kind: "fill", payload: f });
        if (out.length >= limit) break;
      }
      if (out.length >= limit) break;

      for (const r of b.rejections ?? []) {
        if (r.account_id !== accountId) continue;
        out.push({ blockHeight, timestampMs, kind: "rejection", payload: r });
        if (out.length >= limit) break;
      }
      if (out.length >= limit) break;

      for (const e of b.system_events ?? []) {
        const touches = eventTouchesAccount(e, accountId);
        if (!touches) continue;
        out.push({ blockHeight, timestampMs, kind: "system", payload: e });
        if (out.length >= limit) break;
      }
    }
    return out;
  }, [blocks, accountId, limit]);
}

function eventTouchesAccount(
  e: components["schemas"]["SystemEventResponse"],
  accountId: number,
): boolean {
  // SystemEventResponse is a discriminated union: most variants have
  // `account_id`; MarketResolved instead has `affected_accounts: number[]`.
  if ("account_id" in e && e.account_id === accountId) return true;
  if (
    "affected_accounts" in e &&
    Array.isArray(e.affected_accounts) &&
    e.affected_accounts.includes(accountId)
  ) {
    return true;
  }
  return false;
}
