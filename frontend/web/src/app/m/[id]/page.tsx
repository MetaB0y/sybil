"use client";

import Link from "next/link";
import { notFound } from "next/navigation";
import { use, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ChartRangeBar,
  RANGE_MS,
  type ChartRange,
} from "@/components/chart-range-bar";
import { EventActivity } from "@/components/event-activity";
import { EventHoldings } from "@/components/event-holdings";
import { MarketRail } from "@/components/market-rail";
import { PlaceOrderModal } from "@/components/market-rail/place-order-modal";
import { MarketThumb } from "@/components/market-thumb";
import { OutcomeLegend } from "@/components/outcome-legend";
import { PriceChart, PriceHistoryNotice } from "@/components/price-chart";
import {
  formatAge,
  formatCompactDollars,
  formatDate,
  formatInt,
} from "@/lib/format/nanos";
import { getCategoryColor, pickDisplayCategory } from "@/lib/categorize";
import { useMarket } from "@/lib/markets/use-market";
import { SelectOutcomeProvider } from "@/lib/market-detail/active-outcome";
import { chartLineSelection } from "@/lib/market-detail/chart-selection";
import { useEventGroup } from "@/lib/market-detail/use-event-group";
import { useMarketStats } from "@/lib/market-detail/use-market-stats";
import { useEventPriceHistory } from "@/lib/markets/use-event-price-history";
import { useEventRaw } from "@/lib/markets/use-event-raw";
import { useEventTraders } from "@/lib/markets/use-event-traders";
import { selectLatestBlock, useStore } from "@/lib/store";

type RouteParams = { id: string };

/**
 * Chart outcome selection, kept per event id. In-page outcome switches keep the
 * selection via component state, but a real navigation away and back (or landing
 * fresh on a sibling) remounts the chart; keying by event restores the user's
 * added lines instead of snapping back to the favourite-first default.
 */
const chartSelectionByEvent = new Map<string, number[]>();

/** Max simultaneous chart lines — matches the legend's `maxSelected`. */
const MAX_CHART_LINES = 8;
const STAT_W = { vol: 38, h24: 38, traders: 24, liq: 34, age: 28 } as const;

/** Wrap gap (px) between legend chip rows — mirrors OutcomeLegend's `gap`. */
const LEGEND_ROW_GAP = 10;
/**
 * Fallback reserved height (px) for the legend before its real row height has
 * been measured (first paint). Once OutcomeLegend reports a measured row height
 * we reserve exactly two rows; see ChartSection. Multi-outcome events get this
 * as a min-height so adding lines wraps the legend into the already-reserved
 * second row instead of growing the header band and shoving the chart down.
 */
const LEGEND_RESERVED_H = 72;

export default function MarketDetailPage({
  params,
}: {
  params: Promise<RouteParams>;
}) {
  const { id } = use(params);
  const initialId = Number(id);

  if (!Number.isFinite(initialId) || initialId < 0) {
    notFound();
  }

  // The active outcome lives in page state, seeded from the route param. Switching
  // outcomes updates this + rewrites the URL via replaceState (see selectOutcome)
  // rather than navigating, so the [id] segment never remounts and the screen
  // doesn't blink. A fresh load / real navigation re-seeds from the new param.
  const [marketId, setMarketId] = useState(initialId);
  // Outcomes visited during this session, oldest first. Switching to an outcome
  // put its line on the chart, but only as a side effect of it being *active* —
  // the moment you switched again it vanished, which made the rail's outcome
  // picker feel like it hadn't done anything. Recording the switch here makes
  // those lines stick; ChartSection unions them into the chart selection.
  const [visitedOutcomeIds, setVisitedOutcomeIds] = useState<number[]>([]);
  const selectOutcome = useCallback((next: number) => {
    setMarketId(next);
    setVisitedOutcomeIds((prev) =>
      prev.includes(next) ? prev : [...prev, next],
    );
    window.history.replaceState(window.history.state, "", `/m/${next}`);
  }, []);
  // Closing a line from the legend also forgets the visit — otherwise the ✕
  // would be undone on the next render by the union above.
  const keepVisited = useCallback((kept: readonly number[]) => {
    setVisitedOutcomeIds((prev) => {
      const next = prev.filter((id) => kept.includes(id));
      return next.length === prev.length ? prev : next;
    });
  }, []);

  const marketQ = useMarket(marketId);
  const market = marketQ.data;
  const [orderOpen, setOrderOpen] = useState(false);
  const openOrder = useCallback(() => setOrderOpen(true), []);
  const closeOrder = useCallback(() => setOrderOpen(false), []);

  return (
    <SelectOutcomeProvider value={selectOutcome}>
      <main className="market-detail-main">
        {marketQ.isPending && <Placeholder>loading market…</Placeholder>}
        {marketQ.isError && !market && (
          <MarketNotFound onRetry={() => marketQ.refetch()} />
        )}

        {market && (
          <>
            {/* Header band — fixed; the chart/rail split scrolls below it. Closed
                state shows in the status pill + the rail's read-only notice, not
                a separate banner row (which shifted the page). */}
            <div className="market-detail-header-pad">
              <Header
                marketId={marketId}
                market={market}
                {...(market.closed === true ? {} : { onPlaceOrder: openOrder })}
              />
            </div>

            <div
              className="market-detail-grid"
              data-testid="market-detail-grid"
            >
              <div className="no-scrollbar market-detail-content">
                <ChartSection
                  marketId={marketId}
                  visitedOutcomeIds={visitedOutcomeIds}
                  onKeepVisited={keepVisited}
                />
                <EventHoldings marketId={marketId} />
                <DescriptionBlock market={market} />
                <EventActivity marketId={marketId} />
              </div>

              <MarketRail marketId={marketId} />
            </div>

            <PlaceOrderModal
              marketId={marketId}
              open={orderOpen}
              onClose={closeOrder}
            />
          </>
        )}
      </main>
    </SelectOutcomeProvider>
  );
}

/**
 * Title, thumbnail and all five stats are scoped to the single market in the
 * URL — never its parent event. `vol / 24h / traders / liq` are real Phase-B
 * fields (see `derive-market-stats.ts`). `batches ~M` stays a cadence-based
 * timestamp approximation — the API does not expose an exact creation height.
 */
function Header({
  marketId,
  market,
  onPlaceOrder,
}: {
  marketId: number;
  market: {
    name: string;
    status: string;
    closed?: boolean | null;
    volume_nanos?: string;
    market_id: number;
    categories?: string[] | null;
    category?: string | null;
    market_end_date_ms?: number | null;
    expiry_timestamp_ms?: number | null;
    market_image_url?: string | null;
    market_icon_url?: string | null;
    event_image_url?: string | null;
    event_icon_url?: string | null;
    event_id?: string | null;
    polymarket_condition_id?: string | null;
  };
  /** Mobile shortcut; desktop ordering stays in the visually vetted rail. */
  onPlaceOrder?: () => void;
}) {
  const { stats } = useMarketStats(marketId);
  const { primary } = pickDisplayCategory(market.categories, market.category);
  const resolvesMs =
    market.market_end_date_ms ?? market.expiry_timestamp_ms ?? null;
  // Provenance (SYB-149): a `polymarket_condition_id` is the mirror linkage
  // (see `isMirror`); its absence means the market was created natively on Sybil.
  const origin = market.polymarket_condition_id != null ? "mirror" : "native";
  const raw = useEventRaw(market.event_id ?? undefined, !!market.event_id).data;
  const rawQuestion = market.polymarket_condition_id
    ? raw?.get(market.polymarket_condition_id)?.question?.trim()
    : undefined;
  const title = rawQuestion || market.name;

  return (
    <header
      className="market-detail-header"
      /* Outcome changes use the deliberately slower, human-vetted blur shared
         with the Lite picker. The custom property is scoped to this header so
         card thumbnails elsewhere retain the fast generic swap. */
      style={
        {
          ["--dur-swap" as string]: "var(--dur-outcome-swap)",
        } as React.CSSProperties
      }
    >
      {/* Three grid areas — thumb / crumb / headline. On a desktop the thumb
          spans both rows down the left. On a phone the breadcrumb takes the
          full width above and the thumb drops down beside the title, because
          the breadcrumb wraps to two lines there and left the icon stranded
          against it with the title starting below. */}
      <span className="market-detail-thumb">
        <MarketThumb
          key={marketId}
          marketId={market.market_id}
          name={market.name}
          imageUrl={market.market_image_url ?? market.event_image_url ?? null}
          fallbackIconUrl={
            market.market_icon_url ?? market.event_icon_url ?? null
          }
          size={56}
        />
      </span>
      {/* Breadcrumb: Markets / ● Category / resolves <date> · status */}
      <div
        key={marketId}
        className="text-mono market-detail-crumb"
        style={{
          display: "flex",
          alignItems: "center",
          flexWrap: "wrap",
          gap: "var(--space-2)",
          fontSize: "10px",
          letterSpacing: "var(--track-wide)",
          textTransform: "uppercase",
          color: "var(--fg-3)",
        }}
      >
        <Link
          className="mobile-action-link"
          href="/"
          style={{ color: "var(--fg-4)", textDecoration: "none" }}
        >
          markets
        </Link>
        <span style={{ color: "var(--fg-4)" }}>/</span>
        {primary ? (
          <span
            style={{ display: "inline-flex", alignItems: "center", gap: 6 }}
          >
            <span
              aria-hidden
              style={{
                width: 6,
                height: 6,
                borderRadius: "50%",
                background: getCategoryColor(primary),
                display: "inline-block",
              }}
            />
            {primary}
          </span>
        ) : (
          <span style={{ color: "var(--fg-4)" }}>uncategorized</span>
        )}
        {resolvesMs != null && (
          <>
            <span style={{ color: "var(--fg-4)" }}>/</span>
            <span>resolves {formatDate(resolvesMs)}</span>
          </>
        )}
        <span style={{ color: "var(--fg-4)" }}>/</span>
        <span>{origin}</span>
      </div>

      <div className="market-detail-headline">
        {/* Ordering lives in the rail; active markets need no redundant badge. */}
        <div className="market-detail-title-row">
          <h1
            key={marketId}
            className="market-detail-title"
            style={{
              animation: "sybil-fade-swap var(--dur-swap) var(--ease-standard)",
            }}
          >
            {title}
          </h1>
          {market.closed === true && (
            <StatusPill status={market.status} closed />
          )}
          {onPlaceOrder && (
            <button
              type="button"
              className="market-detail-place-order"
              onClick={onPlaceOrder}
            >
              Place order
            </button>
          )}
        </div>

        {/* 5-stat meta row, all scoped to this market. Volume, 24h volume,
            traders and liquidity are backend values; age is timestamp-derived. */}
        <div
          className="text-mono"
          style={{
            display: "flex",
            flexWrap: "wrap",
            gap: "var(--space-4)",
            fontSize: "var(--fs-12)",
            color: "var(--fg-3)",
            animation: "sybil-fade-swap var(--dur-swap) var(--ease-standard)",
          }}
        >
          <MetaStat label="vol" valueWidth={STAT_W.vol}>
            <span className="tabular" style={{ color: "var(--fg-2)" }}>
              {market.volume_nanos
                ? formatCompactDollars(market.volume_nanos)
                : "—"}
            </span>
          </MetaStat>
          <MetaStat label="24h" valueWidth={STAT_W.h24}>
            <span className="tabular" style={{ color: "var(--fg-2)" }}>
              {stats ? formatCompactDollars(stats.volume24hNanos) : "—"}
            </span>
          </MetaStat>
          <MetaStat label="traders" valueWidth={STAT_W.traders}>
            <span className="tabular" style={{ color: "var(--fg-2)" }}>
              {stats ? formatInt(stats.traders) : "—"}
            </span>
          </MetaStat>
          <MetaStat label="liq" valueWidth={STAT_W.liq}>
            <span className="tabular" style={{ color: "var(--fg-2)" }}>
              {stats ? formatCompactDollars(stats.liquidityNanos) : "—"}
            </span>
          </MetaStat>
          <MetaStat label="age" valueWidth={STAT_W.age}>
            <span className="tabular" style={{ color: "var(--fg-2)" }}>
              {stats?.marketAgeMs == null ? "—" : formatAge(stats.marketAgeMs)}
            </span>
          </MetaStat>
        </div>
      </div>
    </header>
  );
}

function MetaStat({
  label,
  children,
  valueWidth,
}: {
  label: string;
  children: React.ReactNode;
  valueWidth?: number;
}) {
  return (
    <span style={{ display: "inline-flex", gap: 6 }}>
      <span style={{ color: "var(--fg-4)" }}>{label}</span>
      <span style={{ display: "inline-block", minWidth: valueWidth }}>
        {children}
      </span>
    </span>
  );
}

/**
 * Small uppercase status chip shown beside the market title, color-coded so the
 * market's state reads at a glance: a tradeable market is ACTIVE in the live
 * green (--yes) with a glowing dot; a closed one shows CLOSED in amber (--warn),
 * as does anything else non-tradeable. `closed` wins over `status` since the
 * backend may still report `status: "active"` for a resolved market.
 */
function StatusPill({ status, closed }: { status: string; closed: boolean }) {
  const isActive = !closed && status === "active";
  const label = closed ? "CLOSED" : (status || "active").toUpperCase();
  const color = isActive ? "var(--yes)" : "var(--warn)";
  const soft = isActive ? "var(--yes-soft)" : "var(--warn-soft)";
  return (
    <span
      className="text-mono"
      style={{
        flexShrink: 0,
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        padding: "3px 9px",
        borderRadius: "var(--radius-md)",
        fontSize: "10px",
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
        color,
        background: soft,
        border: `1px solid color-mix(in srgb, ${color} 32%, transparent)`,
      }}
    >
      <span
        aria-hidden
        style={{
          width: 6,
          height: 6,
          borderRadius: "50%",
          background: color,
          flexShrink: 0,
          // A soft halo on the live state reads as "trading now"; closed is a
          // flat dot.
          boxShadow: isActive
            ? `0 0 0 3px color-mix(in srgb, ${color} 24%, transparent)`
            : "none",
        }}
      />
      {label}
    </span>
  );
}

function ChartSection({
  marketId,
  visitedOutcomeIds,
  onKeepVisited,
}: {
  marketId: number;
  /** Outcomes switched to during this visit; their lines stay on the chart. */
  visitedOutcomeIds: number[];
  /** Report the surviving set after a legend ✕ so the visit is forgotten too. */
  onKeepVisited: (kept: readonly number[]) => void;
}) {
  const [range, setRange] = useState<ChartRange>("ALL");
  // Measured height of one legend chip row (reported by OutcomeLegend). We
  // reserve two of these above the chart so wrapping onto a second row fills
  // already-reserved space instead of pushing the chart down.
  const [legendRowH, setLegendRowH] = useState(0);
  const { group, isPending: groupPending } = useEventGroup(marketId);
  const latestBlock = useStore(selectLatestBlock);
  const eventKey = group?.eventId ?? null;

  // marketIds drawn on the chart. `null` = use the favourite-first default.
  // Persisted per event (see `chartSelectionByEvent`) so highlighting a sibling
  // outcome — which routes to its /m/[id] and remounts this section — keeps the
  // user's added lines instead of snapping back to the default.
  const [selectedIds, setSelectedIdsState] = useState<number[] | null>(null);
  const hydratedEvent = useRef<string | null>(null);
  useEffect(() => {
    if (eventKey == null || hydratedEvent.current === eventKey) return;
    hydratedEvent.current = eventKey;
    setSelectedIdsState(chartSelectionByEvent.get(eventKey) ?? null);
  }, [eventKey]);
  const setSelectedIds = useCallback(
    (next: number[] | null) => {
      setSelectedIdsState(next);
      if (next) onKeepVisited(next);
      if (eventKey == null) return;
      if (next && next.length > 0) chartSelectionByEvent.set(eventKey, next);
      else chartSelectionByEvent.delete(eventKey);
    },
    [eventKey, onKeepVisited],
  );

  const outcomes = useMemo(() => group?.outcomes ?? [], [group]);
  const idSet = useMemo(
    () => new Set(outcomes.map((o) => o.marketId)),
    [outcomes],
  );
  // Default chart lines: top 5 outcomes by 24h volume, tie-broken by the larger
  // absolute 24h price move (so a busy — or, when all are quiet, a fast-moving —
  // outcome leads). The user can still toggle the rest via the legend.
  const defaultIds = useMemo(
    () =>
      [...outcomes]
        .sort((a, b) => {
          if (a.volume24hNanos !== b.volume24hNanos) {
            return a.volume24hNanos > b.volume24hNanos ? -1 : 1;
          }
          return Math.abs(b.delta24Cents) - Math.abs(a.delta24Cents);
        })
        .slice(0, 5)
        .map((o) => o.marketId),
    [outcomes],
  );
  // Self-heals across navigation: stale ids from another event drop out, and an
  // empty result falls back to the favourite-first default. Outcomes you have
  // switched to — plus the active one — are APPENDED after that base, never
  // prepended, so adding a line doesn't reshuffle the chart. At the cap the
  // base gives way first: the lines you asked for outrank the defaults.
  const effectiveSelected = useMemo(
    () =>
      chartLineSelection({
        selectedIds,
        visitedIds: visitedOutcomeIds,
        activeId: marketId,
        availableIds: idSet,
        defaultIds,
        max: MAX_CHART_LINES,
      }),
    [selectedIds, idSet, defaultIds, marketId, visitedOutcomeIds],
  );

  // The selected outcome leads and receives the subtle legend treatment; chart
  // lines themselves remain uniformly weighted.
  const highlightId = group?.isMultiOutcome ? marketId : undefined;

  // Fetch history only for the outcomes actually shown (legend caps at 8).
  const history = useEventPriceHistory(effectiveSelected);

  const drawn = useMemo(
    () =>
      outcomes
        .map((outcome, colorIndex) => ({ outcome, colorIndex }))
        .filter((d) => effectiveSelected.includes(d.outcome.marketId)),
    [outcomes, effectiveSelected],
  );

  // Human-vetted chart treatment: independent uniform lines are easier to
  // read than stacked NegRisk bands, whose geometry obscured actual prices.
  const mode = group?.isMultiOutcome ? "lines" : "area";

  // Latest committed block is our "now" reference — ticks each batch, so the
  // sliding range window stays current without a Date.now() call in render.
  const nowMs = latestBlock?.timestamp_ms ?? 0;
  const windowMs = RANGE_MS[range];
  const sinceMs = windowMs == null || nowMs === 0 ? null : nowMs - windowMs;

  // Full-screen "loading…" only while the event group itself resolves. Once the
  // outcomes are known the chart stays mounted across outcome switches and added
  // lines — buildChartSeries handles a not-yet-loaded lane (holds flat / uses
  // live blocks), so a new line just pops in when its history resolves instead
  // of blanking and rebuilding the whole chart.
  const loading = groupPending;
  const eventTraders = useEventTraders(
    group?.isMultiOutcome ? (group.eventId ?? undefined) : undefined,
  ).data;
  const eventVolumeNanos = useMemo(
    () =>
      outcomes.reduce((sum, outcome) => sum + (outcome.volumeNanos ?? 0n), 0n),
    [outcomes],
  );
  const eventVolume24hNanos = useMemo(
    () => outcomes.reduce((sum, outcome) => sum + outcome.volume24hNanos, 0n),
    [outcomes],
  );

  return (
    <section
      style={{
        padding: "var(--space-4) var(--space-5)",
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-lg)",
        boxShadow: "var(--shadow-inset-top)",
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-3)",
      }}
    >
      <div className="market-detail-chart-head" style={{}}>
        {outcomes.length > 0 ? (
          <div
            className="market-detail-chart-legend"
            style={{
              flex: 1,
              minWidth: 0,
              // Reserve two chip rows up front (multi-outcome only) so the chart
              // stays put as the user adds lines. Uses the measured row height
              // once known; LEGEND_RESERVED_H covers the first paint. The legend
              // sits at the top of this block wrapper, so the reserved space
              // falls below it.
              minHeight: group?.isMultiOutcome
                ? legendRowH > 0
                  ? // two rows + the wrap gap, plus a few px of slack so a row
                    // with the slightly-taller "+N more" chip can't nudge it
                    legendRowH * 2 + LEGEND_ROW_GAP + 6
                  : LEGEND_RESERVED_H
                : undefined,
            }}
          >
            <OutcomeLegend
              outcomes={outcomes}
              selectedIds={effectiveSelected}
              onChange={setSelectedIds}
              highlightId={highlightId}
              onRowHeight={setLegendRowH}
            />
          </div>
        ) : (
          <div className="eyebrow market-detail-chart-legend">
            yes probability
          </div>
        )}
        <div className="market-detail-chart-range" style={{ flexShrink: 0 }}>
          <ChartRangeBar value={range} onChange={setRange} />
        </div>
      </div>
      {loading ? (
        <div
          className="text-mono"
          style={{
            height: 304,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--fg-3)",
          }}
        >
          loading…
        </div>
      ) : (
        <>
          <PriceHistoryNotice
            failureCount={history.failureCount}
            unavailableCount={history.unavailableCount}
            retrying={history.isRetrying}
            onRetry={() => void history.retryFailed()}
          />
          <PriceChart
            drawn={drawn}
            byMarket={history.byMarket}
            mode={mode}
            sinceMs={sinceMs}
            nowMs={nowMs}
            historyPending={history.isPending}
            historyUnavailable={history.unavailableCount > 0}
          />
        </>
      )}

      {!loading && group?.isMultiOutcome && (
        <div
          style={{
            display: "flex",
            flexWrap: "wrap",
            gap: "var(--space-4)",
            paddingTop: "var(--space-3)",
            borderTop: "1px solid var(--border-1)",
            fontFamily: "var(--font-mono)",
            fontSize: "var(--fs-12)",
            color: "var(--fg-3)",
          }}
        >
          <MetaStat label="event vol">
            <span className="tabular" style={{ color: "var(--fg-2)" }}>
              {formatCompactDollars(eventVolumeNanos)}
            </span>
          </MetaStat>
          <MetaStat label="24h vol">
            <span className="tabular" style={{ color: "var(--fg-2)" }}>
              {formatCompactDollars(eventVolume24hNanos)}
            </span>
          </MetaStat>
          <MetaStat label="traders">
            <span className="tabular" style={{ color: "var(--fg-2)" }}>
              {eventTraders != null ? formatInt(eventTraders) : "—"}
            </span>
          </MetaStat>
        </div>
      )}
    </section>
  );
}

function DescriptionBlock({
  market,
}: {
  market: {
    description?: string | null;
    resolution_criteria?: string | null;
    external_url?: string | null;
    event_id?: string | null;
    polymarket_condition_id?: string | null;
  };
}) {
  // Prefer the live Polymarket event JSON (full description + resolution source)
  // over the sparse on-block metadata; join per market by condition id. Falls
  // back to on-block fields when the event has no `/raw` snapshot.
  const raw = useEventRaw(market.event_id ?? undefined, !!market.event_id).data;
  const rawMarket = market.polymarket_condition_id
    ? raw?.get(market.polymarket_condition_id)
    : undefined;
  const description =
    rawMarket?.description?.trim() || market.description?.trim() || null;
  const resolutionCriteria = market.resolution_criteria?.trim() || null;
  // The "source" must be the *specific* resolution source, not a generic page.
  // For Polymarket markets (those with a `/raw` entry) `external_url` is just
  // the Polymarket event page, so we only trust Gamma's `resolutionSource`
  // (e.g. oil markets link pythdata / cmegroup). When that's empty we say so
  // rather than passing off the event page as a source. Sybil-native markets
  // (no `/raw`) have no Gamma field, so there `external_url` is the source.
  const sourceUrl = rawMarket
    ? rawMarket.resolutionSource?.trim() || null
    : market.external_url?.trim() || null;
  // Native markets (SYB-149/151) point `external_url` at the resolution source,
  // so name it as such; for mirrors the link is the upstream Gamma source page.
  const sourceLabel =
    market.polymarket_condition_id != null
      ? "source link"
      : "resolution source";

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
      {description && (
        <div>
          <div className="eyebrow" style={{ marginBottom: "var(--space-2)" }}>
            description
          </div>
          <p
            style={{
              fontSize: "var(--fs-14)",
              lineHeight: "var(--lh-20)",
              color: "var(--fg-2)",
              margin: 0,
              whiteSpace: "pre-wrap",
            }}
          >
            {description}
          </p>
        </div>
      )}
      {resolutionCriteria && (
        <div>
          <div className="eyebrow" style={{ marginBottom: "var(--space-2)" }}>
            resolution
          </div>
          <p
            style={{
              fontSize: "var(--fs-13)",
              lineHeight: "var(--lh-18)",
              color: "var(--fg-3)",
              margin: 0,
              whiteSpace: "pre-wrap",
            }}
          >
            {resolutionCriteria}
          </p>
        </div>
      )}
      <div>
        <div className="eyebrow" style={{ marginBottom: "var(--space-2)" }}>
          {sourceLabel}
        </div>
        {sourceUrl ? (
          <a
            className="mobile-action-link"
            href={sourceUrl}
            target="_blank"
            rel="noreferrer noopener"
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: "var(--space-2)",
              color: "var(--accent)",
              textDecoration: "none",
              fontFamily: "var(--font-mono)",
              fontSize: "var(--fs-12)",
              letterSpacing: "var(--track-wide)",
              textTransform: "uppercase",
            }}
          >
            source ↗
          </a>
        ) : (
          <span
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: "var(--fs-12)",
              color: "var(--fg-4)",
              letterSpacing: "var(--track-wide)",
              textTransform: "uppercase",
            }}
          >
            no specific link
          </span>
        )}
      </div>
    </section>
  );
}

/**
 * Friendly replacement for the raw `error: Error: fetch … failed` string when a
 * market can't be loaded — most often a bad/stale id, but also a transient
 * network drop, so we offer both a way back and a retry.
 */
function MarketNotFound({ onRetry }: { onRetry: () => void }) {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        gap: "var(--space-4)",
        padding: "var(--space-8) var(--space-4)",
        textAlign: "center",
      }}
    >
      <h1
        style={{
          fontFamily: "var(--font-display)",
          fontWeight: 600,
          fontSize: "var(--fs-24)",
          color: "var(--fg-1)",
          margin: 0,
        }}
      >
        Market not found
      </h1>
      <p
        className="text-mono"
        style={{
          color: "var(--fg-3)",
          fontSize: "var(--fs-13)",
          maxWidth: 420,
          margin: 0,
          lineHeight: "var(--lh-20)",
        }}
      >
        This market doesn&apos;t exist or couldn&apos;t be loaded. It may have
        been removed, or the connection dropped.
      </p>
      <div style={{ display: "flex", gap: "var(--space-3)" }}>
        <button
          type="button"
          onClick={onRetry}
          className="text-mono"
          style={{
            minHeight: 40,
            padding: "8px 16px",
            borderRadius: "var(--radius-md)",
            border: "1px solid var(--border-2)",
            background: "var(--surface-1)",
            color: "var(--fg-1)",
            fontSize: "11px",
            textTransform: "uppercase",
            letterSpacing: "var(--track-wide)",
            cursor: "pointer",
          }}
        >
          Retry
        </button>
        <Link
          href="/"
          className="text-mono"
          style={{
            display: "inline-flex",
            alignItems: "center",
            minHeight: 40,
            padding: "8px 16px",
            borderRadius: "var(--radius-md)",
            border: 0,
            background: "var(--accent)",
            color: "var(--fg-on-accent)",
            fontSize: "11px",
            textTransform: "uppercase",
            letterSpacing: "var(--track-wide)",
            textDecoration: "none",
          }}
        >
          Back to all markets
        </Link>
      </div>
    </div>
  );
}

function Placeholder({
  children,
  error,
}: {
  children: React.ReactNode;
  error?: boolean;
}) {
  return (
    <div
      className="text-mono"
      style={{
        color: error ? "var(--no)" : "var(--fg-3)",
        padding: "var(--space-6) 0",
        textAlign: "center",
      }}
    >
      {children}
    </div>
  );
}
