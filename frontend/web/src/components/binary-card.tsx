"use client";

import Link from "next/link";
import { useInViewport } from "@/lib/hooks/use-in-viewport";
import {
  formatCents,
  formatCompactDollars,
  formatPctDelta,
} from "@/lib/format/nanos";
import type { Market } from "@/lib/markets/use-markets";
import { useCardHistory } from "@/lib/markets/use-card-history";
import {
  formatTraders,
  mockCategory,
  mockLiq,
  mockTraders,
} from "@/lib/mock";
import type { MarketPrice } from "@/lib/store";
import { MarketThumb } from "./market-thumb";
import { MockValue } from "./mock-value";
import { Sparkline } from "./sparkline";

type Props = {
  market: Market;
  price: MarketPrice | undefined;
};

const CARD_HEIGHT = 400;

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
  const { points, delta24Pct } = useCardHistory(market.market_id, inView);

  const yesCents = price ? formatCents(price.yes) : "—";
  const noCents = price ? formatCents(price.no) : "—";
  const yesPct = price ? probabilityPercent(price.yes) : null;
  const noPct = price ? probabilityPercent(price.no) : null;

  return (
    <Link
      ref={ref}
      href={`/m/${market.market_id}`}
      style={{
        display: "grid",
        gridTemplateRows: "22px 56px auto 1fr 18px",
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
        label="Yes"
        cents={yesCents}
        delta24Pct={delta24Pct}
        points={points}
        hasPrice={!!price}
        priceTone={
          yesPct == null
            ? "var(--fg-4)"
            : yesPct >= 50
              ? "var(--yes)"
              : "var(--no)"
        }
      />
      <BarsRow
        yesPct={yesPct}
        noPct={noPct}
        yesCents={yesCents}
        noCents={noCents}
      />
      <FooterRow market={market} />
    </Link>
  );
}

function EyebrowRow({ market }: { market: Market }) {
  const cat = mockCategory(market.market_id);
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
        <span
          aria-hidden
          style={{
            width: 6,
            height: 6,
            borderRadius: "50%",
            background: cat.color,
            flexShrink: 0,
          }}
        />
        <MockValue hint="category">{cat.name}</MockValue>
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
        yes / no
      </span>
    </div>
  );
}

function TitleRow({ market }: { market: Market }) {
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "40px 1fr",
        gap: "var(--space-3)",
        alignItems: "start",
      }}
    >
      <MarketThumb marketId={market.market_id} name={market.name} />
      <h3
        style={{
          fontFamily: "var(--font-sans)",
          fontWeight: 600,
          fontSize: "var(--fs-14)",
          lineHeight: "var(--lh-14)",
          margin: 0,
          color: "var(--fg-1)",
          display: "-webkit-box",
          WebkitLineClamp: 2,
          WebkitBoxOrient: "vertical",
          overflow: "hidden",
        }}
      >
        {market.name}
      </h3>
    </div>
  );
}

function FeaturedPriceRow({
  label,
  cents,
  delta24Pct,
  points,
  hasPrice,
  priceTone,
}: {
  label: string;
  cents: string;
  delta24Pct: number | null;
  points: import("@/lib/markets/use-card-history").PricePoint[];
  hasPrice: boolean;
  priceTone: string;
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
              color: hasPrice ? priceTone : "var(--fg-4)",
              letterSpacing: "var(--track-mono)",
            }}
          >
            {cents}
          </span>
          {delta24Pct != null && (
            <span
              className="text-mono tabular"
              style={{
                fontSize: "var(--fs-12)",
                color: delta24Pct >= 0 ? "var(--yes)" : "var(--no)",
              }}
            >
              {formatPctDelta(delta24Pct)}
            </span>
          )}
        </div>
      </div>
      <Sparkline points={points} />
    </div>
  );
}

function BarsRow({
  yesPct,
  noPct,
  yesCents,
  noCents,
}: {
  yesPct: number | null;
  noPct: number | null;
  yesCents: string;
  noCents: string;
}) {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-2)",
        alignSelf: "end",
      }}
    >
      <BarRow tone="yes" pct={yesPct} label="YES" centsLabel={yesCents} />
      <BarRow tone="no" pct={noPct} label="NO" centsLabel={noCents} />
    </div>
  );
}

function BarRow({
  tone,
  pct,
  label,
  centsLabel,
}: {
  tone: "yes" | "no";
  pct: number | null;
  label: string;
  centsLabel: string;
}) {
  const fillColor = tone === "yes" ? "var(--yes)" : "var(--no)";
  const trackColor = tone === "yes" ? "var(--yes-faint)" : "var(--no-faint)";
  const width = pct != null ? `${Math.max(0, Math.min(100, pct))}%` : "0%";
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "28px 1fr 44px",
        alignItems: "center",
        gap: "var(--space-3)",
      }}
    >
      <span
        className="text-mono"
        style={{
          fontSize: "10px",
          letterSpacing: "var(--track-wide)",
          color: pct != null ? fillColor : "var(--fg-4)",
        }}
      >
        {label}
      </span>
      <span
        style={{
          height: 6,
          background: pct != null ? trackColor : "var(--surface-2)",
          borderRadius: 3,
          overflow: "hidden",
        }}
      >
        <span
          style={{
            display: "block",
            height: "100%",
            width,
            background: fillColor,
            transition: "width var(--dur-base) var(--ease-standard)",
          }}
        />
      </span>
      <span
        className="text-mono tabular"
        style={{
          fontSize: "var(--fs-12)",
          color: pct != null ? "var(--fg-2)" : "var(--fg-4)",
          textAlign: "right",
        }}
      >
        {centsLabel}
      </span>
    </div>
  );
}

function FooterRow({ market }: { market: Market }) {
  const volNanos = market.volume_nanos ? BigInt(market.volume_nanos) : 0n;
  const vol = volNanos > 0n ? formatCompactDollars(volNanos) : "—";
  const liqNanos = mockLiq(volNanos, market.market_id);
  const liq = liqNanos > 0n ? formatCompactDollars(liqNanos) : "—";
  const traderCount = mockTraders(market.market_id, volNanos);
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
      }}
    >
      <div style={{ display: "flex", gap: "var(--space-3)" }}>
        <FooterChip label="vol" value={vol} />
        <FooterChip
          label="liq"
          value={<MockValue hint="liquidity">{liq}</MockValue>}
        />
      </div>
      <FooterChip
        label="traders"
        value={<MockValue hint="trader count">{traders}</MockValue>}
      />
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

function probabilityPercent(nanos: bigint): number {
  return Number(nanos) / 1e7;
}
