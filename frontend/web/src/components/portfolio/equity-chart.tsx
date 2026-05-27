"use client";

/**
 * SVG equity-curve chart. Hand-rolled to match handoff `EquityChart`
 * (PortfolioPieces.jsx:75-131). Data is real, from `useEquityCurve`
 * (`GET /v1/accounts/{id}/equity`): X = sample time, Y = portfolio value,
 * dashed line = net deposits.
 */

import type { EquityCurve, EquityRange } from "@/lib/account/use-equity-curve";

interface Props {
  curve: EquityCurve;
}

const W = 480;
const H = 200;
const PAD_L = 12;
const PAD_R = 32;
const PAD_T = 14;
const PAD_B = 26;

export function EquityChart({ curve }: Props) {
  const { points, baseline, range, isLoading, isEmpty } = curve;

  if (isEmpty) {
    return (
      <Wrapper>
        <Centered>
          {isLoading ? "loading…" : "no equity history yet"}
        </Centered>
      </Wrapper>
    );
  }

  const values = points.map((p) => p.value);
  const min = Math.min(baseline, ...values);
  const max = Math.max(baseline, ...values);
  const span = Math.max(1, max - min);
  const tMin = points[0]!.t;
  const tMax = points[points.length - 1]!.t;
  const tSpan = Math.max(1, tMax - tMin);
  const innerW = W - PAD_L - PAD_R;
  const innerH = H - PAD_T - PAD_B;

  const xFor = (t: number) => PAD_L + ((t - tMin) / tSpan) * innerW;
  const yFor = (v: number) => PAD_T + (1 - (v - min) / span) * innerH;

  const pathD = points
    .map(
      (p, i) =>
        `${i === 0 ? "M" : "L"} ${xFor(p.t).toFixed(2)} ${yFor(p.value).toFixed(2)}`,
    )
    .join(" ");

  const baselineY = yFor(baseline);
  const midY = yFor((min + max) / 2);
  const last = points[points.length - 1]!;
  const tMid = (tMin + tMax) / 2;

  return (
    <Wrapper>
      <svg
        viewBox={`0 0 ${W} ${H}`}
        preserveAspectRatio="none"
        width="100%"
        height="100%"
        style={{ display: "block" }}
      >
        {/* Horizontal grid */}
        <line x1={PAD_L} x2={W - PAD_R} y1={PAD_T} y2={PAD_T} stroke="var(--border-1)" strokeWidth={1} />
        <line x1={PAD_L} x2={W - PAD_R} y1={midY} y2={midY} stroke="var(--border-1)" strokeWidth={1} strokeOpacity={0.5} />
        <line x1={PAD_L} x2={W - PAD_R} y1={H - PAD_B} y2={H - PAD_B} stroke="var(--border-1)" strokeWidth={1} />

        {/* Net-deposits baseline (dashed) */}
        <line
          x1={PAD_L}
          x2={W - PAD_R}
          y1={baselineY}
          y2={baselineY}
          stroke="var(--fg-4)"
          strokeWidth={1}
          strokeDasharray="4 4"
        />
        <text x={PAD_L + 4} y={baselineY - 4} fill="var(--fg-4)" fontFamily="var(--font-mono)" fontSize={9} letterSpacing="0.04em" style={{ textTransform: "uppercase" }}>
          net deposits
        </text>

        {/* Equity line */}
        <path d={pathD} fill="none" stroke="var(--accent)" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round" />
        {/* End dot */}
        <circle cx={xFor(last.t)} cy={yFor(last.value)} r={3} fill="var(--accent)" />

        {/* Right-axis value labels */}
        <AxisLabel x={W - PAD_R + 4} y={PAD_T + 4} text={`$${kify(max)}`} />
        <AxisLabel x={W - PAD_R + 4} y={midY + 3} text={`$${kify((min + max) / 2)}`} />
        <AxisLabel x={W - PAD_R + 4} y={H - PAD_B + 4} text={`$${kify(min)}`} />

        {/* Bottom-axis date labels */}
        <AxisLabel x={PAD_L} y={H - 6} text={fmtDate(tMin, range)} anchor="start" />
        <AxisLabel x={PAD_L + innerW / 2} y={H - 6} text={fmtDate(tMid, range)} anchor="middle" />
        <AxisLabel x={W - PAD_R} y={H - 6} text={fmtDate(tMax, range)} anchor="end" />
      </svg>
    </Wrapper>
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

function Wrapper({ children }: { children: React.ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
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
        <span style={{ fontFamily: "var(--font-mono)", fontSize: 11, color: "var(--fg-3)" }}>
          <span style={{ color: "var(--fg-4)" }}>{"// "}</span>
          marked-to-batch · dashed = net deposits
        </span>
      </div>
      <div
        style={{
          position: "relative",
          background: "var(--surface-1)",
          border: "1px solid var(--border-1)",
          borderRadius: 6,
          padding: 4,
          minHeight: 220,
          overflow: "hidden",
        }}
      >
        {children}
      </div>
    </div>
  );
}

function fmtDate(t: number, range: EquityRange): string {
  const d = new Date(t);
  if (range === "24H") {
    return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  }
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

function kify(v: number): string {
  if (Math.abs(v) >= 1000) return `${(v / 1000).toFixed(1)}K`;
  return v.toFixed(0);
}
