"use client";

/**
 * Closed orders tab — terminal (filled / cancelled / expired) orders, rebuilt
 * from the account history feed and grouped by `order_id`. Shares the exact
 * design language of `OpenOrdersList` (card, market thumbnail, click-to-sort
 * headers, `Link` rows, paginated footer) so the two order tabs read as one
 * family. Grid rows:
 *   thumb · market · action · side · status · qty · price · welfare · value ·
 *   P&L · closed
 *
 * Why reconstruct from history rather than a dedicated endpoint: the open-orders
 * feed only carries *resting* orders, so once an order fills/cancels/expires it
 * drops out of there. The history log keeps the lifecycle events, so we fold
 * them per order_id (same logic as the market-page `EventClosedOrders`):
 *   - status  = the last terminal event's type.
 *   - qty     = that terminal event's qty (filled / returned / expired count).
 *   - price   = the terminal event's price, else the last fill price; the order's
 *               limit (requested) price shows struck-through before it when they
 *               differ.
 *   - welfare = Σ (limit − fill) × qty over fills, signed by side (buyer below
 *               limit / seller above = positive surplus). Null without a known
 *               limit/side/fill.
 *   - value   = qty × price (notional $).
 *   - P&L     = realized PnL summed across fill events — SELL orders only.
 * Default order is newest-first by close time; every column is click-to-sort.
 */

import Link from "next/link";
import { useMemo, useState } from "react";
import { MarketThumb } from "@/components/market-thumb";
import { Pager, usePaged, PORTFOLIO_PAGE_SIZE } from "@/components/event-list-pager";
import { Glossary } from "@/components/glossary";
import type { HistoryEvent } from "@/lib/account/use-account-history";
import { formatCents, formatDollars } from "@/lib/format/nanos";
import type { components } from "@/lib/api/schema";
import { SidePill } from "./side-pill";

type Market = components["schemas"]["MarketResponse"];
type Status = "FILLED" | "CANCELLED" | "EXPIRED";

const TERMINAL = new Set<HistoryEvent["type"]>(["filled", "cancelled", "expired"]);
const STATUS_OF: Record<"filled" | "cancelled" | "expired", Status> = {
  filled: "FILLED",
  cancelled: "CANCELLED",
  expired: "EXPIRED",
};

/** Per-order accumulator used while folding the history feed. */
interface Slot {
  marketId: number;
  side?: "BUY" | "SELL";
  outcome?: "YES" | "NO";
  terminal: HistoryEvent | null;
  lastFillPrice: bigint | null;
  realizedPnl: bigint | null;
  requestedPrice: bigint | null;
  filledQty: bigint;
  filledNotional: bigint;
}

/** A closed order with every sortable value derived once. */
interface ClosedRowData {
  orderId: number;
  marketId: number;
  market: Market | undefined;
  label: string;
  status: Status;
  closedAtMs: number;
  side?: "BUY" | "SELL";
  outcome?: "YES" | "NO";
  qty: number | null;
  priceNanos: bigint | null;
  requestedPriceNanos: bigint | null;
  valueNanos: bigint | null;
  realizedPnlNanos: bigint | null;
  welfareNanos: bigint | null;
}

type SortKey =
  | "market"
  | "action"
  | "side"
  | "status"
  | "qty"
  | "price"
  | "welfare"
  | "value"
  | "pnl"
  | "closed";
type SortDir = "asc" | "desc";
type Sort = { key: SortKey; dir: SortDir };

const COLUMNS: {
  key: SortKey;
  label: string;
  align: "left" | "right";
  info?: string;
}[] = [
  { key: "market", label: "Market", align: "left" },
  { key: "action", label: "Action", align: "left" },
  { key: "side", label: "Side", align: "left" },
  { key: "status", label: "Status", align: "left" },
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
    key === "welfare" ||
    key === "value" ||
    key === "pnl" ||
    key === "closed";
  return { key, dir: numeric ? "desc" : "asc" };
}

function cmpBig(a: bigint, b: bigint): number {
  return a > b ? 1 : a < b ? -1 : 0;
}

/** Ascending comparison; null numbers sort lowest. */
function compareBy(a: ClosedRowData, b: ClosedRowData, key: SortKey): number {
  switch (key) {
    case "market":
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
    case "welfare":
      if (a.welfareNanos == null && b.welfareNanos == null) return 0;
      if (a.welfareNanos == null) return -1;
      if (b.welfareNanos == null) return 1;
      return cmpBig(a.welfareNanos, b.welfareNanos);
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

interface Props {
  events: HistoryEvent[];
  marketsById: Map<number, Market>;
}

export function ClosedOrdersList({ events, marketsById }: Props) {
  const [sort, setSort] = useState<Sort | null>(null);

  const rows = useMemo<ClosedRowData[]>(() => {
    // Fold order lifecycle events per order_id: latest terminal event sets
    // status/qty/close-time; placed event carries the limit; fills accumulate
    // realized PnL and the welfare numerator/denominator.
    const byOrder = new Map<number, Slot>();

    for (const e of events) {
      if (e.orderId == null || e.marketId == null) continue;
      if (e.type !== "placed" && e.type !== "partial_fill" && !TERMINAL.has(e.type))
        continue;

      const slot: Slot = byOrder.get(e.orderId) ?? {
        marketId: e.marketId,
        terminal: null,
        lastFillPrice: null,
        realizedPnl: null,
        requestedPrice: null,
        filledQty: 0n,
        filledNotional: 0n,
      };

      if (e.side && slot.side == null) slot.side = e.side;
      if (e.outcome && slot.outcome == null) slot.outcome = e.outcome;
      if (e.type === "placed" && e.priceNanos != null && slot.requestedPrice == null) {
        slot.requestedPrice = e.priceNanos;
      }
      if (e.type === "partial_fill" && e.priceNanos != null) {
        slot.lastFillPrice = e.priceNanos;
      }
      if (
        (e.type === "partial_fill" || e.type === "filled") &&
        e.realizedPnlNanos != null
      ) {
        slot.realizedPnl = (slot.realizedPnl ?? 0n) + e.realizedPnlNanos;
      }
      if (
        (e.type === "partial_fill" || e.type === "filled") &&
        e.priceNanos != null &&
        e.qty != null
      ) {
        slot.filledQty += BigInt(e.qty);
        slot.filledNotional += e.priceNanos * BigInt(e.qty);
      }
      if (
        TERMINAL.has(e.type) &&
        (slot.terminal == null || e.timestampMs >= slot.terminal.timestampMs)
      ) {
        slot.terminal = e;
      }
      byOrder.set(e.orderId, slot);
    }

    const decorated: ClosedRowData[] = [];
    for (const [orderId, slot] of byOrder) {
      const t = slot.terminal;
      if (t == null) continue; // only partial fills so far — still resting
      const qty = t.qty ?? null;
      const priceNanos = t.priceNanos ?? slot.lastFillPrice;
      let welfareNanos: bigint | null = null;
      if (slot.requestedPrice != null && slot.side != null && slot.filledQty > 0n) {
        const edge = slot.requestedPrice * slot.filledQty - slot.filledNotional;
        welfareNanos = slot.side === "BUY" ? edge : -edge;
      }
      const row: ClosedRowData = {
        orderId,
        marketId: slot.marketId,
        market: marketsById.get(slot.marketId),
        label: marketsById.get(slot.marketId)?.name ?? `#${slot.marketId}`,
        status: STATUS_OF[t.type as "filled" | "cancelled" | "expired"],
        closedAtMs: t.timestampMs,
        qty,
        priceNanos,
        requestedPriceNanos: slot.requestedPrice,
        valueNanos: qty != null && priceNanos != null ? BigInt(qty) * priceNanos : null,
        realizedPnlNanos: slot.side === "SELL" ? slot.realizedPnl : null,
        welfareNanos,
      };
      if (slot.side) row.side = slot.side;
      if (slot.outcome) row.outcome = slot.outcome;
      decorated.push(row);
    }

    if (!sort) {
      return decorated.sort((a, b) => b.closedAtMs - a.closedAtMs);
    }
    const factor = sort.dir === "asc" ? 1 : -1;
    return decorated.sort((a, b) => compareBy(a, b, sort.key) * factor);
  }, [events, marketsById, sort]);

  const paged = usePaged(rows, PORTFOLIO_PAGE_SIZE);

  const onSort = (key: SortKey) => {
    setSort((s) => nextSort(s, key));
    paged.setPage(0);
  };

  if (rows.length === 0) {
    return <Empty>No closed orders.</Empty>;
  }
  return (
    <div
      style={{
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: 6,
        overflow: "hidden",
      }}
    >
      <div style={rowGrid("var(--fg-4)")}>
        <span />
        {COLUMNS.map((col) => (
          <SortHeader key={col.key} col={col} sort={sort} onSort={onSort} />
        ))}
      </div>
      {paged.visible.map((r) => (
        <ClosedRow key={r.orderId} row={r} />
      ))}
      <div style={{ padding: "0 14px" }}>
        <Pager paged={paged} />
      </div>
    </div>
  );
}

function ClosedRow({ row }: { row: ClosedRowData }) {
  const { market, label, marketId } = row;
  const isBuy = row.side === "BUY";
  const isSell = row.side === "SELL";
  return (
    <Link
      href={`/m/${marketId}`}
      style={{
        ...rowGrid("var(--fg-2)"),
        textDecoration: "none",
        color: "inherit",
        borderTop: "1px solid var(--border-1)",
      }}
    >
      <MarketThumb
        marketId={marketId}
        name={label}
        imageUrl={market?.market_image_url ?? market?.event_image_url ?? null}
        fallbackIconUrl={market?.market_icon_url ?? market?.event_icon_url ?? null}
        size={28}
      />
      <span
        style={{
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          color: "var(--fg-1)",
          fontFamily: "var(--font-sans)",
          fontSize: 13,
        }}
        title={label}
      >
        {label}
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
      {row.outcome ? <SidePill outcome={row.outcome} /> : <Muted>—</Muted>}
      <span>
        <StatusBadge status={row.status} />
      </span>
      <RightCell mono>{row.qty ?? "—"}</RightCell>
      <RightCell mono>
        <PriceCell
          settledNanos={row.priceNanos}
          requestedNanos={row.requestedPriceNanos}
        />
      </RightCell>
      <RightCell>
        <WelfareCell welfareNanos={row.welfareNanos} />
      </RightCell>
      <RightCell mono>
        {row.valueNanos != null ? formatDollars(row.valueNanos, { decimals: 2 }) : "—"}
      </RightCell>
      <RightCell>
        <PnlCell pnlNanos={row.realizedPnlNanos} />
      </RightCell>
      <RightCell>
        <ClosedTime ms={row.closedAtMs} />
      </RightCell>
    </Link>
  );
}

/**
 * Price cell — the settled (fill) price, with the requested (limit) price shown
 * struck-through before it when the two differ. "—" when nothing filled.
 */
function PriceCell({
  settledNanos,
  requestedNanos,
}: {
  settledNanos: bigint | null;
  requestedNanos: bigint | null;
}) {
  if (settledNanos == null) return <>—</>;
  const settled = formatCents(settledNanos);
  const requested = requestedNanos != null ? formatCents(requestedNanos) : null;
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
 * A positive surplus reads as a green pill, a negative one as a red pill; an
 * exact-limit fill or unknown welfare stays muted and flat. The signed $ amount
 * answers "how much better".
 */
function WelfareCell({ welfareNanos }: { welfareNanos: bigint | null }) {
  if (welfareNanos == null) {
    return <span style={{ color: "var(--fg-4)", fontFamily: "var(--font-mono)" }}>—</span>;
  }
  const positive = welfareNanos > 0n;
  const negative = welfareNanos < 0n;
  const tone = positive ? "var(--yes)" : negative ? "var(--no)" : "var(--fg-3)";
  const bg = positive
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
      {pnlNanos == null ? "—" : formatDollars(pnlNanos, { decimals: 2, sign: true })}
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
        justifySelf: "start",
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

function SortHeader({
  col,
  sort,
  onSort,
}: {
  col: (typeof COLUMNS)[number];
  sort: Sort | null;
  onSort: (key: SortKey) => void;
}) {
  const active = sort?.key === col.key;
  const button = (
    <button
      type="button"
      onClick={() => onSort(col.key)}
      title={`Sort by ${col.label}`}
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
        letterSpacing: "var(--track-wide)",
        color: active ? "var(--fg-2)" : "var(--fg-4)",
      }}
    >
      <span style={{ whiteSpace: "nowrap" }}>{col.label}</span>
      <span style={{ fontSize: 8, lineHeight: 1, opacity: active ? 1 : 0.3 }}>
        {active ? (sort!.dir === "asc" ? "▲" : "▼") : "↕"}
      </span>
    </button>
  );
  if (!col.info) return button;
  // A `?` glossary badge sits beside the sort label (not nested in the button).
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
      {button}
      <Glossary term={col.info} />
    </span>
  );
}

function rowGrid(color: string): React.CSSProperties {
  return {
    display: "grid",
    gridTemplateColumns:
      "28px minmax(0, 1.3fr) 56px 48px 78px 46px 74px 94px 82px 70px 96px",
    gap: 14,
    alignItems: "center",
    padding: "10px 14px",
    color,
    fontFamily: "var(--font-mono)",
    fontSize: 11,
    letterSpacing: "var(--track-wide)",
  };
}

function RightCell({
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
        fontFamily: mono ? "var(--font-mono)" : "inherit",
        fontSize: mono ? 12 : undefined,
        color: mono ? "var(--fg-1)" : undefined,
        whiteSpace: "nowrap",
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
        padding: "32px 16px",
        background: "var(--surface-1)",
        border: "1px dashed var(--border-1)",
        borderRadius: 6,
        color: "var(--fg-4)",
        fontFamily: "var(--font-mono)",
        fontSize: 12,
        textAlign: "center",
      }}
    >
      {children}
    </div>
  );
}
