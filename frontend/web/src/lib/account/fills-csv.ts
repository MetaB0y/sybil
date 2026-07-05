"use client";

/**
 * Client-side CSV export of an account's fill history. No server change — the
 * rows come from the same durable fill events the Trades / History views render
 * (`filled` + `partial_fill` from `useAccountHistory`; the `/fills` endpoint is
 * empty in prod), and the download is a Blob minted in the browser.
 *
 * Units: shares via `unitsToShares` (1000 units = 1 share); prices and cash via
 * the nanos → dollars formatter. Raw units / nanos never reach the file.
 */

import type { components } from "@/lib/api/schema";
import { formatDollars } from "@/lib/format/nanos";
import { notionalNanos, unitsToShares } from "./quantity";
import type { HistoryEvent } from "./use-account-history";

type Market = components["schemas"]["MarketResponse"];

const HEADERS = [
  "Time (UTC)",
  "Block",
  "Market",
  "Order ID",
  "Side",
  "Outcome",
  "Shares",
  "Price ($)",
  "Value ($)",
  "Realized PnL ($)",
] as const;

/** Fixed-point nanos → a bare decimal dollar string (no `$`), e.g. `-12.3400`. */
function dollars(nanos: bigint): string {
  return formatDollars(nanos, { decimals: 4 }).replace("$", "");
}

/** Quote a CSV field when it contains a comma, quote, or newline (RFC 4180). */
function csvField(value: string): string {
  return /[",\n\r]/.test(value) ? `"${value.replace(/"/g, '""')}"` : value;
}

/**
 * Render an account's fill events as a CSV string (one row per fill, newest
 * first — the order the history feed already provides). Only `filled` and
 * `partial_fill` events are fills; everything else is dropped.
 */
export function fillsToCsv(
  events: HistoryEvent[],
  marketsById: Map<number, Market>,
): string {
  const lines: string[] = [HEADERS.join(",")];

  for (const e of events) {
    if (e.type !== "filled" && e.type !== "partial_fill") continue;

    const marketName =
      (e.marketId != null ? marketsById.get(e.marketId)?.name : undefined) ??
      (e.marketId != null ? `#${e.marketId}` : "");
    const shares =
      e.qty != null ? unitsToShares(e.qty).toString() : "";
    const price = e.priceNanos != null ? dollars(e.priceNanos) : "";
    const value =
      e.priceNanos != null && e.qty != null
        ? dollars(notionalNanos(e.priceNanos, e.qty))
        : "";
    // Realized PnL is only attached where a position was reduced/closed.
    const realized = e.realizedPnlNanos != null ? dollars(e.realizedPnlNanos) : "";

    const row = [
      new Date(e.timestampMs).toISOString(),
      String(e.blockHeight),
      marketName,
      e.orderId != null ? String(e.orderId) : "",
      e.side ?? "",
      e.outcome ?? "",
      shares,
      price,
      value,
      realized,
    ];
    lines.push(row.map((f) => csvField(f)).join(","));
  }

  return lines.join("\r\n");
}

/** Count of fill rows a CSV export would contain (for enabling the button). */
export function fillRowCount(events: HistoryEvent[]): number {
  let n = 0;
  for (const e of events) {
    if (e.type === "filled" || e.type === "partial_fill") n += 1;
  }
  return n;
}

/**
 * Trigger a browser download of `csv` as `filename`. Guards on `document` so it
 * is a no-op in a non-DOM (SSR / test) environment.
 */
export function downloadCsv(filename: string, csv: string): void {
  if (typeof document === "undefined") return;
  const blob = new Blob([csv], { type: "text/csv;charset=utf-8;" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.style.display = "none";
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}
