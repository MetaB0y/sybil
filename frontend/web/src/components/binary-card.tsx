"use client";

import Link from "next/link";
import {
  formatCompactDollars,
  formatDate,
  formatProbability,
} from "@/lib/format/nanos";
import type { Market } from "@/lib/markets/use-markets";
import type { MarketPrice } from "@/lib/store";

type Props = {
  market: Market;
  price: MarketPrice | undefined;
};

/**
 * BinaryCard — one card per YES/NO market.
 *
 * 5-row skeleton (handoff): meta · title · featured price · YES/NO bars · footer.
 * 360px min-height so cards in a row align row-for-row.
 *
 * Live prices come from the WS-fed Zustand store via `price`. If the market
 * has never traded, the card shows "—" placeholders and grayed bars.
 */
export function BinaryCard({ market, price }: Props) {
  const yesProb = price ? formatProbability(price.yes) : "—";
  const noProb = price ? formatProbability(price.no) : "—";
  const yesPct = price ? probabilityPercent(price.yes) : null;
  const noPct = price ? probabilityPercent(price.no) : null;

  const statusLabel = (market.status || "active").toUpperCase();

  return (
    <Link
      href={`/m/${market.market_id}`}
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
        textDecoration: "none",
        color: "var(--fg-1)",
        transition: "border-color var(--dur-fast) var(--ease-standard)",
      }}
      onMouseEnter={(e) =>
        (e.currentTarget.style.borderColor = "var(--border-3)")
      }
      onMouseLeave={(e) =>
        (e.currentTarget.style.borderColor = "var(--border-1)")
      }
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
        <span className="text-meta">#{market.market_id}</span>
        <span
          className="text-mono"
          style={{
            fontSize: "10px",
            letterSpacing: "var(--track-wide)",
            color: market.status === "active" ? "var(--fg-3)" : "var(--warn)",
            textTransform: "uppercase",
          }}
        >
          {statusLabel}
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
        {market.name}
      </h3>

      {/* Row 3 · featured price (YES probability, big) */}
      <div
        style={{
          marginTop: "var(--space-2)",
          display: "flex",
          alignItems: "baseline",
          gap: "var(--space-2)",
        }}
      >
        <span
          className="text-mono tabular"
          style={{
            fontSize: "var(--fs-40)",
            lineHeight: "var(--lh-40)",
            color: price ? "var(--fg-1)" : "var(--fg-4)",
            letterSpacing: "var(--track-mono)",
          }}
        >
          {yesProb}
        </span>
        <span className="text-meta">YES</span>
      </div>

      {/* Row 4 · YES/NO bars (visual probability) */}
      <div
        style={{
          marginTop: "auto",
          display: "flex",
          flexDirection: "column",
          gap: "var(--space-2)",
        }}
      >
        <BarRow tone="yes" pct={yesPct} label="YES" probLabel={yesProb} />
        <BarRow tone="no" pct={noPct} label="NO" probLabel={noProb} />
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
            {market.volume_nanos ? formatCompactDollars(market.volume_nanos) : "—"}
          </span>
        </span>
        <span className="text-meta">
          Resolves&nbsp;
          <span className="text-mono tabular" style={{ color: "var(--fg-2)" }}>
            {formatDate(market.expiry_timestamp_ms)}
          </span>
        </span>
      </footer>
    </Link>
  );
}

function BarRow({
  tone,
  pct,
  label,
  probLabel,
}: {
  tone: "yes" | "no";
  pct: number | null;
  label: string;
  probLabel: string;
}) {
  const fillColor = tone === "yes" ? "var(--yes)" : "var(--no)";
  const trackColor = tone === "yes" ? "var(--yes-faint)" : "var(--no-faint)";
  const width = pct != null ? `${Math.max(0, Math.min(100, pct))}%` : "0%";
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "32px 1fr 56px",
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
        {probLabel}
      </span>
    </div>
  );
}

/** 0..1e9 nanos → 0..100 number. Safe (bounded). */
function probabilityPercent(nanos: bigint): number {
  return Number(nanos) / 1e7;
}
