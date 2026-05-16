"use client";

/**
 * PriceChart — hand-rolled SVG, no charting library.
 *
 * Two modes share one crosshair/tooltip shell:
 *  - multi-outcome events → 100%-stacked area, one colored band per outcome
 *    (favourite at the bottom), heights from the column-normalized series.
 *  - binary markets → a single YES-probability area on a 0–100% axis.
 *
 * Live: re-derives from the store's recent-block ring buffer, so the line
 * advances every 2s batch on a normal React render — no imperative chart
 * lifecycle. Matches `StackedAreaChart` in `fed-primitives.jsx:82`.
 */

import { useMemo, useRef, useState } from "react";
import { colorForOutcome } from "@/components/outcome-legend";
import { formatAge } from "@/lib/format/nanos";
import { buildChartSeries } from "@/lib/market-detail/build-chart-series";
import type { EventOutcome } from "@/lib/market-detail/use-event-group";
import type { PricePoint } from "@/lib/markets/use-price-history";
import { selectRecentBlocks, useStore } from "@/lib/store";

const W = 1000;
const H = 280;
const Y_TICKS = [1, 0.75, 0.5, 0.25, 0];

type Props = {
  outcomes: EventOutcome[];
  byMarket: Map<number, PricePoint[]>;
  isMultiOutcome: boolean;
  /** Lower bound of the selected range (ms), or null for ALL. */
  sinceMs: number | null;
};

export function PriceChart({
  outcomes,
  byMarket,
  isMultiOutcome,
  sinceMs,
}: Props) {
  const recent = useStore(selectRecentBlocks);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [hover, setHover] = useState<number | null>(null);

  const series = useMemo(
    () => buildChartSeries(outcomes, byMarket, recent, sinceMs),
    [outcomes, byMarket, recent, sinceMs],
  );
  // Whether any history exists at all — distinguishes "nothing yet" from
  // "nothing in this range".
  const everHadData = useMemo(
    () => buildChartSeries(outcomes, byMarket, recent, null).times.length > 0,
    [outcomes, byMarket, recent],
  );

  const N = series.times.length;

  if (N < 2) {
    return (
      <ChartMessage>
        {everHadData
          ? "no activity in this range — pick a wider window."
          : "no clearing history yet — chart will populate as batches clear."}
      </ChartMessage>
    );
  }

  const stepX = W / (N - 1);
  const yOf = (v: number) => (1 - v) * H;

  // Stacked: favourite (index 0) at the bottom. Each band sits between the
  // cumulative normalized total below it and including it.
  const bands = outcomes.map((o, k) => {
    const color = colorForOutcome(o, k);
    if (isMultiOutcome) {
      const top: number[] = [];
      const bottom: number[] = [];
      for (let i = 0; i < N; i++) {
        let below = 0;
        for (let j = 0; j < k; j++) below += series.norm[j]![i]!;
        bottom.push(below);
        top.push(below + series.norm[k]![i]!);
      }
      return { color, fill: bandPath(top, bottom, stepX, yOf), line: linePath(top, stepX, yOf) };
    }
    // Binary: single area from the 0 baseline to the YES probability.
    const top = series.raw[k]!;
    const base = new Array(N).fill(0);
    return { color, fill: bandPath(top, base, stepX, yOf), line: linePath(top, stepX, yOf) };
  });

  const lastIdx = N - 1;
  const nowMs = series.times[lastIdx]!;

  const onMove = (e: React.MouseEvent) => {
    const el = containerRef.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const x = e.clientX - r.left;
    const idx = Math.max(0, Math.min(lastIdx, Math.round((x / r.width) * lastIdx)));
    setHover(idx);
  };

  const hoverFrac = hover == null ? 0 : hover / lastIdx;

  return (
    <div
      ref={containerRef}
      onMouseMove={onMove}
      onMouseLeave={() => setHover(null)}
      style={{ position: "relative", width: "100%", height: H }}
    >
      <svg
        viewBox={`0 0 ${W} ${H}`}
        width="100%"
        height={H}
        preserveAspectRatio="none"
        style={{ display: "block" }}
      >
        {Y_TICKS.map((y) => (
          <line
            key={y}
            x1={0}
            x2={W}
            y1={yOf(y)}
            y2={yOf(y)}
            stroke="rgba(255,255,255,0.05)"
            strokeDasharray={y === 0 || y === 1 ? undefined : "2 4"}
          />
        ))}
        {bands.map((b, k) => (
          <g key={outcomes[k]!.marketId}>
            <path d={b.fill} fill={b.color} fillOpacity={isMultiOutcome ? 0.34 : 0.18} />
            <path
              d={b.line}
              fill="none"
              stroke={b.color}
              strokeWidth={1.5}
              strokeLinejoin="round"
              vectorEffect="non-scaling-stroke"
            />
          </g>
        ))}
        {hover != null && (
          <line
            x1={hover * stepX}
            x2={hover * stepX}
            y1={0}
            y2={H}
            stroke="rgba(255,255,255,0.4)"
            strokeDasharray="2 3"
          />
        )}
      </svg>

      {/* y-axis labels, overlaid top-right of each gridline */}
      <div
        style={{
          position: "absolute",
          top: 0,
          right: 0,
          height: H,
          pointerEvents: "none",
          display: "flex",
          flexDirection: "column",
          justifyContent: "space-between",
          fontFamily: "var(--font-mono)",
          fontSize: 9,
          color: "var(--fg-4)",
          padding: "2px 2px 0",
        }}
      >
        {Y_TICKS.map((y) => (
          <span key={y}>{Math.round(y * 100)}%</span>
        ))}
      </div>

      {hover != null && (
        <div
          style={{
            position: "absolute",
            top: 8,
            left: `${hoverFrac * 100}%`,
            transform: `translateX(${hoverFrac > 0.6 ? "calc(-100% - 12px)" : "12px"})`,
            background: "var(--surface-3, var(--surface-2))",
            border: "1px solid var(--border-2)",
            borderRadius: 4,
            padding: "8px 10px",
            minWidth: 168,
            pointerEvents: "none",
            boxShadow: "var(--shadow-popover, 0 8px 24px rgba(0,0,0,0.4))",
            fontFamily: "var(--font-mono)",
            fontSize: 10,
          }}
        >
          <div
            style={{
              color: "var(--fg-3)",
              textTransform: "uppercase",
              letterSpacing: "0.04em",
              marginBottom: 6,
              fontSize: 9,
            }}
          >
            {hover === lastIdx ? "now" : formatAge(nowMs - series.times[hover]!)}
          </div>
          {outcomes.map((o, k) => (
            <div
              key={o.marketId}
              style={{
                display: "flex",
                justifyContent: "space-between",
                gap: 14,
                lineHeight: "16px",
              }}
            >
              <span style={{ display: "flex", alignItems: "center", gap: 6, color: "var(--fg-2)", minWidth: 0 }}>
                <span
                  style={{
                    width: 6,
                    height: 6,
                    borderRadius: 1,
                    background: colorForOutcome(o, k),
                    flexShrink: 0,
                  }}
                />
                <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {o.shortLabel}
                </span>
              </span>
              <span style={{ color: "var(--fg-1)", flexShrink: 0 }}>
                {Math.round(series.raw[k]![hover]! * 100)}¢
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/** Path of the top edge only — `M`/`L` along the value array. */
function linePath(vals: number[], stepX: number, yOf: (v: number) => number): string {
  let d = "";
  for (let i = 0; i < vals.length; i++) {
    d += `${i === 0 ? "M" : "L"}${(i * stepX).toFixed(1)} ${yOf(vals[i]!).toFixed(1)} `;
  }
  return d;
}

/** Closed band: forward along `top`, back along `bottom`. */
function bandPath(
  top: number[],
  bottom: number[],
  stepX: number,
  yOf: (v: number) => number,
): string {
  let d = "";
  for (let i = 0; i < top.length; i++) {
    d += `${i === 0 ? "M" : "L"}${(i * stepX).toFixed(1)} ${yOf(top[i]!).toFixed(1)} `;
  }
  for (let i = bottom.length - 1; i >= 0; i--) {
    d += `L${(i * stepX).toFixed(1)} ${yOf(bottom[i]!).toFixed(1)} `;
  }
  return `${d}Z`;
}

function ChartMessage({ children }: { children: React.ReactNode }) {
  return (
    <div
      className="text-mono"
      style={{
        height: H,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        color: "var(--fg-4)",
        fontSize: 12,
      }}
    >
      {children}
    </div>
  );
}
