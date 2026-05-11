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
  const { points, delta24Pct, noDelta24Pct } = useCardHistory(
    market.market_id,
    inView
  );

  const yesCents = price ? formatCents(price.yes) : "—";
  const noCents = price ? formatCents(price.no) : "—";

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
        cents={yesCents}
        delta24Pct={delta24Pct}
        points={points}
        hasPrice={!!price}
      />
      <SideList
        market={market}
        yesCents={yesCents}
        noCents={noCents}
        hasPrice={!!price}
        yesDelta={delta24Pct}
        noDelta={noDelta24Pct}
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
  cents,
  delta24Pct,
  points,
  hasPrice,
}: {
  cents: string;
  delta24Pct: number | null;
  points: import("@/lib/markets/use-card-history").PricePoint[];
  hasPrice: boolean;
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
              color: hasPrice ? "var(--yes)" : "var(--fg-4)",
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
      <Sparkline points={points} tone="yes" />
    </div>
  );
}

function SideList({
  market,
  yesCents,
  noCents,
  hasPrice,
  yesDelta,
  noDelta,
}: {
  market: Market;
  yesCents: string;
  noCents: string;
  hasPrice: boolean;
  yesDelta: number | null;
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
      <SideRow
        side="Yes"
        cents={yesCents}
        centsColor={hasPrice ? "var(--yes)" : "var(--fg-4)"}
        delta={yesDelta}
        vol={vol}
        first
      />
      <SideRow
        side="No"
        cents={noCents}
        centsColor={hasPrice ? "var(--no)" : "var(--fg-4)"}
        delta={noDelta}
        vol={vol}
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
        style={{
          fontSize: "11px",
          color:
            delta == null
              ? "var(--fg-4)"
              : delta >= 0
                ? "var(--yes)"
                : "var(--no)",
          textAlign: "right",
        }}
      >
        {delta != null ? formatPctDelta(delta) : "—"}
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

