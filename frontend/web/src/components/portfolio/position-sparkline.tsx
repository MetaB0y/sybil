"use client";

/**
 * Small SVG sparkline plotting recent YES-side price points for a market.
 * Coverage of `/v1/markets/{id}/prices/history` per-market isn't proven so
 * we render flat when there's no data; the rendered cell still occupies
 * space so positions-list rows stay aligned.
 */

import { useMemo } from "react";
import { usePriceHistory } from "@/lib/account/use-price-history";
import { parseNanos } from "@/lib/format/nanos";

const W = 86;
const H = 22;

export function PositionSparkline({
  marketId,
  outcome,
}: {
  marketId: number;
  outcome: "YES" | "NO" | string;
}) {
  const history = usePriceHistory(marketId);

  const series = useMemo(() => {
    if (!history.data?.points || history.data.points.length < 2) return null;
    const pts = history.data.points.slice(-32); // last 32 points ≈ "7d-ish"
    const upper = outcome.toUpperCase();
    return pts.map((p) =>
      upper === "NO"
        ? Number(parseNanos(p.no_price_nanos)) / 1e7 // cents
        : Number(parseNanos(p.yes_price_nanos)) / 1e7,
    );
  }, [history.data, outcome]);

  if (!series) {
    return (
      <svg width={W} height={H} aria-hidden style={{ display: "block" }}>
        <line
          x1={0}
          y1={H / 2}
          x2={W}
          y2={H / 2}
          stroke="var(--fg-4)"
          strokeWidth={1}
          strokeDasharray="3 3"
          strokeOpacity={0.4}
        />
      </svg>
    );
  }

  const min = Math.min(...series);
  const max = Math.max(...series);
  const span = Math.max(0.5, max - min);
  const yFor = (v: number) => 2 + ((max - v) / span) * (H - 4);
  const xFor = (i: number) => (i / (series.length - 1)) * W;
  const d = series
    .map((v, i) => `${i === 0 ? "M" : "L"} ${xFor(i).toFixed(2)} ${yFor(v).toFixed(2)}`)
    .join(" ");
  const positive = series[series.length - 1]! >= series[0]!;
  const stroke = positive ? "var(--yes)" : "var(--no)";

  return (
    <svg width={W} height={H} aria-hidden style={{ display: "block" }}>
      <path d={d} fill="none" stroke={stroke} strokeWidth={1.2} strokeLinejoin="round" />
    </svg>
  );
}
