"use client";

/**
 * Live "batch #N closing in T" chip for the Activity page header.
 *
 * Distinct from the global <BatchPill> in the nav: this one names the
 * specific batch # currently clearing. Lifted from the handoff's right-aligned
 * indicator. At the 2s FBA cadence the remaining seconds tick fast — we show
 * a single decimal so the number is at least readable.
 */

import { useEffect, useRef, useState } from "react";
import {
  selectConnection,
  selectLatestBlock,
  useStore,
} from "@/lib/store";
import { formatBatchSeconds, formatInt } from "@/lib/format/nanos";
import { BLOCK_INTERVAL_MS } from "@/lib/constants";

const BLOCK_MS = BLOCK_INTERVAL_MS;

export function ActivityBatchChip() {
  const latest = useStore(selectLatestBlock);
  const connection = useStore(selectConnection);

  const [progress, setProgress] = useState(0);
  const rafRef = useRef<number | null>(null);
  const anchorRef = useRef<number | null>(null);

  /* eslint-disable react-hooks/set-state-in-effect, react-hooks/exhaustive-deps -- reset on new block */
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

  if (latest == null) return null;

  const isLive = connection.state === "live";
  const remainingSecs = formatBatchSeconds((1 - progress) * (BLOCK_MS / 1000));
  // The current batch is the *next* one to clear, i.e. latest.height + 1.
  const currentBatchNum = latest.height + 1;

  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        color: "var(--fg-3)",
      }}
    >
      <span
        aria-hidden
        style={{
          width: 6,
          height: 6,
          borderRadius: "50%",
          background: isLive ? "var(--accent)" : "var(--warn)",
        }}
      />
      batch{" "}
      <span style={{ color: "var(--fg-1)" }}>
        #{formatInt(currentBatchNum)}
      </span>{" "}
      closing in{" "}
      <span
        style={{
          color: "var(--accent)",
          fontVariantNumeric: "tabular-nums",
          minWidth: 24,
          display: "inline-block",
        }}
      >
        {remainingSecs}s
      </span>
    </span>
  );
}
