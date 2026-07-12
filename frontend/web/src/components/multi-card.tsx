"use client";

import { useMemo } from "react";
import Link from "next/link";
import { useInViewport } from "@/lib/hooks/use-in-viewport";
import {
  formatPercentPrecise,
  formatPercentDelta,
  formatCompactDollars,
} from "@/lib/format/nanos";
import {
  isMirror,
  isNative,
  type IndexMarket,
} from "@/lib/markets/use-markets";
import { useCardHistory } from "@/lib/markets/use-card-history";
import { formatTraders } from "@/lib/mock";
import { useEventTraders } from "@/lib/markets/use-event-traders";
import { getCategoryColor, pickDisplayCategory } from "@/lib/categorize";
import type { MarketPrice } from "@/lib/store";
import { MarketThumb } from "./market-thumb";
import { Sparkline } from "./sparkline";
import { useEventRaw, type RawEventMarket } from "@/lib/markets/use-event-raw";

const SECONDARY_OUTCOMES = 2;
const CARD_HEIGHT = 384;

type Props = {
  groupName: string;
  markets: IndexMarket[];
  prices: Record<number, MarketPrice>;
};

/**
 * MultiCard — one card per multi-outcome event group.
 *
 * Same 5-row handoff layout as BinaryCard. Featured row shows the leading
 * outcome (top YES prob) with a sparkline + 24h delta lazy-loaded for the
 * leader only. Secondary outcomes render below as a tight list.
 */
export function MultiCard({ groupName, markets, prices }: Props) {
  const [ref, inView] = useInViewport<HTMLElement>();

  // Sort by per-outcome volume (desc), then tie-break by YES price (desc).
  // This is what the eyebrow's "top 2" promises — biggest-traded outcomes
  // surface first; price-only ranking buried high-conviction-but-no-volume
  // outcomes.
  const ranked = [...markets].sort((a, b) => {
    // Closed outcomes always sink below open ones (still shown, just greyed).
    const ca = a.closed === true ? 1 : 0;
    const cb = b.closed === true ? 1 : 0;
    if (ca !== cb) return ca - cb;
    const va = a.volume_nanos ? BigInt(a.volume_nanos) : 0n;
    const vb = b.volume_nanos ? BigInt(b.volume_nanos) : 0n;
    if (va !== vb) return va > vb ? -1 : 1;
    const pa = prices[a.market_id]?.yes ?? -1n;
    const pb = prices[b.market_id]?.yes ?? -1n;
    return pa > pb ? -1 : pa < pb ? 1 : 0;
  });

  const leader = ranked[0];
  const secondary = ranked.slice(1, 1 + SECONDARY_OUTCOMES);
  const hiddenCount = Math.max(0, ranked.length - 1 - secondary.length);

  const allClosed =
    markets.length > 0 && markets.every((m) => m.closed === true);

  const { points, delta24Cents } = useCardHistory(
    leader?.market_id ?? -1,
    inView && !!leader,
  );

  const totalVolumeNanos = sumVolumeNanos(markets);
  const totalVol = totalVolumeNanos
    ? formatCompactDollars(totalVolumeNanos)
    : "—";

  // Event-level traders is a set union over the event's markets — not the
  // sum of per-market `trader_count` (that double-counts cross-market
  // traders). Fetched per event via the dedicated endpoint.
  const eventTradersQ = useEventTraders(markets[0]?.event_id ?? undefined);

  // Outcome short-labels (e.g. "↑ 200,000") live only in the raw Polymarket
  // event JSON; join by polymarket_condition_id. Lazy + cached per event.
  const rawMarkets = useEventRaw(
    markets[0]?.event_id ?? undefined,
    inView,
  ).data;
  const getLabel = useMemo(() => makeLabelResolver(rawMarkets), [rawMarkets]);

  return (
    <article
      ref={ref}
      style={{
        display: "grid",
        gridTemplateRows: "22px 64px auto 1fr 18px",
        gap: "var(--space-3)",
        height: CARD_HEIGHT,
        padding: "var(--space-4)",
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-lg)",
        boxShadow: "var(--shadow-inset-top)",
        boxSizing: "border-box",
        overflow: "hidden",
        cursor: "pointer",
        opacity: allClosed ? 0.5 : 1,
      }}
    >
      <EyebrowRow
        markets={markets}
        count={markets.length}
        hiddenCount={hiddenCount}
        allClosed={allClosed}
      />
      <TitleRow
        groupName={groupName}
        leaderId={leader?.market_id}
        imageUrl={leader?.event_image_url ?? null}
        fallbackIconUrl={leader?.event_icon_url ?? null}
      />
      <FeaturedOutcome
        leader={leader}
        price={leader ? prices[leader.market_id] : undefined}
        points={points}
        delta24Cents={delta24Cents}
        getLabel={getLabel}
      />
      <SecondaryList
        markets={secondary}
        prices={prices}
        inView={inView}
        getLabel={getLabel}
        cardClosed={allClosed}
      />
      <FooterRow
        totalVol={totalVol}
        totalLiqNanos={sumLiquidityNanos(markets)}
        traderCount={eventTradersQ.data ?? 0}
      />
    </article>
  );
}

function EyebrowRow({
  markets,
  count,
  hiddenCount,
  allClosed,
}: {
  markets: IndexMarket[];
  count: number;
  hiddenCount: number;
  allClosed: boolean;
}) {
  // All markets in a group share an event, so they share categories. Use
  // the first market's categories as the source of truth.
  const first = markets[0];
  const primary = first
    ? pickDisplayCategory(first.categories, first.category).primary
    : null;
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: "var(--space-2)",
      }}
    >
      <span
        className="text-mono"
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: "var(--space-2)",
          fontSize: "10px",
          letterSpacing: "var(--track-wide)",
          textTransform: "uppercase",
          color: "var(--fg-3)",
        }}
      >
        {primary ? (
          <>
            <span
              aria-hidden
              style={{
                width: 6,
                height: 6,
                borderRadius: "50%",
                background: getCategoryColor(primary),
                flexShrink: 0,
              }}
            />
            {primary}
          </>
        ) : (
          <span style={{ color: "var(--fg-3)" }}>uncategorized</span>
        )}
      </span>
      <span
        className="text-mono"
        style={{
          fontSize: "11px",
          color: "var(--fg-3)",
        }}
      >
        {markets.some(isMirror) ? (
          <>
            <span>mirror</span>
            <span style={{ margin: "0 4px", color: "var(--fg-4)" }}>·</span>
          </>
        ) : (
          markets.length > 0 &&
          markets.every(isNative) && (
            <>
              <span>native</span>
              <span style={{ margin: "0 4px", color: "var(--fg-4)" }}>·</span>
            </>
          )
        )}
        {allClosed && (
          <>
            <span style={{ color: "var(--fg-4)" }}>closed</span>
            <span style={{ margin: "0 4px", color: "var(--fg-4)" }}>·</span>
          </>
        )}
        <span>{count} outcomes</span>
        {hiddenCount > 0 && (
          <>
            <span style={{ margin: "0 4px", color: "var(--fg-4)" }}>·</span>
            <span style={{ color: "var(--accent)" }}>+{hiddenCount} more</span>
          </>
        )}
      </span>
    </div>
  );
}

function TitleRow({
  groupName,
  leaderId,
  imageUrl,
  fallbackIconUrl,
}: {
  groupName: string;
  leaderId: number | undefined;
  imageUrl: string | null;
  fallbackIconUrl: string | null;
}) {
  const href = leaderId != null ? `/m/${leaderId}` : "#";
  return (
    <Link
      href={href}
      prefetch={false}
      draggable={false}
      onClick={(e) => {
        if (window.getSelection()?.toString()) e.preventDefault();
      }}
      style={{
        display: "grid",
        gridTemplateColumns: "64px 1fr",
        gap: "var(--space-3)",
        alignItems: "start",
        textDecoration: "none",
        color: "var(--fg-1)",
      }}
    >
      <MarketThumb
        marketId={leaderId ?? 0}
        name={groupName}
        imageUrl={imageUrl}
        fallbackIconUrl={fallbackIconUrl}
        size={64}
      />
      <h2
        style={{
          fontFamily: "var(--font-sans)",
          fontWeight: 600,
          fontSize: "var(--fs-20)",
          lineHeight: "var(--lh-20)",
          letterSpacing: "var(--track-tight)",
          margin: 0,
          color: "var(--fg-1)",
          display: "-webkit-box",
          WebkitLineClamp: 2,
          WebkitBoxOrient: "vertical",
          overflow: "hidden",
          userSelect: "text",
          cursor: "pointer",
        }}
      >
        {groupName}
      </h2>
    </Link>
  );
}

function FeaturedOutcome({
  leader,
  price,
  points,
  delta24Cents,
  getLabel,
}: {
  leader: IndexMarket | undefined;
  price: MarketPrice | undefined;
  points: import("@/lib/markets/use-card-history").PricePoint[];
  delta24Cents: number | null;
  getLabel: (m: IndexMarket) => string;
}) {
  if (!leader) {
    return (
      <div
        style={{
          padding: "var(--space-3)",
          background: "var(--surface-2)",
          borderRadius: "var(--radius-md)",
          border: "1px solid var(--border-1)",
          color: "var(--fg-4)",
          fontFamily: "var(--font-mono)",
          fontSize: "var(--fs-12)",
        }}
      >
        no outcomes
      </div>
    );
  }
  const cents = price ? formatPercentPrecise(price.yes) : "—";
  const label = getLabel(leader);
  const leaderVolNanos = leader.volume_nanos ? BigInt(leader.volume_nanos) : 0n;
  const leaderVol =
    leaderVolNanos > 0n ? formatCompactDollars(leaderVolNanos) : "—";
  return (
    <Link
      href={`/m/${leader.market_id}`}
      prefetch={false}
      draggable={false}
      onClick={(e) => {
        if (window.getSelection()?.toString()) e.preventDefault();
      }}
      style={{
        display: "grid",
        gridTemplateColumns: "1fr auto",
        gap: "var(--space-3)",
        alignItems: "center",
        padding: "var(--space-3)",
        background: "var(--surface-2)",
        borderRadius: "var(--radius-md)",
        border: "1px solid var(--border-1)",
        textDecoration: "none",
        color: "var(--fg-1)",
      }}
    >
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          gap: 2,
          minWidth: 0,
        }}
      >
        <span
          style={{
            fontFamily: "var(--font-sans)",
            fontSize: "var(--fs-13)",
            color: "var(--fg-2)",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
            userSelect: "text",
            cursor: "pointer",
          }}
        >
          {label}
        </span>
        <div
          style={{
            display: "flex",
            gap: "var(--space-2)",
            alignItems: "baseline",
          }}
        >
          <span
            className="text-mono tabular"
            style={{
              fontSize: "var(--fs-32)",
              lineHeight: "var(--lh-32)",
              color: deltaTone(delta24Cents, !!price),
              letterSpacing: "var(--track-mono)",
            }}
          >
            {cents}
          </span>
          <span
            className="text-mono tabular"
            title={delta24Cents == null ? "no 24h history yet" : undefined}
            style={{
              fontSize: "var(--fs-12)",
              color: deltaValueColor(delta24Cents ?? 0),
            }}
          >
            {formatPercentDelta(delta24Cents ?? 0)}
          </span>
        </div>
      </div>
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "flex-end",
          gap: 4,
        }}
      >
        <Sparkline points={points} />
        <span
          className="tabular"
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: "10px",
            textTransform: "uppercase",
            letterSpacing: "var(--track-wide)",
            color: "var(--fg-3)",
            whiteSpace: "nowrap",
          }}
        >
          {"vol " + leaderVol}
        </span>
      </div>
    </Link>
  );
}

function SecondaryList({
  markets,
  prices,
  inView,
  getLabel,
  cardClosed,
}: {
  markets: IndexMarket[];
  prices: Record<number, MarketPrice>;
  inView: boolean;
  getLabel: (m: IndexMarket) => string;
  cardClosed: boolean;
}) {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        alignSelf: "start",
        minHeight: 0,
        overflow: "hidden",
      }}
    >
      {markets.map((m, i) => (
        <SecondaryRow
          key={m.market_id}
          market={m}
          price={prices[m.market_id]}
          first={i === 0}
          inView={inView}
          getLabel={getLabel}
          cardClosed={cardClosed}
        />
      ))}
    </div>
  );
}

function SecondaryRow({
  market,
  price,
  first,
  inView,
  getLabel,
  cardClosed,
}: {
  market: IndexMarket;
  price: MarketPrice | undefined;
  first?: boolean;
  inView: boolean;
  getLabel: (m: IndexMarket) => string;
  cardClosed: boolean;
}) {
  const label = getLabel(market);
  // Per-row greying only when this row is closed inside an OPEN card. When the
  // whole card is closed the <article> already dims at 0.5 — self-dimming here
  // would compound to 0.25.
  const rowClosed = market.closed === true && !cardClosed;
  const cents = price ? formatPercentPrecise(price.yes) : "—";
  // Same logic as the leader: real 24h delta from price history, lazy-loaded
  // when the card scrolls into view.
  const { delta24Cents } = useCardHistory(market.market_id, inView);
  const tone = deltaTone(delta24Cents, !!price);
  const volNanos = market.volume_nanos ? BigInt(market.volume_nanos) : 0n;
  const vol = volNanos > 0n ? formatCompactDollars(volNanos) : "—";
  return (
    <Link
      href={`/m/${market.market_id}`}
      prefetch={false}
      draggable={false}
      onClick={(e) => {
        if (window.getSelection()?.toString()) e.preventDefault();
      }}
      style={{
        display: "grid",
        gridTemplateColumns: "minmax(0, 1fr) 44px 52px 52px",
        gap: "var(--space-2)",
        alignItems: "center",
        padding: "10px 0",
        borderTop: first ? "none" : "1px solid var(--border-1)",
        textDecoration: "none",
        color: "var(--fg-1)",
        opacity: rowClosed ? 0.5 : 1,
      }}
    >
      <span
        style={{
          fontFamily: "var(--font-sans)",
          fontSize: "var(--fs-13)",
          color: rowClosed ? "var(--fg-4)" : "var(--fg-2)",
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          userSelect: "text",
          cursor: "pointer",
        }}
      >
        {label}
        {rowClosed && (
          <span
            className="text-mono"
            style={{
              marginLeft: 6,
              fontSize: "9px",
              letterSpacing: "var(--track-wide)",
              textTransform: "uppercase",
              color: "var(--fg-4)",
            }}
          >
            closed
          </span>
        )}
      </span>
      <span
        className="text-mono tabular"
        style={{
          fontSize: "var(--fs-13)",
          color: tone,
          textAlign: "right",
        }}
      >
        {cents}
      </span>
      <span
        className="text-mono tabular"
        title={delta24Cents == null ? "no 24h history yet" : undefined}
        style={{
          fontSize: "11px",
          color: deltaValueColor(delta24Cents ?? 0),
          display: "inline-block",
          width: "100%",
          textAlign: "right",
        }}
      >
        {formatPercentDelta(delta24Cents ?? 0)}
      </span>
      <span
        className="text-mono tabular"
        style={{
          fontSize: "11px",
          color: "var(--fg-3)",
          textAlign: "right",
        }}
      >
        {vol}
      </span>
    </Link>
  );
}

function FooterRow({
  totalVol,
  totalLiqNanos,
  traderCount,
}: {
  totalVol: string;
  totalLiqNanos: bigint;
  traderCount: number;
}) {
  const liq = totalLiqNanos > 0n ? formatCompactDollars(totalLiqNanos) : "—";
  const traders = traderCount > 0 ? formatTraders(traderCount) : "—";
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: "var(--space-3)",
        fontFamily: "var(--font-mono)",
        fontSize: "10px",
        textTransform: "uppercase",
        letterSpacing: "var(--track-wide)",
        color: "var(--fg-3)",
        marginTop: "var(--space-4)",
      }}
    >
      <div style={{ display: "flex", gap: "var(--space-3)" }}>
        <FooterChip label="vol" value={totalVol} />
        <FooterChip label="liq" value={liq} />
      </div>
      <FooterChip label="traders" value={traders} />
    </div>
  );
}

/**
 * Color an outcome's odds by delta sign (handoff convention).
 * No price → fg-4 (dim). No delta yet → neutral fg-1.
 */
function deltaTone(delta: number | null, hasPrice: boolean): string {
  if (!hasPrice) return "var(--fg-4)";
  if (delta == null) return "var(--fg-1)";
  return delta >= 0 ? "var(--yes)" : "var(--no)";
}

/**
 * Color for the 24h-delta value token itself (not the price): dim when there's
 * no history, neutral grey when flat (rounds to +0%), else green/red by sign.
 * Rounds first so the color matches what `formatPercentDelta` actually prints.
 */
function deltaValueColor(delta: number | null): string {
  if (delta == null) return "var(--fg-4)";
  if (Math.round(delta) === 0) return "var(--fg-3)";
  return delta > 0 ? "var(--yes)" : "var(--no)";
}

function FooterChip({
  label,
  value,
}: {
  label: string;
  value: React.ReactNode;
}) {
  return (
    <span style={{ display: "inline-flex", gap: 4, alignItems: "baseline" }}>
      <span>{label}</span>
      <span className="tabular" style={{ color: "var(--fg-2)" }}>
        {value}
      </span>
    </span>
  );
}

/**
 * Build an outcome-label resolver for one event's cards. Prefers the Polymarket
 * `groupItemTitle`, sourced in order: (1) `market.group_item_title` on the
 * markets payload — instant, no extra fetch, so the label doesn't blink; (2) the
 * raw event JSON joined by `polymarket_condition_id`; (3) exact question text
 * (for markets missing a condition id, since a non-NegRisk market's `name` IS
 * its Polymarket question). Falls back to the trimmed name until those resolve
 * (and for NegRisk "event: outcome" names, where the trim already yields the
 * outcome). Sources 2–3 are a pre-deploy/edge fallback for the in-view snapshot.
 */
function makeLabelResolver(
  raw: Map<string, RawEventMarket> | undefined,
): (m: IndexMarket) => string {
  const byQuestion = new Map<string, string>();
  if (raw) {
    for (const rm of raw.values()) {
      if (rm.question && rm.groupItemTitle) {
        byQuestion.set(rm.question, rm.groupItemTitle);
      }
    }
  }
  return (m: IndexMarket) => {
    const gt =
      m.group_item_title ??
      (m.polymarket_condition_id
        ? raw?.get(m.polymarket_condition_id)?.groupItemTitle
        : undefined) ??
      byQuestion.get(m.name);
    return gt?.trim() || trimOutcomeLabel(m.name);
  };
}

function trimOutcomeLabel(name: string): string {
  const idx = name.indexOf(":");
  if (idx < 0 || idx > name.length - 2) return name;
  return name.slice(idx + 1).trim();
}

function sumVolumeNanos(markets: IndexMarket[]): bigint | null {
  let total = 0n;
  let any = false;
  for (const m of markets) {
    if (m.volume_nanos != null) {
      total += BigInt(m.volume_nanos);
      any = true;
    }
  }
  return any ? total : null;
}

/**
 * Event-level liquidity = sum of each member market's `liquidity_avg10_nanos`.
 * Additive scalar — the backend already excludes multi-market orders, so
 * summing per-market scores does not double-count.
 */
function sumLiquidityNanos(markets: IndexMarket[]): bigint {
  let total = 0n;
  for (const m of markets) {
    if (m.liquidity_avg10_nanos != null) {
      total += BigInt(m.liquidity_avg10_nanos);
    }
  }
  return total;
}
