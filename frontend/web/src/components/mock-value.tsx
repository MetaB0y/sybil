"use client";

import type { CSSProperties, ReactNode } from "react";

type Props = {
  children: ReactNode;
  hint: string;
  style?: CSSProperties;
  /**
   * Visual style of the indicator.
   *
   * - `pill` — inline children + a small `MOCK` pill in warn color,
   *   font-mono, 9px. Visible at any font size from 11px to 80px. Use for
   *   prominent values (hero numbers, stat-strip cells, meta-strip values)
   *   that users should immediately recognize as placeholders.
   * - `tint` — wraps children in a subtle warn-soft background highlight,
   *   no pill. Use inside dense tables where a pill per cell would be noisy
   *   — the column header carries the "mocked" marker once instead.
   * - `underline` (default) — faint dotted underline. Backward-compatible
   *   with existing inline usages in market cards and the market-detail dev
   *   page.
   */
  variant?: "pill" | "tint" | "underline";
};

/**
 * Marks a rendered value as a frontend mock. The wrapper carries the
 * tooltip with `hint`; the visual treatment is controlled by `variant`.
 *
 * Remove the wrap (just render `children`) once the backend exposes the
 * underlying field.
 */
export function MockValue({
  children,
  hint,
  style,
  variant = "underline",
}: Props) {
  const tooltip = `${hint} — mocked until backend exposes this field`;

  if (variant === "underline") {
    return (
      <span
        title={tooltip}
        style={{
          borderBottom:
            "1px dotted color-mix(in srgb, var(--warn) 60%, transparent)",
          cursor: "help",
          ...style,
        }}
      >
        {children}
      </span>
    );
  }

  if (variant === "tint") {
    return (
      <span
        title={tooltip}
        style={{
          background: "color-mix(in srgb, var(--warn) 10%, transparent)",
          padding: "0 4px",
          borderRadius: 2,
          cursor: "help",
          ...style,
        }}
      >
        {children}
      </span>
    );
  }

  // pill
  return (
    <span
      title={tooltip}
      style={{
        display: "inline-flex",
        alignItems: "baseline",
        gap: 6,
        cursor: "help",
        ...style,
      }}
    >
      {children}
      <span
        aria-label="mocked value"
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 9,
          lineHeight: 1,
          fontWeight: 600,
          color: "var(--warn)",
          background: "var(--warn-soft)",
          padding: "2px 5px",
          borderRadius: 2,
          letterSpacing: "0.08em",
          textTransform: "uppercase",
          whiteSpace: "nowrap",
          verticalAlign: "baseline",
          // Subtle vertical alignment lift so the pill sits at x-height,
          // not on the baseline — looks balanced next to large numerals.
          transform: "translateY(-1px)",
        }}
      >
        mock
      </span>
    </span>
  );
}
