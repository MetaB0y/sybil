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
 *   - price = the terminal event's price, else the last fill price seen; the
 *     order's limit (requested) price from its `placed` event is shown
 *     struck-through before it when the two differ. A cancelled/expired order
 *     that never traded falls back to that limit price, rendered faded.
 *   - welfare = Σ (limit − fill) × qty over the order's fills, signed by side
 *     (buyer below limit / seller above = positive surplus). Mirrors the engine's
 *     `welfare_contribution`. Null when the limit price has aged out of the feed.
 *   - value = qty × price (notional $), shown for orders that carry both.
 *   - P&L = realized PnL summed across the order's fill events
 *     (`partial_fill` + `filled`), shown for SELL orders only — a buy opens or
 *     adds to a position and realizes nothing. The backend supplies it per fill
 *     (`realized_pnl_nanos`), so no FE cost-basis math is needed.
 * Default order is newest-first by the terminal event's `timestamp_ms`; every
 * column is click-to-sort.
 *
 * Rejected orders (insufficient balance/position, complete-set self-trade)
 * surface as a REJECTED terminal status via `HistoryKind::Rejected`. The
 * expired-at-entry path is separate and still tracked with the degen work.
 */

import { useMemo, useState } from "react";
import {
  formatShareUnits,
  notionalNanos,
  priceNanosFromNotional,
} from "@/lib/account/quantity";
import type { HistoryEvent } from "@/lib/account/use-account-history";
import {
  formatCentsPrecise,
  formatDollars,
  formatDollarsRounded,
} from "@/lib/format/nanos";
import { Pager, usePaged } from "@/components/event-list-pager";
import { SidePill, valueChipStyle } from "@/components/portfolio/side-pill";
import {
  ActionCell,
  cmpNullableBig,
  Empty,
  EventRow,
  EventTable,
  EventTime,
  HeaderCell,
  Muted,
  nextSort,
  OutcomeLabel,
  Right,
  type Column,
  type Sort,
} from "@/components/event-table";

type Status = "FILLED" | "PARTIAL" | "CANCELLED" | "EXPIRED" | "REJECTED";

/** The terminal history event types that close an order. */
const TERMINAL = new Set<HistoryEvent["type"]>([
  "filled",
  "cancelled",
  "expired",
  "rejected",
]);

const STATUS_OF: Record<
  "filled" | "cancelled" | "expired" | "rejected",
  Status
> = {
  filled: "FILLED",
  cancelled: "CANCELLED",
  expired: "EXPIRED",
  rejected: "REJECTED",
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
  /** For a PARTIAL order (filled, then the rest expired/cancelled): the unfilled
   *  remainder and how it closed. `qty` above holds the FILLED shares; this is
   *  the part that never traded, surfaced in the status tooltip. Null otherwise. */
  remainderQty: number | null;
  remainderKind: "expired" | "cancelled" | null;
  /** Settled (fill) price shown in the Price cell. */
  priceNanos: bigint | null;
  /** Limit price from the order's `placed` event; null once it ages out of the
   * feed. Rendered struck-through before the settled price when the two differ. */
  requestedPriceNanos: bigint | null;
  /** qty × price (notional $), or null when either is unknown. */
  valueNanos: bigint | null;
  /** True when the order never traded (cancelled/expired with no fill) and the
   * Price/Value cells fall back to the order's limit price — rendered faded so
   * it reads as "would have" rather than "did". */
  unfilled: boolean;
  /** Realized PnL (nanos) summed over fill events — SELL orders only, else null. */
  realizedPnlNanos: bigint | null;
  /** Consumer surplus (nanos) = Σ (limit − fill) × qty over fills, signed by side
   * (buyer fills below limit / seller above = positive). Null without a known
   * limit, side, or any fill. Mirrors the engine's `welfare_contribution`. */
  welfareNanos: bigint | null;
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
  | "welfare"
  | "closed";
/* Ten columns: the outcome 1fr absorbs the slack, the rest stay compact. Qty
   needs a touch more width for values like "234.375". */
/* Closed needs 86px: "21:17 Jul 21" is ~80px of 11px mono, and the cell is
   right-aligned + nowrap, so anything narrower spills past the card edge
   instead of truncating. Value and P&L give up the width for it. */
const GRID =
  "minmax(0, 1fr) 56px 48px 82px 62px 74px 78px 62px 56px 86px";

const COLUMNS: Column<SortKey>[] = [
  { key: "outcome", label: "Outcome", align: "left" },
  { key: "action", label: "Action", align: "left" },
  { key: "side", label: "Side", align: "left" },
  { key: "status", label: "Status", align: "right" },
  { key: "qty", label: "Qty", align: "right" },
  { key: "price", label: "Price", align: "right" },
  { key: "welfare", label: "Welfare", align: "right", info: "Welfare" },
  { key: "value", label: "Value", align: "right" },
  { key: "pnl", label: "P&L", align: "right" },
  { key: "closed", label: "Closed", align: "right" },
];

/** Every column but the four text ones sorts high→low on first click. */
function isNumericColumn(key: SortKey): boolean {
  return (
    key !== "outcome" &&
    key !== "action" &&
    key !== "side" &&
    key !== "status"
  );
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
      return cmpNullableBig(a.priceNanos, b.priceNanos);
    case "value":
      return cmpNullableBig(a.valueNanos, b.valueNanos);
    case "pnl":
      return cmpNullableBig(a.realizedPnlNanos, b.realizedPnlNanos);
    case "welfare":
      return cmpNullableBig(a.welfareNanos, b.welfareNanos);
    case "closed":
      return a.closedAtMs - b.closedAtMs;
  }
}

export function EventClosedOrders({
  events,
  labelByMarket,
  pageSize,
}: {
  /** Full account history feed (unfiltered). */
  events: HistoryEvent[];
  /** market_id → short outcome label (same map EventHoldings builds). */
  labelByMarket: Map<number, string>;
  /** Rows per page; defaults to the compact market-detail PAGE_SIZE. */
  pageSize?: number;
}) {
  const [sort, setSort] = useState<Sort<SortKey> | null>(null);

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
        /** Limit price from the `placed` event; null if it aged out of the feed. */
        requestedPrice: bigint | null;
        /** Σ qty over fill events — denominator side of the welfare sum. */
        filledQty: bigint;
        /** Σ (fill price × qty) over fill events, for the welfare sum. */
        filledNotional: bigint;
      }
    >();

    for (const e of events) {
      if (e.orderId == null || e.marketId == null) continue;
      if (!eventMarketIds.has(e.marketId)) continue;
      // `placed` joins so we can surface the requested (limit) price + welfare;
      // everything else here is a partial fill or a terminal close.
      if (
        e.type !== "placed" &&
        e.type !== "partial_fill" &&
        !TERMINAL.has(e.type)
      )
        continue;

      const slot =
        byOrder.get(e.orderId) ??
        ({
          marketId: e.marketId,
          terminal: null,
          lastFillPrice: null,
          realizedPnl: null,
          requestedPrice: null,
          filledQty: 0n,
          filledNotional: 0n,
        } as {
          marketId: number;
          side?: "BUY" | "SELL";
          outcome?: "YES" | "NO";
          terminal: HistoryEvent | null;
          lastFillPrice: bigint | null;
          realizedPnl: bigint | null;
          requestedPrice: bigint | null;
          filledQty: bigint;
          filledNotional: bigint;
        });

      // Carry side/outcome from whichever event has them (the `placed` event and
      // fills both do).
      if (e.side && slot.side == null) slot.side = e.side;
      if (e.outcome && slot.outcome == null) slot.outcome = e.outcome;
      // The `placed` event carries the order's limit (requested) price.
      if (
        e.type === "placed" &&
        e.priceNanos != null &&
        slot.requestedPrice == null
      ) {
        slot.requestedPrice = e.priceNanos;
      }
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
      // Accumulate Σqty and Σ(price × qty) across every fill (partial + final) so
      // welfare is exact even when an order fills in pieces at different prices.
      if (
        (e.type === "partial_fill" || e.type === "filled") &&
        e.priceNanos != null &&
        e.qty != null
      ) {
        slot.filledQty += BigInt(e.qty);
        slot.filledNotional += notionalNanos(e.priceNanos, e.qty);
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
      const rawStatus =
        STATUS_OF[t.type as "filled" | "cancelled" | "expired" | "rejected"];

      // A partially-filled order whose remainder then expired/cancelled: fills
      // DID happen, but the terminal event only closed out the unfilled part.
      // Reporting it as plain EXPIRED (and multiplying the expired remainder by
      // the fill price) hides the trade and shows a meaningless value. Detect it
      // and report PARTIAL with the FILLED economics instead.
      const terminalUnfilled = t.type === "expired" || t.type === "cancelled";
      const partial = slot.filledQty > 0n && terminalUnfilled;

      let status: Status;
      let qty: number | null;
      let priceNanos: bigint | null;
      let unfilled: boolean;
      let remainderQty: number | null = null;
      let remainderKind: "expired" | "cancelled" | null = null;

      if (partial) {
        status = "PARTIAL";
        qty = Number(slot.filledQty); // the shares that actually filled
        priceNanos = priceNanosFromNotional(
          slot.filledNotional,
          slot.filledQty,
        ); // WAC
        unfilled = false;
        remainderQty = t.qty ?? null; // the part that never traded
        remainderKind = t.type === "expired" ? "expired" : "cancelled";
      } else {
        status = rawStatus;
        qty = t.qty ?? null;
        const settledNanos = t.priceNanos ?? slot.lastFillPrice;
        // A cancelled/expired order that never filled has no settled price, but it
        // still carries the limit price from its `placed` event. Surface that (and
        // the notional it *would* have been) rather than a blank row, flagged
        // `unfilled` so the Price/Value cells render faded — the trade didn't happen.
        unfilled =
          settledNanos == null &&
          (rawStatus === "CANCELLED" || rawStatus === "EXPIRED") &&
          slot.requestedPrice != null;
        priceNanos = settledNanos ?? (unfilled ? slot.requestedPrice : null);
      }
      // Welfare = Σ (limit − fill) × qty, signed by side: a buyer gains when it
      // fills below the limit, a seller when above. Needs a known limit, side,
      // and at least one fill; else null ("—").
      let welfareNanos: bigint | null = null;
      if (
        slot.requestedPrice != null &&
        slot.side != null &&
        slot.filledQty > 0n
      ) {
        const edge =
          notionalNanos(slot.requestedPrice, slot.filledQty) -
          slot.filledNotional;
        welfareNanos = slot.side === "BUY" ? edge : -edge;
      }
      const row: ClosedOrder = {
        orderId,
        marketId: slot.marketId,
        label: labelByMarket.get(slot.marketId) ?? `#${slot.marketId}`,
        status,
        closedAtMs: t.timestampMs,
        qty,
        remainderQty,
        remainderKind,
        priceNanos,
        requestedPriceNanos: slot.requestedPrice,
        // For a partial, the exact Σ(price×qty) over fills is the true value of
        // what traded; elsewhere derive it from the single settled price × qty.
        valueNanos: partial
          ? slot.filledNotional
          : qty != null && priceNanos != null
            ? notionalNanos(priceNanos, qty)
            : null,
        unfilled,
        // PnL is realized on the closing trade — show it for sells only.
        realizedPnlNanos: slot.side === "SELL" ? slot.realizedPnl : null,
        welfareNanos,
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

  const paged = usePaged(closed, pageSize);

  if (closed.length === 0) {
    return <Empty>No closed orders for this event.</Empty>;
  }

  return (
    <EventTable>
      <EventRow columns={GRID} header>
        {COLUMNS.map((col) => (
          <HeaderCell
            key={col.key}
            col={col}
            sort={sort}
            onSort={() => {
              setSort((s) => nextSort(s, col.key, isNumericColumn(col.key)));
              paged.setPage(0);
            }}
          />
        ))}
      </EventRow>
      {paged.visible.map((o) => (
        <ClosedRow key={o.orderId} order={o} />
      ))}
      <Pager paged={paged} />
    </EventTable>
  );
}

function ClosedRow({ order }: { order: ClosedOrder }) {
  return (
    <EventRow columns={GRID}>
      <OutcomeLabel>{order.label}</OutcomeLabel>
      <ActionCell side={order.side} />
      {order.outcome ? <SidePill outcome={order.outcome} /> : <Muted>—</Muted>}
      <Right>
        <StatusBadge status={order.status} />
      </Right>
      <Right mono>
        {order.qty == null ? (
          "—"
        ) : order.status === "PARTIAL" && order.remainderQty != null ? (
          // filled / ordered — the faded total makes the unfilled (expired /
          // cancelled) part visible without a separate phantom row.
          <span>
            {formatShareUnits(order.qty, 1)}
            <span style={{ color: "var(--fg-4)" }}>
              {` / ${formatShareUnits(order.qty + order.remainderQty, 1)}`}
            </span>
          </span>
        ) : (
          <span>{formatShareUnits(order.qty, 1)}</span>
        )}
      </Right>
      <Right mono dim={order.unfilled}>
        <PriceCell
          settledNanos={order.priceNanos}
          requestedNanos={order.requestedPriceNanos}
        />
      </Right>
      <Right>
        <WelfareCell welfareNanos={order.welfareNanos} />
      </Right>
      <Right mono dim={order.unfilled}>
        {order.valueNanos != null
          ? formatDollarsRounded(order.valueNanos, { decimals: 1 })
          : "—"}
      </Right>
      <Right>
        <PnlCell pnlNanos={order.realizedPnlNanos} />
      </Right>
      <Right>
        <EventTime ms={order.closedAtMs} />
      </Right>
    </EventRow>
  );
}

/**
 * Price cell — the settled (fill) price, with the requested (limit) price shown
 * struck-through before it when the two differ. Falls back to settled-only when
 * the requested price is unknown (aged out of the feed) or rounds to the same
 * cents, and to "—" when nothing filled.
 */
function PriceCell({
  settledNanos,
  requestedNanos,
}: {
  settledNanos: bigint | null;
  requestedNanos: bigint | null;
}) {
  if (settledNanos == null) return <>—</>;
  const settled = formatCentsPrecise(settledNanos);
  const requested =
    requestedNanos != null ? formatCentsPrecise(requestedNanos) : null;
  if (requested == null || requested === settled) return <>{settled}</>;
  return (
    <span
      style={{ display: "inline-flex", gap: 4, justifyContent: "flex-end" }}
    >
      <span style={{ color: "var(--fg-4)", textDecoration: "line-through" }}>
        {requested}
      </span>
      <span>{settled}</span>
    </span>
  );
}

/**
 * Welfare cell — highlights how much better than your limit the order filled.
 * A positive surplus (you beat your limit) reads as a green pill, a negative one
 * (you paid up to your limit's edge) as a red pill; an exact-limit fill or an
 * unknown welfare stays muted and flat, so the eye lands on the orders that
 * actually gained. The signed $ amount answers "how much better".
 */
function WelfareCell({ welfareNanos }: { welfareNanos: bigint | null }) {
  if (welfareNanos == null) {
    return (
      <span style={{ color: "var(--fg-4)", fontFamily: "var(--font-mono)" }}>
        —
      </span>
    );
  }
  const positive = welfareNanos > 0n;
  const negative = welfareNanos < 0n;
  const tone = positive ? "var(--yes)" : negative ? "var(--no)" : "var(--fg-3)";
  const bg = positive
    ? "color-mix(in srgb, var(--yes) 14%, transparent)"
    : negative
      ? "color-mix(in srgb, var(--no) 14%, transparent)"
      : "var(--fill-subtle)";
  // A small tinted chip matching the side pill; the value is bold as the one
  // intended difference from the side chip.
  return (
    <span
      title={
        positive
          ? "Filled better than your limit — surplus you gained"
          : negative
            ? "Filled at a worse edge than your limit"
            : "Filled exactly at your limit"
      }
      style={valueChipStyle({ color: tone, bg, bold: true })}
    >
      {formatDollars(welfareNanos, { decimals: 2, sign: true })}
    </span>
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
        : formatDollarsRounded(pnlNanos, { decimals: 1, sign: true })}
    </span>
  );
}

/**
 * Terminal-status chip. FILLED reads in the position tone, PARTIAL in the amber
 * "incomplete" tone (filled, but the rest didn't), REJECTED in the loss tone;
 * the rest are muted.
 */
function StatusBadge({ status }: { status: Status }) {
  const tone =
    status === "FILLED"
      ? {
          fg: "var(--yes)",
          bg: "color-mix(in srgb, var(--yes) 14%, transparent)",
        }
      : status === "PARTIAL"
        ? {
            fg: "var(--warn)",
            bg: "color-mix(in srgb, var(--warn) 16%, transparent)",
          }
        : status === "REJECTED"
          ? {
              fg: "var(--no)",
              bg: "color-mix(in srgb, var(--no) 14%, transparent)",
            }
          : { fg: "var(--fg-3)", bg: "var(--fill-subtle)" };
  // Same chip as the side pill / welfare (regular weight); only the tone differs.
  return (
    <span style={valueChipStyle({ color: tone.fg, bg: tone.bg })}>
      {status}
    </span>
  );
}
