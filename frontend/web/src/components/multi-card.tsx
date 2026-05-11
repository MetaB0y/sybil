"use client";

import Link from "next/link";
import {
  formatCompactDollars,
  formatProbability,
} from "@/lib/format/nanos";
import type { Market } from "@/lib/markets/use-markets";
import type { MarketPrice } from "@/lib/store";

const TOP_OUTCOMES = 4;

type Props = {
  groupName: string;
  markets: Market[];
  prices: Record<number, MarketPrice>;
};

/**
 * MultiCard — one card per multi-outcome event (an event group with many
 * child markets, e.g. "2026 FIFA World Cup Winner" → 48 candidates).
 *
 * Surfaces the top N outcomes by current YES probability, plus an aggregate
 * footer (total volume across the group, count of outcomes). Same 5-row
 * skeleton as BinaryCard for row-for-row alignment.
 *
 * The card itself isn't a link — there's no single "event detail" page yet;
 * tapping an outcome row goes to that child market's detail.
 */
export function MultiCard({ groupName, markets, prices }: Props) {
  // Sort by current YES probability (priced first), descending.
  const ranked = [...markets].sort((a, b) => {
    const pa = prices[a.market_id]?.yes ?? -1n;
    const pb = prices[b.market_id]?.yes ?? -1n;
    if (pa === pb) return 0;
    return pa > pb ? -1 : 1;
  });
  const visible = ranked.slice(0, TOP_OUTCOMES);
  const hiddenCount = ranked.length - visible.length;

  const totalVolumeNanos = sumVolumeNanos(markets);

  return (
    <article
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-3)",
        minHeight: 360,
        padding: "var(--space-4) var(--space-5)",
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-lg)",
        boxShadow: "var(--shadow-inset-top)",
      }}
    >
      {/* Row 1 · meta */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: "var(--space-2)",
        }}
      >
        <span className="text-meta">
          {markets.length} outcomes
        </span>
        <span
          className="text-mono"
          style={{
            fontSize: "10px",
            letterSpacing: "var(--track-wide)",
            color: "var(--accent)",
            textTransform: "uppercase",
          }}
        >
          event
        </span>
      </div>

      {/* Row 2 · title */}
      <h3
        style={{
          fontFamily: "var(--font-sans)",
          fontWeight: 600,
          fontSize: "var(--fs-16)",
          lineHeight: "var(--lh-20)",
          margin: 0,
          color: "var(--fg-1)",
          display: "-webkit-box",
          WebkitLineClamp: 3,
          WebkitBoxOrient: "vertical",
          overflow: "hidden",
        }}
      >
        {groupName}
      </h3>

      {/* Row 3+4 · top outcomes */}
      <div
        style={{
          marginTop: "var(--space-2)",
          marginBottom: "auto",
          display: "flex",
          flexDirection: "column",
          gap: "var(--space-2)",
        }}
      >
        {visible.map((m) => (
          <OutcomeRow
            key={m.market_id}
            market={m}
            price={prices[m.market_id]}
          />
        ))}
        {hiddenCount > 0 && (
          <div
            className="text-meta"
            style={{
              paddingTop: "var(--space-2)",
              borderTop: "1px solid var(--border-1)",
            }}
          >
            + {hiddenCount} more
          </div>
        )}
      </div>

      {/* Row 5 · footer */}
      <footer
        style={{
          display: "flex",
          justifyContent: "space-between",
          gap: "var(--space-3)",
          paddingTop: "var(--space-3)",
          borderTop: "1px solid var(--border-1)",
        }}
      >
        <span className="text-meta">
          Vol&nbsp;
          <span className="text-mono tabular" style={{ color: "var(--fg-2)" }}>
            {totalVolumeNanos != null
              ? formatCompactDollars(totalVolumeNanos)
              : "—"}
          </span>
        </span>
        <span className="text-meta">
          <span className="text-mono tabular" style={{ color: "var(--fg-2)" }}>
            {markets.length}
          </span>
          &nbsp;markets
        </span>
      </footer>
    </article>
  );
}

function OutcomeRow({
  market,
  price,
}: {
  market: Market;
  price: MarketPrice | undefined;
}) {
  const label = trimOutcomeLabel(market.name);
  const pct = price ? probabilityPercent(price.yes) : null;
  const prob = price ? formatProbability(price.yes) : "—";
  return (
    <Link
      href={`/m/${market.market_id}`}
      style={{
        display: "grid",
        gridTemplateColumns: "1fr 64px",
        alignItems: "center",
        gap: "var(--space-3)",
        padding: "var(--space-2) 0",
        color: "var(--fg-1)",
        textDecoration: "none",
        position: "relative",
        overflow: "hidden",
      }}
    >
      {/* Probability fill bar — sits behind the row, anchored left */}
      <span
        aria-hidden
        style={{
          position: "absolute",
          inset: 0,
          width: pct != null ? `${Math.max(0, Math.min(100, pct))}%` : "0%",
          background: "var(--yes-faint)",
          borderRadius: "var(--radius-sm)",
          transition: "width var(--dur-base) var(--ease-standard)",
        }}
      />
      <span
        style={{
          position: "relative",
          fontFamily: "var(--font-sans)",
          fontSize: "var(--fs-14)",
          color: "var(--fg-1)",
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
          position: "relative",
          fontSize: "var(--fs-14)",
          textAlign: "right",
          color: pct != null ? "var(--yes)" : "var(--fg-4)",
        }}
      >
        {prob}
      </span>
    </Link>
  );
}

/** Strip the parent event prefix from a child market name when present.
 *  e.g. "Democratic Presidential Nominee 2028: Andy Beshear" → "Andy Beshear" */
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

function probabilityPercent(nanos: bigint): number {
  return Number(nanos) / 1e7;
}
