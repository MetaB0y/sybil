"use client";

/**
 * EventHoldings — the connected user's activity for this event, scoped to every
 * market in the event (all outcomes of a multi-outcome event, or just the one
 * binary market). Sits in the left column under the chart. A header switcher
 * toggles three views, all scoped to the same event market_id set
 * (`labelByMarket.keys()`):
 *
 *   - Holdings (default): open positions — the sortable table below.
 *   - Open orders: resting orders (`useAccountOrders`), newest-first.
 *   - Closed orders: filled/cancelled/expired orders reconstructed from the
 *     history feed (`useAccountHistory`), grouped by order_id, newest-first.
 *
 * Renders nothing only when disconnected or the event has none of the three, so
 * it never shows an empty shell; each view carries its own empty state.
 *
 * Holdings mark at the live store price — the same source the chart + rail use
 * (`group.outcomes`, fed by `selectPricesByMarketId`) — so the section agrees
 * with the chart on this page. The portfolio endpoint marks positions at the
 * backend's last *clearing* price, which can lag the live price; we only fall
 * back to it (`current_price_nanos` / `value_nanos`) when the store has no
 * price for a market yet. Cost basis is `quantity * avg_entry`; unrealized PnL
 * is value − cost. Entry + current price collapse into one `entry → mark` cell;
 * every column is click-to-sort.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { useAccountSession } from "@/lib/account/use-account";
import { useAccountFills } from "@/lib/account/use-account-fills";
import { useAccountHistory } from "@/lib/account/use-account-history";
import { useAccountOrders } from "@/lib/account/use-account-orders";
import { avgEntryPriceNanos } from "@/lib/account/positions";
import { usePortfolio, type Portfolio } from "@/lib/account/use-portfolio";
import { formatCents, formatDollars, parseNanos } from "@/lib/format/nanos";
import {
  useEventGroup,
  type EventOutcome,
} from "@/lib/market-detail/use-event-group";
import { colorForOutcome } from "@/components/outcome-legend";
import { EventClosedOrders } from "@/components/event-closed-orders";
import { EventOpenOrders } from "@/components/event-open-orders";
import { Pager, usePaged } from "@/components/event-list-pager";
import { SidePill } from "@/components/portfolio/side-pill";

/** Which sub-view the "your holdings" section is showing. */
type View = "holdings" | "open" | "closed";

const VIEW_TABS: { id: View; label: string }[] = [
  { id: "holdings", label: "Holdings" },
  { id: "open", label: "Open orders" },
  { id: "closed", label: "Closed orders" },
];

/** History event types that terminally close an order. */
const CLOSED_TYPES = new Set(["filled", "cancelled", "expired", "rejected"]);

type Position = Portfolio["positions"][number];

/** A position with every sortable value derived once, up front. */
type Holding = {
  position: Position;
  label: string;
  quantity: number;
  outcome: string;
  /** Avg entry price (nanos), or null when cost basis is unknown. */
  avgNanos: bigint | null;
  /** Current/mark price (nanos). */
  markNanos: bigint;
  valueNanos: bigint;
  /** Unrealized PnL (nanos), or null when cost basis is unknown. */
  pnlNanos: bigint | null;
};

type SortKey = "outcome" | "side" | "shares" | "price" | "value" | "pnl";
type SortDir = "asc" | "desc";
type Sort = { key: SortKey; dir: SortDir };

const COLUMNS: { key: SortKey; label: string; align: "left" | "right" }[] = [
  { key: "outcome", label: "Outcome", align: "left" },
  { key: "side", label: "Side", align: "left" },
  { key: "shares", label: "Shares", align: "right" },
  { key: "price", label: "Price", align: "right" },
  { key: "value", label: "Value", align: "right" },
  { key: "pnl", label: "P&L", align: "right" },
];

/** Text columns sort A→Z first; numeric columns sort high→low first. */
function nextSort(prev: Sort | null, key: SortKey): Sort {
  if (prev && prev.key === key) {
    return { key, dir: prev.dir === "asc" ? "desc" : "asc" };
  }
  const numeric = key !== "outcome" && key !== "side";
  return { key, dir: numeric ? "desc" : "asc" };
}

/** −1 / 0 / 1 ascending comparison for a key; nulls (PnL) sort lowest. */
function compareBy(a: Holding, b: Holding, key: SortKey): number {
  switch (key) {
    case "outcome":
      return a.label.localeCompare(b.label);
    case "side":
      return a.outcome.localeCompare(b.outcome);
    case "shares":
      return a.quantity - b.quantity;
    case "price":
      return cmpBig(a.markNanos, b.markNanos);
    case "value":
      return cmpBig(a.valueNanos, b.valueNanos);
    case "pnl":
      if (a.pnlNanos == null && b.pnlNanos == null) return 0;
      if (a.pnlNanos == null) return -1;
      if (b.pnlNanos == null) return 1;
      return cmpBig(a.pnlNanos, b.pnlNanos);
  }
}

function cmpBig(a: bigint, b: bigint): number {
  return a > b ? 1 : a < b ? -1 : 0;
}

export function EventHoldings({ marketId }: { marketId: number }) {
  const session = useAccountSession();
  const accountId = session?.accountId ?? null;
  const { group } = useEventGroup(marketId);
  const portfolio = usePortfolio(accountId);
  const fillsData = useAccountFills(accountId).data;
  const ordersData = useAccountOrders(accountId).data;
  const historyData = useAccountHistory(accountId).events;

  const [sort, setSort] = useState<Sort | null>(null);
  const [view, setView] = useState<View>("holdings");
  // Outcome filter (null = all outcomes). Scopes every view to one market.
  const [selectedMarket, setSelectedMarket] = useState<number | null>(null);

  // shortLabel per market, so each holding reads like the outcome picker. Its
  // keys are exactly this event's market_ids — the single source of truth for
  // scoping every view (positions, orders, history) to this event.
  const labelByMarket = useMemo(
    () =>
      new Map((group?.outcomes ?? []).map((o) => [o.marketId, o.shortLabel])),
    [group],
  );

  // Live store price per market (yes/no), straight from the chart/rail source.
  const liveByMarket = useMemo(
    () =>
      new Map(
        (group?.outcomes ?? []).map((o) => [
          o.marketId,
          { yes: o.yesPriceNanos, no: o.noPriceNanos },
        ]),
      ),
    [group],
  );

  const holdings = useMemo<Holding[]>(() => {
    const fills = fillsData ?? [];
    const eventMarketIds = new Set(labelByMarket.keys());
    const rows = (portfolio.data?.positions ?? []).filter(
      (p) => eventMarketIds.has(p.market_id) && p.quantity !== 0,
    );
    return rows.map((p) => {
      // Prefer the live store price (matches the chart); fall back to the
      // backend mark when the store has nothing for this market yet.
      const live = liveByMarket.get(p.market_id);
      const liveMark =
        p.outcome === "YES" ? live?.yes : p.outcome === "NO" ? live?.no : null;
      const markNanos = liveMark ?? parseNanos(p.current_price_nanos);
      const valueNanos =
        liveMark != null
          ? BigInt(p.quantity) * markNanos
          : parseNanos(p.value_nanos);
      const avgNanos = avgEntryPriceNanos(fills, p.market_id, p.outcome, p);
      const costNanos = avgNanos == null ? null : BigInt(p.quantity) * avgNanos;
      const pnlNanos = costNanos == null ? null : valueNanos - costNanos;
      return {
        position: p,
        label: labelByMarket.get(p.market_id) ?? `#${p.market_id}`,
        quantity: p.quantity,
        outcome: p.outcome,
        avgNanos,
        markNanos,
        valueNanos,
        pnlNanos,
      };
    });
  }, [portfolio.data, fillsData, labelByMarket, liveByMarket]);

  const sorted = useMemo(() => {
    if (!sort) return holdings;
    const factor = sort.dir === "asc" ? 1 : -1;
    return [...holdings].sort((a, b) => compareBy(a, b, sort.key) * factor);
  }, [holdings, sort]);

  // Resting orders for this event's markets, newest-first (created_at_ms desc).
  const eventOrders = useMemo(() => {
    const eventMarketIds = new Set(labelByMarket.keys());
    return (ordersData ?? [])
      .filter((o) => eventMarketIds.has(o.market_id))
      .sort((a, b) => (b.created_at_ms ?? 0) - (a.created_at_ms ?? 0));
  }, [ordersData, labelByMarket]);

  // Does this event have any terminally-closed order in the history feed? Cheap
  // existence scan so the section can render the Closed view; the full per-order
  // reconstruction lives in EventClosedOrders.
  const hasClosed = useMemo(() => {
    const eventMarketIds = new Set(labelByMarket.keys());
    return historyData.some(
      (e) =>
        e.orderId != null &&
        e.marketId != null &&
        eventMarketIds.has(e.marketId) &&
        CLOSED_TYPES.has(e.type),
    );
  }, [historyData, labelByMarket]);

  // Apply the outcome filter for display. The render gate below still considers
  // the *full* event, so picking an outcome with no rows narrows the table
  // rather than hiding the whole section. A scoped label map both relabels and
  // re-scopes the closed-orders view, which keys off `labelByMarket`.
  const outcomes = group?.outcomes ?? [];
  const visibleHoldings = useMemo(
    () =>
      selectedMarket == null
        ? sorted
        : sorted.filter((h) => h.position.market_id === selectedMarket),
    [sorted, selectedMarket],
  );
  // Holdings tab paginates the (outcome-filtered) holdings 10/page; the open +
  // closed tabs paginate inside their own components.
  const holdingsPage = usePaged(visibleHoldings);
  const visibleOrders = useMemo(
    () =>
      selectedMarket == null
        ? eventOrders
        : eventOrders.filter((o) => o.market_id === selectedMarket),
    [eventOrders, selectedMarket],
  );
  const scopedLabelByMarket = useMemo(() => {
    if (selectedMarket == null) return labelByMarket;
    const label = labelByMarket.get(selectedMarket);
    return new Map<number, string>(
      label != null ? [[selectedMarket, label]] : [],
    );
  }, [labelByMarket, selectedMarket]);

  // Render when connected AND this event has at least one of: positions, open
  // orders, or closed orders. Each view shows its own empty state. Default view
  // stays "holdings" — no effect-driven auto-select (avoids set-state-in-effect).
  const hasAny = holdings.length > 0 || eventOrders.length > 0 || hasClosed;
  if (accountId === null || !hasAny) return null;

  return (
    <section
      style={{
        padding: "var(--space-5)",
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-lg)",
        boxShadow: "var(--shadow-inset-top)",
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-3)",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: "var(--space-3)",
          flexWrap: "wrap",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: "var(--space-3)",
            minWidth: 0,
            flexWrap: "wrap",
          }}
        >
          <div className="eyebrow">{"// your positions & orders"}</div>
          {outcomes.length > 1 && (
            <OutcomeFilter
              outcomes={outcomes}
              selected={selectedMarket}
              onChange={(id) => {
                setSelectedMarket(id);
                holdingsPage.setPage(0);
              }}
            />
          )}
        </div>
        <ViewSwitcher value={view} onChange={setView} />
      </div>

      {view === "holdings" ? (
        visibleHoldings.length === 0 ? (
          <Empty>
            {selectedMarket == null
              ? "No holdings in this event."
              : "No holdings in this outcome."}
          </Empty>
        ) : (
          <div>
            <Row header>
              {COLUMNS.map((col) => (
                <HeaderCell
                  key={col.key}
                  col={col}
                  sort={sort}
                  onSort={() => {
                    setSort((s) => nextSort(s, col.key));
                    holdingsPage.setPage(0);
                  }}
                />
              ))}
            </Row>
            {holdingsPage.visible.map((h) => (
              <HoldingRow
                key={`${h.position.market_id}:${h.outcome}`}
                holding={h}
              />
            ))}
            <Pager paged={holdingsPage} />
          </div>
        )
      ) : view === "open" ? (
        <EventOpenOrders
          orders={visibleOrders}
          fills={fillsData ?? []}
          labelByMarket={scopedLabelByMarket}
          accountId={accountId}
          publicKeyHex={session?.publicKeyHex ?? ""}
        />
      ) : (
        <EventClosedOrders
          events={historyData}
          labelByMarket={scopedLabelByMarket}
        />
      )}
    </section>
  );
}

/**
 * Segmented Holdings / Open / Closed switcher on the right of the section
 * header. Matches `RangeTabs` styling: inline flex in a `var(--bg-2)` track,
 * active button `var(--surface-2)` + `var(--fg-1)`, inactive transparent +
 * `var(--fg-3)`, ~11px uppercase mono, with only `background` transitioned.
 */
function ViewSwitcher({
  value,
  onChange,
}: {
  value: View;
  onChange: (v: View) => void;
}) {
  return (
    <div
      style={{
        display: "inline-flex",
        background: "var(--bg-2)",
        border: "1px solid var(--border-1)",
        borderRadius: 4,
        padding: 2,
        gap: 2,
      }}
    >
      {VIEW_TABS.map((t) => {
        const active = value === t.id;
        return (
          <button
            key={t.id}
            type="button"
            onClick={() => onChange(t.id)}
            style={{
              padding: "4px 10px",
              border: 0,
              borderRadius: 3,
              background: active ? "var(--surface-2)" : "transparent",
              color: active ? "var(--fg-1)" : "var(--fg-3)",
              fontFamily: "var(--font-mono)",
              fontSize: 11,
              textTransform: "uppercase",
              letterSpacing: "var(--track-wide)",
              cursor: "pointer",
              transition: "background 120ms",
            }}
          >
            {t.label}
          </button>
        );
      })}
    </div>
  );
}

/**
 * Outcome filter — a compact dropdown that scopes every view (holdings / open /
 * closed) to one of the event's outcomes, defaulting to all. Only rendered for
 * multi-outcome events; a single binary market has nothing to filter. Colored
 * dots match the chart legend (`colorForOutcome`). Mirrors the rail picker's
 * click-outside + Escape close.
 */
function OutcomeFilter({
  outcomes,
  selected,
  onChange,
}: {
  outcomes: EventOutcome[];
  selected: number | null;
  onChange: (id: number | null) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    function close(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") setOpen(false);
    }
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", onKey);
    };
  }, []);

  const selectedIndex = outcomes.findIndex((o) => o.marketId === selected);
  const selectedOutcome = selectedIndex >= 0 ? outcomes[selectedIndex] : null;
  const selectedColor =
    selectedOutcome != null ? colorForOutcome(selectedOutcome, selectedIndex) : null;

  function pick(id: number | null) {
    setOpen(false);
    onChange(id);
  }

  return (
    <div ref={ref} style={{ position: "relative" }}>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-haspopup="listbox"
        aria-expanded={open}
        title="Filter by outcome"
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: 6,
          maxWidth: 200,
          padding: "4px 8px",
          borderRadius: 4,
          background: "var(--bg-2)",
          border: "1px solid var(--border-1)",
          cursor: "pointer",
          fontFamily: "var(--font-mono)",
          fontSize: 11,
          letterSpacing: "var(--track-wide)",
          color: "var(--fg-2)",
        }}
      >
        {selectedColor != null && (
          <span
            aria-hidden
            style={{
              width: 7,
              height: 7,
              borderRadius: "50%",
              background: selectedColor,
              flexShrink: 0,
            }}
          />
        )}
        <span
          style={{
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {selectedOutcome != null ? selectedOutcome.shortLabel : "All outcomes"}
        </span>
        <svg
          aria-hidden
          width="10"
          height="10"
          viewBox="0 0 12 12"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          style={{
            flexShrink: 0,
            transform: open ? "rotate(180deg)" : "none",
            transition: "transform 120ms",
          }}
        >
          <path d="m3 4.5 3 3 3-3" />
        </svg>
      </button>

      {open && (
        <div
          role="listbox"
          style={{
            position: "absolute",
            top: "calc(100% + 4px)",
            left: 0,
            zIndex: 30,
            minWidth: 200,
            background: "var(--surface-2)",
            border: "1px solid var(--border-2)",
            borderRadius: 6,
            padding: 4,
            boxShadow: "var(--shadow-popover, 0 8px 24px rgba(0,0,0,0.4))",
            display: "flex",
            flexDirection: "column",
            gap: 2,
            maxHeight: 280,
            overflowY: "auto",
          }}
        >
          <OutcomeOption
            label="All outcomes"
            selected={selected == null}
            onClick={() => pick(null)}
          />
          {outcomes.map((o, i) => (
            <OutcomeOption
              key={o.marketId}
              label={o.shortLabel}
              title={o.label}
              color={colorForOutcome(o, i)}
              selected={selected === o.marketId}
              onClick={() => pick(o.marketId)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function OutcomeOption({
  label,
  title,
  color,
  selected,
  onClick,
}: {
  label: string;
  title?: string;
  color?: string;
  selected: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role="option"
      aria-selected={selected}
      onClick={onClick}
      title={title ?? label}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: "7px 10px",
        borderRadius: 4,
        background: selected ? "var(--bg-2)" : "transparent",
        border: 0,
        cursor: "pointer",
        textAlign: "left",
        width: "100%",
      }}
      onMouseEnter={(e) => {
        if (!selected) e.currentTarget.style.background = "var(--bg-2)";
      }}
      onMouseLeave={(e) => {
        if (!selected) e.currentTarget.style.background = "transparent";
      }}
    >
      <span
        aria-hidden
        style={{
          width: 8,
          height: 8,
          borderRadius: "50%",
          background: color ?? "var(--fg-4)",
          flexShrink: 0,
        }}
      />
      <span
        style={{
          flex: 1,
          minWidth: 0,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          fontFamily: "var(--font-sans)",
          fontSize: 13,
          color: "var(--fg-1)",
        }}
      >
        {label}
      </span>
    </button>
  );
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
        gap: 4,
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

function HoldingRow({ holding }: { holding: Holding }) {
  const { label, quantity, outcome, avgNanos, markNanos, valueNanos, pnlNanos } =
    holding;
  return (
    <Row>
      <span
        title={label}
        style={{
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          color: "var(--fg-1)",
          fontFamily: "var(--font-sans)",
          fontSize: 13,
        }}
      >
        {label}
      </span>
      <span>
        <SidePill outcome={outcome} />
      </span>
      <Right mono>{quantity}</Right>
      <Right mono>
        {avgNanos == null
          ? formatCents(markNanos)
          : `${formatCents(avgNanos)} → ${formatCents(markNanos)}`}
      </Right>
      <Right mono>{formatDollars(valueNanos, { decimals: 2 })}</Right>
      <Right>
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 12,
            color:
              pnlNanos == null
                ? "var(--fg-3)"
                : pnlNanos >= 0n
                  ? "var(--yes)"
                  : "var(--no)",
          }}
        >
          {pnlNanos == null
            ? "—"
            : formatDollars(pnlNanos, { decimals: 2, sign: true })}
        </span>
      </Right>
    </Row>
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
        gridTemplateColumns: "minmax(0, 1fr) 52px 60px 104px 78px 80px",
        gap: 10,
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
