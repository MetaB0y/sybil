"use client";

import { useEffect, useRef, useState } from "react";
import { formatBatchSeconds } from "@/lib/format/nanos";
import { selectConnection, selectLatestBlock, useStore } from "@/lib/store";

const BLOCK_MS = 2000; // SYBIL_BLOCK_INTERVAL_MS; tracks the prod cadence.

/**
 * Live batch countdown pill — mirrors the handoff layout:
 *
 *   ● BATCH 1.6 ━━━━━━━━
 *
 * Dot · BATCH label · countdown (seconds remaining until the current
 * batch clears) · inline horizontal progress bar that shrinks from full
 * to empty over each 2s window. The handoff renders `0:56` because it
 * assumed a 60s cadence; we adapt to 2s with a one-decimal seconds
 * display since `0:00 → 0:00` at 2s would be useless.
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
  const remainingSecs = formatBatchSeconds(remainingMs / 1000);
  const remainingPct = 100 - progress * 100;

  const accent = isLive ? "var(--accent)" : "var(--warn)";
  const accentSoft = isLive ? "var(--accent-soft)" : "var(--warn-soft)";
  const labelColor = isLive
    ? "color-mix(in srgb, var(--accent) 70%, transparent)"
    : "color-mix(in srgb, var(--warn) 70%, transparent)";

  return (
    <div
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
      <span className="tabular">{remainingSecs}</span>
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
