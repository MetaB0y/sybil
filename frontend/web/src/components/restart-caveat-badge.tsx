"use client";

/**
 * Inline pill that flags an "all-time" figure as caveat-bearing until
 * production persistence is enabled. Decision Q5 / part 2 of
 * BACKEND_DATA_PLAN.md.
 *
 * Render inline next to any cumulative tracker value backed by an
 * in-memory aggregate (TraderTracker, OrderStatsTracker, FillRecorder
 * counters, etc.). Drop this component once `SYBIL_DATA_DIR` is set in
 * prod so the trackers survive restarts.
 *
 * Eventual consumer list — keep in sync with BACKEND_IMPLEMENTATION_PLAN.md:
 *   - B1 — binary-card.tsx (trader count)
 *           multi-card.tsx (trader count)
 *           activity/page.tsx (all-time unique traders)
 *   - B2 — activity/page.tsx (all-time total volume)
 *   - B6 — activity/page.tsx (all-time orders placed/matched/unmatched)
 *           m/[id]/page.tsx (orders totals)
 *   - B8 — portfolio/portfolio-hero.tsx (first deposit, total fill count)
 *   - C1 — portfolio/portfolio-hero.tsx (realized PnL)
 *
 * NOT used on: B3 (24h is window-bounded), B4 (liquidity is current state),
 * B7 (per-block welfare), C2 (real-time indicative), D1 (event-based).
 */

import type { CSSProperties } from "react";

interface Props {
  /**
   * Optional extra context appended to the tooltip. Default tooltip
   * already explains why the badge is rendered.
   */
  hint?: string;
  style?: CSSProperties;
}

const DEFAULT_HINT =
  "tracker is in-memory; the figure resets on sequencer restart. Will be dropped once SYBIL_DATA_DIR is populated in prod.";

export function RestartCaveatBadge({ hint, style }: Props) {
  const tooltip = hint ? `${DEFAULT_HINT} ${hint}` : DEFAULT_HINT;
  return (
    <span
      aria-label="since last restart"
      title={tooltip}
      style={{
        display: "inline-flex",
        alignItems: "baseline",
        fontFamily: "var(--font-mono)",
        fontSize: 9,
        lineHeight: 1,
        fontWeight: 600,
        color: "var(--fg-3)",
        background: "color-mix(in srgb, var(--fg-3) 12%, transparent)",
        padding: "2px 5px",
        borderRadius: 2,
        letterSpacing: "0.08em",
        textTransform: "uppercase",
        whiteSpace: "nowrap",
        cursor: "help",
        transform: "translateY(-1px)",
        ...style,
      }}
    >
      since last restart
    </span>
  );
}
