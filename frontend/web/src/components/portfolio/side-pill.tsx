"use client";

import type { CSSProperties } from "react";

/**
 * YES / NO outcome chip, shared by the portfolio tables and the market-detail
 * holdings / orders lists.
 *
 * `inline-flex` + centred content rather than a bare `inline-block`: most
 * callers drop the pill straight into a grid cell, where it is blockified and
 * stretched to the column width — the label then hugged the left edge of its
 * own tinted chip ("NO" sat ~12px left of centre in a 50px column). `justifySelf`
 * keeps the chip sized to its label when it *is* a grid item, and `minWidth`
 * makes YES and NO the same width so a column of them doesn't look ragged.
 */

/**
 * Shared style for the small tinted "value chips" in the account tables — the
 * side (YES/NO) pill and the welfare cell. They share size, font, padding,
 * radius and an always-tinted background so they read as the same object; the
 * only intended difference is that the welfare value is bold (`bold`).
 */
export function valueChipStyle({
  color,
  bg,
  bold = false,
}: {
  color: string;
  bg: string;
  bold?: boolean;
}): CSSProperties {
  return {
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    minWidth: 34,
    padding: "1px 7px",
    background: bg,
    color,
    borderRadius: 3,
    fontFamily: "var(--font-mono)",
    fontSize: 11,
    fontWeight: bold ? 700 : 500,
    letterSpacing: "var(--track-wide)",
    whiteSpace: "nowrap",
  };
}

export function SidePill({
  outcome,
}: {
  outcome: "YES" | "NO" | string;
}) {
  const upper = outcome.toUpperCase();
  const isYes = upper === "YES";
  const isNo = upper === "NO";
  const color = isYes ? "var(--yes)" : isNo ? "var(--no)" : "var(--fg-3)";
  const bg = isYes
    ? "color-mix(in srgb, var(--yes) 14%, transparent)"
    : isNo
      ? "color-mix(in srgb, var(--no) 14%, transparent)"
      : "var(--fill-subtle)";
  // The side status is the regular-weight chip; welfare is the bold one.
  return (
    <span style={{ ...valueChipStyle({ color, bg }), justifySelf: "start" }}>
      {upper}
    </span>
  );
}
