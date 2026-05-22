"use client";

import type React from "react";
import type { DegenSide } from "@/lib/degen";
import type { DegenPhase } from "@/lib/degen/track";

/** Format price nanos as cents with one decimal (5.4e8 -> "54.0"). */
function cents(n: bigint): string {
  return (Number(n) / 1e7).toFixed(1);
}

export interface DegenProgressProps {
  phase: DegenPhase;
  side: DegenSide;
  secondsLeft: number;
  timeProgress01: number;
  filledQty: bigint;
  targetQty: bigint;
  limitPriceNanos: bigint;
  avgPriceNanos: bigint | null;
  onBetAgain: () => void;
}

export function DegenProgress(props: DegenProgressProps) {
  const accent = props.side === "YES" ? "var(--yes)" : "var(--no)";

  if (props.phase === "tracking") {
    return (
      <div style={cardStyle}>
        <div style={rowStyle}>
          <span style={labelStyle}>FILLING…</span>
          <span style={monoStyle}>⏱ {props.secondsLeft}s</span>
        </div>
        <div style={barTrackStyle}>
          <div
            style={{
              width: `${Math.round(props.timeProgress01 * 100)}%`,
              height: "100%",
              background: "var(--accent)",
              transition: "width 120ms linear",
            }}
          />
        </div>
        <div style={monoStyle}>
          {props.filledQty.toString()} / {props.targetQty.toString()} sh @ ≤
          {cents(props.limitPriceNanos)}¢
        </div>
      </div>
    );
  }

  const result =
    props.phase === "filled"
      ? `✅ FILLED ${props.targetQty.toString()} sh @ ${cents(
          props.avgPriceNanos ?? props.limitPriceNanos,
        )}¢`
      : props.phase === "partial"
        ? `◐ PARTIAL — ${props.filledQty.toString()} of ${props.targetQty.toString()} filled, rest expired`
        : `✕ NO FILL — nobody took the other side`;

  return (
    <div style={cardStyle}>
      <div style={{ ...rowStyle, color: accent }}>
        <span style={{ fontFamily: "var(--font-sans)", fontSize: 14, fontWeight: 700 }}>
          {result}
        </span>
      </div>
      <button type="button" onClick={props.onBetAgain} style={betAgainStyle}>
        Bet again
      </button>
    </div>
  );
}

const cardStyle: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 10,
  padding: "16px",
  borderRadius: 6,
  border: "1px solid var(--border-2)",
  background: "var(--surface-2)",
};
const rowStyle: React.CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "center",
};
const labelStyle: React.CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 10,
  textTransform: "uppercase",
  letterSpacing: "0.06em",
  color: "var(--fg-3)",
};
const monoStyle: React.CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 12,
  color: "var(--fg-2)",
};
const barTrackStyle: React.CSSProperties = {
  height: 4,
  borderRadius: 2,
  background: "var(--border-1)",
  overflow: "hidden",
};
const betAgainStyle: React.CSSProperties = {
  padding: "12px 0",
  borderRadius: 6,
  border: "1px solid var(--border-2)",
  background: "transparent",
  color: "var(--fg-1)",
  fontFamily: "var(--font-sans)",
  fontSize: 14,
  fontWeight: 600,
  cursor: "pointer",
};
