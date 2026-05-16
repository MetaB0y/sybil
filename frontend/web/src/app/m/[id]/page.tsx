"use client";

import Link from "next/link";
import { notFound } from "next/navigation";
import { use, useMemo, useState } from "react";
import {
  ChartRangeBar,
  RANGE_MS,
  type ChartRange,
} from "@/components/chart-range-bar";
import { MarketRail } from "@/components/market-rail";
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
import { detectStackable } from "@/lib/market-detail/build-chart-series";
import { useEventGroup } from "@/lib/market-detail/use-event-group";
import { useMarketStats } from "@/lib/market-detail/use-market-stats";
import { useEventPriceHistory } from "@/lib/markets/use-event-price-history";
import { selectLatestBlock, useStore } from "@/lib/store";

type RouteParams = { id: string };

export default function MarketDetailPage({
  params,
}: {
  params: Promise<RouteParams>;
}) {
  const { id } = use(params);
  const marketId = Number(id);

  if (!Number.isFinite(marketId) || marketId < 0) {
    notFound();
  }

  const marketQ = useMarket(marketId);

  const market = marketQ.data;

  return (
    <main
      style={{
        width: "100%",
        padding: "var(--space-6) var(--space-5) var(--space-9)",
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-6)",
      }}
    >
      {marketQ.isPending && <Placeholder>loading market…</Placeholder>}
      {marketQ.isError && (
        <Placeholder error>error: {String(marketQ.error)}</Placeholder>
      )}

      {market && (
        <>
          {/* Header spans the full width; the chart/rail split sits below it. */}
          <Header marketId={marketId} market={market} />

          <div
            style={{
              display: "grid",
              gridTemplateColumns: "minmax(0, 1fr) 420px",
              gap: "var(--space-6)",
              alignItems: "start",
            }}
          >
            <div
              style={{
                display: "flex",
                flexDirection: "column",
                gap: "var(--space-5)",
              }}
            >
              <ChartSection marketId={marketId} />
              <DescriptionBlock market={market} />
              <DiscussionPlaceholder />
            </div>

            <MarketRail marketId={marketId} />
          </div>
        </>
      )}
    </main>
  );
}

/**
 * Page header. Mirrors `MarketHeader` in `frontend/handoff/data/fed-primitives.jsx:246`:
 *
 *   ┌──────┐ Markets / ● Category / resolves <date>
 *   │ thumb│ <market name>
 *   │      │ vol $X   24h $Y   traders N   liq $Z   batches ~M
 *   └──────┘
 *
 * Title, thumbnail and all five stats are scoped to the single market in the
 * URL — never its parent event. `vol / 24h / traders / liq` are real Phase-B
 * fields (see `derive-market-stats.ts`). `batches ~M` stays a 2s-cadence
 * timestamp approximation — an exact count needs a backend `created_at_height`
 * (OPEN_QUESTIONS #9).
 */
function Header({
  marketId,
  market,
}: {
  marketId: number;
  market: {
    name: string;
    status: string;
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
  };
}) {
  const { stats } = useMarketStats(marketId);
  const { primary } = pickDisplayCategory(market.categories, market.category);
  const resolvesMs = market.market_end_date_ms ?? market.expiry_timestamp_ms ?? null;
  const isActive = market.status === "active";

  return (
    <header
      style={{
        display: "grid",
        gridTemplateColumns: "56px 1fr",
        gap: "var(--space-4)",
        alignItems: "start",
        // Full-bleed divider separating the header from the chart/rail split,
        // mirroring `MarketHeader` in `fed-primitives.jsx:248`. Negative side
        // margins cancel `<main>`'s padding so the rule spans edge-to-edge.
        marginLeft: "calc(-1 * var(--space-5))",
        marginRight: "calc(-1 * var(--space-5))",
        paddingLeft: "var(--space-5)",
        paddingRight: "var(--space-5)",
        paddingBottom: "var(--space-5)",
        borderBottom: "1px solid var(--border-1)",
      }}
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
        </div>

        {/* Title + status pill */}
        <div
          style={{
            display: "flex",
            alignItems: "baseline",
            flexWrap: "wrap",
            gap: "var(--space-3)",
          }}
        >
          <h1
            style={{
              fontFamily: "var(--font-display)",
              fontWeight: 600,
              fontSize: "var(--fs-32)",
              lineHeight: "var(--lh-32)",
              letterSpacing: "var(--track-tight)",
              margin: 0,
              color: "var(--fg-1)",
            }}
          >
            {market.name}
          </h1>
          <StatusPill status={market.status} isActive={isActive} />
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
 * Small uppercase status chip shown beside the market title. `active` markets
 * read muted; anything else (resolved / paused / cancelled) reads in the warn
 * color so a non-tradeable market stands out.
 */
function StatusPill({
  status,
  isActive,
}: {
  status: string;
  isActive: boolean;
}) {
  return (
    <span
      className="text-mono"
      style={{
        flexShrink: 0,
        padding: "3px 8px",
        borderRadius: "var(--radius-md)",
        fontSize: "10px",
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
        color: isActive ? "var(--fg-3)" : "var(--warn)",
        background: isActive ? "var(--surface-1)" : "var(--warn-soft)",
        border: `1px solid ${isActive ? "var(--border-2)" : "var(--warn-soft)"}`,
      }}
    >
      {(status || "active").toUpperCase()}
    </span>
  );
}

function ChartSection({ marketId }: { marketId: number }) {
  const [range, setRange] = useState<ChartRange>("1W");
  // marketIds drawn on the chart. `null` = use the favourite-first default.
  const [selectedIds, setSelectedIds] = useState<number[] | null>(null);
  const { group, isPending: groupPending } = useEventGroup(marketId);
  const latestBlock = useStore(selectLatestBlock);

  const outcomes = useMemo(() => group?.outcomes ?? [], [group]);
  const idSet = useMemo(
    () => new Set(outcomes.map((o) => o.marketId)),
    [outcomes],
  );
  const defaultIds = useMemo(
    () => outcomes.slice(0, 4).map((o) => o.marketId),
    [outcomes],
  );
  // Self-heals across navigation: stale ids from another event drop out, and
  // an empty result falls back to the favourite-first default.
  const effectiveSelected = useMemo(() => {
    const valid = (selectedIds ?? []).filter((id) => idSet.has(id));
    return valid.length > 0 ? valid : defaultIds;
  }, [selectedIds, idSet, defaultIds]);

  // Fetch history only for the outcomes actually shown (legend caps at 8).
  const { byMarket, isPending: historyPending } =
    useEventPriceHistory(effectiveSelected);

  const drawn = useMemo(
    () =>
      outcomes
        .map((outcome, colorIndex) => ({ outcome, colorIndex }))
        .filter((d) => effectiveSelected.includes(d.outcome.marketId)),
    [outcomes, effectiveSelected],
  );

  // Binary → area. Multi → stacked when the group looks NegRisk (prices
  // partition to ~100¢), else overlaid independent lines.
  const mode = !group?.isMultiOutcome
    ? "area"
    : detectStackable(outcomes)
      ? "stacked"
      : "lines";

  // Latest committed block is our "now" reference — ticks every 2s, so the
  // sliding range window stays current without a Date.now() call in render.
  const nowMs = latestBlock?.timestamp_ms ?? 0;
  const windowMs = RANGE_MS[range];
  const sinceMs = windowMs == null || nowMs === 0 ? null : nowMs - windowMs;

  const loading = groupPending || (historyPending && drawn.length > 0);

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
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: "var(--space-3)",
          flexWrap: "wrap",
        }}
      >
        {outcomes.length > 0 ? (
          <OutcomeLegend
            outcomes={outcomes}
            selectedIds={effectiveSelected}
            onChange={setSelectedIds}
          />
        ) : (
          <div className="eyebrow">{"// yes probability"}</div>
        )}
        <ChartRangeBar value={range} onChange={setRange} />
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
  market: { description?: string | null; resolution_criteria?: string | null; external_url?: string | null };
}) {
  if (!market.description && !market.resolution_criteria && !market.external_url) {
    return null;
  }
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
      {market.description && (
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
            {market.description}
          </p>
        </div>
      )}
      {market.resolution_criteria && (
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
            {market.resolution_criteria}
          </p>
        </div>
      )}
      {market.external_url && (
        <a
          href={market.external_url}
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
      )}
    </section>
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
