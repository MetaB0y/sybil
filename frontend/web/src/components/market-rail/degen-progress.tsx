"use client";

import type React from "react";
import type { DegenSide } from "@/lib/degen";
import type { DegenPhase } from "@/lib/degen/track";

export interface DegenProgressProps {
  phase: DegenPhase;
  side: DegenSide;
  secondsLeft: number;
  timeProgress01: number;
  filledQty: bigint;
  targetQty: bigint;
  /** The dollar amount the user bet (the full intended stake). */
  betUsd: number;
  onBetAgain: () => void;
  /** Cancel the in-flight bet (tracking phase only). Omit to hide the control. */
  onCancel?: () => void;
  /** False while the order id isn't bound yet — keeps Cancel disabled. */
  canCancel?: boolean;
  /** A cancel request is in flight. */
  cancelling?: boolean;
}

/** "$10" for whole amounts, "$12.50" otherwise. */
function money(n: number): string {
  return Number.isInteger(n) ? `$${n}` : `$${n.toFixed(2)}`;
}

export function DegenProgress(props: DegenProgressProps) {
  if (props.phase === "tracking") {
    return (
      <div style={cardStyle}>
        <div style={rowStyle}>
          <span style={labelStyle}>Placing your bet…</span>
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
          {props.filledQty.toString()} / {props.targetQty.toString()} shares
          bought
        </div>
        {props.onCancel && (
          <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
            <button
              type="button"
              onClick={props.onCancel}
              disabled={!props.canCancel || props.cancelling}
              title={
                props.canCancel
                  ? "Cancel this bet"
                  : "Cancel unlocks the moment your order registers (about a second)."
              }
              style={{
                ...cancelStyle,
                cursor:
                  !props.canCancel || props.cancelling
                    ? "not-allowed"
                    : "pointer",
                opacity: !props.canCancel || props.cancelling ? 0.55 : 1,
              }}
            >
              {props.cancelling ? "Cancelling…" : "Cancel bet"}
            </button>
            {/* Cancel can't fire until the order's id registers (~1s after
                submit). Brief, but say so the greyed button doesn't read as
                broken. */}
            {!props.canCancel && !props.cancelling && (
              <span style={cancelHintStyle}>unlocking cancel…</span>
            )}
          </div>
        )}
      </div>
    );
  }

  // Partial fill: the filled portion of the intended stake, proportional to
  // shares filled (rounded to cents to avoid float dust).
  const filledUsd =
    props.targetQty > 0n
      ? Math.round(
          ((props.betUsd * Number(props.filledQty)) / Number(props.targetQty)) *
            100,
        ) / 100
      : 0;
  // Success (full or partial fill) always reads green; a miss always reads red
  // — independent of whether the user bet YES or NO, so the colour signals
  // outcome, not side. A user-initiated cancel is neither win nor loss, so it
  // reads neutral rather than red.
  const success = props.phase === "filled" || props.phase === "partial";
  const cancelled = props.phase === "cancelled";
  const resultColor = cancelled
    ? "var(--fg-2)"
    : success
      ? "var(--yes)"
      : "var(--no)";
  const result =
    props.phase === "filled"
      ? `Successfully bet ${money(props.betUsd)} on ${props.side}!`
      : props.phase === "partial"
        ? `◐ Half in! Successfully bet ${money(filledUsd)} out of ${money(props.betUsd)} on ${props.side}!`
        : cancelled
          ? `Bet cancelled.`
          : `Oops, your order failed. Try again!`;

  return (
    <div style={cardStyle}>
      <div style={{ ...rowStyle, color: resultColor }}>
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
  fontSize: 12.5,
  fontWeight: 500,
  color: "var(--fg-2)",
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
// Quiet outline (matching the open-orders Cancel) so it sits under the live
// meter without competing with it.
const cancelStyle: React.CSSProperties = {
  marginTop: 2,
  padding: "9px 0",
  borderRadius: 6,
  border: "1px solid color-mix(in srgb, var(--no) 32%, transparent)",
  background: "transparent",
  color: "var(--no)",
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  fontWeight: 600,
  textTransform: "uppercase",
  letterSpacing: "var(--track-wide)",
};
// Muted caption under a greyed Cancel — explains why it's not yet tappable.
const cancelHintStyle: React.CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 10,
  color: "var(--fg-4)",
  textAlign: "center",
  letterSpacing: "0.02em",
};
// Filled accent button so "Bet again" reads as the obvious next tap, not a
// faint outline.
const betAgainStyle: React.CSSProperties = {
  padding: "13px 0",
  borderRadius: 6,
  border: 0,
  background: "var(--accent)",
  color: "var(--fg-on-accent)",
  fontFamily: "var(--font-sans)",
  fontSize: 14,
  fontWeight: 700,
  letterSpacing: "-0.005em",
  cursor: "pointer",
  boxShadow: "0 0 0 1px color-mix(in srgb, var(--accent) 40%, transparent)",
};
