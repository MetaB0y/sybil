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
import {
  formatShareUnits,
  notionalNanos,
  unitsToShares,
} from "@/lib/account/quantity";
import { usePortfolio, type Portfolio } from "@/lib/account/use-portfolio";
import {
  formatCentsPrecise,
  formatDollarsRounded,
  parseNanos,
} from "@/lib/format/nanos";
import {
  useEventGroup,
  type EventOutcome,
} from "@/lib/market-detail/use-event-group";
import { colorForOutcome } from "@/components/outcome-legend";
import { EventClosedOrders } from "@/components/event-closed-orders";
import { EventOpenOrders } from "@/components/event-open-orders";
import { Pager, usePaged } from "@/components/event-list-pager";
import { SidePill } from "@/components/portfolio/side-pill";
import { DataCard } from "@/components/data-card";
import { useCompactLayout } from "@/lib/responsive/use-compact";
import {
  cmpBig,
  cmpNullableBig,
  Empty,
  EventRow,
  EventTable,
  HeaderCell,
  nextSort,
  OutcomeLabel,
  Right,
  type Column,
  type Sort,
} from "@/components/event-table";

/** Which sub-view the "your holdings" section is showing. */
type View = "holdings" | "open" | "closed";

// `short` is what a phone shows: three full labels do not fit one line at
// 390px, and the section heading above them ("your positions & orders") already
// says what kind of thing is being switched.
const VIEW_TABS: { id: View; label: string; short: string }[] = [
  { id: "holdings", label: "Holdings", short: "Holdings" },
  { id: "open", label: "Open orders", short: "Open" },
  { id: "closed", label: "Closed orders", short: "Closed" },
];

const sectionStyle: React.CSSProperties = {
  padding: "var(--space-5)",
  background: "var(--surface-1)",
  border: "1px solid var(--border-1)",
  borderRadius: "var(--radius-lg)",
  boxShadow: "var(--shadow-inset-top)",
  display: "flex",
  flexDirection: "column",
  gap: "var(--space-3)",
};

const readNoticeStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "var(--space-3)",
  color: "var(--warn)",
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-12)",
};

function readRetryStyle(disabled: boolean): React.CSSProperties {
  return {
    minHeight: 32,
    padding: "0 var(--space-3)",
    border: "1px solid var(--border-2)",
    borderRadius: "var(--radius-sm)",
    background: "var(--surface-2)",
    color: "var(--fg-1)",
    font: "inherit",
    cursor: disabled ? "wait" : "pointer",
  };
}

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

/* Outcome / Side / Qty / Price / Value / P&L, laid out on the same slots the
   closed-orders list uses so the two tables line up where they overlap. */
const GRID = "minmax(0, 1fr) 48px 62px 104px 78px 70px";

const COLUMNS: Column<SortKey>[] = [
  { key: "outcome", label: "Outcome", align: "left" },
  { key: "side", label: "Side", align: "left" },
  { key: "shares", label: "Qty", align: "right" },
  { key: "price", label: "Price", align: "right" },
  { key: "value", label: "Value", align: "right" },
  { key: "pnl", label: "P&L", align: "right" },
];

/** Every column but the two text ones sorts high→low on first click. */
function isNumericColumn(key: SortKey): boolean {
  return key !== "outcome" && key !== "side";
}

/** −1 / 0 / 1 ascending comparison for a key; nulls (PnL) sort lowest. */
function compareBy(a: Holding, b: Holding, key: SortKey): number {
  switch (key) {
    case "outcome":
      return a.label.localeCompare(b.label);
    case "side":
      return a.outcome.localeCompare(b.outcome);
    case "shares":
      return unitsToShares(a.quantity) - unitsToShares(b.quantity);
    case "price":
      return cmpBig(a.markNanos, b.markNanos);
    case "value":
      return cmpBig(a.valueNanos, b.valueNanos);
    case "pnl":
      return cmpNullableBig(a.pnlNanos, b.pnlNanos);
  }
}

export function EventHoldings({ marketId }: { marketId: number }) {
  const session = useAccountSession();
  const accountId = session?.accountId ?? null;
  const eventGroup = useEventGroup(marketId);
  const { group } = eventGroup;
  const portfolio = usePortfolio(accountId);
  const fills = useAccountFills(accountId);
  const orders = useAccountOrders(accountId);
  const history = useAccountHistory(accountId);
  const fillsData = fills.data;
  const ordersData = orders.data;
  const historyData = history.events;

  const [sort, setSort] = useState<Sort<SortKey> | null>(null);
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
          ? notionalNanos(markNanos, p.quantity)
          : parseNanos(p.value_nanos);
      const avgNanos = avgEntryPriceNanos(fills, p.market_id, p.outcome, p);
      const costNanos = avgNanos == null ? null : notionalNanos(avgNanos, p.quantity);
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

  // Distinct terminally-closed orders per market in this event. Cheap scan over
  // the history feed — enough to decide whether the Closed view has anything and
  // which outcomes to mark in the filter; the full per-order reconstruction
  // lives in EventClosedOrders.
  const closedOrdersByMarket = useMemo(() => {
    const eventMarketIds = new Set(labelByMarket.keys());
    const byMarket = new Map<number, Set<number>>();
    for (const e of historyData) {
      if (e.orderId == null || e.marketId == null) continue;
      if (!eventMarketIds.has(e.marketId)) continue;
      if (!CLOSED_TYPES.has(e.type)) continue;
      const seen = byMarket.get(e.marketId) ?? new Set<number>();
      seen.add(e.orderId);
      byMarket.set(e.marketId, seen);
    }
    return byMarket;
  }, [historyData, labelByMarket]);
  const hasClosed = closedOrdersByMarket.size > 0;

  // Which outcomes the filter should mark as "you have something here" — a
  // position, a resting order, or a closed order. Without it every outcome in a
  // 12-way event looks equally worth opening, and most lead to an empty table.
  const activeMarkets = useMemo(() => {
    const ids = new Set<number>();
    for (const h of holdings) ids.add(h.position.market_id);
    for (const o of eventOrders) ids.add(o.market_id);
    for (const id of closedOrdersByMarket.keys()) ids.add(id);
    return ids;
  }, [holdings, eventOrders, closedOrdersByMarket]);

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
  const reads = [
    {
      error: eventGroup.error,
      hasData: group !== null,
      isPending: eventGroup.isPending,
      isFetching: eventGroup.isFetching,
      refetch: eventGroup.refetch,
    },
    {
      error: portfolio.error,
      hasData: portfolio.data !== undefined,
      isPending: portfolio.isPending,
      isFetching: portfolio.isFetching,
      refetch: portfolio.refetch,
    },
    {
      error: fills.error,
      hasData: fills.data !== undefined,
      isPending: fills.isPending,
      isFetching: fills.isFetching,
      refetch: fills.refetch,
    },
    {
      error: orders.error,
      hasData: orders.data !== undefined,
      isPending: orders.isPending,
      isFetching: orders.isFetching,
      refetch: orders.refetch,
    },
    {
      error: history.error,
      hasData: history.hasData,
      isPending: history.isPending,
      isFetching: history.isFetching,
      refetch: history.refetch,
    },
  ];
  const failedReads = reads.filter((read) => read.error != null);
  const gate = deriveEventHoldingsGate({
    connected: accountId !== null,
    hasAny,
    pendingWithoutData: reads.some(
      (read) => read.isPending && !read.hasData,
    ),
    failureCount: failedReads.length,
    missingFailureCount: failedReads.filter((read) => !read.hasData).length,
  });
  const retrying = failedReads.some((read) => read.isFetching);
  const retryFailed = () => {
    void Promise.all(failedReads.map((read) => read.refetch()));
  };

  if (accountId === null) return null;
  if (gate === "hidden") return null;
  if (gate === "loading" || gate === "unavailable" || !hasAny) {
    return (
      <section style={sectionStyle}>
        <EventHoldingsReadNotice
          state={
            gate === "loading"
              ? "loading"
              : gate === "unavailable"
                ? "unavailable"
                : "stale"
          }
          retrying={retrying}
          onRetry={retryFailed}
        />
      </section>
    );
  }

  return (
    <section style={sectionStyle}>
      {gate === "stale" && (
        <EventHoldingsReadNotice
          state="stale"
          retrying={retrying}
          onRetry={retryFailed}
        />
      )}
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
          <div className="eyebrow">{"your positions & orders"}</div>
          {outcomes.length > 1 && (
            <OutcomeFilter
              outcomes={outcomes}
              activeMarkets={activeMarkets}
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
          <EventTable>
            <EventRow columns={GRID} header>
              {COLUMNS.map((col) => (
                <HeaderCell
                  key={col.key}
                  col={col}
                  sort={sort}
                  onSort={() => {
                    setSort((s) =>
                      nextSort(s, col.key, isNumericColumn(col.key)),
                    );
                    holdingsPage.setPage(0);
                  }}
                />
              ))}
            </EventRow>
            {holdingsPage.visible.map((h) => (
              <HoldingRow
                key={`${h.position.market_id}:${h.outcome}`}
                holding={h}
              />
            ))}
            <Pager paged={holdingsPage} />
          </EventTable>
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

export type EventHoldingsGate =
  | "hidden"
  | "loading"
  | "unavailable"
  | "stale"
  | "ready";

export function deriveEventHoldingsGate({
  connected,
  hasAny,
  pendingWithoutData,
  failureCount,
  missingFailureCount,
}: {
  connected: boolean;
  hasAny: boolean;
  pendingWithoutData: boolean;
  failureCount: number;
  missingFailureCount: number;
}): EventHoldingsGate {
  if (!connected) return "hidden";
  if (missingFailureCount > 0) return "unavailable";
  if (pendingWithoutData) return "loading";
  if (failureCount > 0) return "stale";
  return hasAny ? "ready" : "hidden";
}

export function EventHoldingsReadNotice({
  state,
  retrying,
  onRetry,
}: {
  state: "loading" | "unavailable" | "stale";
  retrying: boolean;
  onRetry: () => void;
}) {
  if (state === "loading") {
    return (
      <div role="status" aria-live="polite" style={readNoticeStyle}>
        loading your positions & orders…
      </div>
    );
  }

  const stale = state === "stale";
  return (
    <div
      role={stale ? "status" : "alert"}
      aria-live={stale ? "polite" : undefined}
      style={readNoticeStyle}
    >
      <span>
        {stale
          ? "positions & orders refresh failed · showing saved data"
          : "positions & orders unavailable · no account data is shown as empty"}
      </span>
      <button
        type="button"
        disabled={retrying}
        onClick={onRetry}
        style={readRetryStyle(retrying)}
      >
        {retrying ? "retrying…" : "retry"}
      </button>
    </div>
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
  const compact = useCompactLayout();
  return (
    <div
      /* Three 11px labels in one track — see `.hit-target-group`. At the coarse
         floor each grew to 44px, which wrapped "open orders" and "closed
         orders" over two lines and made the switch taller than the card's
         title. */
      className="hit-target-group"
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
              whiteSpace: "nowrap",
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
            {compact ? t.short : t.label}
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
 *
 * Outcomes you actually hold or have traded read at full strength; the rest are
 * dimmed, so a 12-way event's menu shows at a glance which two are worth
 * opening. Empty outcomes stay selectable — the table just says it's empty.
 */
function OutcomeFilter({
  outcomes,
  activeMarkets,
  selected,
  onChange,
}: {
  outcomes: EventOutcome[];
  /** market_ids with a position, resting order, or closed order. */
  activeMarkets: Set<number>;
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
            active
            selected={selected == null}
            onClick={() => pick(null)}
          />
          {outcomes.map((o, i) => (
            <OutcomeOption
              key={o.marketId}
              label={o.shortLabel}
              color={colorForOutcome(o, i)}
              active={activeMarkets.has(o.marketId)}
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
  color,
  active,
  selected,
  onClick,
}: {
  label: string;
  color?: string;
  /** You have a position or an order here — render it at full strength. */
  active: boolean;
  selected: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role="option"
      aria-selected={selected}
      onClick={onClick}
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
          opacity: active ? 1 : 0.4,
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
          fontWeight: active ? 500 : 400,
          color: active ? "var(--fg-1)" : "var(--fg-4)",
        }}
      >
        {label}
      </span>
    </button>
  );
}

function HoldingRow({ holding }: { holding: Holding }) {
  const { label, quantity, outcome, avgNanos, markNanos, valueNanos, pnlNanos } =
    holding;
  const compact = useCompactLayout();

  const price =
    avgNanos == null ? (
      formatCentsPrecise(markNanos)
    ) : (
      // entry → mark. Fade the entry (what you paid, historical) so the eye
      // lands on the mark — the live price that's actually true right now.
      <span>
        <span style={{ color: "var(--fg-4)" }}>
          {formatCentsPrecise(avgNanos)}
        </span>
        <span style={{ color: "var(--fg-4)" }}>{" → "}</span>
        {formatCentsPrecise(markNanos)}
      </span>
    );

  const pnl = (
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
        : formatDollarsRounded(pnlNanos, { decimals: 1, sign: true })}
    </span>
  );

  if (compact) {
    return (
      <DataCard
        title={label}
        chips={
          <>
            <SidePill outcome={outcome} />
            <span>{formatShareUnits(quantity, 1)} shares</span>
          </>
        }
        pairs={[
          { label: "Price", value: price, wide: true },
          {
            label: "Value",
            value: formatDollarsRounded(valueNanos, { decimals: 1 }),
          },
          { label: "P&L", value: pnl },
        ]}
      />
    );
  }

  return (
    <EventRow columns={GRID}>
      <OutcomeLabel>{label}</OutcomeLabel>
      <SidePill outcome={outcome} />
      <Right mono>{formatShareUnits(quantity, 1)}</Right>
      <Right mono>{price}</Right>
      <Right mono>{formatDollarsRounded(valueNanos, { decimals: 1 })}</Right>
      <Right>{pnl}</Right>
    </EventRow>
  );
}
