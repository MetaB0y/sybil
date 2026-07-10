"use client";

/**
 * 24h pulse strip — four real `last_24h` figures from
 * `GET /v1/activity/overview` (see use-activity-overview.ts), one per cell.
 * Values show "—" until the first response lands.
 */

import { formatCompactInt } from "@/lib/format/nanos";
import type { Last24hStats } from "@/lib/activity/types";

type Cell = {
  label: string;
  value: string;
  accent?: string;
};

const fmtCount = (n: number | null): string =>
  n == null ? "—" : formatCompactInt(n);

export function PulseStrip({ last24h }: { last24h: Last24hStats }) {
  const items: Cell[] = [
    {
      label: "Matched volume",
      value: last24h.matchedVolume,
    },
    {
      label: "Active traders",
      value: fmtCount(last24h.traders),
    },
    {
      label: "Placed orders",
      value: fmtCount(last24h.ordersPlacedDistinct),
    },
    {
      label: "Matched orders",
      value: fmtCount(last24h.ordersMatched),
      accent: "var(--yes)",
    },
  ];
  return (
    <section className="activity-pulse-section" style={{ padding: "20px 24px 4px" }}>
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          gap: 14,
          paddingBottom: 14,
        }}
      >
        <h3
          style={{
            fontFamily: "var(--font-sans)",
            fontSize: 13,
            fontWeight: 600,
            margin: 0,
            color: "var(--fg-2)",
            textTransform: "uppercase",
            letterSpacing: "0.06em",
          }}
        >
          Last 24 hours
        </h3>
        <span className="text-annotation" style={{ fontSize: 11 }}>
          rolling window
        </span>
      </div>
      <div
        className="activity-pulse-grid"
      >
        {items.map((it, i) => (
          <div
            key={it.label}
            className="activity-pulse-cell"
            style={{
              padding: "14px 18px",
              borderRight:
                i < items.length - 1 ? "1px solid var(--border-1)" : 0,
              display: "flex",
              flexDirection: "column",
              gap: 8,
              minWidth: 0,
            }}
          >
            <span className="eyebrow">{it.label}</span>
            <span
              style={{
                fontFamily: "var(--font-sans)",
                fontSize: 22,
                fontWeight: 600,
                color: it.accent ?? "var(--fg-1)",
                fontVariantNumeric: "tabular-nums",
                letterSpacing: "-0.01em",
                lineHeight: 1,
              }}
            >
              {it.value}
            </span>
          </div>
        ))}
      </div>
    </section>
  );
}
