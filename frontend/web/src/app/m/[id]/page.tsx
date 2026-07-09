"use client";

import Link from "next/link";
import { notFound } from "next/navigation";
import { use, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ChartRangeBar,
  RANGE_MS,
  type ChartRange,
} from "@/components/chart-range-bar";
import { EventHoldings } from "@/components/event-holdings";
import { MarketRail } from "@/components/market-rail";
import { PlaceOrderModal } from "@/components/market-rail/place-order-modal";
import { MarketThumb } from "@/components/market-thumb";
import { OutcomeLegend } from "@/components/outcome-legend";
import { PriceChart } from "@/components/price-chart";
import {
  formatAge,
  formatCompactDollars,
  formatDate,
  formatInt,
} from "@/lib/format/nanos";
import { getCategoryColor, pickDisplayCategory } from "@/lib/categorize";
import { useMarket } from "@/lib/markets/use-market";
import { SelectOutcomeProvider } from "@/lib/market-detail/active-outcome";
import { useEventGroup } from "@/lib/market-detail/use-event-group";
import { useMarketStats } from "@/lib/market-detail/use-market-stats";
import { useEventPriceHistory } from "@/lib/markets/use-event-price-history";
import { useEventRaw } from "@/lib/markets/use-event-raw";
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
  const selectOutcome = useCallback((next: number) => {
    setMarketId(next);
    window.history.replaceState(window.history.state, "", `/m/${next}`);
  }, []);

  const marketQ = useMarket(marketId);
  const market = marketQ.data;

  // Place-order modal (SYB-54) — launched from the header CTA, renders the
  // shared BuyBox for the currently-selected outcome.
  const [orderOpen, setOrderOpen] = useState(false);

  return (
    <SelectOutcomeProvider value={selectOutcome}>
      <main
        className="market-detail-main"
      >
        {marketQ.isPending && <Placeholder>loading market…</Placeholder>}
        {marketQ.isError && (
          <Placeholder error>error: {String(marketQ.error)}</Placeholder>
        )}

        {market && (
          <>
            {/* Header band — fixed; the chart/rail split scrolls below it. Closed
                state shows in the status pill + the rail's read-only notice, not
                a separate banner row (which shifted the page). */}
            <div
              className="market-detail-header-pad"
            >
              <Header
                marketId={marketId}
                market={market}
                {...(market.closed === true
                  ? {}
                  : { onPlaceOrder: () => setOrderOpen(true) })}
              />
            </div>

            <div
              className="market-detail-grid"
            >
              <div
                className="no-scrollbar market-detail-content"
              >
                <ChartSection marketId={marketId} />
                <EventHoldings marketId={marketId} />
                <DescriptionBlock market={market} />
                <DiscussionPlaceholder />
              </div>

              <MarketRail marketId={marketId} />
            </div>

            <PlaceOrderModal
              marketId={marketId}
              open={orderOpen}
              onClose={() => setOrderOpen(false)}
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
 * timestamp approximation — an exact count needs a backend `created_at_height`
 * (OPEN_QUESTIONS #9).
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
    polymarket_condition_id?: string | null;
  };
  /** Opens the place-order modal. Omitted (no button) when the market is closed. */
  onPlaceOrder?: () => void;
}) {
  const { stats } = useMarketStats(marketId);
  const { primary } = pickDisplayCategory(market.categories, market.category);
  const resolvesMs = market.market_end_date_ms ?? market.expiry_timestamp_ms ?? null;
  // Provenance (SYB-149): a `polymarket_condition_id` is the mirror linkage
  // (see `isMirror`); its absence means the market was created natively on Sybil.
  const origin = market.polymarket_condition_id != null ? "mirror" : "native";

  return (
    <header
      className="market-detail-header"
    >
      <MarketThumb
        marketId={market.market_id}
        name={market.name}
        imageUrl={market.market_image_url ?? market.event_image_url ?? null}
        fallbackIconUrl={
          market.market_icon_url ?? market.event_icon_url ?? null
        }
        size={56}
      />
      <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-2)", minWidth: 0 }}>
        {/* Breadcrumb: Markets / ● Category / resolves <date> · status */}
        <div
          className="text-mono"
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
          <Link href="/" style={{ color: "var(--fg-4)", textDecoration: "none" }}>
            markets
          </Link>
          <span style={{ color: "var(--fg-4)" }}>/</span>
          {primary ? (
            <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
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

        {/* Title + status pill */}
        <div
          className="market-detail-title-row"
          style={{
          }}
        >
          <h1
            className="market-detail-title"
          >
            {market.name}
          </h1>
          <StatusPill status={market.status} closed={market.closed === true} />
          {onPlaceOrder && (
            <button
              type="button"
              onClick={onPlaceOrder}
              style={{
                marginLeft: "auto",
                flexShrink: 0,
                minHeight: 40,
                padding: "8px 16px",
                borderRadius: "var(--radius-md)",
                border: 0,
                background: "var(--accent)",
                color: "var(--fg-on-accent)",
                fontFamily: "var(--font-sans)",
                fontSize: "var(--fs-13)",
                fontWeight: 600,
                letterSpacing: "0.01em",
                cursor: "pointer",
              }}
            >
              Place order
            </button>
          )}
        </div>

        {/* 5-stat meta row, all scoped to this market. Only `batches` is an
            approximation (OPEN_QUESTIONS #9); vol / 24h / traders / liq are real. */}
        <div
          className="text-mono"
          style={{
            display: "flex",
            flexWrap: "wrap",
            gap: "var(--space-4)",
            fontSize: "var(--fs-12)",
            color: "var(--fg-3)",
          }}
        >
          <MetaStat label="vol">
            <span className="tabular" style={{ color: "var(--fg-2)" }}>
              {market.volume_nanos ? formatCompactDollars(market.volume_nanos) : "—"}
            </span>
          </MetaStat>
          <MetaStat label="24h">
            <span className="tabular" style={{ color: "var(--fg-2)" }}>
              {stats ? formatCompactDollars(stats.volume24hNanos) : "—"}
            </span>
          </MetaStat>
          <MetaStat label="traders">
            <span className="tabular" style={{ color: "var(--fg-2)" }}>
              {stats ? formatInt(stats.traders) : "—"}
            </span>
          </MetaStat>
          <MetaStat label="liq">
            <span className="tabular" style={{ color: "var(--fg-2)" }}>
              {stats ? formatCompactDollars(stats.liquidityNanos) : "—"}
            </span>
          </MetaStat>
          <MetaStat label="age">
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
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <span style={{ display: "inline-flex", gap: 6 }}>
      <span style={{ color: "var(--fg-4)" }}>{label}</span>
      {children}
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
function StatusPill({
  status,
  closed,
}: {
  status: string;
  closed: boolean;
}) {
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

function ChartSection({ marketId }: { marketId: number }) {
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
      if (eventKey == null) return;
      if (next && next.length > 0) chartSelectionByEvent.set(eventKey, next);
      else chartSelectionByEvent.delete(eventKey);
    },
    [eventKey],
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
  // Self-heals across navigation: stale ids from another event drop out, and
  // an empty result falls back to the favourite-first default. The active
  // outcome is always on the chart so it can be highlighted — but APPENDED as
  // the last line (replacing the last when already at the cap), never prepended.
  // Switching to an off-chart outcome then just adds a line at the end instead
  // of reshuffling the whole chart.
  const effectiveSelected = useMemo(() => {
    const valid = (selectedIds ?? []).filter((id) => idSet.has(id));
    const base = valid.length > 0 ? valid : defaultIds;
    if (idSet.has(marketId) && !base.includes(marketId)) {
      return base.length >= MAX_CHART_LINES
        ? [...base.slice(0, MAX_CHART_LINES - 1), marketId]
        : [...base, marketId];
    }
    return base;
  }, [selectedIds, idSet, defaultIds, marketId]);

  // Only emphasize a single line when there's more than one to distinguish it
  // from. Binary (area) markets have one line, so highlighting is moot there.
  const highlightId = group?.isMultiOutcome ? marketId : undefined;

  // Fetch history only for the outcomes actually shown (legend caps at 8).
  const { byMarket } = useEventPriceHistory(effectiveSelected);

  const drawn = useMemo(
    () =>
      outcomes
        .map((outcome, colorIndex) => ({ outcome, colorIndex }))
        .filter((d) => effectiveSelected.includes(d.outcome.marketId)),
    [outcomes, effectiveSelected],
  );

  // Binary → area; any multi-outcome event → overlaid independent lines.
  // NegRisk events are drawn exactly like any other group — no stacked-band
  // special case, so the event's `negRisk` flag no longer gates the chart.
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
      <div
        className="market-detail-chart-head"
        style={{
        }}
      >
        {outcomes.length > 0 ? (
          <div
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
          <div className="eyebrow">{"// yes probability"}</div>
        )}
        <div style={{ flexShrink: 0 }}>
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
        <PriceChart
          drawn={drawn}
          byMarket={byMarket}
          mode={mode}
          sinceMs={sinceMs}
          nowMs={nowMs}
        />
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
    market.polymarket_condition_id != null ? "source link" : "resolution source";

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
            {"// description"}
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
            {"// resolution"}
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
          {"// " + sourceLabel}
        </div>
        {sourceUrl ? (
          <a
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

      <ProposeResolution />
    </section>
  );
}

/**
 * Inactive "Propose resolution" affordance. Resolution proposals aren't wired
 * to the backend yet, so the button is disabled with a "coming soon" hint —
 * it reserves the spot next to the resolution criteria the way the Discussion
 * card reserves the comments slot.
 */
function ProposeResolution() {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "row",
        alignItems: "center",
        gap: "var(--space-3)",
        paddingTop: "var(--space-3)",
        borderTop: "1px solid var(--border-1)",
      }}
    >
      <button
        type="button"
        disabled
        style={{
          flexShrink: 0,
          padding: "10px 16px",
          borderRadius: "var(--radius-md)",
          border: "1px solid var(--border-2)",
          background: "var(--surface-2)",
          color: "var(--fg-3)",
          fontFamily: "var(--font-sans)",
          fontSize: "var(--fs-13)",
          fontWeight: 600,
          cursor: "not-allowed",
        }}
      >
        Propose resolution
      </button>
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 10,
          color: "var(--fg-4)",
          textTransform: "uppercase",
          letterSpacing: "var(--track-wide)",
        }}
      >
        coming soon
      </span>
    </div>
  );
}

/**
 * Empty Discussion card. Backend has no comments endpoint today; this slot
 * reserves the layout so the page doesn't shift later when the thread lands.
 */
function DiscussionPlaceholder() {
  return (
    <section
      style={{
        padding: "var(--space-5)",
        background: "var(--surface-1)",
        border: "1px dashed var(--border-2)",
        borderRadius: "var(--radius-lg)",
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-2)",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          gap: "var(--space-3)",
        }}
      >
        <h3
          style={{
            fontFamily: "var(--font-sans)",
            fontSize: 16,
            fontWeight: 600,
            color: "var(--fg-1)",
            margin: 0,
          }}
        >
          Discussion
        </h3>
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 10,
            color: "var(--fg-4)",
            textTransform: "uppercase",
            letterSpacing: "0.04em",
          }}
        >
          coming soon
        </span>
      </div>
      <p
        style={{
          margin: 0,
          fontFamily: "var(--font-sans)",
          fontSize: 13,
          lineHeight: "20px",
          color: "var(--fg-3)",
        }}
      >
        Comments are coming with the next backend cycle. This card reserves
        the spot so the layout stays stable.
      </p>
    </section>
  );
}

function Placeholder({ children, error }: { children: React.ReactNode; error?: boolean }) {
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
