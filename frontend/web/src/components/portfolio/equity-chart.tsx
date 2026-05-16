"use client";

/**
 * SVG equity-curve chart. Hand-rolled to match handoff `EquityChart`
 * (PortfolioPieces.jsx:75-131). The underlying data is mocked via
 * `useEquityCurve` (OPEN_QUESTIONS #12), so the entire frame wears a
 * MockValue pill in the corner.
 */

import { MockValue } from "@/components/mock-value";
import type { EquityCurve } from "@/lib/account/use-equity-curve";

interface Props {
  curve: EquityCurve;
}

const W = 480;
const H = 200;
const PAD_L = 12;
const PAD_R = 32;
const PAD_T = 14;
const PAD_B = 24;

export function EquityChart({ curve }: Props) {
  const { points, baseline } = curve;
  if (points.length < 2) {
    return (
      <Wrapper>
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
          insufficient data
        </div>
      </Wrapper>
    );
  }

  const min = Math.min(baseline, ...points);
  const max = Math.max(baseline, ...points);
  const span = Math.max(1, max - min);
  const innerW = W - PAD_L - PAD_R;
  const innerH = H - PAD_T - PAD_B;

  const xFor = (i: number) =>
    PAD_L + (i / (points.length - 1)) * innerW;
  const yFor = (v: number) => PAD_T + (1 - (v - min) / span) * innerH;

  const pathD = points
    .map((p, i) => `${i === 0 ? "M" : "L"} ${xFor(i).toFixed(2)} ${yFor(p).toFixed(2)}`)
    .join(" ");

  const baselineY = yFor(baseline);
  const midY = yFor((min + max) / 2);

  // Tick labels on the right axis (top, mid, bottom).
  const labelTop = `$${kify(max)}`;
  const labelMid = `$${kify((min + max) / 2)}`;
  const labelBottom = `$${kify(min)}`;

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
        <line
          x1={PAD_L}
          x2={W - PAD_R}
          y1={PAD_T}
          y2={PAD_T}
          stroke="var(--border-1)"
          strokeWidth={1}
        />
        <line
          x1={PAD_L}
          x2={W - PAD_R}
          y1={midY}
          y2={midY}
          stroke="var(--border-1)"
          strokeWidth={1}
          strokeOpacity={0.5}
        />
        <line
          x1={PAD_L}
          x2={W - PAD_R}
          y1={H - PAD_B}
          y2={H - PAD_B}
          stroke="var(--border-1)"
          strokeWidth={1}
        />

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
        <text
          x={PAD_L + 4}
          y={baselineY - 4}
          fill="var(--fg-4)"
          fontFamily="var(--font-mono)"
          fontSize={9}
          letterSpacing="0.04em"
          style={{ textTransform: "uppercase" }}
        >
          net deposits
        </text>

        {/* Equity line */}
        <path
          d={pathD}
          fill="none"
          stroke="var(--accent)"
          strokeWidth={1.6}
          strokeLinecap="round"
          strokeLinejoin="round"
        />
        {/* End dot */}
        <circle
          cx={xFor(points.length - 1)}
          cy={yFor(points[points.length - 1]!)}
          r={3}
          fill="var(--accent)"
        />

        {/* Right-axis labels */}
        <text
          x={W - PAD_R + 4}
          y={PAD_T + 4}
          fill="var(--fg-4)"
          fontFamily="var(--font-mono)"
          fontSize={9}
          letterSpacing="0.04em"
        >
          {labelTop}
        </text>
        <text
          x={W - PAD_R + 4}
          y={midY + 3}
          fill="var(--fg-4)"
          fontFamily="var(--font-mono)"
          fontSize={9}
          letterSpacing="0.04em"
        >
          {labelMid}
        </text>
        <text
          x={W - PAD_R + 4}
          y={H - PAD_B + 4}
          fill="var(--fg-4)"
          fontFamily="var(--font-mono)"
          fontSize={9}
          letterSpacing="0.04em"
        >
          {labelBottom}
        </text>
      </svg>
    </Wrapper>
  );
}

function Wrapper({ children }: { children: React.ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          gap: 10,
        }}
      >
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
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 11,
            color: "var(--fg-3)",
          }}
        >
          <span style={{ color: "var(--fg-4)" }}>{"// "}</span>
          marked-to-batch · dashed = net deposits
        </span>
        <span style={{ marginLeft: "auto" }}>
          <MockValue
            hint="NOT NOW — equity history is mocked; backend has no per-account time series (OPEN_QUESTIONS #12)"
            variant="pill"
          >
            {" "}
          </MockValue>
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

function kify(v: number): string {
  if (Math.abs(v) >= 1000) return `${(v / 1000).toFixed(1)}K`;
  return v.toFixed(0);
}
