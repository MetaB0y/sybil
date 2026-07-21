"use client";

import { useSyncExternalStore } from "react";

/**
 * Width below which the data tables stop being tables.
 *
 * Every grid table on the site (the portfolio tabs, the market-detail lists,
 * leaderboard, activity batches) is 860px wide at minimum — see the
 * `min-width` on `.portfolio-grid-table > *` and its siblings in globals.css.
 * Narrower than that and the table can only be read by swiping sideways, with
 * the identity column truncated to "U.S. e…". So the switch happens exactly
 * where the table stops fitting: below 860px the rows render as stacked cards
 * instead (see `DataCard`), and no viewport ever scrolls a table sideways.
 */
export const COMPACT_BREAKPOINT_PX = 859;

const QUERY = `(max-width: ${COMPACT_BREAKPOINT_PX}px)`;

function subscribe(onChange: () => void): () => void {
  if (typeof window === "undefined" || !window.matchMedia) return () => {};
  const mql = window.matchMedia(QUERY);
  mql.addEventListener("change", onChange);
  return () => mql.removeEventListener("change", onChange);
}

function getSnapshot(): boolean {
  if (typeof window === "undefined" || !window.matchMedia) return false;
  return window.matchMedia(QUERY).matches;
}

/**
 * `true` on phone-width viewports. Server-rendered as `false` (the desktop
 * table), which is safe here because every table that consults it renders from
 * client-fetched account/market data — the first paint with rows in it happens
 * after hydration, by which point the real viewport width is known.
 */
export function useCompactLayout(): boolean {
  return useSyncExternalStore(subscribe, getSnapshot, () => false);
}
