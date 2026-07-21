"use client";

/**
 * Positions tab. Grid rows matching `PortfolioVariants.jsx:11-60`, now with a
 * market thumbnail, click-to-sort column headers, and pagination:
 *   thumb · market · side · shares · entry¢ · mark¢ · 7d spark · value · pnl · resolves
 *
 * Row chrome (card, grid metrics, headers, cells, hover) comes from
 * `./table` so the four portfolio tabs stay aligned with each other.
 */

import Link from "next/link";
import { useMemo, useState } from "react";
import { MarketThumb } from "@/components/market-thumb";
import {
  Pager,
  usePaged,
  PORTFOLIO_PAGE_SIZE,
} from "@/components/event-list-pager";
import { PortfolioToolbar } from "./portfolio-toolbar";
import { PositionSparkline } from "./position-sparkline";
import { SearchField } from "./search-field";
import { SidePill } from "./side-pill";
import {
  bodyRowGrid,
  cmpBig,
  cmpNullableBig,
  Empty,
  headerRowGrid,
  MarketLabel,
  nextSort,
  PagerFooter,
  RightCell,
  SortHeader,
  TableCard,
  type Column,
  type Sort,
} from "./table";
import { avgEntryPriceNanos } from "@/lib/account/positions";
import type { AccountFill } from "@/lib/account/use-account-fills";
import type { Portfolio } from "@/lib/account/use-portfolio";
import {
  formatShareUnits,
  notionalNanos,
  unitsToShares,
} from "@/lib/account/quantity";
import {
  formatCentsPrecise,
  formatDollars,
  parseNanos,
} from "@/lib/format/nanos";
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

/** P&L is 116px because its amount and percent share one line. */
const GRID = "32px minmax(0, 1.4fr) 50px 70px 60px 60px 88px 80px 116px 110px";

/** The 7d sparkline is the one column with nothing to sort by. */
const SPARK_COLUMN = { spark: "7d" } as const;

const COLUMNS: (Column<SortKey> | typeof SPARK_COLUMN)[] = [
  { key: "market", label: "Market", align: "left" },
  { key: "side", label: "Side", align: "left" },
  { key: "shares", label: "Shares", align: "right" },
  { key: "entry", label: "Entry", align: "right" },
  { key: "mark", label: "Mark", align: "right" },
  SPARK_COLUMN,
  { key: "value", label: "Value", align: "right" },
  { key: "pnl", label: "P&L", align: "right" },
  { key: "resolves", label: "Resolves", align: "right" },
];

/** Ascending comparison; null numbers sort lowest. */
function compareBy(
  a: PositionRowData,
  b: PositionRowData,
  key: SortKey,
): number {
  switch (key) {
    case "market":
      return a.label.localeCompare(b.label);
    case "side":
      return a.outcome.localeCompare(b.outcome);
    case "shares":
      return a.shares - b.shares;
    case "entry":
      return cmpNullableBig(a.avgNanos, b.avgNanos);
    case "mark":
      return cmpBig(a.markNanos, b.markNanos);
    case "value":
      return cmpBig(a.valueNanos, b.valueNanos);
    case "pnl":
      return cmpNullableBig(a.pnlNanos, b.pnlNanos);
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
  const [sort, setSort] = useState<Sort<SortKey> | null>(null);
  const [query, setQuery] = useState("");

  const rows = useMemo<PositionRowData[]>(() => {
    const decorated = positions.map((p) => {
      const market = marketsById.get(p.market_id);
      const avgNanos = avgEntryPriceNanos(fills, p.market_id, p.outcome, p);
      const valueNanos = parseNanos(p.value_nanos);
      const markNanos = parseNanos(p.current_price_nanos);
      const costNanos =
        avgNanos == null ? null : notionalNanos(avgNanos, p.quantity);
      const pnlNanos = costNanos == null ? null : valueNanos - costNanos;
      const pnlPct =
        costNanos == null || costNanos === 0n
          ? null
          : Number((pnlNanos! * 10000n) / costNanos) / 100;
      return {
        position: p,
        market,
        label:
          titleByMarket.get(p.market_id) ?? market?.name ?? `#${p.market_id}`,
        outcome: p.outcome,
        shares: unitsToShares(p.quantity),
        avgNanos,
        markNanos,
        valueNanos,
        pnlNanos,
        pnlPct,
        resolveMs:
          market?.market_end_date_ms ?? market?.event_end_date_ms ?? null,
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
    setSort((s) => nextSort(s, key, key !== "market" && key !== "side"));
    paged.setPage(0);
  };

  const onSearch = (v: string) => {
    setQuery(v);
    paged.setPage(0);
  };

  const isEmpty = positions.length === 0;
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-3)",
      }}
    >
      <PortfolioToolbar
        tabs={tabs}
        search={!isEmpty && <SearchField value={query} onChange={onSearch} />}
      />
      {isEmpty ? (
        <Empty>No open positions.</Empty>
      ) : visibleRows.length === 0 ? (
        <Empty>No positions match “{query}”.</Empty>
      ) : (
        <TableCard>
          <div style={headerRowGrid(GRID)}>
            <span />
            {COLUMNS.map((col) =>
              "spark" in col ? (
                <span key={col.spark} style={{ textAlign: "center" }}>
                  {col.spark}
                </span>
              ) : (
                <SortHeader
                  key={col.key}
                  col={col}
                  sort={sort}
                  onSort={onSort}
                />
              ),
            )}
          </div>
          {paged.visible.map((r) => (
            <PositionRow
              key={`${r.position.market_id}:${r.position.outcome}`}
              row={r}
            />
          ))}
          <PagerFooter>
            <Pager paged={paged} />
          </PagerFooter>
        </TableCard>
      )}
    </div>
  );
}

function PositionRow({ row }: { row: PositionRowData }) {
  const {
    position,
    market,
    label,
    markNanos,
    avgNanos,
    valueNanos,
    pnlNanos,
    pnlPct,
  } = row;

  return (
    <Link
      className="portfolio-row"
      href={`/m/${position.market_id}`}
      style={{
        ...bodyRowGrid(GRID),
        textDecoration: "none",
        color: "inherit",
      }}
    >
      <MarketThumb
        marketId={position.market_id}
        name={label}
        imageUrl={market?.market_image_url ?? market?.event_image_url ?? null}
        fallbackIconUrl={
          market?.market_icon_url ?? market?.event_icon_url ?? null
        }
        size={28}
      />
      <MarketLabel>{label}</MarketLabel>
      <SidePill outcome={position.outcome} />
      <RightCell mono>{formatShareUnits(position.quantity)}</RightCell>
      <RightCell mono>
        {avgNanos == null ? "—" : formatCentsPrecise(avgNanos)}
      </RightCell>
      <RightCell mono>{formatCentsPrecise(markNanos)}</RightCell>
      <span style={{ display: "flex", justifyContent: "center" }}>
        <PositionSparkline
          marketId={position.market_id}
          outcome={position.outcome}
        />
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
