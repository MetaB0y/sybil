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
import type { MarketPrice } from "@/lib/store";
import { MarketThumb } from "./market-thumb";
import { Sparkline } from "./sparkline";

const SECONDARY_OUTCOMES = 3;
const CARD_HEIGHT = 360;

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
      }}
    >
      <EyebrowRow count={markets.length} />
      <TitleRow groupName={groupName} leaderId={leader?.market_id} />
      <FeaturedOutcome
        leader={leader}
        price={leader ? prices[leader.market_id] : undefined}
        points={points}
        delta24Pct={delta24Pct}
      />
      <SecondaryList
        markets={secondary}
        prices={prices}
        hiddenCount={hiddenCount}
        leaderId={leader?.market_id ?? -1}
      />
      <FooterRow totalVol={totalVol} />
    </article>
  );
}

function EyebrowRow({ count }: { count: number }) {
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
          fontSize: "10px",
          letterSpacing: "var(--track-wide)",
          textTransform: "uppercase",
          color: "var(--fg-3)",
        }}
      >
        {"// event"}
      </span>
      <span
        className="text-mono"
        style={{
          fontSize: "10px",
          letterSpacing: "var(--track-wide)",
          textTransform: "uppercase",
          color: "var(--accent)",
        }}
      >
        {count} outcomes
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
              color: priceTone(price?.yes),
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
  hiddenCount,
  leaderId,
}: {
  markets: Market[];
  prices: Record<number, MarketPrice>;
  hiddenCount: number;
  leaderId: number;
}) {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-2)",
        alignSelf: "start",
      }}
    >
      {markets.map((m) => (
        <SecondaryRow
          key={m.market_id}
          market={m}
          price={prices[m.market_id]}
        />
      ))}
      {hiddenCount > 0 && (
        <Link
          href={`/m/${leaderId}`}
          className="text-mono"
          style={{
            paddingTop: "var(--space-2)",
            borderTop: "1px solid var(--border-1)",
            fontSize: "11px",
            color: "var(--fg-3)",
            textDecoration: "none",
            letterSpacing: "var(--track-wide)",
            textTransform: "uppercase",
          }}
        >
          + {hiddenCount} more →
        </Link>
      )}
    </div>
  );
}

function SecondaryRow({
  market,
  price,
}: {
  market: Market;
  price: MarketPrice | undefined;
}) {
  const label = trimOutcomeLabel(market.name);
  const cents = price ? formatCents(price.yes) : "—";
  return (
    <Link
      href={`/m/${market.market_id}`}
      style={{
        display: "grid",
        gridTemplateColumns: "1fr auto",
        gap: "var(--space-3)",
        alignItems: "center",
        padding: "var(--space-1) 0",
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
          color: priceTone(price?.yes),
          textAlign: "right",
        }}
      >
        {cents}
      </span>
    </Link>
  );
}

function FooterRow({ totalVol }: { totalVol: string }) {
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
        <FooterChip label="liq" value="—" />
      </div>
      <FooterChip label="traders" value="—" />
    </div>
  );
}

function priceTone(yes: bigint | undefined): string {
  if (yes == null) return "var(--fg-4)";
  return yes >= 500_000_000n ? "var(--yes)" : "var(--no)";
}

function FooterChip({ label, value }: { label: string; value: string }) {
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
