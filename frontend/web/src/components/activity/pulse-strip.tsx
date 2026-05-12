"use client";

/**
 * 24h pulse strip — 5 cells with big numbers + ±% delta vs prior 24h.
 *
 * Every cell is mocked until /v1/activity/overview lands (OPEN_QUESTIONS #3).
 * At the 2s FBA cadence on this network we can't compute a 24h window
 * client-side. The MockValue underline makes the placeholder visible.
 */

import { MockValue } from "@/components/mock-value";
import { formatCompactInt, formatPctDelta } from "@/lib/format/nanos";
import { MOCK_24H } from "@/lib/activity/mocks";

const MOCK_HINT =
  "last-24h rollups — needs /v1/activity/overview (OPEN_QUESTIONS #3, infeasible client-side at 2s cadence)";

type Cell = {
  label: string;
  value: string;
  deltaPct?: number;
  accent?: string;
};

export function PulseStrip() {
  const items: Cell[] = [
    {
      label: "Matched volume",
      value: MOCK_24H.matchedVolume,
      deltaPct: MOCK_24H.matchedVolumeDeltaPct,
    },
    {
      label: "Active traders",
      value: formatCompactInt(MOCK_24H.traders),
      deltaPct: MOCK_24H.tradersDeltaPct,
    },
    {
      label: "Placed orders",
      value: formatCompactInt(MOCK_24H.ordersPlaced),
    },
    {
      label: "Matched orders",
      value: formatCompactInt(MOCK_24H.ordersMatched),
      accent: "var(--yes)",
    },
    {
      label: "Unmatched orders",
      value: formatCompactInt(MOCK_24H.ordersUnmatched),
      accent: "var(--fg-2)",
    },
  ];
  return (
    <section style={{ padding: "20px 24px 4px" }}>
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
          rolling window · vs prior 24 h
        </span>
        <span
          style={{
            marginLeft: "auto",
            fontFamily: "var(--font-mono)",
            fontSize: 10,
            color: "var(--fg-3)",
            textTransform: "uppercase",
            letterSpacing: "0.04em",
          }}
        >
          mocked
        </span>
      </div>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(5, 1fr)",
          borderTop: "1px solid var(--border-1)",
          borderBottom: "1px solid var(--border-1)",
        }}
      >
        {items.map((it, i) => (
          <div
            key={it.label}
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
            <MockValue hint={MOCK_HINT}>
              <span className="eyebrow">{it.label}</span>
            </MockValue>
            <div
              style={{
                display: "flex",
                alignItems: "baseline",
                justifyContent: "space-between",
                gap: 10,
              }}
            >
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
              {it.deltaPct != null && (
                <span
                  style={{
                    fontFamily: "var(--font-mono)",
                    fontSize: 11,
                    color: it.deltaPct >= 0 ? "var(--yes)" : "var(--no)",
                    fontVariantNumeric: "tabular-nums",
                  }}
                >
                  {formatPctDelta(it.deltaPct)}
                </span>
              )}
            </div>
          </div>
        ))}
      </div>
    </section>
  );
}
