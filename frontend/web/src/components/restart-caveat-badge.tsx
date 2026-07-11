"use client";

/**
 * Compatibility pill for deployments that run without `SYBIL_DATA_DIR` and
 * therefore lose aggregate read models on restart.
 *
 * Production sets `SYBIL_DATA_DIR=/data`, so current product surfaces do not
 * render this badge. Keep it available for explicitly ephemeral deployments.
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
