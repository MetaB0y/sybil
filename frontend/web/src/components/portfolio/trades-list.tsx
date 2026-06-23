"use client";

/**
 * Trades tab — every execution the account took, one row per fill. Built from
 * the account history feed: we keep `filled` and `partial_fill` events (each is
 * a discrete on-block execution with its own qty/price increment) and drop the
 * order-lifecycle noise (placed / cancelled / expired / rejected). So a user
 * sees exactly the trades that happened, partial fills included.
 *
 * Shares the design language of `OpenOrdersList` / the history feed (card,
 * thumbnail, click-to-sort headers, `Link` rows, paginated footer). Grid rows:
 *   thumb · market · action · side · qty · price · welfare · value · P&L · time
 *
 * Per-row derivations:
 *   - qty     = the fill event's qty (this execution's size).
 *   - price   = the fill price; the order's limit (requested) price shows
 *               struck-through before it when they differ.
 *   - welfare = (limit − fill) × qty, signed by side (buyer below limit /
 *               seller above = positive surplus). Null without a known limit.
 *               The limit is joined from the order's `placed` event.
 *   - value   = qty × price (notional $).
 *   - P&L     = the fill event's realized PnL — SELL fills only.
 * Default order is newest-first by fill time; every column is click-to-sort.
 */

import Link from "next/link";
import { useMemo, useState } from "react";
import { MarketThumb } from "@/components/market-thumb";
import { Pager, usePaged, PORTFOLIO_PAGE_SIZE } from "@/components/event-list-pager";
import { Glossary } from "@/components/glossary";
import type { HistoryEvent } from "@/lib/account/use-account-history";
import { formatCentsPrecise, formatDollars } from "@/lib/format/nanos";
import type { components } from "@/lib/api/schema";
import { PortfolioToolbar } from "./portfolio-toolbar";
import { SearchField } from "./search-field";
import { SidePill } from "./side-pill";

type Market = components["schemas"]["MarketResponse"];

/** A single fill with every sortable value derived once. */
interface TradeRowData {
  id: string;
  marketId: number;
  market: Market | undefined;
  label: string;
  filledAtMs: number;
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
  | "qty"
  | "price"
  | "welfare"
  | "value"
  | "pnl"
  | "time";
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
  { key: "qty", label: "Qty", align: "right" },
  { key: "price", label: "Price", align: "right" },
  { key: "welfare", label: "Welfare", align: "right", info: "Welfare" },
  { key: "value", label: "Value", align: "right" },
  { key: "pnl", label: "P&L", align: "right" },
  { key: "time", label: "Time", align: "right" },
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
    key === "time";
  return { key, dir: numeric ? "desc" : "asc" };
}

function cmpBig(a: bigint, b: bigint): number {
  return a > b ? 1 : a < b ? -1 : 0;
}

/** Ascending comparison; null numbers sort lowest. */
function compareBy(a: TradeRowData, b: TradeRowData, key: SortKey): number {
  switch (key) {
    case "market":
      return a.label.localeCompare(b.label);
    case "action":
      return (a.side ?? "").localeCompare(b.side ?? "");
    case "side":
      return (a.outcome ?? "").localeCompare(b.outcome ?? "");
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
    case "time":
      return a.filledAtMs - b.filledAtMs;
  }
}

interface Props {
  tabs: React.ReactNode;
  events: HistoryEvent[];
  marketsById: Map<number, Market>;
}

export function TradesList({ tabs, events, marketsById }: Props) {
  const [sort, setSort] = useState<Sort | null>(null);
  const [query, setQuery] = useState("");

  const rows = useMemo<TradeRowData[]>(() => {
    // First pass: the limit (requested) price per order, from `placed` events,
    // so each fill can show its welfare vs the order's limit.
    const limitByOrder = new Map<number, bigint>();
    for (const e of events) {
      if (
        e.type === "placed" &&
        e.orderId != null &&
        e.priceNanos != null &&
        !limitByOrder.has(e.orderId)
      ) {
        limitByOrder.set(e.orderId, e.priceNanos);
      }
    }

    // Second pass: one row per fill / partial fill.
    const decorated: TradeRowData[] = [];
    for (const e of events) {
      if (e.type !== "filled" && e.type !== "partial_fill") continue;
      if (e.marketId == null) continue;

      const qty = e.qty ?? null;
      const priceNanos = e.priceNanos ?? null;
      const requestedPriceNanos =
        e.orderId != null ? limitByOrder.get(e.orderId) ?? null : null;

      let welfareNanos: bigint | null = null;
      if (
        requestedPriceNanos != null &&
        e.side != null &&
        priceNanos != null &&
        qty != null
      ) {
        const edge = (requestedPriceNanos - priceNanos) * BigInt(qty);
        welfareNanos = e.side === "BUY" ? edge : -edge;
      }

      const row: TradeRowData = {
        id: e.id,
        marketId: e.marketId,
        market: marketsById.get(e.marketId),
        label: marketsById.get(e.marketId)?.name ?? `#${e.marketId}`,
        filledAtMs: e.timestampMs,
        qty,
        priceNanos,
        requestedPriceNanos,
        valueNanos:
          qty != null && priceNanos != null ? BigInt(qty) * priceNanos : null,
        realizedPnlNanos: e.side === "SELL" ? e.realizedPnlNanos ?? null : null,
        welfareNanos,
      };
      if (e.side) row.side = e.side;
      if (e.outcome) row.outcome = e.outcome;
      decorated.push(row);
    }

    if (!sort) {
      return decorated.sort((a, b) => b.filledAtMs - a.filledAtMs);
    }
    const factor = sort.dir === "asc" ? 1 : -1;
    return decorated.sort((a, b) => compareBy(a, b, sort.key) * factor);
  }, [events, marketsById, sort]);

  const visibleRows = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return rows;
    return rows.filter((r) => r.label.toLowerCase().includes(q));
  }, [rows, query]);

  const paged = usePaged(visibleRows, PORTFOLIO_PAGE_SIZE);

  const onSort = (key: SortKey) => {
    setSort((s) => nextSort(s, key));
    paged.setPage(0);
  };

  const onSearch = (v: string) => {
    setQuery(v);
    paged.setPage(0);
  };

  const isEmpty = rows.length === 0;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-3)" }}>
      <PortfolioToolbar
        tabs={tabs}
        search={!isEmpty && <SearchField value={query} onChange={onSearch} />}
      />
      {isEmpty ? (
        <Empty>No trades yet.</Empty>
      ) : visibleRows.length === 0 ? (
        <Empty>No trades match “{query}”.</Empty>
      ) : (
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
            <TradeRow key={r.id} row={r} />
          ))}
          <div style={{ padding: "0 14px" }}>
            <Pager paged={paged} />
          </div>
        </div>
      )}
    </div>
  );
}

function TradeRow({ row }: { row: TradeRowData }) {
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
        <FilledTime ms={row.filledAtMs} />
      </RightCell>
    </Link>
  );
}

/**
 * Price cell — the fill price, with the requested (limit) price shown
 * struck-through before it when the two differ. "—" when price is unknown.
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

/** Fill time — short date over wall-clock, like the history feed's stamps. */
function FilledTime({ ms }: { ms: number }) {
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
      "28px minmax(0, 1.3fr) 56px 48px 46px 74px 94px 82px 70px 96px",
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
