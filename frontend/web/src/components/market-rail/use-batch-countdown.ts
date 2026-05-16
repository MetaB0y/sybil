"use client";

/**
 * Shared 2s batch countdown helper. Returns:
 *   - `progress01` — 0..1 linearly across the current batch window
 *   - `secondsLeft` — display-friendly integer (0..2)
 *   - `latestHeight` — committed block height (the open batch is +1)
 *
 * Animation pattern mirrors `frontend/web/src/components/batch-theater.tsx`:
 * anchor on every new committed block via useEffect, then RAF-tick locally.
 * Linear easing keyed to height — not wall-clock springs (would jank at 2s).
 */

import { useEffect, useRef, useState } from "react";
import { selectLatestBlock, useStore } from "@/lib/store";

const BATCH_MS = 2000;

export function useBatchCountdown(): {
  progress01: number;
  secondsLeft: number;
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
    const step = () => {
      if (anchorRef.current != null) {
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

  const secondsLeft = Math.max(0, Math.ceil((1 - progress01) * (BATCH_MS / 1000)));

  return {
    progress01,
    secondsLeft,
    latestHeight: latest?.height ?? null,
  };
}
