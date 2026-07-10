"use client";

/**
 * Positions tab. Grid rows matching `PortfolioVariants.jsx:11-60`, now with a
 * market thumbnail, click-to-sort column headers, and pagination:
 *   thumb · market · side · shares · entry¢ · mark¢ · 7d spark · value · pnl · resolves
 */

import Link from "next/link";
import { useMemo, useState } from "react";
import { MarketThumb } from "@/components/market-thumb";
import { Pager, usePaged, PORTFOLIO_PAGE_SIZE } from "@/components/event-list-pager";
import { PortfolioToolbar } from "./portfolio-toolbar";
import { PositionSparkline } from "./position-sparkline";
import { SearchField } from "./search-field";
import { SidePill } from "./side-pill";
import { avgEntryPriceNanos } from "@/lib/account/positions";
import type { AccountFill } from "@/lib/account/use-account-fills";
import type { Portfolio } from "@/lib/account/use-portfolio";
import {
  formatShareUnits,
  notionalNanos,
  unitsToShares,
} from "@/lib/account/quantity";
import { formatCentsPrecise, formatDollars, parseNanos } from "@/lib/format/nanos";
import type { components } from "@/lib/api/schema";

type Market = components["schemas"]["MarketResponse"];
type Position = Portfolio["positions"][number];

interface Props {
  tabs: React.ReactNode;
  positions: Position[];
  fills: AccountFill[];
  marketsById: Map<number, Market>;
  /** market_id → natural question title (see `portfolio/page.tsx`). Falls back
   *  to `market.name`, which for grouped markets is "{event}: {outcome}". */
  titleByMarket: Map<number, string>;
}

/** A position with every sortable value derived once. */
interface PositionRowData {
  position: Position;
  market: Market | undefined;
  label: string;
  outcome: string;
  shares: number;
  avgNanos: bigint | null;
  markNanos: bigint;
  valueNanos: bigint;
  pnlNanos: bigint | null;
  pnlPct: number | null;
  resolveMs: number | null;
}

type SortKey =
  | "market"
  | "side"
  | "shares"
  | "entry"
  | "mark"
  | "value"
  | "pnl"
  | "resolves";
type SortDir = "asc" | "desc";
type Sort = { key: SortKey; dir: SortDir };

/** Text columns sort A→Z first; numeric columns sort high→low first. */
function nextSort(prev: Sort | null, key: SortKey): Sort {
  if (prev && prev.key === key) {
    return { key, dir: prev.dir === "asc" ? "desc" : "asc" };
  }
  const numeric = key !== "market" && key !== "side";
  return { key, dir: numeric ? "desc" : "asc" };
}

function cmpBig(a: bigint, b: bigint): number {
  return a > b ? 1 : a < b ? -1 : 0;
}

/** Ascending comparison; null numbers sort lowest. */
function compareBy(a: PositionRowData, b: PositionRowData, key: SortKey): number {
  switch (key) {
    case "market":
      return a.label.localeCompare(b.label);
    case "side":
      return a.outcome.localeCompare(b.outcome);
    case "shares":
      return a.shares - b.shares;
    case "entry":
      if (a.avgNanos == null && b.avgNanos == null) return 0;
      if (a.avgNanos == null) return -1;
      if (b.avgNanos == null) return 1;
      return cmpBig(a.avgNanos, b.avgNanos);
    case "mark":
      return cmpBig(a.markNanos, b.markNanos);
    case "value":
      return cmpBig(a.valueNanos, b.valueNanos);
    case "pnl":
      if (a.pnlNanos == null && b.pnlNanos == null) return 0;
      if (a.pnlNanos == null) return -1;
      if (b.pnlNanos == null) return 1;
      return cmpBig(a.pnlNanos, b.pnlNanos);
    case "resolves":
      return (a.resolveMs ?? Infinity) - (b.resolveMs ?? Infinity);
  }
}

export function PositionsList({
  tabs,
  positions,
  fills,
  marketsById,
  titleByMarket,
}: Props) {
  const [sort, setSort] = useState<Sort | null>(null);
  const [query, setQuery] = useState("");

  const rows = useMemo<PositionRowData[]>(() => {
    const decorated = positions.map((p) => {
      const market = marketsById.get(p.market_id);
      const avgNanos = avgEntryPriceNanos(fills, p.market_id, p.outcome, p);
      const valueNanos = parseNanos(p.value_nanos);
      const markNanos = parseNanos(p.current_price_nanos);
      const costNanos = avgNanos == null ? null : notionalNanos(avgNanos, p.quantity);
      const pnlNanos = costNanos == null ? null : valueNanos - costNanos;
      const pnlPct =
        costNanos == null || costNanos === 0n
          ? null
          : Number((pnlNanos! * 10000n) / costNanos) / 100;
      return {
        position: p,
        market,
        label: titleByMarket.get(p.market_id) ?? market?.name ?? `#${p.market_id}`,
        outcome: p.outcome,
        shares: unitsToShares(p.quantity),
        avgNanos,
        markNanos,
        valueNanos,
        pnlNanos,
        pnlPct,
        resolveMs: market?.market_end_date_ms ?? market?.event_end_date_ms ?? null,
      } satisfies PositionRowData;
    });
    if (!sort) return decorated;
    const factor = sort.dir === "asc" ? 1 : -1;
    return [...decorated].sort((a, b) => compareBy(a, b, sort.key) * factor);
  }, [positions, fills, marketsById, titleByMarket, sort]);

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

  const isEmpty = positions.length === 0;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-3)" }}>
      <PortfolioToolbar
        tabs={tabs}
        search={!isEmpty && <SearchField value={query} onChange={onSearch} />}
      />
      {isEmpty ? (
        <Empty>No open positions.</Empty>
      ) : visibleRows.length === 0 ? (
        <Empty>No positions match “{query}”.</Empty>
      ) : (
        <div
          className="portfolio-grid-table"
          style={{
            background: "var(--surface-1)",
            border: "1px solid var(--border-1)",
            borderRadius: 6,
            overflowY: "hidden",
          }}
        >
          <div style={rowGrid("var(--fg-4)")}>
            <span />
            <SortHeader col="market" label="Market" align="left" sort={sort} onSort={onSort} />
            <SortHeader col="side" label="Side" align="left" sort={sort} onSort={onSort} />
            <SortHeader col="shares" label="Shares" align="right" sort={sort} onSort={onSort} />
            <SortHeader col="entry" label="Entry" align="right" sort={sort} onSort={onSort} />
            <SortHeader col="mark" label="Mark" align="right" sort={sort} onSort={onSort} />
            <span style={{ textAlign: "center" }}>7d</span>
            <SortHeader col="value" label="Value" align="right" sort={sort} onSort={onSort} />
            <SortHeader col="pnl" label="P&amp;L" align="right" sort={sort} onSort={onSort} />
            <SortHeader col="resolves" label="Resolves" align="right" sort={sort} onSort={onSort} />
          </div>
          {paged.visible.map((r) => (
            <PositionRow key={`${r.position.market_id}:${r.position.outcome}`} row={r} />
          ))}
          <div style={{ padding: "0 14px" }}>
            <Pager paged={paged} />
          </div>
        </div>
      )}
    </div>
  );
}

function PositionRow({ row }: { row: PositionRowData }) {
  const { position, market, label, markNanos, avgNanos, valueNanos, pnlNanos, pnlPct } =
    row;

  return (
    <Link
      href={`/m/${position.market_id}`}
      style={{
        ...rowGrid("var(--fg-2)"),
        textDecoration: "none",
        color: "inherit",
        borderTop: "1px solid var(--border-1)",
        transition: "background var(--dur-fast) var(--ease-standard)",
      }}
    >
      <MarketThumb
        marketId={position.market_id}
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
      <SidePill outcome={position.outcome} />
      <RightCell mono>{formatShareUnits(position.quantity)}</RightCell>
      <RightCell mono>{avgNanos == null ? "—" : formatCentsPrecise(avgNanos)}</RightCell>
      <RightCell mono>{formatCentsPrecise(markNanos)}</RightCell>
      <span style={{ display: "flex", justifyContent: "center" }}>
        <PositionSparkline marketId={position.market_id} outcome={position.outcome} />
      </span>
      <RightCell mono>{formatDollars(valueNanos, { decimals: 2 })}</RightCell>
      <RightCell>
        {/* Amount and percent on ONE line: the percent is a restatement of the
            amount, not a second fact, so it reads as a suffix rather than a
            stacked value. Keeps every row a single text line tall. */}
        <span
          style={{
            display: "inline-flex",
            alignItems: "baseline",
            justifyContent: "flex-end",
            gap: 5,
            fontFamily: "var(--font-mono)",
            whiteSpace: "nowrap",
            color:
              pnlNanos == null
                ? "var(--fg-3)"
                : pnlNanos >= 0n
                  ? "var(--yes)"
                  : "var(--no)",
          }}
        >
          <span style={{ fontSize: 12 }}>
            {pnlNanos == null
              ? "—"
              : formatDollars(pnlNanos, { decimals: 2, sign: true })}
          </span>
          {pnlPct != null && (
            <span style={{ fontSize: 10, opacity: 0.75 }}>
              {`${pnlPct >= 0 ? "+" : ""}${pnlPct.toFixed(2)}%`}
            </span>
          )}
        </span>
      </RightCell>
      <RightCell mono>{formatResolves(market)}</RightCell>
    </Link>
  );
}

function SortHeader({
  col,
  label,
  align,
  sort,
  onSort,
}: {
  col: SortKey;
  label: string;
  align: "left" | "right";
  sort: Sort | null;
  onSort: (key: SortKey) => void;
}) {
  const active = sort?.key === col;
  return (
    <button
      type="button"
      onClick={() => onSort(col)}
      title={`Sort by ${label}`}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 3,
        width: "100%",
        justifyContent: align === "right" ? "flex-end" : "flex-start",
        padding: 0,
        border: 0,
        background: "transparent",
        cursor: "pointer",
        font: "inherit",
        letterSpacing: "var(--track-wide)",
        color: active ? "var(--fg-2)" : "var(--fg-4)",
      }}
    >
      <span style={{ whiteSpace: "nowrap" }}>{label}</span>
      <span style={{ fontSize: 8, lineHeight: 1, opacity: active ? 1 : 0.3 }}>
        {active ? (sort!.dir === "asc" ? "▲" : "▼") : "↕"}
      </span>
    </button>
  );
}

function rowGrid(color: string): React.CSSProperties {
  return {
    display: "grid",
    // P&L is 116px because its amount and percent now share one line.
    gridTemplateColumns:
      "32px minmax(0, 1.4fr) 50px 70px 60px 60px 88px 80px 116px 110px",
    gap: 10,
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

function formatResolves(market: Market | undefined): string {
  const ms = market?.market_end_date_ms ?? market?.event_end_date_ms ?? null;
  if (!ms) return "—";
  const date = new Date(ms);
  const days = Math.round((ms - Date.now()) / 86400000);
  const dateStr = `${date.toLocaleString(undefined, { month: "short" })} ${date.getDate()}`;
  if (days < 0) return `${dateStr} · past`;
  if (days === 0) return `${dateStr} · today`;
  return `${dateStr} · ${days}d`;
}
