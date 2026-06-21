"use client";

/**
 * Unified portfolio history feed (event log).
 *
 * Backed by `GET /v1/accounts/{id}/events` — a per-account, off-block event log
 * merging order lifecycle (placed / partial_fill / filled / cancelled /
 * expired), funding (created / deposit / withdrawal) and settlement (resolved),
 * newest-first. The log is in-memory and bounded (5k events/account); it resets
 * on backend restart, same as the equity curve and price chart.
 *
 * We fetch a single page (`HISTORY_PAGE`) and invalidate per block so fresh
 * events appear as batches clear. The endpoint also supports a `before` cursor
 * (`"<block>.<seq>"`, i.e. an event's `id`) for pagination, which a future
 * load-more can use; `hasMore` reports whether the page came back full.
 */

import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api/client";
import type { components } from "@/lib/api/schema";
import { parseNanos } from "@/lib/format/nanos";
import { selectLatestBlock, useStore } from "@/lib/store";

type HistoryEventResponse = components["schemas"]["HistoryEventResponse"];

/** Default page size — the endpoint caps at 500. */
const HISTORY_PAGE = 200;

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
  // Pagination stubs for the future load-more (the endpoint takes a `before`
  // cursor). `hasMore` is true when the first page came back full.
  hasMore: boolean;
  loadMore: () => void;
}

export function useAccountHistory(accountId: number | null): AccountHistory {
  const qc = useQueryClient();
  const latest = useStore(selectLatestBlock);

  useEffect(() => {
    if (accountId === null) return;
    qc.invalidateQueries({ queryKey: ["account", accountId, "history"] });
  }, [accountId, latest?.height, qc]);

  const q = useQuery({
    enabled: accountId !== null,
    queryKey: ["account", accountId, "history", { limit: HISTORY_PAGE }],
    queryFn: async (): Promise<HistoryEvent[]> => {
      if (accountId === null) throw new Error("no account");
      const { data, error } = await api.GET("/v1/accounts/{id}/events", {
        params: { path: { id: accountId }, query: { limit: HISTORY_PAGE } },
      });
      if (error || !data) throw new Error("fetch account history failed");
      return data.map(mapEvent);
    },
    staleTime: 0,
    refetchOnWindowFocus: false,
  });

  const events = q.data ?? [];
  return {
    events,
    isMock: false,
    hasMore: events.length >= HISTORY_PAGE,
    loadMore: () => {},
  };
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
