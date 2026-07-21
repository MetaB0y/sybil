"use client";

/**
 * Trades tab — one row per ORDER that executed (not per fill). Built from the
 * account history feed: we group `filled` + `partial_fill` events by `order_id`
 * and aggregate them, dropping the order-lifecycle noise (placed / cancelled /
 * expired / rejected). A single order can fill across hundreds of batches (the
 * matching engine nibbles a resting limit order a few shares per block), so one
 * bet is one row here — users think in orders, not partial executions.
 *
 * Shares the design language of `OpenOrdersList` / the history feed (card,
 * thumbnail, click-to-sort headers, `Link` rows, paginated footer). Grid rows:
 *   thumb · market · action · side · qty · price · welfare · value · P&L · time
 *
 * Per-row (per-order) derivations, summed over the order's fills:
 *   - qty     = total filled shares across all fills.
 *   - price   = volume-weighted avg execution price (notional ÷ total qty); the
 *               order's limit (requested) price shows struck-through before it
 *               when they differ. The limit comes from the `placed` event.
 *   - welfare = Σ (limit − fill) × qty, signed by side (buyer below limit /
 *               seller above = positive surplus). Null without a known limit.
 *   - value   = Σ qty × price (total notional $).
 *   - P&L     = Σ realized PnL across the order's SELL fills (buys show "—").
 *   - time    = the order's most recent fill.
 * Default order is newest-first by last fill; every column is click-to-sort.
 *
 * Toolbar: a market filter (shared `FilterDropdown`, same as History). Every row
 * links to its market; orders with more than one fill also get a "show partial
 * fills" button that expands their individual partial fills inline, paginated
 * (`FILLS_SUBPAGE`) since one order can fill across hundreds of batches.
 */

import { Download } from "lucide-react";
import Link from "next/link";
import { useMemo, useState } from "react";
import { MarketThumb } from "@/components/market-thumb";
import {
  Pager,
  usePaged,
  PORTFOLIO_PAGE_SIZE,
} from "@/components/event-list-pager";
import { Glossary } from "@/components/glossary";
import { fillRowCount, fillsToCsv, downloadCsv } from "@/lib/account/fills-csv";
import {
  formatShareUnits,
  notionalNanos,
  priceNanosFromNotional,
} from "@/lib/account/quantity";
import type { HistoryEvent } from "@/lib/account/use-account-history";
import { formatCentsPrecise, formatDollars } from "@/lib/format/nanos";
import type { components } from "@/lib/api/schema";
import { FilterDropdown } from "./filter-dropdown";
import { PortfolioToolbar } from "./portfolio-toolbar";
import { SearchField } from "./search-field";
import { SidePill, valueChipStyle } from "./side-pill";
import {
  ActionCell,
  bodyRowGrid,
  cmpNullableBig,
  Empty,
  headerRowGrid,
  MarketLabel,
  Muted,
  nextSort,
  PagerFooter,
  RightCell,
  SortHeader,
  TableCard,
  TimeCell,
  type Column,
  type Sort,
} from "./table";

type Market = components["schemas"]["MarketResponse"];

/** Page size for an expanded order's partial-fills sub-list. */
const FILLS_SUBPAGE = 12;

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
  /** This order's individual partial/full fills, newest-first (for expansion). */
  fills: HistoryEvent[];
}

/** Mutable accumulator while folding an order's fills into one row. */
interface TradeAgg {
  orderId: number | null;
  marketId: number;
  side?: "BUY" | "SELL";
  outcome?: "YES" | "NO";
  totalQty: number;
  hasQty: boolean;
  valueNanos: bigint; // Σ qty × price (notional) + VWAP numerator
  hasValue: boolean;
  welfareNanos: bigint;
  hasWelfare: boolean;
  realizedPnlNanos: bigint;
  hasPnl: boolean;
  lastAtMs: number;
  fills: HistoryEvent[];
}

/**
 * Group key for collapsing fills into one trade row: by order when known, else
 * the fill's own event id (so an order-less fill still shows as its own row).
 */
function tradeGroupKey(e: HistoryEvent): string {
  return e.orderId != null ? `o${e.orderId}` : `e${e.id}`;
}

/**
 * Count of distinct executed orders (= number of Trades-tab rows). Exported so
 * the tab badge in `portfolio/page.tsx` matches the list exactly.
 */
export function tradeOrderCount(events: HistoryEvent[]): number {
  const keys = new Set<string>();
  for (const e of events) {
    if (e.type === "filled" || e.type === "partial_fill")
      keys.add(tradeGroupKey(e));
  }
  return keys.size;
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
/** Qty is 62px because it renders shares ("150.25"), not raw units. */
const GRID = "28px minmax(0, 1.3fr) 56px 48px 62px 74px 94px 82px 70px 96px";

const COLUMNS: Column<SortKey>[] = [
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

/** Every column but the three text ones sorts high→low on first click. */
function isNumericColumn(key: SortKey): boolean {
  return key !== "market" && key !== "action" && key !== "side";
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
      return cmpNullableBig(a.priceNanos, b.priceNanos);
    case "welfare":
      return cmpNullableBig(a.welfareNanos, b.welfareNanos);
    case "value":
      return cmpNullableBig(a.valueNanos, b.valueNanos);
    case "pnl":
      return cmpNullableBig(a.realizedPnlNanos, b.realizedPnlNanos);
    case "time":
      return a.filledAtMs - b.filledAtMs;
  }
}

interface Props {
  tabs: React.ReactNode;
  events: HistoryEvent[];
  marketsById: Map<number, Market>;
  /** market_id → natural question title (see `portfolio/page.tsx`). */
  titleByMarket: Map<number, string>;
  retentionLimited?: boolean;
}

/**
 * Client-side CSV export of the account's retained fill history (one row per
 * fill). Pure browser Blob download — no server call. Disabled when there are
 * no fills to export.
 */
function ExportCsvButton({
  events,
  marketsById,
  retentionLimited = false,
}: {
  events: HistoryEvent[];
  marketsById: Map<number, Market>;
  retentionLimited?: boolean;
}) {
  const count = useMemo(() => fillRowCount(events), [events]);
  const onExport = () => {
    if (count === 0) return;
    const stamp = new Date().toISOString().slice(0, 10); // YYYY-MM-DD
    const scope = retentionLimited ? "retained-fills" : "fills";
    downloadCsv(`sybil-${scope}-${stamp}.csv`, fillsToCsv(events, marketsById));
  };
  return (
    <button
      type="button"
      onClick={onExport}
      disabled={count === 0}
      aria-label="Export fills as CSV"
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        padding: "5px 10px",
        background: "var(--surface-1)",
        border: "1px solid var(--border-2)",
        borderRadius: 6,
        color: count === 0 ? "var(--fg-4)" : "var(--fg-2)",
        fontFamily: "var(--font-sans)",
        fontSize: 12,
        fontWeight: 500,
        cursor: count === 0 ? "not-allowed" : "pointer",
        whiteSpace: "nowrap",
      }}
    >
      <Download size={13} aria-hidden />
      {retentionLimited ? "Export retained CSV" : "Export CSV"}
    </button>
  );
}

export function TradesList({
  tabs,
  events,
  marketsById,
  titleByMarket,
  retentionLimited = false,
}: Props) {
  const [sort, setSort] = useState<Sort<SortKey> | null>(null);
  const [query, setQuery] = useState("");
  const [marketId, setMarketId] = useState<number | "all">("all");

  const rows = useMemo<TradeRowData[]>(() => {
    // First pass: the limit (requested) price per order, from `placed` events,
    // so each row can show its welfare vs the order's limit.
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

    // Second pass: fold every fill / partial fill into its order's accumulator.
    const byOrder = new Map<string, TradeAgg>();
    for (const e of events) {
      if (e.type !== "filled" && e.type !== "partial_fill") continue;
      if (e.marketId == null) continue;

      const key = tradeGroupKey(e);
      let agg = byOrder.get(key);
      if (!agg) {
        agg = {
          orderId: e.orderId ?? null,
          marketId: e.marketId,
          totalQty: 0,
          hasQty: false,
          valueNanos: 0n,
          hasValue: false,
          welfareNanos: 0n,
          hasWelfare: false,
          realizedPnlNanos: 0n,
          hasPnl: false,
          lastAtMs: e.timestampMs,
          fills: [],
        };
        if (e.side) agg.side = e.side;
        if (e.outcome) agg.outcome = e.outcome;
        byOrder.set(key, agg);
      }

      agg.fills.push(e); // events arrive newest-first, so fills stay newest-first
      if (e.qty != null) {
        agg.totalQty += e.qty;
        agg.hasQty = true;
      }
      if (e.qty != null && e.priceNanos != null) {
        agg.valueNanos += notionalNanos(e.priceNanos, e.qty);
        agg.hasValue = true;
      }
      const limit = e.orderId != null ? limitByOrder.get(e.orderId) : undefined;
      if (
        limit != null &&
        e.side != null &&
        e.priceNanos != null &&
        e.qty != null
      ) {
        const edge = notionalNanos(limit - e.priceNanos, e.qty);
        agg.welfareNanos += e.side === "BUY" ? edge : -edge;
        agg.hasWelfare = true;
      }
      if (e.side === "SELL" && e.realizedPnlNanos != null) {
        agg.realizedPnlNanos += e.realizedPnlNanos;
        agg.hasPnl = true;
      }
      if (e.timestampMs > agg.lastAtMs) agg.lastAtMs = e.timestampMs;
    }

    // Third pass: materialize one row per order with the aggregates resolved.
    const decorated: TradeRowData[] = [];
    for (const [key, agg] of byOrder) {
      const totalQty = agg.hasQty ? agg.totalQty : null;
      const valueNanos = agg.hasValue ? agg.valueNanos : null;
      // Execution price = volume-weighted average = notional ÷ total qty.
      const priceNanos =
        agg.hasValue && agg.totalQty > 0
          ? priceNanosFromNotional(agg.valueNanos, agg.totalQty)
          : null;
      const row: TradeRowData = {
        id: key,
        marketId: agg.marketId,
        market: marketsById.get(agg.marketId),
        label:
          titleByMarket.get(agg.marketId) ??
          marketsById.get(agg.marketId)?.name ??
          `#${agg.marketId}`,
        filledAtMs: agg.lastAtMs,
        qty: totalQty,
        priceNanos,
        requestedPriceNanos:
          agg.orderId != null ? (limitByOrder.get(agg.orderId) ?? null) : null,
        valueNanos,
        realizedPnlNanos: agg.hasPnl ? agg.realizedPnlNanos : null,
        welfareNanos: agg.hasWelfare ? agg.welfareNanos : null,
        fills: agg.fills,
      };
      if (agg.side) row.side = agg.side;
      if (agg.outcome) row.outcome = agg.outcome;
      decorated.push(row);
    }

    if (!sort) {
      return decorated.sort((a, b) => b.filledAtMs - a.filledAtMs);
    }
    const factor = sort.dir === "asc" ? 1 : -1;
    return decorated.sort((a, b) => compareBy(a, b, sort.key) * factor);
  }, [events, marketsById, titleByMarket, sort]);

  // Markets present in the trades, for the market filter dropdown.
  const marketOptions = useMemo(() => {
    const ids = new Map<number, string>();
    for (const r of rows) {
      if (!ids.has(r.marketId)) ids.set(r.marketId, r.label);
    }
    return [...ids.entries()]
      .map(([id, name]) => ({ id, name }))
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [rows]);

  const visibleRows = useMemo(() => {
    const q = query.trim().toLowerCase();
    return rows.filter((r) => {
      if (marketId !== "all" && r.marketId !== marketId) return false;
      if (q && !r.label.toLowerCase().includes(q)) return false;
      return true;
    });
  }, [rows, query, marketId]);

  const paged = usePaged(visibleRows, PORTFOLIO_PAGE_SIZE);

  const onSort = (key: SortKey) => {
    setSort((s) => nextSort(s, key, isNumericColumn(key)));
    paged.setPage(0);
  };

  const onSearch = (v: string) => {
    setQuery(v);
    paged.setPage(0);
  };

  const isEmpty = rows.length === 0;
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
      >
        {!isEmpty && (
          <>
            <FilterDropdown
              ariaLabel="Filter by market"
              value={String(marketId)}
              onChange={(v) => {
                setMarketId(v === "all" ? "all" : Number(v));
                paged.setPage(0);
              }}
              options={[
                { value: "all", label: "All markets" },
                ...marketOptions.map((m) => ({
                  value: String(m.id),
                  label: m.name,
                })),
              ]}
            />
            <ExportCsvButton
              events={events}
              marketsById={marketsById}
              retentionLimited={retentionLimited}
            />
          </>
        )}
      </PortfolioToolbar>
      {isEmpty ? (
        <Empty>No trades yet.</Empty>
      ) : visibleRows.length === 0 ? (
        <Empty>No trades match these filters.</Empty>
      ) : (
        <TableCard>
          <div style={headerRowGrid(GRID)}>
            <span />
            {COLUMNS.map((col) => (
              <SortHeader key={col.key} col={col} sort={sort} onSort={onSort}>
                {col.info ? <Glossary term={col.info} /> : null}
              </SortHeader>
            ))}
          </div>
          {paged.visible.map((r) => (
            <TradeRow key={r.id} row={r} />
          ))}
          <PagerFooter>
            <Pager paged={paged} />
          </PagerFooter>
        </TableCard>
      )}
    </div>
  );
}

function TradeRow({ row }: { row: TradeRowData }) {
  const [expanded, setExpanded] = useState(false);
  const { market, label, marketId } = row;
  // Only orders with more than one fill are worth expanding into partials.
  const canExpand = row.fills.length > 1;

  // The toggle lives inside the row's <Link>, so stop the click from both
  // navigating to the market and scrolling the focused control into view.
  const toggle = (e: React.SyntheticEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setExpanded((x) => !x);
  };

  return (
    <div>
      <Link
        className="portfolio-row"
        href={`/m/${marketId}`}
        style={{
          ...bodyRowGrid(GRID),
          textDecoration: "none",
          color: "inherit",
          background: expanded ? "var(--surface-2)" : undefined,
        }}
      >
        <MarketThumb
          marketId={marketId}
          name={label}
          imageUrl={market?.market_image_url ?? market?.event_image_url ?? null}
          fallbackIconUrl={
            market?.market_icon_url ?? market?.event_icon_url ?? null
          }
          size={28}
        />
        <span
          style={{
            display: "flex",
            alignItems: "center",
            gap: 10,
            overflow: "hidden",
            minWidth: 0,
          }}
        >
          <MarketLabel>{label}</MarketLabel>
          {canExpand && (
            <span
              role="button"
              tabIndex={0}
              aria-expanded={expanded}
              onMouseDown={(e) => e.preventDefault()}
              onClick={toggle}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") toggle(e);
              }}
              style={{
                flexShrink: 0,
                cursor: "pointer",
                fontFamily: "var(--font-mono)",
                fontSize: 10,
                letterSpacing: "var(--track-wide)",
                color: "var(--accent)",
                whiteSpace: "nowrap",
              }}
            >
              {expanded ? "hide partial fills" : "show partial fills"}
            </span>
          )}
        </span>
        <ActionCell side={row.side} />
        {row.outcome ? <SidePill outcome={row.outcome} /> : <Muted>—</Muted>}
        <RightCell mono>
          {row.qty == null ? "—" : formatShareUnits(row.qty)}
        </RightCell>
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
          {row.valueNanos != null
            ? formatDollars(row.valueNanos, { decimals: 2 })
            : "—"}
        </RightCell>
        <RightCell>
          <PnlCell pnlNanos={row.realizedPnlNanos} />
        </RightCell>
        <RightCell>
          <TimeCell ms={row.filledAtMs} />
        </RightCell>
      </Link>
      {canExpand && expanded && (
        <ExpandedFills
          fills={row.fills}
          requestedPriceNanos={row.requestedPriceNanos}
        />
      )}
    </div>
  );
}

/**
 * The expanded order's individual partial/full fills, paginated (an order can
 * have hundreds). Renders on the same grey panel as the summary row and reuses
 * the main `rowGrid`, so each fill's qty / price / welfare / value / time line
 * up directly under the table's columns (the left identity columns and the P&L
 * column are blank for a fill). Newest-first, matching the summary's grouping.
 */
function ExpandedFills({
  fills,
  requestedPriceNanos,
}: {
  fills: HistoryEvent[];
  requestedPriceNanos: bigint | null;
}) {
  const paged = usePaged(fills, FILLS_SUBPAGE);
  return (
    <div style={{ background: "var(--surface-2)" }}>
      {paged.visible.map((f) => (
        <FillSubRow
          key={f.id}
          fill={f}
          requestedPriceNanos={requestedPriceNanos}
        />
      ))}
      <PagerFooter>
        <Pager paged={paged} />
      </PagerFooter>
    </div>
  );
}

function FillSubRow({
  fill,
  requestedPriceNanos,
}: {
  fill: HistoryEvent;
  requestedPriceNanos: bigint | null;
}) {
  const qty = fill.qty ?? null;
  const price = fill.priceNanos ?? null;
  const valueNanos =
    qty != null && price != null ? notionalNanos(price, qty) : null;
  let welfareNanos: bigint | null = null;
  if (
    requestedPriceNanos != null &&
    fill.side != null &&
    price != null &&
    qty != null
  ) {
    const edge = notionalNanos(requestedPriceNanos - price, qty);
    welfareNanos = fill.side === "BUY" ? edge : -edge;
  }
  // Same 10-column grid as a trade row → columns align. Identity columns
  // (thumb/market/action/side) and the P&L column stay blank for a fill.
  return (
    <div style={bodyRowGrid(GRID)}>
      <span />
      <span />
      <span />
      <span />
      <RightCell mono>{qty == null ? "—" : formatShareUnits(qty)}</RightCell>
      <RightCell mono>
        <PriceCell settledNanos={price} requestedNanos={requestedPriceNanos} />
      </RightCell>
      <RightCell>
        <WelfareCell welfareNanos={welfareNanos} />
      </RightCell>
      <RightCell mono>
        {valueNanos != null ? formatDollars(valueNanos, { decimals: 2 }) : "—"}
      </RightCell>
      {/* Time on one line, spanning the main table's P&L + Time columns. */}
      <span style={{ gridColumn: "span 2", textAlign: "right" }}>
        <TimeCell ms={fill.timestampMs} />
      </span>
    </div>
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
 * Welfare cell — how much better than your limit the order filled, as a small
 * tinted chip that matches the side pill (green surplus / red shortfall / muted
 * exact-fill), with the value in bold as the one intended difference from the
 * side chip. The signed $ answers "how much better".
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
        : formatDollars(pnlNanos, { decimals: 2, sign: true })}
    </span>
  );
}
