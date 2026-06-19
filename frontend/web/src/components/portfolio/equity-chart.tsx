"use client";

/**
 * Interactive SVG equity-curve chart. Data is real, from `useEquityCurve`
 * (`GET /v1/accounts/{id}/equity`): X = sample time, Y = portfolio value.
 *
 * Design follows the conventions of exchange value charts (Polymarket / Kalshi /
 * Robinhood):
 *   - A *fitted* Y-axis — scaled to the data's own range with ~8% headroom and
 *     rounded "nice" gridlines/labels — not anchored to $0, so real movement is
 *     visible instead of a flat band. The equity series here can be genuinely
 *     volatile (positions mark up/down between batches); a fitted axis keeps
 *     that legible rather than gluing the line to an edge.
 *   - A clean single line (no heavy area fill, which turns a volatile series
 *     into solid columns).
 *   - Sparse, rounded axis labels + a few horizontal gridlines for reference.
 *   - Hover: crosshair + dot + a compact readout (time + value) pinned to the
 *     corner opposite the cursor, so the text never covers the curve.
 *
 * Fully responsive: a ResizeObserver feeds the plot box's px size into the
 * viewBox (1 unit = 1px), so it fills whatever space the layout gives it.
 */

import { useEffect, useRef, useState } from "react";
import type { EquityCurve, EquityRange } from "@/lib/account/use-equity-curve";

interface Props {
  curve: EquityCurve;
  /** Optional controls rendered at the right of the title row (e.g. RangeTabs). */
  headerRight?: React.ReactNode;
}

const PAD_L = 12;
const PAD_R = 52;
const PAD_T = 14;
const PAD_B = 26;
const MIN_W = 320;
const MIN_H = 200;

export function EquityChart({ curve, headerRight }: Props) {
  const { points, range, isLoading, isEmpty } = curve;
  const boxRef = useRef<HTMLDivElement>(null);
  const [box, setBox] = useState({ w: 560, h: 320 });
  const [hoverIdx, setHoverIdx] = useState<number | null>(null);

  // Track the plot box size so the SVG renders at 1 viewBox-unit-per-pixel.
  useEffect(() => {
    const el = boxRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      const r = entries[0]?.contentRect;
      if (r && r.width > 0 && r.height > 0) setBox({ w: r.width, h: r.height });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const W = Math.max(MIN_W, box.w);
  const H = Math.max(MIN_H, box.h);
  const innerW = W - PAD_L - PAD_R;
  const innerH = H - PAD_T - PAD_B;
  const bottom = H - PAD_B;

  // Fitted Y-axis: scale to the data range + 8% headroom, snapped to "nice"
  // round gridlines. Never anchored at $0 — that would flatten a $1k±$200
  // series into a band hugging the top edge.
  const values = points.map((p) => p.value);
  let dMin = values.length ? Math.min(...values) : 0;
  let dMax = values.length ? Math.max(...values) : 1;
  if (!(dMax > dMin)) {
    const c = dMax || 1;
    const pad = Math.max(1, Math.abs(c) * 0.02);
    dMin = c - pad;
    dMax = c + pad;
  }
  const headroom = (dMax - dMin) * 0.08;
  const scale = niceScale(dMin - headroom, dMax + headroom, 5);
  const yMin = scale.lo;
  const yMax = scale.hi;
  const ySpan = Math.max(1e-9, yMax - yMin);

  const tMin = points.length ? points[0]!.t : 0;
  const tMax = points.length ? points[points.length - 1]!.t : 1;
  const tSpan = Math.max(1, tMax - tMin);

  const xFor = (t: number) => PAD_L + ((t - tMin) / tSpan) * innerW;
  const yFor = (v: number) => PAD_T + (1 - (v - yMin) / ySpan) * innerH;

  const lineD = points
    .map((p, i) => `${i === 0 ? "M" : "L"} ${xFor(p.t).toFixed(2)} ${yFor(p.value).toFixed(2)}`)
    .join(" ");

  const last = points[points.length - 1];
  const hovered = hoverIdx != null ? points[hoverIdx] : null;
  const gridTicks = scale.ticks.filter((t) => t >= yMin - 1e-6 && t <= yMax + 1e-6);

  function onMove(e: React.PointerEvent<SVGSVGElement>) {
    if (points.length < 2) return;
    const rect = e.currentTarget.getBoundingClientRect();
    if (rect.width === 0) return;
    const vbX = ((e.clientX - rect.left) / rect.width) * W;
    const clampedX = Math.max(PAD_L, Math.min(W - PAD_R, vbX));
    const targetT = tMin + ((clampedX - PAD_L) / Math.max(1, innerW)) * tSpan;
    let best = 0;
    let bestD = Infinity;
    for (let i = 0; i < points.length; i++) {
      const d = Math.abs(points[i]!.t - targetT);
      if (d < bestD) {
        bestD = d;
        best = i;
      }
    }
    setHoverIdx(best);
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10, height: "100%", minHeight: 0 }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12, flexWrap: "wrap" }}>
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 10,
            color: "var(--fg-3)",
            letterSpacing: "var(--track-wide)",
            textTransform: "uppercase",
          }}
        >
          Equity curve
        </span>
        {headerRight}
      </div>
      <div
        ref={boxRef}
        style={{
          position: "relative",
          flex: 1,
          minHeight: 280,
          background: "var(--surface-1)",
          border: "1px solid var(--border-1)",
          borderRadius: 6,
          overflow: "hidden",
        }}
      >
        {isEmpty ? (
          <Centered>{isLoading ? "loading…" : "no equity history yet"}</Centered>
        ) : (
          <>
            <svg
              viewBox={`0 0 ${W} ${H}`}
              width="100%"
              height="100%"
              preserveAspectRatio="none"
              style={{ display: "block", touchAction: "none" }}
              onPointerMove={onMove}
              onPointerLeave={() => setHoverIdx(null)}
            >
              {/* Horizontal gridlines at nice round values + right-axis labels */}
              {gridTicks.map((t) => {
                const gy = yFor(t);
                return (
                  <g key={t}>
                    <line
                      x1={PAD_L}
                      x2={W - PAD_R}
                      y1={gy}
                      y2={gy}
                      stroke="var(--border-1)"
                      strokeWidth={1}
                      strokeOpacity={0.55}
                      vectorEffect="non-scaling-stroke"
                    />
                    <text
                      x={W - PAD_R + 7}
                      y={gy + 3}
                      fill="var(--fg-4)"
                      fontFamily="var(--font-mono)"
                      fontSize={9}
                      letterSpacing="0.04em"
                      textAnchor="start"
                    >
                      {fmtAxisY(t)}
                    </text>
                  </g>
                );
              })}

              {/* Equity line */}
              <path
                d={lineD}
                fill="none"
                stroke="var(--accent)"
                strokeWidth={1.6}
                strokeLinecap="round"
                strokeLinejoin="round"
                vectorEffect="non-scaling-stroke"
              />

              {/* Crosshair + hover dot */}
              {hovered && (
                <>
                  <line
                    x1={xFor(hovered.t)}
                    x2={xFor(hovered.t)}
                    y1={PAD_T}
                    y2={bottom}
                    stroke="var(--fg-3)"
                    strokeWidth={1}
                    strokeDasharray="3 3"
                    vectorEffect="non-scaling-stroke"
                  />
                  <circle cx={xFor(hovered.t)} cy={yFor(hovered.value)} r={4.5} fill="var(--accent)" stroke="var(--surface-1)" strokeWidth={2} vectorEffect="non-scaling-stroke" />
                </>
              )}

              {/* End dot when not hovering */}
              {!hovered && last && (
                <circle cx={xFor(last.t)} cy={yFor(last.value)} r={3} fill="var(--accent)" vectorEffect="non-scaling-stroke" />
              )}

              {/* Bottom-axis date labels */}
              <AxisLabel x={PAD_L} y={H - 6} text={fmtAxisDate(tMin, range)} anchor="start" />
              <AxisLabel x={PAD_L + innerW / 2} y={H - 6} text={fmtAxisDate((tMin + tMax) / 2, range)} anchor="middle" />
              <AxisLabel x={W - PAD_R} y={H - 6} text={fmtAxisDate(tMax, range)} anchor="end" />
            </svg>

            {hovered && <HoverReadout point={hovered} />}
          </>
        )}
      </div>
    </div>
  );
}

/** Compact readout pinned to the top-left, so it stays put while you scrub the
 *  curve (no jumping). Pointer-transparent. */
function HoverReadout({ point }: { point: { t: number; value: number } }) {
  return (
    <div
      style={{
        position: "absolute",
        top: 8,
        left: 8,
        pointerEvents: "none",
        background: "color-mix(in srgb, var(--surface-3) 90%, transparent)",
        border: "1px solid var(--border-2)",
        borderRadius: 6,
        padding: "6px 9px",
        fontFamily: "var(--font-mono)",
      }}
    >
      <div style={{ fontSize: 9.5, color: "var(--fg-4)", letterSpacing: "0.04em", textTransform: "uppercase" }}>
        {fmtReadoutDate(point.t)}
      </div>
      <div style={{ fontSize: 16, color: "var(--fg-1)", marginTop: 2 }}>{usd(point.value)}</div>
    </div>
  );
}

function AxisLabel({
  x,
  y,
  text,
  anchor = "start",
}: {
  x: number;
  y: number;
  text: string;
  anchor?: "start" | "middle" | "end";
}) {
  return (
    <text
      x={x}
      y={y}
      fill="var(--fg-4)"
      fontFamily="var(--font-mono)"
      fontSize={9}
      letterSpacing="0.04em"
      textAnchor={anchor}
    >
      {text}
    </text>
  );
}

function Centered({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        position: "absolute",
        inset: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        color: "var(--fg-4)",
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
      }}
    >
      {children}
    </div>
  );
}

/** Round a [lo, hi] range out to "nice" bounds and produce evenly spaced ticks
 *  at human-friendly values (1/2/5 × 10ⁿ). Standard axis-scaling algorithm. */
function niceScale(lo: number, hi: number, maxTicks: number): { lo: number; hi: number; ticks: number[] } {
  const range = niceNum(Math.max(1e-9, hi - lo), false);
  const step = niceNum(range / Math.max(1, maxTicks - 1), true);
  const niceLo = Math.floor(lo / step) * step;
  const niceHi = Math.ceil(hi / step) * step;
  const ticks: number[] = [];
  for (let v = niceLo; v <= niceHi + step * 0.5; v += step) {
    ticks.push(Number(v.toFixed(6)));
  }
  return { lo: niceLo, hi: niceHi, ticks };
}

function niceNum(range: number, round: boolean): number {
  const exp = Math.floor(Math.log10(range));
  const frac = range / Math.pow(10, exp);
  let nf: number;
  if (round) {
    nf = frac < 1.5 ? 1 : frac < 3 ? 2 : frac < 7 ? 5 : 10;
  } else {
    nf = frac <= 1 ? 1 : frac <= 2 ? 2 : frac <= 5 ? 5 : 10;
  }
  return nf * Math.pow(10, exp);
}

function fmtAxisDate(t: number, range: EquityRange): string {
  const d = new Date(t);
  if (range === "24H") {
    return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  }
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

function fmtReadoutDate(t: number): string {
  return new Date(t).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function usd(v: number): string {
  return v.toLocaleString(undefined, {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  });
}

/** Y-axis tick label — comma dollars under $100k, compact K above. */
function fmtAxisY(v: number): string {
  const a = Math.abs(v);
  if (a >= 100000) return `$${Math.round(v / 1000)}K`;
  if (a >= 10000) return `$${(v / 1000).toFixed(1)}K`;
  return `$${Math.round(v).toLocaleString()}`;
}
