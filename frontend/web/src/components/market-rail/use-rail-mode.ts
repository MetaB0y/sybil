"use client";

/**
 * Persisted Degen/Pro rail mode. Defaults to Degen on first visit per
 * design intent ("tap & win" is the friendlier landing). Survives reloads
 * via localStorage; not synced across tabs (read once on mount).
 */

import { useEffect, useState } from "react";

export type RailMode = "degen" | "pro";

const STORAGE_KEY = "m:rail-mode";

export function useRailMode(): [RailMode, (m: RailMode) => void] {
  // Initial render is "degen" on both server and client to avoid a hydration
  // mismatch. The useEffect below upgrades to the persisted choice once the
  // localStorage value is readable.
  const [mode, setModeState] = useState<RailMode>("degen");

  /* eslint-disable react-hooks/set-state-in-effect -- read localStorage post-mount to avoid SSR mismatch */
  useEffect(() => {
    try {
      const raw = window.localStorage.getItem(STORAGE_KEY);
      if (raw === "degen" || raw === "pro") setModeState(raw);
    } catch {
      // localStorage unavailable (private mode, etc.) — silent fallback to default.
    }
  }, []);
  /* eslint-enable */

  const setMode = (next: RailMode) => {
    setModeState(next);
    try {
      window.localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // ignore
    }
  };

  return [mode, setMode];
}
