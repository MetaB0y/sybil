"use client";

import Link from "next/link";
import { useInViewport } from "@/lib/hooks/use-in-viewport";
import {
  formatCents,
  formatCentsDelta,
  formatCompactDollars,
} from "@/lib/format/nanos";
import type { Market } from "@/lib/markets/use-markets";
import { useCardHistory } from "@/lib/markets/use-card-history";
import { formatTraders } from "@/lib/mock";
import {
  getCategoryColor,
  pickDisplayCategory,
} from "@/lib/categorize";
import type { MarketPrice } from "@/lib/store";
import { MarketThumb } from "./market-thumb";
import { Sparkline } from "./sparkline";

type Props = {
  market: Market;
  price: MarketPrice | undefined;
};

const CARD_HEIGHT = 384;

/**
 * BinaryCard — one card per YES/NO market.
 *
 * 5-row handoff layout: eyebrow · title+thumb · featured price+sparkline ·
 * YES/NO bars · footer KV. 360px fixed height so cards align row-for-row.
 *
 * Sparkline + 24h delta lazy-load via IntersectionObserver — the card is
 * fully usable before history arrives.
 */
export function BinaryCard({ market, price }: Props) {
  const [ref, inView] = useInViewport<HTMLAnchorElement>();
  const { points, delta24Cents, noDelta24Cents } = useCardHistory(
    market.market_id,
    inView
  );

  const yesCents = price ? formatCents(price.yes) : "—";
  const noCents = price ? formatCents(price.no) : "—";

  return (
    <Link
      ref={ref}
      href={`/m/${market.market_id}`}
      draggable={false}
      onClick={(e) => {
        if (window.getSelection()?.toString()) e.preventDefault();
      }}
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
        textDecoration: "none",
        color: "var(--fg-1)",
        transition: "border-color var(--dur-fast) var(--ease-standard)",
        boxSizing: "border-box",
        overflow: "hidden",
        cursor: "pointer",
        opacity: market.closed === true ? 0.5 : 1,
      }}
      onMouseEnter={(e) =>
        (e.currentTarget.style.borderColor = "var(--border-3)")
      }
      onMouseLeave={(e) =>
        (e.currentTarget.style.borderColor = "var(--border-1)")
      }
    >
      <EyebrowRow market={market} />
      <TitleRow market={market} />
      <FeaturedPriceRow
        cents={yesCents}
        delta24Cents={delta24Cents}
        points={points}
        tone={deltaTone(delta24Cents, !!price)}
      />
      <SideList
        market={market}
        noCents={noCents}
        hasPrice={!!price}
        noDelta={noDelta24Cents}
      />
      <FooterRow market={market} />
    </Link>
  );
}

function EyebrowRow({ market }: { market: Market }) {
  const { primary } = pickDisplayCategory(market.categories, market.category);
  const endDate = formatEndDate(market.market_end_date_ms);
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
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
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
          <span style={{ color: "var(--fg-4)" }}>uncategorized</span>
        )}
      </span>
      <span
        className="text-mono"
        style={{
          fontSize: "10px",
          letterSpacing: "var(--track-wide)",
          textTransform: "uppercase",
          color: "var(--fg-3)",
        }}
      >
        {market.closed === true ? "closed" : (endDate ?? "yes / no")}
      </span>
    </div>
  );
}

function TitleRow({ market }: { market: Market }) {
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "64px 1fr",
        gap: "var(--space-3)",
        alignItems: "start",
      }}
    >
      <MarketThumb
        marketId={market.market_id}
        name={market.name}
        imageUrl={market.market_image_url ?? null}
        fallbackIconUrl={market.market_icon_url ?? null}
        size={64}
      />
      <h3
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
        {market.name}
      </h3>
    </div>
  );
}

function FeaturedPriceRow({
  cents,
  delta24Cents,
  points,
  tone,
}: {
  cents: string;
  delta24Cents: number | null;
  points: import("@/lib/markets/use-card-history").PricePoint[];
  tone: string;
}) {
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "1fr auto",
        gap: "var(--space-3)",
        alignItems: "center",
        padding: "var(--space-3)",
        background: "var(--surface-2)",
        borderRadius: "var(--radius-md)",
        border: "1px solid var(--border-1)",
      }}
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
        <span
          style={{
            fontFamily: "var(--font-sans)",
            fontSize: "var(--fs-13)",
            color: "var(--fg-2)",
          }}
        >
          Yes
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
              color: tone,
              letterSpacing: "var(--track-mono)",
            }}
          >
            {cents}
          </span>
          {delta24Cents != null && (
            <span
              className="text-mono tabular"
              style={{
                fontSize: "var(--fs-12)",
                color: deltaValueColor(delta24Cents),
              }}
            >
              {formatCentsDelta(delta24Cents)}
            </span>
          )}
        </div>
      </div>
      <Sparkline points={points} tone="yes" />
    </div>
  );
}

function SideList({
  market,
  noCents,
  hasPrice,
  noDelta,
}: {
  market: Market;
  noCents: string;
  hasPrice: boolean;
  noDelta: number | null;
}) {
  const volNanos = market.volume_nanos ? BigInt(market.volume_nanos) : 0n;
  const vol = volNanos > 0n ? formatCompactDollars(volNanos) : "—";
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
      {/* Featured panel already shows Yes — mirror MultiCard's
          "featured + the others" rule and list only the No outcome. */}
      <SideRow
        side="No"
        cents={noCents}
        centsColor={deltaTone(noDelta, hasPrice)}
        delta={noDelta}
        vol={vol}
        first
      />
    </div>
  );
}

function SideRow({
  side,
  cents,
  centsColor,
  delta,
  vol,
  first,
}: {
  side: "Yes" | "No";
  cents: string;
  centsColor: string;
  delta: number | null;
  vol: string;
  first?: boolean;
}) {
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "minmax(0, 1fr) 44px 52px 52px",
        gap: "var(--space-2)",
        alignItems: "center",
        padding: "10px 0",
        borderTop: first ? "none" : "1px solid var(--border-1)",
      }}
    >
      <span
        style={{
          fontFamily: "var(--font-sans)",
          fontSize: "var(--fs-13)",
          color: "var(--fg-2)",
        }}
      >
        {side}
      </span>
      <span
        className="text-mono tabular"
        style={{
          fontSize: "var(--fs-13)",
          color: centsColor,
          textAlign: "right",
        }}
      >
        {cents}
      </span>
      <span
        className="text-mono tabular"
        title={delta == null ? "no 24h history yet" : undefined}
        style={{
          fontSize: "11px",
          color: deltaValueColor(delta),
          textAlign: "right",
        }}
      >
        {delta != null ? formatCentsDelta(delta) : "—"}
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
    </div>
  );
}

function FooterRow({ market }: { market: Market }) {
  const volNanos = market.volume_nanos ? BigInt(market.volume_nanos) : 0n;
  const vol = volNanos > 0n ? formatCompactDollars(volNanos) : "—";
  const liqNanos = market.liquidity_avg10_nanos
    ? BigInt(market.liquidity_avg10_nanos)
    : 0n;
  const liq = liqNanos > 0n ? formatCompactDollars(liqNanos) : "—";
  const traderCount = market.trader_count ?? 0;
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
        <FooterChip label="vol" value={vol} />
        <FooterChip label="liq" value={liq} />
      </div>
      <FooterChip label="traders" value={traders} />
    </div>
  );
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

function deltaTone(delta: number | null, hasPrice: boolean): string {
  if (!hasPrice) return "var(--fg-4)";
  if (delta == null) return "var(--fg-1)";
  return delta >= 0 ? "var(--yes)" : "var(--no)";
}

/**
 * Color for the 24h-delta value token itself (not the price): dim when there's
 * no history, neutral grey when flat (rounds to ±0¢), else green/red by sign.
 * Rounds first so the color matches what `formatCentsDelta` actually prints.
 */
function deltaValueColor(delta: number | null): string {
  if (delta == null) return "var(--fg-4)";
  if (Math.round(delta) === 0) return "var(--fg-3)";
  return delta > 0 ? "var(--yes)" : "var(--no)";
}

/**
 * "closes Mar 5" if same year, "closes Mar 5 '26" otherwise. Returns null
 * for absent / past-or-bogus dates so the eyebrow falls back to its
 * neutral "yes / no" tagline.
 */
function formatEndDate(ms: number | null | undefined): string | null {
  if (ms == null) return null;
  const d = new Date(ms);
  if (Number.isNaN(d.getTime())) return null;
  const now = new Date();
  const month = d.toLocaleString("en-US", { month: "short" });
  const day = d.getDate();
  if (d.getFullYear() === now.getFullYear()) {
    return `closes ${month} ${day}`;
  }
  const yy = String(d.getFullYear()).slice(-2);
  return `closes ${month} ${day} '${yy}`;
}

