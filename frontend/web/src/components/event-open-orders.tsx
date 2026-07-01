"use client";

/**
 * EventOpenOrders — the connected user's resting orders for the markets in this
 * event. A trimmed sibling of the portfolio `OpenOrdersList`: scoped to the
 * event (the parent already filters to the event's market ids) and read-only
 * (no cancel button — the rail handles order management), so the rows stay
 * compact under the chart.
 *
 * Filled-vs-original mirrors `OpenOrdersList`: placed = `original_quantity`,
 * filled = placed − remaining (0 for pre-B8 orders that report
 * `original_quantity: 0`). Avg fill price is the WAC of the account's visible
 * fills for that order_id, same aggregation as the portfolio list.
 *
 * Every column is click-to-sort; default order is newest-first (the parent
 * pre-sorts by `created_at_ms` desc, preserved while no sort is picked).
 */

import { useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { cancelSignedOrder } from "@/lib/account/orders";
import {
  formatShareUnits,
  notionalNanos,
  notionalNanosCeil,
  priceNanosFromNotional,
} from "@/lib/account/quantity";
import type { AccountFill } from "@/lib/account/use-account-fills";
import type { AccountOrder } from "@/lib/account/use-account-orders";
import { formatAge, formatCentsPrecise, formatDollars, parseNanos } from "@/lib/format/nanos";
import { selectLatestBlock, useStore } from "@/lib/store";
import { Pager, usePaged } from "@/components/event-list-pager";
import { SidePill } from "@/components/portfolio/side-pill";
import { TifCell } from "@/components/portfolio/tif-cell";

/** WAC avg fill price (nanos) + fill count for one order. */
interface OrderFillAgg {
  count: number;
  avgPriceNanos: bigint | null;
}

/** An order with every sortable value derived once, plus its render inputs. */
interface OpenRow {
  order: AccountOrder;
  label: string;
  action: "BUY" | "SELL";
  outcome: string;
  placed: number;
  filled: number;
  limitNanos: bigint;
  /** Notional $ of the resting order = limit × remaining (nanos). */
  valueNanos: bigint;
  /** Avg fill price (WAC of visible fills) — already side-relative, like Limit. */
  avgPriceNanos: bigint | null;
  fillCount: number;
  expiresAtBlock: number;
  /** Wall-clock admit time (ms). 0 for pre-B8 orders that don't report it. */
  placedAtMs: number;
}

type SortKey =
  | "outcome"
  | "action"
  | "side"
  | "placed"
  | "limit"
  | "avgfill"
  | "value"
  | "created"
  | "tif";
type SortDir = "asc" | "desc";
type Sort = { key: SortKey; dir: SortDir };

const COLUMNS: { key: SortKey; label: string; align: "left" | "right" }[] = [
  { key: "outcome", label: "Outcome", align: "left" },
  { key: "action", label: "Action", align: "left" },
  { key: "side", label: "Side", align: "left" },
  { key: "placed", label: "Placed/Filled", align: "right" },
  { key: "limit", label: "Limit", align: "right" },
  { key: "avgfill", label: "Avg fill", align: "right" },
  { key: "value", label: "Value", align: "right" },
  { key: "created", label: "Created", align: "right" },
  { key: "tif", label: "TIF", align: "right" },
];

/** Text columns sort A→Z first; numeric columns sort high→low first. */
function nextSort(prev: Sort | null, key: SortKey): Sort {
  if (prev && prev.key === key) {
    return { key, dir: prev.dir === "asc" ? "desc" : "asc" };
  }
  const numeric =
    key === "placed" ||
    key === "limit" ||
    key === "avgfill" ||
    key === "value" ||
    key === "created" ||
    key === "tif";
  return { key, dir: numeric ? "desc" : "asc" };
}

function cmpBig(a: bigint, b: bigint): number {
  return a > b ? 1 : a < b ? -1 : 0;
}

/** Ascending comparison for a key; null avg-fill sorts lowest. */
function compareBy(a: OpenRow, b: OpenRow, key: SortKey): number {
  switch (key) {
    case "outcome":
      return a.label.localeCompare(b.label);
    case "action":
      return a.action.localeCompare(b.action);
    case "side":
      return a.outcome.localeCompare(b.outcome);
    case "placed":
      return (a.placed || a.order.remaining_quantity) - (b.placed || b.order.remaining_quantity);
    case "limit":
      return cmpBig(a.limitNanos, b.limitNanos);
    case "avgfill":
      if (a.avgPriceNanos == null && b.avgPriceNanos == null) return 0;
      if (a.avgPriceNanos == null) return -1;
      if (b.avgPriceNanos == null) return 1;
      return cmpBig(a.avgPriceNanos, b.avgPriceNanos);
    case "value":
      return cmpBig(a.valueNanos, b.valueNanos);
    case "created":
      // Older (smaller ms) sorts first ascending; unknown (0) sorts oldest.
      return a.placedAtMs - b.placedAtMs;
    case "tif":
      return a.expiresAtBlock - b.expiresAtBlock;
  }
}

export function EventOpenOrders({
  orders,
  fills,
  labelByMarket,
  accountId,
  publicKeyHex,
}: {
  /** Already filtered to this event's markets + sorted newest-first. */
  orders: AccountOrder[];
  fills: AccountFill[];
  /** market_id → short outcome label (same map EventHoldings builds). */
  labelByMarket: Map<number, string>;
  /** Connected account — these are always its own orders, so cancel is allowed. */
  accountId: number;
  publicKeyHex: string;
}) {
  const [sort, setSort] = useState<Sort | null>(null);

  // Aggregate visible fills by order_id → count + WAC price (mirrors
  // OpenOrdersList). Bounded by the fills window, so very old / heavily-filled
  // orders may undercount — fine for typical recent open orders.
  const fillsByOrder = useMemo(() => {
    const acc = new Map<number, { count: number; qty: bigint; cost: bigint }>();
    for (const f of fills) {
      const e = acc.get(f.order_id) ?? { count: 0, qty: 0n, cost: 0n };
      const qty = BigInt(f.fill_qty);
      const price = parseNanos(f.fill_price_nanos);
      e.count += 1;
      e.qty += qty;
      e.cost += notionalNanos(price, qty);
      acc.set(f.order_id, e);
    }
    const out = new Map<number, OrderFillAgg>();
    for (const [id, e] of acc) {
      out.set(id, { count: e.count, avgPriceNanos: priceNanosFromNotional(e.cost, e.qty) });
    }
    return out;
  }, [fills]);

  const rows = useMemo<OpenRow[]>(() => {
    const decorated = orders.map((o) => {
      const sideRaw = o.side.toLowerCase();
      const agg = fillsByOrder.get(o.order_id);
      const placed = o.original_quantity ?? 0;
      const outcome = sideRaw.includes("yes") ? "YES" : sideRaw.includes("no") ? "NO" : "";
      // agg.avgPriceNanos is already this side's own price (matches Limit).
      const avgPriceNanos = agg?.avgPriceNanos ?? null;
      const limitNanos = parseNanos(o.limit_price_nanos);
      return {
        order: o,
        label: labelByMarket.get(o.market_id) ?? `#${o.market_id}`,
        action: sideRaw.includes("buy") ? "BUY" : "SELL",
        outcome,
        placed,
        filled: placed > 0 ? Math.max(0, placed - o.remaining_quantity) : 0,
        limitNanos,
        valueNanos: notionalNanosCeil(limitNanos, o.remaining_quantity),
        avgPriceNanos,
        fillCount: agg?.count ?? 0,
        expiresAtBlock: o.expires_at_block,
        placedAtMs: o.created_at_ms && o.created_at_ms > 0 ? o.created_at_ms : 0,
      } satisfies OpenRow;
    });
    if (!sort) return decorated;
    const factor = sort.dir === "asc" ? 1 : -1;
    return [...decorated].sort((a, b) => compareBy(a, b, sort.key) * factor);
  }, [orders, fillsByOrder, labelByMarket, sort]);

  const paged = usePaged(rows);
  const qc = useQueryClient();
  // "Now" for the age column = latest committed block time (same reference the
  // chart uses), so the render stays deterministic — no Date.now() per row.
  const nowMs = useStore(selectLatestBlock)?.timestamp_ms ?? null;

  // Cancellation refresh: the resting-orders feed (useAccountOrders) self-
  // refetches per block, but invalidate immediately so the row drops as soon as
  // the cancel is acknowledged rather than on the next batch.
  function onCancelled() {
    qc.invalidateQueries({ queryKey: ["account", accountId, "orders"] });
    qc.invalidateQueries({ queryKey: ["account", accountId, "portfolio"] });
    qc.invalidateQueries({ queryKey: ["orders", "pending"] });
  }

  if (orders.length === 0) {
    return <Empty>No open orders for this event.</Empty>;
  }

  return (
    <div>
      <Row header>
        {COLUMNS.map((col) => (
          <HeaderCell
            key={col.key}
            col={col}
            sort={sort}
            onSort={() => {
              setSort((s) => nextSort(s, col.key));
              paged.setPage(0);
            }}
          />
        ))}
        <span />
      </Row>
      {paged.visible.map((r) => (
        <OrderRow
          key={r.order.order_id}
          row={r}
          nowMs={nowMs}
          accountId={accountId}
          publicKeyHex={publicKeyHex}
          onCancelled={onCancelled}
        />
      ))}
      <Pager paged={paged} />
    </div>
  );
}

function OrderRow({
  row,
  nowMs,
  accountId,
  publicKeyHex,
  onCancelled,
}: {
  row: OpenRow;
  nowMs: number | null;
  accountId: number;
  publicKeyHex: string;
  onCancelled: () => void;
}) {
  const {
    order,
    label,
    action,
    outcome,
    placed,
    filled,
    limitNanos,
    valueNanos,
    avgPriceNanos,
    fillCount,
    placedAtMs,
  } = row;
  const isBuy = action === "BUY";
  const [cancelling, setCancelling] = useState(false);
  const [cancelError, setCancelError] = useState<string | null>(null);

  async function onCancel() {
    setCancelError(null);
    setCancelling(true);
    try {
      await cancelSignedOrder({
        accountId,
        publicKeyHex,
        orderId: order.order_id,
        context: {
          marketId: order.market_id,
          side: order.side,
          qty: order.remaining_quantity,
          limitPriceNanos: String(order.limit_price_nanos),
        },
      });
      onCancelled();
    } catch (e) {
      setCancelError(e instanceof Error ? e.message : String(e));
      setCancelling(false);
    }
  }

  return (
    <Row>
      <span
        title={label}
        style={{
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          color: "var(--fg-1)",
          fontFamily: "var(--font-sans)",
          fontSize: 13,
        }}
      >
        {label}
      </span>
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 11,
          color: isBuy ? "var(--accent)" : "var(--no)",
          fontWeight: 600,
          letterSpacing: "var(--track-wide)",
        }}
      >
        {action}
      </span>
      <span>
        <SidePill outcome={outcome} />
      </span>
      <Right mono>
        {placed === 0 ? (
          <>{formatShareUnits(order.remaining_quantity)}</>
        ) : (
          <span
            title={`${formatShareUnits(filled)} filled of ${formatShareUnits(placed)} placed`}
          >
            {`${formatShareUnits(placed)} / ${formatShareUnits(filled)}`}
          </span>
        )}
      </Right>
      <Right mono>{formatCentsPrecise(limitNanos)}</Right>
      <Right mono>
        <AvgFillCell priceNanos={avgPriceNanos} count={fillCount} />
      </Right>
      <Right mono>{formatDollars(valueNanos, { decimals: 2 })}</Right>
      <Right mono>
        <CreatedCell placedAtMs={placedAtMs} nowMs={nowMs} />
      </Right>
      <Right>
        <TifCell expiresAtBlock={order.expires_at_block} />
      </Right>
      <Right>
        <button
          type="button"
          onClick={onCancel}
          disabled={cancelling}
          title={cancelError ?? "Cancel order"}
          style={{
            padding: "3px 8px",
            background: "transparent",
            border: "1px solid color-mix(in srgb, var(--no) 32%, transparent)",
            borderRadius: 3,
            color: "var(--no)",
            fontFamily: "var(--font-mono)",
            fontSize: 9.5,
            cursor: cancelling ? "not-allowed" : "pointer",
            textTransform: "uppercase",
            letterSpacing: "var(--track-wide)",
            opacity: cancelling ? 0.6 : 1,
          }}
        >
          {cancelling ? "…" : "Cancel"}
        </button>
      </Right>
    </Row>
  );
}

/** When the order was created — shown as time-since ("5m ago", "2h ago") vs the
 *  latest block time. Unknown (pre-B8 orders without created_at_ms) or no block
 *  yet → "—". */
function CreatedCell({ placedAtMs, nowMs }: { placedAtMs: number; nowMs: number | null }) {
  if (placedAtMs <= 0 || nowMs == null) {
    return <span style={{ color: "var(--fg-4)" }}>—</span>;
  }
  return (
    <span title={new Date(placedAtMs).toLocaleString()}>
      {formatAge(nowMs - placedAtMs)} ago
    </span>
  );
}

/** Avg fill price (WAC, side-adjusted) with fill count beneath. */
function AvgFillCell({ priceNanos, count }: { priceNanos: bigint | null; count: number }) {
  return (
    <span
      style={{
        display: "inline-flex",
        flexDirection: "column",
        alignItems: "flex-end",
        gap: 1,
        fontFamily: "var(--font-mono)",
      }}
    >
      <span style={{ fontSize: 12, color: count > 0 ? "var(--fg-1)" : "var(--fg-3)" }}>
        {priceNanos != null ? formatCentsPrecise(priceNanos) : "—"}
      </span>
      <span
        style={{
          fontSize: 9.5,
          color: "var(--fg-4)",
          letterSpacing: "var(--track-wide)",
        }}
      >
        {count === 1 ? "1 fill" : `${count} fills`}
      </span>
    </span>
  );
}

function HeaderCell({
  col,
  sort,
  onSort,
}: {
  col: (typeof COLUMNS)[number];
  sort: Sort | null;
  onSort: () => void;
}) {
  const active = sort?.key === col.key;
  return (
    <button
      type="button"
      onClick={onSort}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 4,
        width: "100%",
        justifyContent: col.align === "right" ? "flex-end" : "flex-start",
        padding: 0,
        border: 0,
        background: "transparent",
        cursor: "pointer",
        font: "inherit",
        textTransform: "uppercase",
        letterSpacing: "var(--track-wide)",
        color: active ? "var(--fg-2)" : "var(--fg-4)",
      }}
      title={`Sort by ${col.label}`}
    >
      <span style={{ whiteSpace: "nowrap" }}>{col.label}</span>
      <span style={{ fontSize: 8, lineHeight: 1, opacity: active ? 1 : 0.3 }}>
        {active ? (sort!.dir === "asc" ? "▲" : "▼") : "↕"}
      </span>
    </button>
  );
}

function Row({
  children,
  header,
}: {
  children: React.ReactNode;
  header?: boolean;
}) {
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns:
          "minmax(0, 1fr) 56px 48px 108px 56px 74px 78px 70px 76px 62px",
        gap: 14,
        alignItems: "center",
        padding: "9px 0",
        borderTop: header ? undefined : "1px solid var(--border-1)",
        fontFamily: "var(--font-mono)",
        fontSize: header ? 10 : 11,
        letterSpacing: "var(--track-wide)",
        textTransform: header ? "uppercase" : undefined,
        color: header ? "var(--fg-4)" : "var(--fg-2)",
      }}
    >
      {children}
    </div>
  );
}

function Right({
  children,
  mono,
}: {
  children: React.ReactNode;
  mono?: boolean;
}) {
  return (
    <span
      style={{
        textAlign: "right",
        whiteSpace: "nowrap",
        fontFamily: mono ? "var(--font-mono)" : "inherit",
        fontSize: mono ? 12 : undefined,
        color: mono ? "var(--fg-1)" : undefined,
      }}
    >
      {children}
    </span>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        padding: "24px 0",
        color: "var(--fg-4)",
        fontFamily: "var(--font-mono)",
        fontSize: 12,
        letterSpacing: "var(--track-wide)",
        textAlign: "center",
      }}
    >
      {children}
    </div>
  );
}
