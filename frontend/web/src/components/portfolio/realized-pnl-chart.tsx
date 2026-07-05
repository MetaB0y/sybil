"use client";

/**
 * Cumulative realized-PnL chart. X = fill/settlement time, Y = running realized
 * PnL in dollars. Data comes from `cumulativeRealizedPnl` (a chronological sum
 * of the backend's per-fill `realizedPnlNanos`), so the curve matches sybil-api
 * exactly — see `lib/account/realized-pnl.ts`.
 *
 * Visual idiom mirrors `EquityChart` (fitted axis, gradient area fill, end-value
 * tag, crosshair hover), with two realized-PnL-specific twists: a dashed zero
 * reference line, and a fill/line tone driven by the SIGN of total realized PnL
 * (mint in profit, coral in loss) rather than by direction of travel.
 *
 * Responsive: a ResizeObserver feeds the box's px size into the viewBox
 * (1 unit = 1px). Pure presentational — hand it a points array.
 */

import { useEffect, useId, useRef, useState } from "react";
import type { RealizedPnlPoint } from "@/lib/account/realized-pnl";
import { NANOS_PER_UNIT } from "@/lib/format/nanos";

interface Props {
  points: RealizedPnlPoint[];
  isLoading?: boolean;
}

const PAD_L = 12;
const PAD_R = 56;
const PAD_T = 16;
const PAD_B = 28;
const MIN_W = 320;
const MIN_H = 200;

interface Pt {
  t: number;
  value: number; // dollars
}

export function RealizedPnlChart({ points, isLoading = false }: Props) {
  const boxRef = useRef<HTMLDivElement>(null);
  const [box, setBox] = useState({ w: 560, h: 320 });
  const [hoverIdx, setHoverIdx] = useState<number | null>(null);
  const gradId = useId();

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

  const series: Pt[] = points.map((p) => ({
    t: p.t,
    value: Number(p.cumNanos) / Number(NANOS_PER_UNIT),
  }));
  const isEmpty = series.length < 2;

  const W = Math.max(MIN_W, box.w);
  const H = Math.max(MIN_H, box.h);
  const innerW = W - PAD_L - PAD_R;
  const innerH = H - PAD_T - PAD_B;
  const bottom = H - PAD_B;

  // Fitted Y-axis that ALWAYS includes 0 (the break-even line), with ~10%
  // headroom, snapped to nice round gridlines.
  const values = series.map((p) => p.value);
  let dMin = Math.min(0, ...(values.length ? values : [0]));
  let dMax = Math.max(0, ...(values.length ? values : [0]));
  if (!(dMax > dMin)) {
    dMin -= 1;
    dMax += 1;
  }
  const headroom = (dMax - dMin) * 0.1;
  const scale = niceScale(dMin - headroom, dMax + headroom, 5);
  const yMin = scale.lo;
  const yMax = scale.hi;
  const ySpan = Math.max(1e-9, yMax - yMin);

  const tMin = series.length ? series[0]!.t : 0;
  const tMax = series.length ? series[series.length - 1]!.t : 1;
  const tSpan = Math.max(1, tMax - tMin);

  const xFor = (t: number) => PAD_L + ((t - tMin) / tSpan) * innerW;
  const yFor = (v: number) => PAD_T + (1 - (v - yMin) / ySpan) * innerH;

  const lineD = series
    .map((p, i) => `${i === 0 ? "M" : "L"} ${xFor(p.t).toFixed(2)} ${yFor(p.value).toFixed(2)}`)
    .join(" ");
  const areaD =
    series.length >= 2
      ? `${lineD} L ${xFor(tMax).toFixed(2)} ${bottom.toFixed(2)} L ${xFor(tMin).toFixed(2)} ${bottom.toFixed(2)} Z`
      : "";

  // Tone by SIGN of the ending (total) realized PnL.
  const endV = series.length ? series[series.length - 1]!.value : 0;
  const tone =
    Math.abs(endV) < 1e-9 ? "var(--accent)" : endV >= 0 ? "var(--yes)" : "var(--no)";

  const last = series[series.length - 1];
  const hovered = hoverIdx != null ? series[hoverIdx] : null;
  const gridTicks = scale.ticks.filter((t) => t >= yMin - 1e-6 && t <= yMax + 1e-6);
  const intraday = series.length >= 2 && sameDay(tMin, tMax);
  const zeroY = yFor(0);

  const tagW = 58;
  const tagH = 17;
  const tagX = Math.min(W - PAD_R + 4, W - tagW - 1);
  const tagY = last ? Math.max(1, Math.min(H - tagH - 1, yFor(last.value) - tagH / 2)) : 0;

  function onMove(e: React.PointerEvent<SVGSVGElement>) {
    if (series.length < 2) return;
    const rect = e.currentTarget.getBoundingClientRect();
    if (rect.width === 0) return;
    const vbX = ((e.clientX - rect.left) / rect.width) * W;
    const clampedX = Math.max(PAD_L, Math.min(W - PAD_R, vbX));
    const targetT = tMin + ((clampedX - PAD_L) / Math.max(1, innerW)) * tSpan;
    let best = 0;
    let bestD = Infinity;
    for (let i = 0; i < series.length; i++) {
      const d = Math.abs(series[i]!.t - targetT);
      if (d < bestD) {
        bestD = d;
        best = i;
      }
    }
    setHoverIdx(best);
  }

  return (
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
        <Centered>{isLoading ? "loading…" : "no realized P&L yet"}</Centered>
      ) : (
        <>
          <svg
            viewBox={`0 0 ${W} ${H}`}
            width="100%"
            height="100%"
            preserveAspectRatio="none"
            style={{ position: "absolute", inset: 0, display: "block", touchAction: "none" }}
            onPointerMove={onMove}
            onPointerLeave={() => setHoverIdx(null)}
          >
            <defs>
              <linearGradient id={gradId} x1="0" y1="0" x2="0" y2="1">
                <stop offset="0" stopColor={tone} stopOpacity={0.24} />
                <stop offset="0.55" stopColor={tone} stopOpacity={0.08} />
                <stop offset="1" stopColor={tone} stopOpacity={0} />
              </linearGradient>
            </defs>

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

            {/* Break-even (zero) reference line. */}
            <line
              x1={PAD_L}
              x2={W - PAD_R}
              y1={zeroY}
              y2={zeroY}
              stroke="var(--chart-axis)"
              strokeWidth={1}
              strokeDasharray="3 3"
              vectorEffect="non-scaling-stroke"
            />

            {areaD && <path d={areaD} fill={`url(#${gradId})`} stroke="none" />}

            <path
              d={lineD}
              fill="none"
              stroke={tone}
              strokeWidth={1.75}
              strokeLinecap="round"
              strokeLinejoin="round"
              vectorEffect="non-scaling-stroke"
            />

            {!hovered && last && (
              <>
                <circle cx={xFor(last.t)} cy={yFor(last.value)} r={3} fill={tone} vectorEffect="non-scaling-stroke" />
                <rect x={tagX} y={tagY} width={tagW} height={tagH} rx={3} fill={tone} />
                <text
                  x={tagX + tagW / 2}
                  y={tagY + tagH / 2 + 3}
                  fill="var(--fg-on-accent)"
                  fontFamily="var(--font-mono)"
                  fontSize={9.5}
                  fontWeight={600}
                  textAnchor="middle"
                >
                  {fmtSignedAxisY(last.value)}
                </text>
              </>
            )}

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
                  fill={tone}
                  stroke="var(--surface-1)"
                  strokeWidth={2}
                  vectorEffect="non-scaling-stroke"
                />
              </>
            )}

            <AxisLabel x={PAD_L} y={H - 8} text={fmtAxisX(tMin, intraday)} anchor="start" />
            <AxisLabel x={PAD_L + innerW / 2} y={H - 8} text={fmtAxisX((tMin + tMax) / 2, intraday)} anchor="middle" />
            <AxisLabel x={W - PAD_R} y={H - 8} text={fmtAxisX(tMax, intraday)} anchor="end" />
          </svg>

          {hovered && <HoverReadout point={hovered} x={xFor(hovered.t)} W={W} boxW={box.w} intraday={intraday} />}
        </>
      )}
    </div>
  );
}

function HoverReadout({
  point,
  x,
  W,
  boxW,
  intraday,
}: {
  point: Pt;
  x: number;
  W: number;
  boxW: number;
  intraday: boolean;
}) {
  const HALF = 64;
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
        {usdSigned(point.value)}
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
  return x.getFullYear() === y.getFullYear() && x.getMonth() === y.getMonth() && x.getDate() === y.getDate();
}

function fmtAxisX(t: number, intraday: boolean): string {
  const d = new Date(t);
  if (intraday) return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

function fmtReadoutDate(t: number, intraday: boolean): string {
  const d = new Date(t);
  if (intraday) return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  return d.toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

function usdSigned(v: number): string {
  const sign = v > 0 ? "+" : "";
  return (
    sign +
    v.toLocaleString(undefined, {
      style: "currency",
      currency: "USD",
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    })
  );
}

/** Y-axis tick label — signed comma dollars under $100k, compact K above. */
function fmtAxisY(v: number): string {
  const a = Math.abs(v);
  const sign = v < 0 ? "-" : "";
  if (a >= 100000) return `${sign}$${Math.round(a / 1000)}K`;
  if (a >= 10000) return `${sign}$${(a / 1000).toFixed(1)}K`;
  return `${sign}$${Math.round(a).toLocaleString()}`;
}

/** End-tag label — always shows a leading sign so gains/losses read at a glance. */
function fmtSignedAxisY(v: number): string {
  const s = fmtAxisY(v);
  return v > 0 ? `+${s}` : s;
}
