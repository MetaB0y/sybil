"use client";

/**
 * Shared batch countdown helper (window = BLOCK_INTERVAL_MS). Returns:
 *   - `progress01` — 0..1 linearly across the current batch window
 *   - `secondsLeft` — display-friendly integer seconds remaining
 *   - `secondsLeftPrecise` — float seconds remaining; feed `formatBatchSeconds`
 *   - `latestHeight` — committed block height (the open batch is +1)
 *
 * Progress is derived from a single monotonic anchor stamped in the store when
 * each block is received (`latestBlockAnchorPerf`), NOT from a per-component
 * mount timestamp. That's the fix for the timer restarting at the full window
 * when the rail remounts (e.g. switching outcomes) — the anchor is shared and
 * outlives any one mount, so the countdown stays glued to the real block clock.
 * Linear easing keyed to the block cadence — not wall-clock springs (which
 * would drift) and not Date.now() (which would be wrong under client clock skew).
 */

import { useEffect, useRef, useState } from "react";
import {
  selectLatestBlock,
  selectLatestBlockAnchor,
  useStore,
} from "@/lib/store";
import { BLOCK_INTERVAL_MS } from "@/lib/constants";

const BATCH_MS = BLOCK_INTERVAL_MS;

function progressFor(anchorPerf: number | null): number {
  if (anchorPerf == null) return 0;
  return Math.min(1, Math.max(0, (performance.now() - anchorPerf) / BATCH_MS));
}

export function useBatchCountdown(): {
  progress01: number;
  secondsLeft: number;
  secondsLeftPrecise: number;
  latestHeight: number | null;
} {
  const latest = useStore(selectLatestBlock);
  const anchorPerf = useStore(selectLatestBlockAnchor);
  // Start at 0 (matches SSR) and let the first RAF tick fill in the real
  // value within ~one frame, so a mid-batch remount snaps to the correct
  // remaining time instead of flashing the full window.
  const [progress01, setProgress01] = useState(0);
  const rafRef = useRef<number | null>(null);

  useEffect(() => {
    // Throttle to ~10fps (see git history: 60fps churned ~120 renders/sec
    // across the rail's two countdowns and stuttered the gauge).
    let lastTickMs = 0;
    const step = (frameMs: number) => {
      if (frameMs - lastTickMs >= 100) {
        lastTickMs = frameMs;
        setProgress01(progressFor(anchorPerf));
      }
      rafRef.current = requestAnimationFrame(step);
    };
    rafRef.current = requestAnimationFrame(step);
    return () => {
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
    };
  }, [anchorPerf]);

  // Open the countdown one tenth below the full window (9.9 at a 10s batch) so
  // every batch clock — the Pro hero gauge and the "queued for batch" timer —
  // reads as already ticking down, never a momentary static "10.0".
  const fullSeconds = BATCH_MS / 1000;
  const secondsLeftPrecise = Math.min(
    fullSeconds - 0.1,
    Math.max(0, (1 - progress01) * fullSeconds),
  );
  const secondsLeft = Math.ceil(secondsLeftPrecise);

  return {
    progress01,
    secondsLeft,
    secondsLeftPrecise,
    latestHeight: latest?.height ?? null,
  };
}
