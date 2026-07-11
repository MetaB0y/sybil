"use client";

/**
 * PriceChart — hand-rolled SVG, no charting library.
 *
 * Two modes share one crosshair / hover / time-axis shell:
 *  - `area`   binary single market → one YES-probability area, 0–100% axis.
 *  - `lines`  any multi-outcome event → one independent YES line per outcome
 *             on a shared 0–100% axis, no fill. NegRisk events render the same
 *             way as any other group — no stacked-partition special-casing.
 *
 * The x-axis is proportional to wall-clock time: a point's x position is
 * `(t - t0) / span`, so a 4h gap is drawn wide and back-to-back batches
 * narrow. Ticks fall at even time intervals across the window.
 *
 * Only the outcomes passed in `drawn` are plotted (the legend caps this at
 * 8). Live ticks come from the recent-block ring buffer — the line advances
 * each batch on a normal render, no imperative chart lifecycle.
 *
 * On hover the crosshair drops one small pill per line, pinned at that line's
 * value at the cursor time and de-collided — no single bulk readout box.
 */

import { useMemo, useRef, useState } from "react";
import { colorForOutcome } from "@/components/outcome-legend";
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
/** Min vertical spacing (px) between two hover pills, and the top/bottom inset
 *  the de-collision keeps them within. */
const PILL_GAP = 22;
const PILL_PAD = 12;

export type ChartMode = "area" | "lines";

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

export function PriceChart({
  drawn,
  byMarket,
  mode,
  sinceMs,
  nowMs,
}: Props) {
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

  // Axis domain = the selected window; the line itself may start later.
  const t0 = series.domainStart;
  const tEnd = series.domainEnd;
  const span = Math.max(1, tEnd - t0);

  // x is proportional to time, not to point index.
  const xs = series.times.map((t) => ((t - t0) / span) * W);
  const yOf = (v: number) => (1 - v) * PLOT_H;

  // Geometry. `area` (binary) draws one filled probability band; `lines`
  // (every multi-outcome event, NegRisk or not) overlays independent YES lines
  // with no fill — no stacked partition, no per-mode colour treatment.
  const layers = drawn.map((d, k) => {
    const color = colorForOutcome(d.outcome, d.colorIndex);
    const row = series.raw[k]!;
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
    setHover(Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)));
  };

  // The crosshair follows the cursor itself; the readout is the line's value
  // at that exact time (flat-held between clearings), not snapped to a point.
  const hoverT = hover == null ? null : t0 + hover * span;
  const showHover = hoverT != null && hoverT >= series.times[0]!;

  // One pill per line at the cursor time. `dotY` is the true value (where the
  // marker sits on the line); `y` is the same to start, then spreadLabels nudges
  // it so overlapping pills stay readable — the marker stays put, the pill moves.
  const hoverPoints = showHover
    ? drawn.map((d, k) => {
        const v = valueAt(series.times, series.raw[k]!, hoverT!);
        return {
          marketId: d.outcome.marketId,
          color: layers[k]!.color,
          label: d.outcome.shortLabel,
          closed: d.outcome.closed,
          priceText: d.outcome.closed ? "closed" : `${Math.round(v * 100)}¢`,
          dotY: yOf(v),
          y: yOf(v),
        };
      })
    : [];
  spreadLabels(hoverPoints, PILL_GAP, PILL_PAD, PLOT_H - PILL_PAD);
  // Pills sit to the right of the crosshair, flipping left past mid-chart so
  // they never run off the plot.
  const pillsLeft = hover != null && hover > 0.62;

  return (
    <div style={{ width: "100%" }}>
      {/* Hover timestamp — floats in its own strip above the plot and tracks the
          crosshair horizontally, so it reads clear of the lines instead of
          overlapping them. Height is reserved even when idle so the chart never
          jumps as you move on and off it. */}
      <div style={{ position: "relative", height: 20 }}>
        {showHover && (
          <div
            style={{
              position: "absolute",
              left: `${hover! * 100}%`,
              bottom: 1,
              transform: `translateX(${
                hover! < 0.12 ? "0" : hover! > 0.88 ? "-100%" : "-50%"
              })`,
              padding: "2px 7px",
              borderRadius: 4,
              background: "var(--surface-2)",
              border: "1px solid var(--border-2)",
              color: "var(--fg-2)",
              fontFamily: "var(--font-mono)",
              fontSize: 9,
              letterSpacing: "0.04em",
              whiteSpace: "nowrap",
              pointerEvents: "none",
            }}
          >
            {tEnd - hoverT! < 1500 ? "now" : formatHoverTime(hoverT!)}
          </div>
        )}
      </div>
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
              stroke="var(--chart-grid)"
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
              stroke="var(--chart-grid)"
            />
          ))}
          {layers.map((l, k) => {
            const isClosed = drawn[k]!.outcome.closed;
            // Every open line is drawn at one uniform weight — no "chosen
            // outcome" emphasis. Closed/resolved outcomes still read back.
            const baseFill = 0.16;
            return (
              <g key={drawn[k]!.outcome.marketId}>
                {l.filled && (
                  <path
                    d={l.fill}
                    fill={l.color}
                    fillOpacity={isClosed ? baseFill * 0.4 : baseFill}
                  />
                )}
                <path
                  d={l.line}
                  fill="none"
                  stroke={l.color}
                  strokeWidth={isClosed ? 1.25 : 1.75}
                  strokeOpacity={isClosed ? 0.4 : 1}
                  strokeLinejoin="round"
                  vectorEffect="non-scaling-stroke"
                />
              </g>
            );
          })}
          {showHover && (
            <line
              x1={hover! * W}
              x2={hover! * W}
              y1={0}
              y2={PLOT_H}
              stroke="var(--chart-axis)"
              strokeDasharray="2 3"
            />
          )}
        </svg>

        {/* y-axis labels, overlaid top-left of each gridline — the right edge
            is reserved for the per-line price tags. */}
        <div
          style={{
            position: "absolute",
            top: 0,
            left: 0,
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

        {showHover && (
          <>
            {/* dot marker where each line meets the crosshair */}
            {hoverPoints.map((p) => (
              <div
                key={`dot-${p.marketId}`}
                style={{
                  position: "absolute",
                  left: `${hover! * 100}%`,
                  top: p.dotY,
                  transform: "translate(-50%, -50%)",
                  width: 9,
                  height: 9,
                  borderRadius: "50%",
                  background: p.closed ? "var(--fg-4)" : p.color,
                  border: "1.5px solid var(--surface-1)",
                  opacity: p.closed ? 0.6 : 1,
                  pointerEvents: "none",
                }}
              />
            ))}
            {/* one pill per line, stacked to one side of the crosshair */}
            {hoverPoints.map((p) => (
              <div
                key={`pill-${p.marketId}`}
                style={{
                  position: "absolute",
                  left: `${hover! * 100}%`,
                  top: p.y,
                  transform: pillsLeft
                    ? "translate(calc(-100% - 12px), -50%)"
                    : "translate(12px, -50%)",
                  display: "flex",
                  alignItems: "center",
                  gap: 6,
                  maxWidth: 190,
                  padding: "2px 8px",
                  borderRadius: 5,
                  background: p.closed
                    ? "var(--surface-2)"
                    : `color-mix(in srgb, ${p.color} 18%, var(--surface-2))`,
                  border: `1px solid ${
                    p.closed
                      ? "var(--border-2)"
                      : `color-mix(in srgb, ${p.color} 42%, transparent)`
                  }`,
                  boxShadow: "var(--shadow-popover, 0 4px 14px rgba(0,0,0,0.35))",
                  fontFamily: "var(--font-mono)",
                  fontSize: 11,
                  lineHeight: "16px",
                  whiteSpace: "nowrap",
                  opacity: p.closed ? 0.7 : 1,
                  pointerEvents: "none",
                }}
              >
                <span
                  style={{
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    color: "var(--fg-1)",
                  }}
                >
                  {p.label}
                </span>
                <span
                  style={{
                    flexShrink: 0,
                    fontWeight: 600,
                    color: p.closed ? "var(--fg-4)" : p.color,
                  }}
                >
                  {p.priceText}
                </span>
              </div>
            ))}
          </>
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

/** Crosshair header timestamp — short date + clock time (e.g. "Jul 6, 4:21 AM"). */
function formatHoverTime(ms: number): string {
  const d = new Date(ms);
  const date = d.toLocaleDateString("en-US", { month: "short", day: "numeric" });
  const time = d.toLocaleTimeString("en-US", {
    hour: "numeric",
    minute: "2-digit",
  });
  return `${date}, ${time}`;
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

/**
 * Value of a series at an arbitrary time — linear interpolation between the
 * two surrounding grid points, matching what the SVG line draws. Over a gap
 * (no clearings) the two endpoints are equal, so this reads the held price.
 */
function valueAt(times: number[], row: number[], t: number): number {
  const last = times.length - 1;
  if (t <= times[0]!) return row[0]!;
  if (t >= times[last]!) return row[last]!;
  let i = 0;
  while (i < last && times[i + 1]! <= t) i++;
  const ta = times[i]!;
  const tb = times[i + 1]!;
  const f = tb > ta ? (t - ta) / (tb - ta) : 0;
  return row[i]! + f * (row[i + 1]! - row[i]!);
}

/**
 * Nudge the right-edge price tags apart so close prices stay readable. Sorts by
 * target y, pushes each down to clear the one above, then clamps the bottom and
 * pushes back up — settling the stack within `[min, max]`. Mutates each label's
 * `y` in place (they're shared with the render list, kept in original order).
 */
function spreadLabels(
  labels: { y: number }[],
  gap: number,
  min: number,
  max: number,
): void {
  if (labels.length === 0) return;
  const order = [...labels].sort((a, b) => a.y - b.y);
  // Forward: hold each at/below the top inset and at least `gap` under the one
  // above it.
  for (let i = 0; i < order.length; i++) {
    const floor = i === 0 ? min : order[i - 1]!.y + gap;
    if (order[i]!.y < floor) order[i]!.y = floor;
  }
  // Backward: pull any that overran the bottom inset back up, keeping the gap —
  // so a top-heavy cluster settles evenly instead of piling on the first tag.
  for (let i = order.length - 1; i >= 0; i--) {
    const ceil = i === order.length - 1 ? max : order[i + 1]!.y - gap;
    if (order[i]!.y > ceil) order[i]!.y = ceil;
  }
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
