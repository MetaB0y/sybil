"use client";

/**
 * Unified portfolio history feed (event log) — the account's FULL history.
 *
 * Backed by `GET /v1/accounts/{id}/events` — a per-account, off-block event log
 * merging order lifecycle (placed / partial_fill / filled / cancelled /
 * expired / rejected), funding (created / deposit / withdrawal) and settlement
 * (resolved), newest-first. In prod this is served from the DURABLE, append-only
 * redb `history_events` table — uncapped and restart-safe; the in-memory ring is
 * 0 there and only a read-error fallback. See `frontend/DATA_MAP.md`.
 *
 * We walk the `before` cursor (`"<block>.<seq>"`, an event's `id`) to load the
 * account's ENTIRE history — not just the newest page. Fetching a single page
 * made the History count saturate at the page size and the Trades count (fills
 * within that page) *shrink* as `placed`/`rejected` events evicted older fills
 * from the window — even though no data was lost. `MAX_PAGES` bounds the walk;
 * `hasMore` reports when it tripped and `loadMore` extends it.
 *
 * Invalidated per block so fresh events appear as batches clear. The walk starts
 * from the newest end each block and returns a consistent snapshot (events that
 * arrive mid-walk are simply picked up next block). NOTE: this re-walks every
 * loaded page each block; if an account ever accumulates many pages, split this
 * into a live newest-page query plus a cached immutable backfill.
 */

import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api/client";
import type { components } from "@/lib/api/schema";
import { parseNanos } from "@/lib/format/nanos";
import { selectLatestBlock, useStore } from "@/lib/store";

type HistoryEventResponse = components["schemas"]["HistoryEventResponse"];

/** Per-page size for the cursor walk — the endpoint caps at 500. */
const HISTORY_PAGE = 500;
/**
 * Safety bound on the cursor walk: at most `HISTORY_PAGE * MAX_PAGES` events are
 * loaded per pass. `hasMore` is true when this trips; `loadMore` raises it.
 */
const MAX_PAGES = 25;

export type HistoryEventType =
  | "created"
  | "placed"
  | "partial_fill"
  | "filled"
  | "cancelled"
  | "expired"
  | "deposit"
  | "withdrawal"
  | "resolved"
  | "rejected";

export type HistoryCategory = "all" | "trades" | "funding" | "settlement";

export interface HistoryEvent {
  id: string;
  type: HistoryEventType;
  timestampMs: number;
  blockHeight: number;
  marketId?: number;
  orderId?: number;
  side?: "BUY" | "SELL";
  outcome?: "YES" | "NO";
  qty?: number;
  priceNanos?: bigint; // limit (placed) or fill price (fills)
  amountNanos?: bigint; // signed cash impact, nanos-dollars (+in / -out)
  realizedPnlNanos?: bigint; // filled / resolved
  payoutOutcome?: "YES" | "NO"; // resolved only
  reason?: string; // rejected only: reason code
  requiredNanos?: bigint; // rejected: balance/position
  availableNanos?: bigint; // rejected: balance/position
}

/** Which filter chip an event type falls under. */
export const CATEGORY_OF: Record<
  HistoryEventType,
  Exclude<HistoryCategory, "all">
> = {
  created: "funding",
  placed: "trades",
  partial_fill: "trades",
  filled: "trades",
  cancelled: "trades",
  expired: "trades",
  deposit: "funding",
  withdrawal: "funding",
  resolved: "settlement",
  rejected: "trades",
};

export interface AccountHistory {
  events: HistoryEvent[];
  isMock: boolean;
  // `hasMore` is true only when the cursor walk hit the `MAX_PAGES` safety cap
  // (i.e. older events exist beyond what we loaded); `loadMore` raises the cap.
  hasMore: boolean;
  loadMore: () => void;
}

export function useAccountHistory(accountId: number | null): AccountHistory {
  const qc = useQueryClient();
  const latest = useStore(selectLatestBlock);
  const [maxPages, setMaxPages] = useState(MAX_PAGES);

  useEffect(() => {
    if (accountId === null) return;
    qc.invalidateQueries({ queryKey: ["account", accountId, "history"] });
  }, [accountId, latest?.height, qc]);

  const q = useQuery({
    enabled: accountId !== null,
    queryKey: ["account", accountId, "history", { page: HISTORY_PAGE, maxPages }],
    queryFn: async (): Promise<{ events: HistoryEvent[]; truncated: boolean }> => {
      if (accountId === null) throw new Error("no account");
      // Walk the `before` cursor from the newest event to the oldest, paging in
      // 500s, so counts/lists reflect the account's whole history — not a window.
      const all: HistoryEvent[] = [];
      let before: string | undefined;
      let truncated = false;
      for (let page = 0; ; page += 1) {
        const { data, error } = await api.GET("/v1/accounts/{id}/events", {
          params: {
            path: { id: accountId },
            query: { limit: HISTORY_PAGE, ...(before ? { before } : {}) },
          },
        });
        if (error || !data) throw new Error("fetch account history failed");
        for (const r of data) all.push(mapEvent(r));
        if (data.length < HISTORY_PAGE) break; // reached the oldest event
        if (page + 1 >= maxPages) {
          truncated = true; // hit the safety cap; older events not loaded
          break;
        }
        // The oldest event of this page is the exclusive cursor for the next.
        const oldest = data[data.length - 1];
        if (!oldest) break;
        before = oldest.id;
      }
      return { events: all, truncated };
    },
    staleTime: 0,
    refetchOnWindowFocus: false,
  });

  const events = q.data?.events ?? [];
  return {
    events,
    isMock: false,
    hasMore: q.data?.truncated ?? false,
    loadMore: () => setMaxPages((p) => p + MAX_PAGES),
  };
}

/** Per-order fill aggregate: execution count + volume-weighted avg fill price. */
export interface OrderFillAgg {
  count: number;
  avgPriceNanos: bigint | null;
}

/**
 * Aggregate an account's `partial_fill` + `filled` history events by `order_id`
 * into a per-order fill count and volume-weighted average fill price. This is
 * the authoritative fill source for the UI: the `/v1/accounts/{id}/fills`
 * endpoint returns `[]` in prod (it reads an in-memory recorder), but these fill
 * events live in the durable, full history log loaded by `useAccountHistory`.
 * Used by Open Orders' "Avg fill" column and the hero trade count.
 */
export function fillAggByOrder(events: HistoryEvent[]): Map<number, OrderFillAgg> {
  const acc = new Map<number, { count: number; qty: bigint; cost: bigint }>();
  for (const e of events) {
    if (e.type !== "filled" && e.type !== "partial_fill") continue;
    if (e.orderId == null) continue;
    const cur = acc.get(e.orderId) ?? { count: 0, qty: 0n, cost: 0n };
    cur.count += 1;
    if (e.qty != null) {
      const q = BigInt(e.qty);
      cur.qty += q;
      if (e.priceNanos != null) cur.cost += q * e.priceNanos;
    }
    acc.set(e.orderId, cur);
  }
  const out = new Map<number, OrderFillAgg>();
  for (const [id, e] of acc) {
    out.set(id, {
      count: e.count,
      avgPriceNanos: e.qty > 0n ? e.cost / e.qty : null,
    });
  }
  return out;
}

/** Normalize a wire `HistoryEventResponse` into the FE `HistoryEvent` model. */
function mapEvent(r: HistoryEventResponse): HistoryEvent {
  const e: HistoryEvent = {
    id: r.id,
    type: r.type as HistoryEventType,
    timestampMs: r.timestamp_ms,
    blockHeight: r.block_height,
  };
  if (r.market_id != null) e.marketId = r.market_id;
  if (r.order_id != null) e.orderId = r.order_id;
  if (r.side != null) e.side = r.side as "BUY" | "SELL";
  if (r.outcome != null) e.outcome = r.outcome as "YES" | "NO";
  if (r.qty != null) e.qty = r.qty;
  if (r.price_nanos != null) e.priceNanos = parseNanos(r.price_nanos);
  if (r.amount_nanos != null) e.amountNanos = parseNanos(r.amount_nanos);
  if (r.realized_pnl_nanos != null)
    e.realizedPnlNanos = parseNanos(r.realized_pnl_nanos);
  if (r.payout_outcome != null) e.payoutOutcome = r.payout_outcome as "YES" | "NO";
  if (r.reason != null) e.reason = r.reason;
  if (r.required_nanos != null) e.requiredNanos = parseNanos(r.required_nanos);
  if (r.available_nanos != null) e.availableNanos = parseNanos(r.available_nanos);
  return e;
}
