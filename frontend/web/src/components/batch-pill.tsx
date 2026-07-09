"use client";

import { useEffect, useRef, useState } from "react";
import { formatBatchSeconds } from "@/lib/format/nanos";
import { selectConnection, selectLatestBlock, useStore } from "@/lib/store";
import { BLOCK_INTERVAL_MS } from "@/lib/constants";

const BLOCK_MS = BLOCK_INTERVAL_MS;
// Cap the displayed countdown just under the full window so a fresh batch reads
// "9.9" instead of flashing "10.0" for the first frame after a new block.
const MAX_DISPLAY_MS = BLOCK_MS - 100;

/**
 * Live batch countdown pill — mirrors the handoff layout:
 *
 *   ● BATCH 1.6 ━━━━━━━━
 *
 * Dot · BATCH label · countdown (seconds remaining until the current
 * batch clears) · inline horizontal progress bar that shrinks from full
 * to empty over each batch window. The handoff renders `0:56` because it
 * assumed a 60s cadence; we use a one-decimal seconds display (e.g. `9.7`)
 * driven by BLOCK_INTERVAL_MS.
 *
 * Anchored to performance.now() at each new block so the countdown
 * doesn't drift across tab sleeps.
 */
export function BatchPill() {
  const connection = useStore(selectConnection);
  const latest = useStore(selectLatestBlock);

  const [progress, setProgress] = useState(0); // 0..1 (elapsed fraction)
  const rafRef = useRef<number | null>(null);
  const anchorRef = useRef<number | null>(null);

  /* eslint-disable react-hooks/set-state-in-effect, react-hooks/exhaustive-deps -- reset progress on new block */
  useEffect(() => {
    if (latest == null) return;
    anchorRef.current = performance.now();
    setProgress(0);
  }, [latest?.height]);
  /* eslint-enable react-hooks/set-state-in-effect, react-hooks/exhaustive-deps */

  useEffect(() => {
    const step = () => {
      if (anchorRef.current != null) {
        const elapsed = performance.now() - anchorRef.current;
        setProgress(Math.min(1, elapsed / BLOCK_MS));
      }
      rafRef.current = requestAnimationFrame(step);
    };
    rafRef.current = requestAnimationFrame(step);
    return () => {
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
    };
  }, []);

  const isLive = connection.state === "live";
  const remainingMs = Math.max(0, BLOCK_MS - progress * BLOCK_MS);
  const remainingSecs = formatBatchSeconds(
    Math.min(remainingMs, MAX_DISPLAY_MS) / 1000
  );
  const remainingPct = 100 - progress * 100;

  const accent = isLive ? "var(--accent)" : "var(--warn)";
  const accentSoft = isLive ? "var(--accent-soft)" : "var(--warn-soft)";
  const labelColor = isLive
    ? "color-mix(in srgb, var(--accent) 70%, transparent)"
    : "color-mix(in srgb, var(--warn) 70%, transparent)";

  return (
    <div
      className="batch-pill"
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: "var(--space-2)",
        padding: "5px 10px",
        background: accentSoft,
        borderRadius: "var(--radius-md)",
        fontFamily: "var(--font-mono)",
        fontSize: "var(--fs-12)",
        color: accent,
        fontVariantNumeric: "tabular-nums",
      }}
      title={`connection: ${connection.state}`}
    >
      <span
        aria-hidden
        style={{
          width: 6,
          height: 6,
          borderRadius: "50%",
          background: accent,
          boxShadow: isLive ? `0 0 6px ${accentSoft}` : "0 0 6px transparent",
          animation: isLive ? "none" : "sybil-pulse 1.6s ease-in-out infinite",
        }}
      />
      <span
        style={{
          letterSpacing: "var(--track-wide)",
          textTransform: "uppercase",
          fontSize: "10px",
          color: labelColor,
        }}
      >
        batch
      </span>
      <span
        className="tabular"
        style={{
          // Reserve room for the widest value we ever show ("9.9") — the
          // countdown is capped just under the full window so it never reaches
          // "10.0". Right-aligned keeps the decimal point fixed as it shrinks.
          display: "inline-block",
          width: `${formatBatchSeconds(MAX_DISPLAY_MS / 1000).length}ch`,
          textAlign: "right",
        }}
      >
        {remainingSecs}
      </span>
      <span
        aria-hidden
        style={{
          width: 48,
          height: 2,
          background: accentSoft,
          borderRadius: 1,
          overflow: "hidden",
        }}
      >
        <span
          style={{
            display: "block",
            width: `${remainingPct}%`,
            height: "100%",
            background: accent,
            transition: "none",
          }}
        />
      </span>
    </div>
  );
}
