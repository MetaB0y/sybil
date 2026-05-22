"use client";

/**
 * Shared batch countdown helper (window = BLOCK_INTERVAL_MS). Returns:
 *   - `progress01` — 0..1 linearly across the current batch window
 *   - `secondsLeft` — display-friendly integer seconds remaining
 *   - `secondsLeftPrecise` — float seconds remaining; feed `formatBatchSeconds`
 *   - `latestHeight` — committed block height (the open batch is +1)
 *
 * Animation pattern mirrors `frontend/web/src/components/batch-theater.tsx`:
 * anchor on every new committed block via useEffect, then RAF-tick locally.
 * Linear easing keyed to height — not wall-clock springs (which would drift
 * from the block clock).
 */

import { useEffect, useRef, useState } from "react";
import { selectLatestBlock, useStore } from "@/lib/store";
import { BLOCK_INTERVAL_MS } from "@/lib/constants";

const BATCH_MS = BLOCK_INTERVAL_MS;

export function useBatchCountdown(): {
  progress01: number;
  secondsLeft: number;
  secondsLeftPrecise: number;
  latestHeight: number | null;
} {
  const latest = useStore(selectLatestBlock);
  const [progress01, setProgress01] = useState(0);
  const anchorRef = useRef<number | null>(null);
  const rafRef = useRef<number | null>(null);

  /* eslint-disable react-hooks/set-state-in-effect, react-hooks/exhaustive-deps -- reset on new block */
  useEffect(() => {
    if (latest == null) return;
    anchorRef.current = performance.now();
    setProgress01(0);
  }, [latest?.height]);
  /* eslint-enable */

  useEffect(() => {
    // Throttle to ~10fps. At 60fps this setState re-renders every consumer each
    // frame, and the rail mounts two countdowns (hero gauge + buy-box), so the
    // page churned ~120 renders/sec and the gauge stuttered. 100ms steps plus
    // the ring's 60ms CSS transition stay visually smooth at a fraction of the
    // render cost.
    let lastTickMs = 0;
    const step = (frameMs: number) => {
      if (anchorRef.current != null && frameMs - lastTickMs >= 100) {
        lastTickMs = frameMs;
        const elapsed = performance.now() - anchorRef.current;
        setProgress01(Math.min(1, elapsed / BATCH_MS));
      }
      rafRef.current = requestAnimationFrame(step);
    };
    rafRef.current = requestAnimationFrame(step);
    return () => {
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
    };
  }, []);

  const secondsLeftPrecise = Math.max(0, (1 - progress01) * (BATCH_MS / 1000));
  const secondsLeft = Math.ceil(secondsLeftPrecise);

  return {
    progress01,
    secondsLeft,
    secondsLeftPrecise,
    latestHeight: latest?.height ?? null,
  };
}
