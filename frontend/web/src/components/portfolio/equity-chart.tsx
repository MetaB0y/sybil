"use client";

/**
 * Interactive SVG equity-curve chart. Data is real, from `useEquityCurve`
 * (`GET /v1/accounts/{id}/equity`): X = sample time, Y = portfolio value.
 *
 * REDESIGN (drop-in replacement — same props, same tokens, same data hook):
 *   - A *fitted* Y-axis — scaled to the data's own range with ~8% headroom and
 *     rounded "nice" gridlines/labels — not anchored to $0. (Unchanged.)
 *   - A monotone-cubic stroke rather than a raw polyline, so the equity series
 *     reads as a curve instead of a run of sharp corners. Monotone (Fritsch–
 *     Carlson) specifically: the spline passes through every sample and cannot
 *     overshoot between them, so it never invents a peak the data doesn't have.
 *   - Range swaps crossfade. `useEquityCurve` holds the previous range's series
 *     while the new one loads (`placeholderData`); we blur it, then re-key on
 *     the range that actually landed so the incoming curve focuses in — the
 *     same `sybil-fade-swap` motion the market-page activity chart uses.
 *   - A gradient AREA FILL under the line, tinted by the range's net P&L:
 *     mint (`--yes`) when up, coral (`--no`) when down, neutral cyan when flat.
 *     The fill gives the volatile "canyon" marks visible mass so they read as
 *     real movement rather than rendering glitches. The stroke itself is
 *     ALWAYS brand cyan (`--accent`) — direction is carried by the fill.
 *   - An end-value tag pinned to the right axis at the last point.
 *   - Hover: a single crosshair + dot snapped to the nearest sample, with a
 *     readout pill that TRACKS the crosshair (clamped to the box) instead of
 *     sitting in a fixed corner — so value never floats away from the cursor.
 *   - Smart X labels: intraday ranges show times; multi-day show dates; never
 *     three identical labels.
 *
 * Fully responsive: a ResizeObserver feeds the plot box's px size into the
 * viewBox (1 unit = 1px), so it fills whatever space the layout gives it.
 */

import { useEffect, useId, useRef, useState } from "react";
import type { EquityCurve, EquityRange } from "@/lib/account/use-equity-curve";

interface Props {
  curve: EquityCurve;
  /** Optional controls rendered at the right of the title row (e.g. RangeTabs). */
  headerRight?: React.ReactNode;
}

const PAD_L = 12;
const PAD_R = 56;
const PAD_T = 16;
const PAD_B = 28;
const MIN_W = 320;
const MIN_H = 200;

// Below this fractional move over the visible range, treat as "flat" and use a
// neutral cyan fill instead of mint/coral.
const FLAT_EPS = 0.001;

export function EquityChart({ curve, headerRight }: Props) {
  const { points, drawnRange: range, isLoading, isEmpty, isSwapping } = curve;
  const boxRef = useRef<HTMLDivElement>(null);
  const [box, setBox] = useState({ w: 560, h: 320 });
  const [hoverIdx, setHoverIdx] = useState<number | null>(null);
  const gradId = useId();

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
  // round gridlines. Never anchored at $0.
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

  const lineD = monotoneCubicPath(
    points.map((p) => ({ x: xFor(p.t), y: yFor(p.value) })),
  );
  // Close the area down to the plot floor for the gradient fill.
  const areaD =
    points.length >= 2
      ? `${lineD} L ${xFor(tMax).toFixed(2)} ${bottom.toFixed(2)} L ${xFor(tMin).toFixed(2)} ${bottom.toFixed(2)} Z`
      : "";

  // Sign of the move over the visible range → fill tone. Line stays cyan.
  const startV = points.length ? points[0]!.value : 0;
  const endV = points.length ? points[points.length - 1]!.value : 0;
  const pctMove = startV !== 0 ? Math.abs((endV - startV) / startV) : 0;
  const fillTone =
    pctMove < FLAT_EPS ? "var(--accent)" : endV >= startV ? "var(--yes)" : "var(--no)";

  const last = points[points.length - 1];
  const hovered = hoverIdx != null ? points[hoverIdx] : null;
  const gridTicks = scale.ticks.filter((t) => t >= yMin - 1e-6 && t <= yMax + 1e-6);

  const intraday = points.length >= 2 && sameDay(tMin, tMax);

  // End-value tag geometry.
  const tagW = 54;
  const tagH = 17;
  const tagX = Math.min(W - PAD_R + 4, W - tagW - 1);
  const tagY = last ? Math.max(1, Math.min(H - tagH - 1, yFor(last.value) - tagH / 2)) : 0;

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
          boxShadow: "var(--shadow-inset-top)",
        }}
      >
        {isEmpty ? (
          <Centered>{isLoading ? "loading…" : "no equity history yet"}</Centered>
        ) : (
          <div
            // Outer layer carries the blur/dim while the newly-picked range is
            // in flight; the inner layer re-keys on the range that landed, so
            // the incoming curve focuses in rather than hard-cutting.
            style={{
              position: "absolute",
              inset: 0,
              filter: isSwapping ? "blur(5px)" : undefined,
              opacity: isSwapping ? 0.5 : 1,
              transition:
                "filter var(--dur-swap) var(--ease-standard), opacity var(--dur-swap) var(--ease-standard)",
            }}
          >
            <div
              key={range}
              style={{
                position: "absolute",
                inset: 0,
                animation: "sybil-fade-swap var(--dur-swap) var(--ease-standard)",
              }}
            >
            <svg
              viewBox={`0 0 ${W} ${H}`}
              width="100%"
              height="100%"
              preserveAspectRatio="none"
              // Absolutely positioned so the SVG contributes ZERO intrinsic
              // height: an in-flow SVG with a viewBox collapses to its aspect
              // ratio during the grid's intrinsic-sizing pass, which—fed by the
              // ResizeObserver below—creates a feedback loop that ratchets the
              // box taller on every window resize. Out of flow, the box height
              // is driven solely by the grid row (matching the hero).
              style={{ position: "absolute", inset: 0, display: "block", touchAction: "none" }}
              onPointerMove={onMove}
              onPointerLeave={() => setHoverIdx(null)}
            >
              <defs>
                <linearGradient id={gradId} x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0" stopColor={fillTone} stopOpacity={0.24} />
                  <stop offset="0.55" stopColor={fillTone} stopOpacity={0.08} />
                  <stop offset="1" stopColor={fillTone} stopOpacity={0} />
                </linearGradient>
              </defs>

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
                      stroke="var(--chart-grid)"
                      strokeWidth={1}
                      vectorEffect="non-scaling-stroke"
                    />
                    <text
                      x={W - PAD_R + 8}
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

              {/* Gradient area fill — sign-tinted */}
              {areaD && <path d={areaD} fill={`url(#${gradId})`} stroke="none" />}

              {/* Equity line — ALWAYS cyan */}
              <path
                d={lineD}
                fill="none"
                stroke="var(--accent)"
                strokeWidth={1.75}
                strokeLinecap="round"
                strokeLinejoin="round"
                vectorEffect="non-scaling-stroke"
              />

              {/* End dot + value tag (hidden while scrubbing) */}
              {!hovered && last && (
                <>
                  <circle cx={xFor(last.t)} cy={yFor(last.value)} r={3} fill="var(--accent)" vectorEffect="non-scaling-stroke" />
                  <rect x={tagX} y={tagY} width={tagW} height={tagH} rx={3} fill="var(--accent)" />
                  <text
                    x={tagX + tagW / 2}
                    y={tagY + tagH / 2 + 3}
                    fill="var(--fg-on-accent)"
                    fontFamily="var(--font-mono)"
                    fontSize={9.5}
                    fontWeight={600}
                    textAnchor="middle"
                  >
                    {fmtAxisY(last.value)}
                  </text>
                </>
              )}

              {/* Crosshair + hover dot */}
              {hovered && (
                <>
                  <line
                    x1={xFor(hovered.t)}
                    x2={xFor(hovered.t)}
                    y1={PAD_T}
                    y2={bottom}
                    stroke="var(--chart-axis)"
                    strokeWidth={1}
                    strokeDasharray="3 3"
                    vectorEffect="non-scaling-stroke"
                  />
                  <circle
                    cx={xFor(hovered.t)}
                    cy={yFor(hovered.value)}
                    r={4.5}
                    fill="var(--accent)"
                    stroke="var(--surface-1)"
                    strokeWidth={2}
                    vectorEffect="non-scaling-stroke"
                  />
                </>
              )}

              {/* Bottom-axis labels — intraday → times, else dates */}
              <AxisLabel x={PAD_L} y={H - 8} text={fmtAxisX(tMin, range, intraday)} anchor="start" />
              <AxisLabel x={PAD_L + innerW / 2} y={H - 8} text={fmtAxisX((tMin + tMax) / 2, range, intraday)} anchor="middle" />
              <AxisLabel x={W - PAD_R} y={H - 8} text={fmtAxisX(tMax, range, intraday)} anchor="end" />
            </svg>

            {hovered && <HoverReadout point={hovered} x={xFor(hovered.t)} W={W} boxW={box.w} intraday={intraday} />}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

/** Readout pill that tracks the crosshair, clamped within the box so it never
 *  overflows the edges. Pointer-transparent. */
function HoverReadout({
  point,
  x,
  W,
  boxW,
  intraday,
}: {
  point: { t: number; value: number };
  x: number; // crosshair X in viewBox units
  W: number; // viewBox width
  boxW: number; // box width in px
  intraday: boolean;
}) {
  const HALF = 64; // approx half the pill width, for edge clamping
  const px = (x / W) * boxW;
  const left = Math.max(HALF + 6, Math.min(boxW - HALF - 6, px));
  return (
    <div
      style={{
        position: "absolute",
        top: 12,
        left,
        transform: "translateX(-50%)",
        pointerEvents: "none",
        background: "color-mix(in srgb, var(--surface-3) 92%, transparent)",
        border: "1px solid var(--border-2)",
        borderRadius: 4,
        padding: "7px 11px",
        boxShadow: "var(--shadow-popover)",
        whiteSpace: "nowrap",
      }}
    >
      <div style={{ fontSize: 9.5, color: "var(--fg-4)", letterSpacing: "0.04em", textTransform: "uppercase", fontFamily: "var(--font-mono)" }}>
        {fmtReadoutDate(point.t, intraday)}
      </div>
      <div style={{ fontSize: 16, color: "var(--fg-1)", marginTop: 3, fontFamily: "var(--font-mono)", fontVariantNumeric: "tabular-nums" }}>
        {usd(point.value)}
      </div>
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
    <text x={x} y={y} fill="var(--fg-4)" fontFamily="var(--font-mono)" fontSize={9} letterSpacing="0.04em" textAnchor={anchor}>
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

/**
 * Monotone cubic (Fritsch–Carlson) interpolation through `pts`, emitted as an
 * SVG path of cubic Béziers. Softens the polyline's corners while guaranteeing
 * the curve stays inside each segment's own value range — a plain Catmull-Rom
 * would bulge past a local max and show equity the account never had.
 *
 * Operates in screen space; `xFor`/`yFor` are affine, so monotonicity there is
 * monotonicity in the data.
 */
function monotoneCubicPath(pts: { x: number; y: number }[]): string {
  const n = pts.length;
  if (n === 0) return "";
  const head = `M ${pts[0]!.x.toFixed(2)} ${pts[0]!.y.toFixed(2)}`;
  if (n === 1) return head;
  if (n === 2) return `${head} L ${pts[1]!.x.toFixed(2)} ${pts[1]!.y.toFixed(2)}`;

  // Secant slopes between consecutive samples.
  const dx: number[] = [];
  const slope: number[] = [];
  for (let i = 0; i < n - 1; i++) {
    const h = pts[i + 1]!.x - pts[i]!.x;
    dx.push(h);
    slope.push(h === 0 ? 0 : (pts[i + 1]!.y - pts[i]!.y) / h);
  }

  // Tangents: zero at every local extremum (that's what kills overshoot),
  // weighted harmonic mean of the neighbouring secants elsewhere.
  const m: number[] = new Array(n);
  m[0] = slope[0]!;
  m[n - 1] = slope[n - 2]!;
  for (let i = 1; i < n - 1; i++) {
    const s0 = slope[i - 1]!;
    const s1 = slope[i]!;
    if (s0 * s1 <= 0) {
      m[i] = 0;
    } else {
      const w1 = 2 * dx[i]! + dx[i - 1]!;
      const w2 = dx[i]! + 2 * dx[i - 1]!;
      m[i] = (w1 + w2) / (w1 / s0 + w2 / s1);
    }
  }

  let d = head;
  for (let i = 0; i < n - 1; i++) {
    const t = dx[i]! / 3;
    const c1x = pts[i]!.x + t;
    const c1y = pts[i]!.y + m[i]! * t;
    const c2x = pts[i + 1]!.x - t;
    const c2y = pts[i + 1]!.y - m[i + 1]! * t;
    d += ` C ${c1x.toFixed(2)} ${c1y.toFixed(2)} ${c2x.toFixed(2)} ${c2y.toFixed(2)} ${pts[i + 1]!.x.toFixed(2)} ${pts[i + 1]!.y.toFixed(2)}`;
  }
  return d;
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

function sameDay(a: number, b: number): boolean {
  const x = new Date(a);
  const y = new Date(b);
  return (
    x.getFullYear() === y.getFullYear() && x.getMonth() === y.getMonth() && x.getDate() === y.getDate()
  );
}

function fmtAxisX(t: number, range: EquityRange, intraday: boolean): string {
  const d = new Date(t);
  if (range === "24H" || intraday) {
    return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  }
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

function fmtReadoutDate(t: number, intraday: boolean): string {
  const d = new Date(t);
  if (intraday) return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  return d.toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
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
