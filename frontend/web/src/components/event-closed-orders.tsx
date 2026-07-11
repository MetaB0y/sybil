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
import { SidePill } from "@/components/portfolio/side-pill";
import { Glossary } from "@/components/glossary";

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
type SortDir = "asc" | "desc";
type Sort = { key: SortKey; dir: SortDir };

const COLUMNS: {
  key: SortKey;
  label: string;
  align: "left" | "right";
  /** Glossary term — renders a "?" tooltip badge beside the sort label. */
  info?: string;
}[] = [
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
    key === "welfare" ||
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
    case "welfare":
      if (a.welfareNanos == null && b.welfareNanos == null) return 0;
      if (a.welfareNanos == null) return -1;
      if (b.welfareNanos == null) return 1;
      return cmpBig(a.welfareNanos, b.welfareNanos);
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
      if (e.type !== "placed" && e.type !== "partial_fill" && !TERMINAL.has(e.type))
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
      if (e.type === "placed" && e.priceNanos != null && slot.requestedPrice == null) {
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
        priceNanos = priceNanosFromNotional(slot.filledNotional, slot.filledQty); // WAC
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
      if (slot.requestedPrice != null && slot.side != null && slot.filledQty > 0n) {
        const edge = notionalNanos(slot.requestedPrice, slot.filledQty) - slot.filledNotional;
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
        {order.valueNanos != null ? formatDollarsRounded(order.valueNanos, { decimals: 1 }) : "—"}
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
  const requested = requestedNanos != null ? formatCentsPrecise(requestedNanos) : null;
  if (requested == null || requested === settled) return <>{settled}</>;
  return (
    <span style={{ display: "inline-flex", gap: 4, justifyContent: "flex-end" }}>
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
    return <span style={{ color: "var(--fg-4)", fontFamily: "var(--font-mono)" }}>—</span>;
  }
  const positive = welfareNanos > 0n;
  const negative = welfareNanos < 0n;
  const tone = positive ? "var(--yes)" : negative ? "var(--no)" : "var(--fg-3)";
  const bg =
    positive
      ? "color-mix(in srgb, var(--yes) 16%, transparent)"
      : negative
        ? "color-mix(in srgb, var(--no) 14%, transparent)"
        : "transparent";
  return (
    <span
      title={
        positive
          ? "Filled better than your limit — surplus you gained"
          : negative
            ? "Filled at a worse edge than your limit"
            : "Filled exactly at your limit"
      }
      style={{
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "flex-end",
        gap: 3,
        padding: positive || negative ? "1px 6px" : 0,
        borderRadius: 3,
        background: bg,
        color: tone,
        fontFamily: "var(--font-mono)",
        fontSize: 12,
        fontWeight: positive || negative ? 600 : 400,
        whiteSpace: "nowrap",
      }}
    >
      {positive && (
        <span aria-hidden style={{ fontSize: 8, lineHeight: 1 }}>
          ▲
        </span>
      )}
      {negative && (
        <span aria-hidden style={{ fontSize: 8, lineHeight: 1 }}>
          ▼
        </span>
      )}
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
function StatusBadge({
  status,
}: {
  status: Status;
}) {
  const tone =
    status === "FILLED"
      ? { fg: "var(--yes)", bg: "color-mix(in srgb, var(--yes) 14%, transparent)" }
      : status === "PARTIAL"
        ? { fg: "var(--warn)", bg: "color-mix(in srgb, var(--warn) 16%, transparent)" }
        : status === "REJECTED"
          ? { fg: "var(--no)", bg: "color-mix(in srgb, var(--no) 14%, transparent)" }
          : { fg: "var(--fg-3)", bg: "var(--fill-subtle)" };
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

/** Close time — wall-clock first, then the short date faded after it, on one
 *  line. 24-hour so it stays compact enough to sit beside the date. */
function ClosedTime({ ms }: { ms: number }) {
  const d = new Date(ms);
  const date = d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  const time = d.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
  return (
    <span
      title={d.toLocaleString()}
      style={{
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        color: "var(--fg-2)",
        whiteSpace: "nowrap",
      }}
    >
      {time}
      <span style={{ color: "var(--fg-4)" }}>{` ${date}`}</span>
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
  const sortButton = (
    <button
      type="button"
      onClick={onSort}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 3,
        width: col.info ? "auto" : "100%",
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
    >
      <span>{col.label}</span>
      <span style={{ fontSize: 8, lineHeight: 1, opacity: active ? 1 : 0.3 }}>
        {active ? (sort!.dir === "asc" ? "▲" : "▼") : "↕"}
      </span>
    </button>
  );
  // A `?` glossary badge sits beside the sort label as a sibling (not nested in
  // the button — that would be invalid markup) for columns that explain a term.
  if (!col.info) return sortButton;
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 3,
        width: "100%",
        justifyContent: col.align === "right" ? "flex-end" : "flex-start",
      }}
    >
      {sortButton}
      <Glossary term={col.info} />
    </span>
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
        // Ten columns in one row: keep the data columns compact, but give them a
        // roomy gap so they breathe instead of bunching (the outcome 1fr absorbs
        // the slack on wide screens). qty needs a touch more width for values
        // like "234.375".
        gridTemplateColumns:
          "minmax(0, 1fr) 52px 46px 62px 62px 74px 78px 54px 52px 80px",
        gap: 18,
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
  dim,
}: {
  children: React.ReactNode;
  mono?: boolean;
  /** Fade the value to fg-4 — used for the limit price/value of orders that
   *  never traded, so they read as "would have" rather than settled. */
  dim?: boolean;
}) {
  return (
    <span
      style={{
        textAlign: "right",
        whiteSpace: "nowrap",
        fontFamily: mono ? "var(--font-mono)" : "inherit",
        fontSize: mono ? 12 : undefined,
        color: dim ? "var(--fg-4)" : mono ? "var(--fg-1)" : undefined,
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
