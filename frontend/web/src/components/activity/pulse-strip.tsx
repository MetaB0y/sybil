"use client";

/**
 * 24h pulse strip — five real `last_24h` figures from
 * `GET /v1/activity/overview` (see use-activity-overview.ts), one per cell.
 *
 * The ±% deltas on Matched volume and Active traders stay mocked — the
 * overview response carries no `prior_24h` bucket — and wear a MockValue
 * pill so they read as placeholders. Values show "—" until the first
 * response lands.
 */

import { MockValue } from "@/components/mock-value";
import { formatCompactInt, formatPctDelta } from "@/lib/format/nanos";
import { MOCK_24H_DELTAS } from "@/lib/activity/mocks";
import type { Last24hStats } from "@/lib/activity/types";

const DELTA_HINT = "vs prior 24h — backend has no prior_24h bucket";

type Cell = {
  label: string;
  value: string;
  /** Mocked ±% vs prior 24h — only the first two cells carry one. */
  deltaPct?: number;
  accent?: string;
};

const fmtCount = (n: number | null): string =>
  n == null ? "—" : formatCompactInt(n);

export function PulseStrip({ last24h }: { last24h: Last24hStats }) {
  const items: Cell[] = [
    {
      label: "Matched volume",
      value: last24h.matchedVolume,
      deltaPct: MOCK_24H_DELTAS.matchedVolumeDeltaPct,
    },
    {
      label: "Active traders",
      value: fmtCount(last24h.traders),
      deltaPct: MOCK_24H_DELTAS.tradersDeltaPct,
    },
    {
      label: "Placed orders",
      value: fmtCount(last24h.ordersPlaced),
    },
    {
      label: "Matched orders",
      value: fmtCount(last24h.ordersMatched),
      accent: "var(--yes)",
    },
    {
      label: "Unmatched orders",
      value: fmtCount(last24h.ordersUnmatched),
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
            <span className="eyebrow">{it.label}</span>
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
                <MockValue hint={DELTA_HINT} variant="pill">
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
                </MockValue>
              )}
            </div>
          </div>
        ))}
      </div>
    </section>
  );
}
