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
  mockDelta,
  mockLiq,
  mockTraders,
} from "@/lib/mock";
import type { MarketPrice } from "@/lib/store";
import { MarketThumb } from "./market-thumb";
import { MockValue } from "./mock-value";
import { Sparkline } from "./sparkline";

const SECONDARY_OUTCOMES = 3;
const CARD_HEIGHT = 400;

type Props = {
  groupName: string;
  markets: Market[];
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

  const ranked = [...markets].sort((a, b) => {
    const pa = prices[a.market_id]?.yes ?? -1n;
    const pb = prices[b.market_id]?.yes ?? -1n;
    if (pa === pb) return 0;
    return pa > pb ? -1 : 1;
  });

  const leader = ranked[0];
  const secondary = ranked.slice(1, 1 + SECONDARY_OUTCOMES);
  const hiddenCount = Math.max(0, ranked.length - 1 - secondary.length);

  const { points, delta24Pct } = useCardHistory(
    leader?.market_id ?? -1,
    inView && !!leader
  );

  const totalVolumeNanos = sumVolumeNanos(markets);
  const totalVol = totalVolumeNanos
    ? formatCompactDollars(totalVolumeNanos)
    : "—";

  return (
    <article
      ref={ref}
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
        boxSizing: "border-box",
        overflow: "hidden",
      }}
    >
      <EyebrowRow
        groupName={groupName}
        count={markets.length}
        hiddenCount={hiddenCount}
      />
      <TitleRow groupName={groupName} leaderId={leader?.market_id} />
      <FeaturedOutcome
        leader={leader}
        price={leader ? prices[leader.market_id] : undefined}
        points={points}
        delta24Pct={delta24Pct}
      />
      <SecondaryList markets={secondary} prices={prices} />
      <FooterRow totalVol={totalVol} totalVolNanos={sumVolumeNanos(markets) ?? 0n} seed={groupName} />
    </article>
  );
}

function EyebrowRow({
  groupName,
  count,
  hiddenCount,
}: {
  groupName: string;
  count: number;
  hiddenCount: number;
}) {
  const cat = mockCategory(groupName);
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
          fontSize: "11px",
          color: "var(--fg-3)",
        }}
      >
        {count} outcomes
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
}: {
  groupName: string;
  leaderId: number | undefined;
}) {
  const href = leaderId != null ? `/m/${leaderId}` : "#";
  return (
    <Link
      href={href}
      style={{
        display: "grid",
        gridTemplateColumns: "40px 1fr",
        gap: "var(--space-3)",
        alignItems: "start",
        textDecoration: "none",
        color: "var(--fg-1)",
      }}
    >
      <MarketThumb marketId={leaderId ?? 0} name={groupName} />
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
        {groupName}
      </h3>
    </Link>
  );
}

function FeaturedOutcome({
  leader,
  price,
  points,
  delta24Pct,
}: {
  leader: Market | undefined;
  price: MarketPrice | undefined;
  points: import("@/lib/markets/use-card-history").PricePoint[];
  delta24Pct: number | null;
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
  const cents = price ? formatCents(price.yes) : "—";
  const label = trimOutcomeLabel(leader.name);
  return (
    <Link
      href={`/m/${leader.market_id}`}
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
      <div style={{ display: "flex", flexDirection: "column", gap: 2, minWidth: 0 }}>
        <span
          style={{
            fontFamily: "var(--font-sans)",
            fontSize: "var(--fs-13)",
            color: "var(--fg-2)",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
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
              color: deltaTone(delta24Pct, !!price),
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
    </Link>
  );
}

function SecondaryList({
  markets,
  prices,
}: {
  markets: Market[];
  prices: Record<number, MarketPrice>;
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
        />
      ))}
    </div>
  );
}

function SecondaryRow({
  market,
  price,
  first,
}: {
  market: Market;
  price: MarketPrice | undefined;
  first?: boolean;
}) {
  const label = trimOutcomeLabel(market.name);
  const yesPct = price ? Number(price.yes) / 1e7 : null;
  const cents = price ? formatCents(price.yes) : "—";
  const delta = mockDelta(market.market_id, yesPct);
  const tone = deltaTone(price ? delta : null, !!price);
  const volNanos = market.volume_nanos ? BigInt(market.volume_nanos) : 0n;
  const vol = volNanos > 0n ? formatCompactDollars(volNanos) : "—";
  return (
    <Link
      href={`/m/${market.market_id}`}
      style={{
        display: "grid",
        gridTemplateColumns: "minmax(0, 1fr) 44px 52px 52px",
        gap: "var(--space-2)",
        alignItems: "center",
        padding: "10px 0",
        borderTop: first ? "none" : "1px solid var(--border-1)",
        textDecoration: "none",
        color: "var(--fg-1)",
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
        }}
      >
        {label}
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
      <MockValue hint="24h delta" style={{ textAlign: "right" }}>
        <span
          className="text-mono tabular"
          style={{
            fontSize: "11px",
            color: price ? (delta >= 0 ? "var(--yes)" : "var(--no)") : "var(--fg-4)",
            display: "inline-block",
            width: "100%",
            textAlign: "right",
          }}
        >
          {price ? formatPctDelta(delta) : "—"}
        </span>
      </MockValue>
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
  totalVolNanos,
  seed,
}: {
  totalVol: string;
  totalVolNanos: bigint;
  seed: string;
}) {
  const liqNanos = mockLiq(totalVolNanos, seed);
  const liq = liqNanos > 0n ? formatCompactDollars(liqNanos) : "—";
  const traderCount = mockTraders(seed, totalVolNanos);
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
        <FooterChip label="vol" value={totalVol} />
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

/**
 * Color an outcome's cents by delta sign (handoff convention).
 * No price → fg-4 (dim). No delta yet → neutral fg-1.
 */
function deltaTone(delta: number | null, hasPrice: boolean): string {
  if (!hasPrice) return "var(--fg-4)";
  if (delta == null) return "var(--fg-1)";
  return delta >= 0 ? "var(--yes)" : "var(--no)";
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

function trimOutcomeLabel(name: string): string {
  const idx = name.indexOf(":");
  if (idx < 0 || idx > name.length - 2) return name;
  return name.slice(idx + 1).trim();
}

function sumVolumeNanos(markets: Market[]): bigint | null {
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
