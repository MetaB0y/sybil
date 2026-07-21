"use client";

/**
 * Unified history feed — the single History tab (Activity merged in).
 *
 * A sortable, paginated table over the normalized `HistoryEvent` model from
 * `useAccountHistory`. Columns: time · type · market · side · qty · price ·
 * amount · block. Filters: category chips (all/trades/funding/settlement) plus
 * type / market / side dropdowns. Every column is click-to-sort; default order
 * is newest-first.
 */

import Link from "next/link";
import { useMemo, useState } from "react";
import { MarketThumb } from "@/components/market-thumb";
import {
  Pager,
  usePaged,
  PORTFOLIO_PAGE_SIZE,
} from "@/components/event-list-pager";
import {
  CATEGORY_OF,
  type HistoryCategory,
  type HistoryEvent,
  type HistoryEventType,
} from "@/lib/account/use-account-history";
import { formatCentsPrecise, formatDollars } from "@/lib/format/nanos";
import { formatShareUnits, notionalNanosCeil } from "@/lib/account/quantity";
import type { components } from "@/lib/api/schema";
import { DataCard } from "@/components/data-card";
import { useCompactLayout } from "@/lib/responsive/use-compact";
import { FilterDropdown } from "./filter-dropdown";
import { PortfolioToolbar } from "./portfolio-toolbar";
import { SearchField } from "./search-field";
import { SidePill, valueChipStyle } from "./side-pill";
import {
  ActionCell,
  bodyRowGrid,
  cmpNullableBig,
  Empty,
  MarketLabel,
  Muted,
  nextSort,
  PagerFooter,
  RightCell,
  SortHeader,
  TableCard,
  TableHead,
  TimeCell,
  type Column,
  type Sort,
} from "./table";

type Market = components["schemas"]["MarketResponse"];

interface Props {
  tabs: React.ReactNode;
  events: HistoryEvent[];
  marketsById: Map<number, Market>;
  /** market_id → natural question title (see `portfolio/page.tsx`). */
  titleByMarket: Map<number, string>;
}

const CHIPS: { id: HistoryCategory; label: string }[] = [
  { id: "all", label: "All" },
  { id: "trades", label: "Trades" },
  { id: "funding", label: "Funding" },
  { id: "settlement", label: "Settlement" },
];

const TYPE_OPTIONS: { value: HistoryEventType | "all"; label: string }[] = [
  { value: "all", label: "All types" },
  { value: "placed", label: "Placed" },
  { value: "partial_fill", label: "Partial fill" },
  { value: "filled", label: "Filled" },
  { value: "cancelled", label: "Cancelled" },
  { value: "expired", label: "Expired" },
  { value: "rejected", label: "Rejected" },
  { value: "deposit", label: "Deposit" },
  { value: "withdrawal", label: "Withdrawal" },
  { value: "resolved", label: "Resolved" },
  { value: "created", label: "Created" },
];

type SortKey =
  | "market"
  | "type"
  | "action"
  | "side"
  | "qty"
  | "price"
  | "amount"
  | "block"
  | "time";

/* Same shape as the other three tabs: thumbnail and market lead, the timestamp
   closes the row. Qty / price / time widths are the Trades tab's, so the two
   fill-bearing tables line up column for column. */
const GRID =
  "28px minmax(0, 1.3fr) 84px 56px 48px 62px 74px 92px 76px 96px";

const COLUMNS: Column<SortKey>[] = [
  { key: "market", label: "Market", align: "left" },
  { key: "type", label: "Type", align: "left" },
  { key: "action", label: "Action", align: "left" },
  { key: "side", label: "Side", align: "left" },
  { key: "qty", label: "Qty", align: "right" },
  { key: "price", label: "Price", align: "right" },
  { key: "amount", label: "Amount", align: "right" },
  { key: "block", label: "Block", align: "right" },
  { key: "time", label: "Time", align: "right" },
];

/** Every column but the four text ones sorts high→low on first click. */
function isNumericColumn(key: SortKey): boolean {
  return (
    key !== "market" && key !== "type" && key !== "action" && key !== "side"
  );
}

interface HistoryRow {
  event: HistoryEvent;
  market: Market | undefined;
  marketName: string;
}

/** Ascending comparison; null price/amount sort lowest. */
function compareBy(a: HistoryRow, b: HistoryRow, key: SortKey): number {
  const ea = a.event;
  const eb = b.event;
  switch (key) {
    case "time":
      return ea.timestampMs - eb.timestampMs;
    case "type":
      return ea.type.localeCompare(eb.type);
    case "market":
      return a.marketName.localeCompare(b.marketName);
    case "action":
      return (ea.side ?? "").localeCompare(eb.side ?? "");
    case "side":
      return (ea.outcome ?? "").localeCompare(eb.outcome ?? "");
    case "qty":
      return (ea.qty ?? -1) - (eb.qty ?? -1);
    case "price":
      return cmpNullableBig(ea.priceNanos, eb.priceNanos);
    case "amount":
      return cmpNullableBig(ea.amountNanos, eb.amountNanos);
    case "block":
      return ea.blockHeight - eb.blockHeight;
  }
}

export function HistoryFeed({
  tabs,
  events,
  marketsById,
  titleByMarket,
}: Props) {
  const [category, setCategory] = useState<HistoryCategory>("all");
  const [type, setType] = useState<HistoryEventType | "all">("all");
  const [marketId, setMarketId] = useState<number | "all">("all");
  const [side, setSide] = useState<"BUY" | "SELL" | "all">("all");
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<Sort<SortKey> | null>(null);

  // Markets present in the feed, for the market dropdown.
  const marketOptions = useMemo(() => {
    const ids = new Map<number, string>();
    for (const e of events) {
      if (e.marketId != null && !ids.has(e.marketId)) {
        ids.set(
          e.marketId,
          titleByMarket.get(e.marketId) ??
            marketsById.get(e.marketId)?.name ??
            `#${e.marketId}`,
        );
      }
    }
    return [...ids.entries()]
      .map(([id, name]) => ({ id, name }))
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [events, marketsById, titleByMarket]);

  const rows = useMemo<HistoryRow[]>(() => {
    const q = query.trim().toLowerCase();
    const filtered = events.filter((e) => {
      if (category !== "all" && CATEGORY_OF[e.type] !== category) return false;
      if (type !== "all" && e.type !== type) return false;
      if (marketId !== "all" && e.marketId !== marketId) return false;
      if (side !== "all" && e.side !== side) return false;
      return true;
    });
    let decorated = filtered.map((e) => ({
      event: e,
      market: e.marketId != null ? marketsById.get(e.marketId) : undefined,
      marketName:
        e.marketId != null
          ? (titleByMarket.get(e.marketId) ??
            marketsById.get(e.marketId)?.name ??
            `#${e.marketId}`)
          : "",
    }));
    if (q) {
      decorated = decorated.filter((r) =>
        r.marketName.toLowerCase().includes(q),
      );
    }
    if (!sort) {
      return decorated.sort(
        (a, b) => b.event.timestampMs - a.event.timestampMs,
      );
    }
    const factor = sort.dir === "asc" ? 1 : -1;
    return decorated.sort((a, b) => compareBy(a, b, sort.key) * factor);
  }, [
    events,
    marketsById,
    titleByMarket,
    category,
    type,
    marketId,
    side,
    query,
    sort,
  ]);

  const paged = usePaged(rows, PORTFOLIO_PAGE_SIZE);

  const onSort = (key: SortKey) => {
    setSort((s) => nextSort(s, key, isNumericColumn(key)));
    paged.setPage(0);
  };

  const noData = events.length === 0;
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-3)",
      }}
    >
      {/* Tabs + controls share one row. Controls (search + category chips +
          type / market / side dropdowns) only show once there's history. */}
      <PortfolioToolbar
        tabs={tabs}
        search={
          !noData && (
            <SearchField
              value={query}
              onChange={(v) => {
                setQuery(v);
                paged.setPage(0);
              }}
            />
          )
        }
      >
        {!noData && (
          <div style={{ display: "flex", gap: 6 }}>
            {CHIPS.map((c) => (
              <Chip
                key={c.id}
                label={c.label}
                active={category === c.id}
                onClick={() => {
                  setCategory(c.id);
                  paged.setPage(0);
                }}
              />
            ))}
          </div>
        )}
        {!noData && (
          <div style={{ display: "flex", gap: 8 }}>
            <FilterDropdown
              ariaLabel="Filter by event type"
              value={String(type)}
              onChange={(v) => {
                setType(v as HistoryEventType | "all");
                paged.setPage(0);
              }}
              options={TYPE_OPTIONS.map((o) => ({
                value: String(o.value),
                label: o.label,
              }))}
            />
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
            <FilterDropdown
              ariaLabel="Filter by side"
              value={side}
              onChange={(v) => {
                setSide(v as "BUY" | "SELL" | "all");
                paged.setPage(0);
              }}
              options={[
                { value: "all", label: "All sides" },
                { value: "BUY", label: "Buy" },
                { value: "SELL", label: "Sell" },
              ]}
            />
          </div>
        )}
      </PortfolioToolbar>

      {rows.length === 0 ? (
        <Empty>
          {noData ? "No history yet." : "No events match these filters."}
        </Empty>
      ) : (
        <TableCard>
          <TableHead columns={GRID}>
            <span />
            {COLUMNS.map((col) => (
              <SortHeader key={col.key} col={col} sort={sort} onSort={onSort} />
            ))}
          </TableHead>
          {paged.visible.map((r) => (
            <EventRow key={r.event.id} row={r} />
          ))}
          <PagerFooter>
            <Pager paged={paged} />
          </PagerFooter>
        </TableCard>
      )}
    </div>
  );
}

function EventRow({ row }: { row: HistoryRow }) {
  const { event, market, marketName } = row;
  const compact = useCompactLayout();

  const thumb =
    event.marketId != null ? (
      <MarketThumb
        marketId={event.marketId}
        name={marketName}
        imageUrl={market?.market_image_url ?? market?.event_image_url ?? null}
        fallbackIconUrl={
          market?.market_icon_url ?? market?.event_icon_url ?? null
        }
        size={28}
      />
    ) : null;

  if (compact) {
    return (
      <DataCard
        {...(event.marketId != null ? { href: `/m/${event.marketId}` } : {})}
        {...(thumb ? { thumb } : {})}
        title={marketName || <Muted>account event</Muted>}
        chips={
          <>
            <TypeBadge type={event.type} side={event.side} />
            <ActionCell side={event.side} />
            {event.outcome ? <SidePill outcome={event.outcome} /> : null}
            <TimeCell ms={event.timestampMs} />
          </>
        }
        pairs={[
          {
            label: "Qty",
            value: event.qty == null ? "—" : formatShareUnits(event.qty),
          },
          { label: "Price", value: priceLabel(event) },
          { label: "Amount", value: <AmountCell event={event} /> },
          {
            label: "Block",
            value: (
              <span style={{ color: "var(--accent)" }}>
                #{event.blockHeight.toLocaleString()}
              </span>
            ),
          },
        ]}
      />
    );
  }

  const body = (
    <>
      {/* Funding and account events have no market: the thumbnail slot stays
          empty and the label reads "—", so the grid still lines up. */}
      {thumb ?? <span />}
      {marketName ? (
        <MarketLabel>{marketName}</MarketLabel>
      ) : (
        <Muted>—</Muted>
      )}
      <span>
        <TypeBadge type={event.type} side={event.side} />
      </span>
      <ActionCell side={event.side} />
      {event.outcome ? <SidePill outcome={event.outcome} /> : <Muted>—</Muted>}
      {/* `qty` is in SHARE_SCALE units (1000 = 1 share) — printing it raw showed
          a 12.5-share fill as "12500". */}
      <RightCell mono>
        {event.qty == null ? "—" : formatShareUnits(event.qty)}
      </RightCell>
      <RightCell mono>{priceLabel(event)}</RightCell>
      <AmountCell event={event} />
      <RightCell mono>
        <span style={{ color: "var(--accent)" }}>
          #{event.blockHeight.toLocaleString()}
        </span>
      </RightCell>
      <RightCell>
        <TimeCell ms={event.timestampMs} />
      </RightCell>
    </>
  );

  if (event.marketId != null) {
    return (
      <Link
        className="portfolio-row"
        href={`/m/${event.marketId}`}
        style={{
          ...bodyRowGrid(GRID),
          textDecoration: "none",
          color: "inherit",
        }}
      >
        {body}
      </Link>
    );
  }
  // No market to open, so no hover affordance — this row goes nowhere.
  return <div style={bodyRowGrid(GRID)}>{body}</div>;
}

function priceLabel(event: HistoryEvent): React.ReactNode {
  if (
    event.priceNanos != null &&
    ["placed", "partial_fill", "filled"].includes(event.type)
  ) {
    return formatCentsPrecise(event.priceNanos);
  }
  return "—";
}

function AmountCell({ event }: { event: HistoryEvent }) {
  const bold = ["deposit", "withdrawal", "partial_fill", "filled", "resolved"];
  if (
    event.amountNanos != null &&
    event.amountNanos !== 0n &&
    bold.includes(event.type)
  ) {
    const positive = event.amountNanos > 0n;
    return (
      <RightCell mono>
        <span style={{ color: positive ? "var(--yes)" : "var(--no)" }}>
          {formatDollars(event.amountNanos, { decimals: 2, sign: true })}
        </span>
      </RightCell>
    );
  }
  // placed (reserved margin) / cancelled / expired / created → muted reserve or —
  if (
    event.type === "placed" &&
    event.priceNanos != null &&
    event.qty != null
  ) {
    const reserved = notionalNanosCeil(event.priceNanos, event.qty);
    return (
      <RightCell mono>
        <span style={{ color: "var(--fg-4)" }}>
          {formatDollars(reserved, { decimals: 2 })}
        </span>
      </RightCell>
    );
  }
  return (
    <RightCell mono>
      <span style={{ color: "var(--fg-4)" }}>—</span>
    </RightCell>
  );
}

function TypeBadge({
  type,
  side,
}: {
  type: HistoryEventType;
  side?: "BUY" | "SELL" | undefined;
}) {
  const { label, tone } = badgeMeta(type, side);
  const palette: Record<string, { fg: string; bg: string }> = {
    yes: {
      fg: "var(--yes)",
      bg: "color-mix(in srgb, var(--yes) 14%, transparent)",
    },
    no: {
      fg: "var(--no)",
      bg: "color-mix(in srgb, var(--no) 14%, transparent)",
    },
    accent: {
      fg: "var(--accent)",
      bg: "color-mix(in srgb, var(--accent) 14%, transparent)",
    },
    muted: { fg: "var(--fg-3)", bg: "var(--fill-subtle)" },
  };
  const c = palette[tone]!;
  // Same chip as the side pill / status badge (regular weight), left-aligned.
  return (
    <span
      style={{
        ...valueChipStyle({ color: c.fg, bg: c.bg }),
        justifySelf: "start",
      }}
    >
      {label}
    </span>
  );
}

function badgeMeta(
  type: HistoryEventType,
  side?: "BUY" | "SELL" | undefined,
): { label: string; tone: "yes" | "no" | "accent" | "muted" } {
  switch (type) {
    case "created":
      return { label: "CREATED", tone: "muted" };
    case "placed":
      return { label: "PLACED", tone: "accent" };
    case "partial_fill":
      return { label: "PARTIAL", tone: side === "SELL" ? "no" : "yes" };
    case "filled":
      return { label: "FILLED", tone: side === "SELL" ? "no" : "yes" };
    case "cancelled":
      return { label: "CANCELLED", tone: "muted" };
    case "expired":
      return { label: "EXPIRED", tone: "muted" };
    case "rejected":
      return { label: "REJECTED", tone: "no" };
    case "deposit":
      return { label: "DEPOSIT", tone: "yes" };
    case "withdrawal":
      return { label: "WITHDRAWAL", tone: "no" };
    case "resolved":
      return { label: "RESOLVED", tone: "accent" };
  }
}

function Chip({
  label,
  active,
  onClick,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        padding: "4px 12px",
        background: active ? "var(--accent)" : "transparent",
        border: active ? 0 : "1px solid var(--border-1)",
        // Rectangular (radius-sm) to match the markets filter buttons, not a pill.
        borderRadius: "var(--radius-sm)",
        color: active ? "var(--bg-1)" : "var(--fg-3)",
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        fontWeight: active ? 600 : 500,
        letterSpacing: "var(--track-wide)",
        cursor: "pointer",
      }}
    >
      {label}
    </button>
  );
}
