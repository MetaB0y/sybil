"use client";

/**
 * PriceChart — hand-rolled SVG, no charting library.
 *
 * Three modes share one crosshair / tooltip / time-axis shell:
 *  - `area`    binary single market → one YES-probability area, 0–100% axis.
 *  - `stacked` NegRisk multi-outcome → 100%-stacked bands. Heights are
 *              normalized across the *shown* outcomes, so hiding some still
 *              fills 0–100% ("share among shown"); tooltip shows raw ¢.
 *  - `lines`   non-NegRisk grouped event → one independent YES line per
 *              outcome on a shared 0–100% axis, no fill — their prices are
 *              uncorrelated, so a stacked partition would be misleading.
 *
 * The x-axis is proportional to wall-clock time: a point's x position is
 * `(t - t0) / span`, so a 4h gap is drawn wide and back-to-back 2s batches
 * narrow. Ticks fall at even time intervals across the window.
 *
 * Only the outcomes passed in `drawn` are plotted (the legend caps this at
 * 8). Live ticks come from the recent-block ring buffer — the line advances
 * every 2s batch on a normal render, no imperative chart lifecycle.
 */

import { useMemo, useRef, useState } from "react";
import { colorForOutcome } from "@/components/outcome-legend";
import { formatAge } from "@/lib/format/nanos";
import { buildChartSeries } from "@/lib/market-detail/build-chart-series";
import type { EventOutcome } from "@/lib/market-detail/use-event-group";
import type { PricePoint } from "@/lib/markets/use-price-history";
import { selectRecentBlocks, useStore } from "@/lib/store";

const W = 1000;
/** Plot height; the time axis sits in an extra strip below. */
const PLOT_H = 280;
const AXIS_H = 24;
const Y_TICKS = [1, 0.75, 0.5, 0.25, 0];
const X_TICKS = 5;

export type ChartMode = "area" | "stacked" | "lines";

/** An outcome to plot, plus its stable color index in the full group. */
export type DrawnOutcome = { outcome: EventOutcome; colorIndex: number };

type Props = {
  drawn: DrawnOutcome[];
  byMarket: Map<number, PricePoint[]>;
  mode: ChartMode;
  /** Lower bound of the selected range (ms), or null for ALL. */
  sinceMs: number | null;
  /** Reference "now" — latest committed block time; the axis right edge. */
  nowMs: number;
};

export function PriceChart({ drawn, byMarket, mode, sinceMs, nowMs }: Props) {
  const recent = useStore(selectRecentBlocks);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [hover, setHover] = useState<number | null>(null);

  const outcomes = useMemo(() => drawn.map((d) => d.outcome), [drawn]);

  const series = useMemo(
    () => buildChartSeries(outcomes, byMarket, recent, sinceMs, nowMs),
    [outcomes, byMarket, recent, sinceMs, nowMs],
  );

  const N = series.times.length;

  if (!series.hasData || N < 2) {
    return (
      <ChartMessage>
        {series.hasData
          ? "no activity in this range — pick a wider window."
          : "no clearing history yet — chart will populate as batches clear."}
      </ChartMessage>
    );
  }

  const lastIdx = N - 1;
  // Axis domain = the selected window; the line itself may start later.
  const t0 = series.domainStart;
  const tEnd = series.domainEnd;
  const span = Math.max(1, tEnd - t0);

  // x is proportional to time, not to point index.
  const xs = series.times.map((t) => ((t - t0) / span) * W);
  const yOf = (v: number) => (1 - v) * PLOT_H;

  // Per-mode geometry. `stacked` re-normalizes across the shown outcomes;
  // `lines` / `area` plot raw probabilities directly.
  const layers = drawn.map((d, k) => {
    const color = colorForOutcome(d.outcome, d.colorIndex);
    const row = series.raw[k]!;
    if (mode === "stacked") {
      const top: number[] = [];
      const bottom: number[] = [];
      for (let i = 0; i < N; i++) {
        let sum = 0;
        for (let j = 0; j < drawn.length; j++) sum += series.raw[j]![i]!;
        let below = 0;
        for (let j = 0; j < k; j++) {
          below += sum > 0 ? series.raw[j]![i]! / sum : 1 / drawn.length;
        }
        const self = sum > 0 ? row[i]! / sum : 1 / drawn.length;
        bottom.push(below);
        top.push(below + self);
      }
      return { color, fill: bandPath(top, bottom, xs, yOf), line: linePath(top, xs, yOf), filled: true };
    }
    if (mode === "area") {
      return {
        color,
        fill: bandPath(row, new Array(N).fill(0), xs, yOf),
        line: linePath(row, xs, yOf),
        filled: true,
      };
    }
    // lines — no fill
    return { color, fill: "", line: linePath(row, xs, yOf), filled: false };
  });

  // Ticks at even time intervals across the window.
  const count = Math.max(2, Math.min(X_TICKS, N));
  const xTicks = Array.from({ length: count }, (_, i) => {
    const frac = i / (count - 1);
    return { frac, t: t0 + frac * span };
  });

  const onMove = (e: React.MouseEvent) => {
    const el = containerRef.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const frac = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width));
    const targetT = t0 + frac * span;
    // Nearest point in time to the cursor.
    let best = 0;
    let bestD = Infinity;
    for (let i = 0; i < N; i++) {
      const d = Math.abs(series.times[i]! - targetT);
      if (d < bestD) {
        bestD = d;
        best = i;
      }
    }
    setHover(best);
  };

  const hoverFrac = hover == null ? 0 : xs[hover]! / W;

  return (
    <div style={{ width: "100%" }}>
      <div
        ref={containerRef}
        onMouseMove={onMove}
        onMouseLeave={() => setHover(null)}
        style={{ position: "relative", width: "100%", height: PLOT_H }}
      >
        <svg
          viewBox={`0 0 ${W} ${PLOT_H}`}
          width="100%"
          height={PLOT_H}
          preserveAspectRatio="none"
          style={{ display: "block" }}
        >
          {Y_TICKS.map((y) => (
            <line
              key={`y${y}`}
              x1={0}
              x2={W}
              y1={yOf(y)}
              y2={yOf(y)}
              stroke="rgba(255,255,255,0.05)"
              strokeDasharray={y === 0 || y === 1 ? undefined : "2 4"}
            />
          ))}
          {xTicks.map((tick) => (
            <line
              key={`x${tick.frac}`}
              x1={tick.frac * W}
              x2={tick.frac * W}
              y1={0}
              y2={PLOT_H}
              stroke="rgba(255,255,255,0.04)"
            />
          ))}
          {layers.map((l, k) => (
            <g key={drawn[k]!.outcome.marketId}>
              {l.filled && (
                <path
                  d={l.fill}
                  fill={l.color}
                  fillOpacity={mode === "stacked" ? 0.34 : 0.16}
                />
              )}
              <path
                d={l.line}
                fill="none"
                stroke={l.color}
                strokeWidth={1.5}
                strokeLinejoin="round"
                vectorEffect="non-scaling-stroke"
              />
            </g>
          ))}
          {hover != null && (
            <line
              x1={xs[hover]!}
              x2={xs[hover]!}
              y1={0}
              y2={PLOT_H}
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
            height: PLOT_H,
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
              {hover === lastIdx ? "now" : `${formatAge(tEnd - series.times[hover]!)} ago`}
            </div>
            {drawn.map((d, k) => (
              <div
                key={d.outcome.marketId}
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  gap: 14,
                  lineHeight: "16px",
                }}
              >
                <span
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 6,
                    color: "var(--fg-2)",
                    minWidth: 0,
                  }}
                >
                  <span
                    style={{
                      width: 6,
                      height: 6,
                      borderRadius: 1,
                      background: colorForOutcome(d.outcome, d.colorIndex),
                      flexShrink: 0,
                    }}
                  />
                  <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    {d.outcome.shortLabel}
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

      {/* time axis */}
      <div
        style={{
          position: "relative",
          height: AXIS_H,
          fontFamily: "var(--font-mono)",
          fontSize: 9,
          color: "var(--fg-4)",
        }}
      >
        {xTicks.map((tick, i) => {
          const align =
            i === 0 ? "0" : i === xTicks.length - 1 ? "-100%" : "-50%";
          return (
            <span
              key={tick.frac}
              style={{
                position: "absolute",
                top: 6,
                left: `${tick.frac * 100}%`,
                transform: `translateX(${align})`,
                whiteSpace: "nowrap",
              }}
            >
              {formatAxisTime(tick.t, span)}
            </span>
          );
        })}
      </div>
    </div>
  );
}

/** Axis label — resolution scales with the window: seconds for a couple of
 *  minutes, clock for intraday, date for longer spans. */
function formatAxisTime(ms: number, spanMs: number): string {
  const d = new Date(ms);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  if (spanMs <= 10 * 60_000) {
    return `${hh}:${mm}:${String(d.getSeconds()).padStart(2, "0")}`;
  }
  if (spanMs <= 36 * 3600_000) return `${hh}:${mm}`;
  const mon = d.toLocaleString("en-US", { month: "short" });
  if (spanMs <= 200 * 24 * 3600_000) return `${mon} ${d.getDate()}`;
  return `${mon} '${String(d.getFullYear()).slice(2)}`;
}

/** Path of the top edge only — `M`/`L` along `(xs[i], yOf(vals[i]))`. */
function linePath(vals: number[], xs: number[], yOf: (v: number) => number): string {
  let d = "";
  for (let i = 0; i < vals.length; i++) {
    d += `${i === 0 ? "M" : "L"}${xs[i]!.toFixed(1)} ${yOf(vals[i]!).toFixed(1)} `;
  }
  return d;
}

/** Closed band: forward along `top`, back along `bottom`. */
function bandPath(
  top: number[],
  bottom: number[],
  xs: number[],
  yOf: (v: number) => number,
): string {
  let d = "";
  for (let i = 0; i < top.length; i++) {
    d += `${i === 0 ? "M" : "L"}${xs[i]!.toFixed(1)} ${yOf(top[i]!).toFixed(1)} `;
  }
  for (let i = bottom.length - 1; i >= 0; i--) {
    d += `L${xs[i]!.toFixed(1)} ${yOf(bottom[i]!).toFixed(1)} `;
  }
  return `${d}Z`;
}

function ChartMessage({ children }: { children: React.ReactNode }) {
  return (
    <div
      className="text-mono"
      style={{
        height: PLOT_H + AXIS_H,
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
