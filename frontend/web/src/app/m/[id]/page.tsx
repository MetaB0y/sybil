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
import { MockValue } from "@/components/mock-value";
import { OutcomeLegend } from "@/components/outcome-legend";
import { PriceChart } from "@/components/price-chart";
import {
  formatCompactDollars,
  formatDate,
  formatInt,
} from "@/lib/format/nanos";
import { getCategoryColor, pickDisplayCategory } from "@/lib/categorize";
import { useMarket } from "@/lib/markets/use-market";
import { useEventGroup } from "@/lib/market-detail/use-event-group";
import { useMarketStats } from "@/lib/market-detail/use-market-stats";
import { usePriceHistory } from "@/lib/markets/use-price-history";
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
  const historyQ = usePriceHistory(marketId);

  const market = marketQ.data;
  const history = historyQ.data ?? [];

  return (
    <main
      style={{
        width: "100%",
        padding: "var(--space-6) var(--space-5) var(--space-9)",
        display: "grid",
        gridTemplateColumns: "minmax(0, 1fr) 420px",
        gap: "var(--space-6)",
      }}
    >
      <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-5)" }}>
        {marketQ.isPending && <Placeholder>loading market…</Placeholder>}
        {marketQ.isError && (
          <Placeholder error>error: {String(marketQ.error)}</Placeholder>
        )}

        {market && (
          <>
            <Header marketId={marketId} market={market} />
            <ChartSection
              marketId={marketId}
              history={history}
              isPending={historyQ.isPending}
            />
            <DescriptionBlock market={market} />
            <DiscussionPlaceholder />
          </>
        )}
      </div>

      <MarketRail marketId={marketId} />
    </main>
  );
}

/**
 * Page header. Mirrors `MarketHeader` in `frontend/handoff/data/fed-primitives.jsx:246`:
 *
 *   ┌──────┐ Markets / ● Category / resolves <date>
 *   │ thumb│ <title>
 *   │      │ vol $X   24h $Y*   traders N*   liq $Z*   batches ~M*
 *   └──────┘
 *
 * `vol` is real (MarketResponse.volume_nanos). `24h / traders / liq` are
 * mocked via useMarketStats (OPEN_QUESTIONS #3, #2, #1). `batches ~M` is a
 * 2s-cadence timestamp approximation (OPEN_QUESTIONS #9).
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
    event_title?: string | null;
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
      }}
    >
      <MarketThumb
        marketId={market.market_id}
        name={market.event_title ?? market.name}
        imageUrl={market.event_image_url ?? market.market_image_url ?? null}
        fallbackIconUrl={
          market.event_icon_url ?? market.market_icon_url ?? null
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
          <span style={{ color: "var(--fg-4)", marginLeft: "auto" }}>
            #{market.market_id}
          </span>
          <span
            style={{
              color: isActive ? "var(--fg-3)" : "var(--warn)",
            }}
          >
            {(market.status || "active").toUpperCase()}
          </span>
        </div>

        {/* Title */}
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
          {market.event_title ?? market.name}
        </h1>

        {/* 5-stat meta row. Three values are wrapped in <MockValue> + one
            in an "approx" wrapper. See OPEN_QUESTIONS #1, #2, #3, #9. */}
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
          <MetaStat label="batches cleared">
            {stats?.batchesExistedFor == null ? (
              <span className="tabular" style={{ color: "var(--fg-2)" }}>
                —
              </span>
            ) : (
              <MockValue hint="NOT NOW — approximate at 2s FBA cadence; backend lacks created_at_height (OPEN_QUESTIONS #9)">
                <span className="tabular" style={{ color: "var(--fg-2)" }}>
                  ~{formatInt(stats.batchesExistedFor)}
                </span>
              </MockValue>
            )}
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

function ChartSection({
  marketId,
  history,
  isPending,
}: {
  marketId: number;
  history: import("@/lib/markets/use-price-history").PricePoint[];
  isPending: boolean;
}) {
  const [range, setRange] = useState<ChartRange>("1W");
  const { group } = useEventGroup(marketId);
  const latestBlock = useStore(selectLatestBlock);
  // Use the latest committed block as our "now" reference. Ticks every 2s,
  // so the sliding window stays current without calling Date.now() in render
  // (which would violate react-hooks/purity).
  const nowMs = latestBlock?.timestamp_ms ?? history[history.length - 1]?.timestamp_ms ?? 0;

  const filteredHistory = useMemo(() => {
    const windowMs = RANGE_MS[range];
    if (windowMs == null || nowMs === 0) return history;
    const since = nowMs - windowMs;
    return history.filter((p) => p.timestamp_ms >= since);
  }, [history, range, nowMs]);

  const outcomes = group?.outcomes ?? [];

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
          <OutcomeLegend outcomes={outcomes} />
        ) : (
          <div className="eyebrow">{"// yes probability"}</div>
        )}
        <ChartRangeBar value={range} onChange={setRange} />
      </div>
      {isPending && filteredHistory.length === 0 ? (
        <div
          className="text-mono"
          style={{
            height: 280,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--fg-3)",
          }}
        >
          loading…
        </div>
      ) : filteredHistory.length === 0 ? (
        <div
          className="text-mono"
          style={{
            height: 280,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--fg-4)",
          }}
        >
          {history.length === 0
            ? "no clearing history yet — chart will populate as batches clear."
            : `no activity in the last ${range.toLowerCase()} — pick a wider range.`}
        </div>
      ) : (
        <PriceChart marketId={marketId} history={filteredHistory} />
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
