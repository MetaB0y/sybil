"use client";

import type React from "react";
import { formatShareUnits } from "@/lib/account/quantity";
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
  /** Volume-weighted average fill price (nanos), or null before any priced
   *  fill. With `filledQty` this yields the ACTUAL dollars spent — usually below
   *  the nominal stake because the batch clears at a better price than the limit. */
  avgPriceNanos?: bigint | null;
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
  // Dollar value filled so far — the filled fraction of the intended stake,
  // rounded to cents to avoid float dust. Drives both the live "placing"
  // readout and the partial-fill result line below.
  const filledUsd =
    props.targetQty > 0n
      ? Math.round(
          ((props.betUsd * Number(props.filledQty)) / Number(props.targetQty)) *
            100,
        ) / 100
      : 0;

  // What the user ACTUALLY spent = filled shares × the average price they really
  // got (usually below their limit, so below the nominal stake). `filledQty` is
  // in share-units (1000 = 1 share); `avgPriceNanos` is a per-share price in
  // nanos (1e9 = $1). Falls back to the nominal proportional before any priced
  // fill. `savedUsd` is the welfare — what the better-than-limit clear saved.
  const actualUsd =
    props.avgPriceNanos != null &&
    props.avgPriceNanos > 0n &&
    props.filledQty > 0n
      ? Math.round(
          (Number(props.filledQty) / 1000) *
            (Number(props.avgPriceNanos) / 1e9) *
            100,
        ) / 100
      : filledUsd;
  const savedUsd = Math.max(0, Math.round((filledUsd - actualUsd) * 100) / 100);

  if (props.phase === "tracking") {
    return (
      <div style={cardStyle}>
        <div style={rowStyle}>
          <span style={labelStyle}>Waiting for a taker…</span>
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
        {/* One number the eye can land on — how much of the stake has gone in —
            then the share target as a quiet aside. The old "$0 / $10 · 0 / 15.11
            shares" read as two competing fractions; this reads as a sentence. */}
        <div style={monoStyle}>
          <span style={{ color: "var(--fg-1)", fontWeight: 600 }}>
            {money(actualUsd)}
          </span>
          {` of ${money(props.betUsd)} in`}
          <span style={{ color: "var(--fg-4)" }}>
            {`  ·  ${formatShareUnits(props.targetQty)} shares`}
          </span>
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
      ? `Successfully bet ${money(actualUsd)} on ${props.side}!`
      : props.phase === "partial"
        ? `◐ Half in! Bet ${money(actualUsd)} of ${money(props.betUsd)} on ${props.side} — the rest is back in your balance.`
        : cancelled
          ? `Bet cancelled.`
          : `No match within ~2 min — your funds are back in your balance. Try again.`;

  return (
    <div style={cardStyle}>
      <div style={{ ...rowStyle, color: resultColor }}>
        <span
          style={{
            fontFamily: "var(--font-sans)",
            fontSize: 14,
            fontWeight: 700,
          }}
        >
          {result}
        </span>
      </div>
      {success && savedUsd >= 0.01 && (
        <div style={{ ...monoStyle, color: "var(--fg-3)" }}>
          {`better price · saved ${money(savedUsd)}`}
        </div>
      )}
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
