"use client";

/**
 * EventClosedOrders — terminal (filled / cancelled / expired) orders for this
 * event's markets, reconstructed from the account history feed and grouped by
 * `order_id`.
 *
 * Why reconstruct from history rather than a dedicated endpoint: the open-orders
 * feed only carries *resting* orders, so once an order fills/cancels/expires it
 * drops out of there. The history log (`useAccountHistory`) keeps the lifecycle
 * events, so we fold them per order_id:
 *   - status = the chronologically-last terminal event's type
 *     (filled → FILLED, cancelled → CANCELLED, expired → EXPIRED).
 *   - qty = that terminal event's qty (filled count / returned / expired count);
 *     `partial_fill` events are only used to surface a price when the terminal
 *     event lacks one.
 *   - price = the terminal event's price, else the last fill price seen.
 *   - value = qty × price (notional $), shown for orders that carry both.
 *   - P&L = realized PnL summed across the order's fill events
 *     (`partial_fill` + `filled`), shown for SELL orders only — a buy opens or
 *     adds to a position and realizes nothing. The backend supplies it per fill
 *     (`realized_pnl_nanos`), so no FE cost-basis math is needed.
 * Default order is newest-first by the terminal event's `timestamp_ms`; every
 * column is click-to-sort.
 *
 * Failed/rejected orders never reach the history log (product decision — not
 * persisted), so they're naturally excluded.
 */

import { useMemo, useState } from "react";
import type { HistoryEvent } from "@/lib/account/use-account-history";
import { formatCents, formatDollars } from "@/lib/format/nanos";
import { Pager, usePaged } from "@/components/event-list-pager";
import { SidePill } from "@/components/portfolio/side-pill";

type Status = "FILLED" | "CANCELLED" | "EXPIRED";

/** The terminal history event types that close an order. */
const TERMINAL = new Set<HistoryEvent["type"]>(["filled", "cancelled", "expired"]);

const STATUS_OF: Record<"filled" | "cancelled" | "expired", Status> = {
  filled: "FILLED",
  cancelled: "CANCELLED",
  expired: "EXPIRED",
};

interface ClosedOrder {
  orderId: number;
  marketId: number;
  label: string;
  status: Status;
  closedAtMs: number;
  side?: "BUY" | "SELL";
  outcome?: "YES" | "NO";
  qty: number | null;
  priceNanos: bigint | null;
  /** qty × price (notional $), or null when either is unknown. */
  valueNanos: bigint | null;
  /** Realized PnL (nanos) summed over fill events — SELL orders only, else null. */
  realizedPnlNanos: bigint | null;
}

type SortKey =
  | "outcome"
  | "action"
  | "side"
  | "status"
  | "qty"
  | "price"
  | "value"
  | "pnl"
  | "closed";
type SortDir = "asc" | "desc";
type Sort = { key: SortKey; dir: SortDir };

const COLUMNS: { key: SortKey; label: string; align: "left" | "right" }[] = [
  { key: "outcome", label: "Outcome", align: "left" },
  { key: "action", label: "Action", align: "left" },
  { key: "side", label: "Side", align: "left" },
  { key: "status", label: "Status", align: "right" },
  { key: "qty", label: "Qty", align: "right" },
  { key: "price", label: "Price", align: "right" },
  { key: "value", label: "Value", align: "right" },
  { key: "pnl", label: "P&L", align: "right" },
  { key: "closed", label: "Closed", align: "right" },
];

/** Text columns sort A→Z first; numeric columns sort high→low first. */
function nextSort(prev: Sort | null, key: SortKey): Sort {
  if (prev && prev.key === key) {
    return { key, dir: prev.dir === "asc" ? "desc" : "asc" };
  }
  const numeric =
    key === "qty" ||
    key === "price" ||
    key === "value" ||
    key === "pnl" ||
    key === "closed";
  return { key, dir: numeric ? "desc" : "asc" };
}

function cmpBig(a: bigint, b: bigint): number {
  return a > b ? 1 : a < b ? -1 : 0;
}

/** Ascending comparison for a key; null numbers sort lowest. */
function compareBy(a: ClosedOrder, b: ClosedOrder, key: SortKey): number {
  switch (key) {
    case "outcome":
      return a.label.localeCompare(b.label);
    case "action":
      return (a.side ?? "").localeCompare(b.side ?? "");
    case "side":
      return (a.outcome ?? "").localeCompare(b.outcome ?? "");
    case "status":
      return a.status.localeCompare(b.status);
    case "qty":
      return (a.qty ?? -1) - (b.qty ?? -1);
    case "price":
      if (a.priceNanos == null && b.priceNanos == null) return 0;
      if (a.priceNanos == null) return -1;
      if (b.priceNanos == null) return 1;
      return cmpBig(a.priceNanos, b.priceNanos);
    case "value":
      if (a.valueNanos == null && b.valueNanos == null) return 0;
      if (a.valueNanos == null) return -1;
      if (b.valueNanos == null) return 1;
      return cmpBig(a.valueNanos, b.valueNanos);
    case "pnl":
      if (a.realizedPnlNanos == null && b.realizedPnlNanos == null) return 0;
      if (a.realizedPnlNanos == null) return -1;
      if (b.realizedPnlNanos == null) return 1;
      return cmpBig(a.realizedPnlNanos, b.realizedPnlNanos);
    case "closed":
      return a.closedAtMs - b.closedAtMs;
  }
}

export function EventClosedOrders({
  events,
  labelByMarket,
}: {
  /** Full account history feed (unfiltered). */
  events: HistoryEvent[];
  /** market_id → short outcome label (same map EventHoldings builds). */
  labelByMarket: Map<number, string>;
}) {
  const [sort, setSort] = useState<Sort | null>(null);

  const closed = useMemo<ClosedOrder[]>(() => {
    const eventMarketIds = new Set(labelByMarket.keys());
    // Fold order lifecycle events per order_id, scoped to this event's markets.
    // We track the latest terminal event (sets status/qty/close-time) plus a
    // last-seen price fallback from partial fills.
    const byOrder = new Map<
      number,
      {
        marketId: number;
        side?: "BUY" | "SELL";
        outcome?: "YES" | "NO";
        terminal: HistoryEvent | null;
        lastFillPrice: bigint | null;
        /** Σ realized PnL over the order's fill events; null until one carries it. */
        realizedPnl: bigint | null;
      }
    >();

    for (const e of events) {
      if (e.orderId == null || e.marketId == null) continue;
      if (!eventMarketIds.has(e.marketId)) continue;
      if (e.type !== "partial_fill" && !TERMINAL.has(e.type)) continue;

      const slot =
        byOrder.get(e.orderId) ??
        ({
          marketId: e.marketId,
          terminal: null,
          lastFillPrice: null,
          realizedPnl: null,
        } as {
          marketId: number;
          side?: "BUY" | "SELL";
          outcome?: "YES" | "NO";
          terminal: HistoryEvent | null;
          lastFillPrice: bigint | null;
          realizedPnl: bigint | null;
        });

      // Carry side/outcome from whichever event has them (fills usually do).
      if (e.side && slot.side == null) slot.side = e.side;
      if (e.outcome && slot.outcome == null) slot.outcome = e.outcome;
      if (e.type === "partial_fill" && e.priceNanos != null) {
        slot.lastFillPrice = e.priceNanos;
      }
      // Accumulate realized PnL across every fill event (partial + final), so a
      // sell that filled in pieces — even one later cancelled — totals correctly.
      if (
        (e.type === "partial_fill" || e.type === "filled") &&
        e.realizedPnlNanos != null
      ) {
        slot.realizedPnl = (slot.realizedPnl ?? 0n) + e.realizedPnlNanos;
      }
      if (
        TERMINAL.has(e.type) &&
        (slot.terminal == null || e.timestampMs >= slot.terminal.timestampMs)
      ) {
        slot.terminal = e;
      }
      byOrder.set(e.orderId, slot);
    }

    const rows: ClosedOrder[] = [];
    for (const [orderId, slot] of byOrder) {
      const t = slot.terminal;
      if (t == null) continue; // only partial fills so far — still resting
      const qty = t.qty ?? null;
      const priceNanos = t.priceNanos ?? slot.lastFillPrice;
      const row: ClosedOrder = {
        orderId,
        marketId: slot.marketId,
        label: labelByMarket.get(slot.marketId) ?? `#${slot.marketId}`,
        status: STATUS_OF[t.type as "filled" | "cancelled" | "expired"],
        closedAtMs: t.timestampMs,
        qty,
        priceNanos,
        valueNanos: qty != null && priceNanos != null ? BigInt(qty) * priceNanos : null,
        // PnL is realized on the closing trade — show it for sells only.
        realizedPnlNanos: slot.side === "SELL" ? slot.realizedPnl : null,
      };
      if (slot.side) row.side = slot.side;
      if (slot.outcome) row.outcome = slot.outcome;
      rows.push(row);
    }
    rows.sort((a, b) => b.closedAtMs - a.closedAtMs);

    if (!sort) return rows;
    const factor = sort.dir === "asc" ? 1 : -1;
    return rows.sort((a, b) => compareBy(a, b, sort.key) * factor);
  }, [events, labelByMarket, sort]);

  const paged = usePaged(closed);

  if (closed.length === 0) {
    return <Empty>No closed orders for this event.</Empty>;
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
      </Row>
      {paged.visible.map((o) => (
        <ClosedRow key={o.orderId} order={o} />
      ))}
      <Pager paged={paged} />
    </div>
  );
}

function ClosedRow({ order }: { order: ClosedOrder }) {
  const isBuy = order.side === "BUY";
  const isSell = order.side === "SELL";
  return (
    <Row>
      <span
        title={order.label}
        style={{
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          color: "var(--fg-1)",
          fontFamily: "var(--font-sans)",
          fontSize: 13,
        }}
      >
        {order.label}
      </span>
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 11,
          color: isBuy ? "var(--accent)" : isSell ? "var(--no)" : "var(--fg-4)",
          fontWeight: 600,
          letterSpacing: "var(--track-wide)",
        }}
      >
        {isBuy ? "BUY" : isSell ? "SELL" : "—"}
      </span>
      <span>{order.outcome ? <SidePill outcome={order.outcome} /> : <Muted>—</Muted>}</span>
      <Right>
        <StatusBadge status={order.status} />
      </Right>
      <Right mono>{order.qty ?? "—"}</Right>
      <Right mono>{order.priceNanos != null ? formatCents(order.priceNanos) : "—"}</Right>
      <Right mono>
        {order.valueNanos != null ? formatDollars(order.valueNanos, { decimals: 2 }) : "—"}
      </Right>
      <Right>
        <PnlCell pnlNanos={order.realizedPnlNanos} />
      </Right>
      <Right>
        <ClosedTime ms={order.closedAtMs} />
      </Right>
    </Row>
  );
}

/** Realized PnL for a sell — green/red signed $; "—" for buys and unknowns. */
function PnlCell({ pnlNanos }: { pnlNanos: bigint | null }) {
  return (
    <span
      style={{
        fontFamily: "var(--font-mono)",
        fontSize: 12,
        color:
          pnlNanos == null
            ? "var(--fg-4)"
            : pnlNanos >= 0n
              ? "var(--yes)"
              : "var(--no)",
      }}
    >
      {pnlNanos == null
        ? "—"
        : formatDollars(pnlNanos, { decimals: 2, sign: true })}
    </span>
  );
}

/** Terminal-status chip. FILLED reads in the position tone; the rest are muted. */
function StatusBadge({ status }: { status: Status }) {
  const tone =
    status === "FILLED"
      ? { fg: "var(--yes)", bg: "color-mix(in srgb, var(--yes) 14%, transparent)" }
      : { fg: "var(--fg-3)", bg: "rgba(255,255,255,0.04)" };
  return (
    <span
      style={{
        padding: "1px 7px",
        background: tone.bg,
        color: tone.fg,
        borderRadius: 3,
        fontFamily: "var(--font-mono)",
        fontSize: 9.5,
        fontWeight: 600,
        letterSpacing: "var(--track-wide)",
        whiteSpace: "nowrap",
      }}
    >
      {status}
    </span>
  );
}

/** Close time — short date over wall-clock, like the history feed's stamps. */
function ClosedTime({ ms }: { ms: number }) {
  const d = new Date(ms);
  const date = d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  const time = d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
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
      <span style={{ fontSize: 11, color: "var(--fg-2)" }}>{date}</span>
      <span
        style={{
          fontSize: 9.5,
          color: "var(--fg-4)",
          letterSpacing: "var(--track-wide)",
        }}
      >
        {time}
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
        gap: 3,
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
      <span>{col.label}</span>
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
          "minmax(0, 1fr) 38px 36px 62px 32px 44px 52px 58px 56px",
        gap: 8,
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

function Muted({ children }: { children: React.ReactNode }) {
  return <span style={{ color: "var(--fg-4)" }}>{children}</span>;
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
