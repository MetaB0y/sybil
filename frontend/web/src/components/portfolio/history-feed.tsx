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
import { MockValue } from "@/components/mock-value";
import { Pager, usePaged, PORTFOLIO_PAGE_SIZE } from "@/components/event-list-pager";
import {
  CATEGORY_OF,
  type HistoryCategory,
  type HistoryEvent,
  type HistoryEventType,
} from "@/lib/account/use-account-history";
import { formatCentsPrecise, formatDollars } from "@/lib/format/nanos";
import { notionalNanosCeil } from "@/lib/account/quantity";
import type { components } from "@/lib/api/schema";
import { FilterDropdown } from "./filter-dropdown";
import { PortfolioToolbar } from "./portfolio-toolbar";
import { SearchField } from "./search-field";
import { SidePill } from "./side-pill";

type Market = components["schemas"]["MarketResponse"];

interface Props {
  tabs: React.ReactNode;
  events: HistoryEvent[];
  marketsById: Map<number, Market>;
  isMock?: boolean;
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
  | "time"
  | "type"
  | "market"
  | "action"
  | "side"
  | "qty"
  | "price"
  | "amount"
  | "block";
type SortDir = "asc" | "desc";
type Sort = { key: SortKey; dir: SortDir };

const COLUMNS: { key: SortKey; label: string; align: "left" | "right" }[] = [
  { key: "time", label: "Time", align: "left" },
  { key: "type", label: "Type", align: "left" },
  { key: "market", label: "Market", align: "left" },
  { key: "action", label: "Action", align: "left" },
  { key: "side", label: "Side", align: "left" },
  { key: "qty", label: "Qty", align: "right" },
  { key: "price", label: "Price", align: "right" },
  { key: "amount", label: "Amount", align: "right" },
  { key: "block", label: "Block", align: "right" },
];

function nextSort(prev: Sort | null, key: SortKey): Sort {
  if (prev && prev.key === key) {
    return { key, dir: prev.dir === "asc" ? "desc" : "asc" };
  }
  const numeric = key === "qty" || key === "price" || key === "amount" || key === "block" || key === "time";
  return { key, dir: numeric ? "desc" : "asc" };
}

function cmpBig(a: bigint, b: bigint): number {
  return a > b ? 1 : a < b ? -1 : 0;
}

interface HistoryRow {
  event: HistoryEvent;
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
      if (ea.priceNanos == null && eb.priceNanos == null) return 0;
      if (ea.priceNanos == null) return -1;
      if (eb.priceNanos == null) return 1;
      return cmpBig(ea.priceNanos, eb.priceNanos);
    case "amount":
      if (ea.amountNanos == null && eb.amountNanos == null) return 0;
      if (ea.amountNanos == null) return -1;
      if (eb.amountNanos == null) return 1;
      return cmpBig(ea.amountNanos, eb.amountNanos);
    case "block":
      return ea.blockHeight - eb.blockHeight;
  }
}

export function HistoryFeed({ tabs, events, marketsById, isMock }: Props) {
  const [category, setCategory] = useState<HistoryCategory>("all");
  const [type, setType] = useState<HistoryEventType | "all">("all");
  const [marketId, setMarketId] = useState<number | "all">("all");
  const [side, setSide] = useState<"BUY" | "SELL" | "all">("all");
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<Sort | null>(null);

  // Markets present in the feed, for the market dropdown.
  const marketOptions = useMemo(() => {
    const ids = new Map<number, string>();
    for (const e of events) {
      if (e.marketId != null && !ids.has(e.marketId)) {
        ids.set(e.marketId, marketsById.get(e.marketId)?.name ?? `#${e.marketId}`);
      }
    }
    return [...ids.entries()]
      .map(([id, name]) => ({ id, name }))
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [events, marketsById]);

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
      marketName:
        e.marketId != null
          ? marketsById.get(e.marketId)?.name ?? `#${e.marketId}`
          : "",
    }));
    if (q) {
      decorated = decorated.filter((r) => r.marketName.toLowerCase().includes(q));
    }
    if (!sort) {
      return decorated.sort((a, b) => b.event.timestampMs - a.event.timestampMs);
    }
    const factor = sort.dir === "asc" ? 1 : -1;
    return decorated.sort((a, b) => compareBy(a, b, sort.key) * factor);
  }, [events, marketsById, category, type, marketId, side, query, sort]);

  const paged = usePaged(rows, PORTFOLIO_PAGE_SIZE);

  const onSort = (key: SortKey) => {
    setSort((s) => nextSort(s, key));
    paged.setPage(0);
  };

  const noData = events.length === 0;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-3)" }}>
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
        {isMock && (
          <span>
            <MockValue
              hint="history feed is mocked; pending backend /events endpoint (per-account event log)"
              variant="pill"
            >
              {" "}
            </MockValue>
          </span>
        )}
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
              options={TYPE_OPTIONS.map((o) => ({ value: String(o.value), label: o.label }))}
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
                ...marketOptions.map((m) => ({ value: String(m.id), label: m.name })),
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
        <div
          style={{
            background: "var(--surface-1)",
            border: "1px solid var(--border-1)",
            borderRadius: 6,
            overflow: "hidden",
          }}
        >
          <div style={rowGrid("var(--fg-4)")}>
            {COLUMNS.map((col) => (
              <SortHeader key={col.key} col={col} sort={sort} onSort={onSort} />
            ))}
          </div>
          {paged.visible.map((r) => (
            <EventRow key={r.event.id} row={r} />
          ))}
          <div style={{ padding: "0 14px" }}>
            <Pager paged={paged} />
          </div>
        </div>
      )}
    </div>
  );
}

function EventRow({ row }: { row: HistoryRow }) {
  const { event, marketName } = row;
  const body = (
    <>
      <TimeCell ms={event.timestampMs} />
      <span>
        <TypeBadge type={event.type} side={event.side} />
      </span>
      <span
        style={{
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          color: marketName ? "var(--fg-1)" : "var(--fg-4)",
          fontFamily: "var(--font-sans)",
          fontSize: 13,
        }}
        title={marketName || undefined}
      >
        {marketName || "—"}
      </span>
      <ActionCell side={event.side} />
      <span>{event.outcome ? <SidePill outcome={event.outcome} /> : <Muted>—</Muted>}</span>
      <RightCell mono>{event.qty ?? "—"}</RightCell>
      <RightCell mono>{priceLabel(event)}</RightCell>
      <AmountCell event={event} />
      <RightCell mono>
        <span style={{ color: "var(--accent)" }}>
          #{event.blockHeight.toLocaleString()}
        </span>
      </RightCell>
    </>
  );

  const style: React.CSSProperties = {
    ...rowGrid("var(--fg-2)"),
    borderTop: "1px solid var(--border-1)",
  };

  if (event.marketId != null) {
    return (
      <Link
        href={`/m/${event.marketId}`}
        style={{ ...style, textDecoration: "none", color: "inherit" }}
      >
        {body}
      </Link>
    );
  }
  return <div style={style}>{body}</div>;
}

/** Buy/sell action — accent for BUY, red for SELL, "—" for non-order events. */
function ActionCell({ side }: { side?: "BUY" | "SELL" | undefined }) {
  const isBuy = side === "BUY";
  const isSell = side === "SELL";
  return (
    <span
      style={{
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        fontWeight: 600,
        letterSpacing: "var(--track-wide)",
        color: isBuy ? "var(--accent)" : isSell ? "var(--no)" : "var(--fg-4)",
      }}
    >
      {isBuy ? "BUY" : isSell ? "SELL" : "—"}
    </span>
  );
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
  if (event.amountNanos != null && event.amountNanos !== 0n && bold.includes(event.type)) {
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
  if (event.type === "placed" && event.priceNanos != null && event.qty != null) {
    const reserved = notionalNanosCeil(event.priceNanos, event.qty);
    return (
      <RightCell mono>
        <span style={{ color: "var(--fg-4)" }} title="reserved margin">
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

/** Date over wall-clock time, like the closed-orders close stamp. */
function TimeCell({ ms }: { ms: number }) {
  const d = new Date(ms);
  const date = d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  const time = d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  return (
    <span
      style={{
        display: "inline-flex",
        flexDirection: "column",
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

function TypeBadge({
  type,
  side,
}: {
  type: HistoryEventType;
  side?: "BUY" | "SELL" | undefined;
}) {
  const { label, tone } = badgeMeta(type, side);
  const palette: Record<string, { fg: string; bg: string }> = {
    yes: { fg: "var(--yes)", bg: "color-mix(in srgb, var(--yes) 14%, transparent)" },
    no: { fg: "var(--no)", bg: "color-mix(in srgb, var(--no) 14%, transparent)" },
    accent: {
      fg: "var(--accent)",
      bg: "color-mix(in srgb, var(--accent) 14%, transparent)",
    },
    muted: { fg: "var(--fg-3)", bg: "var(--fill-subtle)" },
  };
  const c = palette[tone]!;
  return (
    <span
      style={{
        justifySelf: "start",
        padding: "1px 7px",
        background: c.bg,
        color: c.fg,
        borderRadius: 3,
        fontFamily: "var(--font-mono)",
        fontSize: 9.5,
        fontWeight: 600,
        letterSpacing: "var(--track-wide)",
        whiteSpace: "nowrap",
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
  return (
    <button
      type="button"
      onClick={() => onSort(col.key)}
      title={`Sort by ${col.label}`}
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
}

/**
 * Themed filter dropdown — a pill trigger + an anchored popover menu, replacing
 * the native `<select>` (whose option list is unstyled OS chrome that clashes
 * with the dark theme). Closes on outside-click / Escape; the trigger border
 * goes accent when a non-default value is active so set filters read at a
 * glance. The menu right-aligns to the trigger since the filter bar sits at the
 * right edge.
 */

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
        borderRadius: 999,
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

function rowGrid(color: string): React.CSSProperties {
  return {
    display: "grid",
    gridTemplateColumns:
      "64px 84px minmax(0, 1fr) 52px 44px 50px 56px 92px 84px",
    gap: 10,
    alignItems: "center",
    padding: "9px 14px",
    color,
    fontFamily: "var(--font-mono)",
    fontSize: 11,
    letterSpacing: "var(--track-wide)",
  };
}

function RightCell({ children, mono }: { children: React.ReactNode; mono?: boolean }) {
  return (
    <span
      style={{
        textAlign: "right",
        fontFamily: mono ? "var(--font-mono)" : "inherit",
        fontSize: mono ? 12 : undefined,
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
