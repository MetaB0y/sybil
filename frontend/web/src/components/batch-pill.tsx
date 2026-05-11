"use client";

import { useEffect, useRef, useState } from "react";
import { formatInt } from "@/lib/format/nanos";
import {
  selectConnection,
  selectLatestBlock,
  useStore,
} from "@/lib/store";

const BLOCK_MS = 2000; // SYBIL_BLOCK_INTERVAL_MS; tracks the prod cadence.

/**
 * Live block indicator. Shows current height + a 2s linear progress bar
 * that resets every time a new block lands. Pulses yellow when not live.
 *
 * Keyed to incoming `block.timestamp_ms` rather than wall-clock so we don't
 * drift across tabs / sleeps. If no block has arrived yet, the bar idles.
 */
export function BatchPill() {
  const connection = useStore(selectConnection);
  const latest = useStore(selectLatestBlock);

  const [progress, setProgress] = useState(0); // 0..1
  const rafRef = useRef<number | null>(null);
  const anchorRef = useRef<number | null>(null);

  // Restart the progress bar each time a new block lands.
  /* eslint-disable react-hooks/set-state-in-effect, react-hooks/exhaustive-deps -- reset progress bar synchronously on new block */
  useEffect(() => {
    if (latest == null) return;
    anchorRef.current = performance.now();
    setProgress(0);
  }, [latest?.height]);
  /* eslint-enable react-hooks/set-state-in-effect, react-hooks/exhaustive-deps */

  // Tick the progress bar.
  useEffect(() => {
    const step = () => {
      if (anchorRef.current != null) {
        const elapsed = performance.now() - anchorRef.current;
        const p = Math.min(1, elapsed / BLOCK_MS);
        setProgress(p);
      }
      rafRef.current = requestAnimationFrame(step);
    };
    rafRef.current = requestAnimationFrame(step);
    return () => {
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
    };
  }, []);

  const isLive = connection.state === "live";
  const height = latest?.height ?? connection.lastSeenHeight ?? null;

  const barColor = isLive ? "var(--accent)" : "var(--warn)";

  return (
    <div
      style={{
        position: "relative",
        display: "inline-flex",
        alignItems: "center",
        gap: "var(--space-2)",
        padding: "0 var(--space-3)",
        height: "28px",
        background: "var(--surface-2)",
        border: "1px solid var(--border-2)",
        borderRadius: "var(--radius-pill)",
        fontFamily: "var(--font-mono)",
        fontSize: "var(--fs-12)",
        color: "var(--fg-2)",
        overflow: "hidden",
      }}
      title={`connection: ${connection.state}`}
    >
      <span
        aria-hidden
        style={{
          width: 6,
          height: 6,
          borderRadius: "50%",
          background: barColor,
          boxShadow: isLive
            ? `0 0 6px var(--accent-soft)`
            : "0 0 6px var(--warn-soft)",
          animation: isLive ? "none" : "sybil-pulse 1.6s ease-in-out infinite",
        }}
      />
      <span style={{ letterSpacing: "var(--track-wide)", textTransform: "uppercase" }}>
        block
      </span>
      <span className="tabular" style={{ color: "var(--fg-1)" }}>
        {height != null ? formatInt(height) : "—"}
      </span>
      {/* Linear 2s progress bar, repeating per block */}
      <span
        aria-hidden
        style={{
          position: "absolute",
          bottom: 0,
          left: 0,
          height: 1,
          width: `${progress * 100}%`,
          background: barColor,
          opacity: 0.6,
          transition: "none",
        }}
      />
    </div>
  );
}
