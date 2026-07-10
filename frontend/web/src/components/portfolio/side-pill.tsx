"use client";

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

export function SidePill({
  outcome,
}: {
  outcome: "YES" | "NO" | string;
}) {
  const upper = outcome.toUpperCase();
  const isYes = upper === "YES";
  const isNo = upper === "NO";
  const color = isYes
    ? "var(--yes)"
    : isNo
      ? "var(--no)"
      : "var(--fg-3)";
  const bg = isYes
    ? "color-mix(in srgb, var(--yes) 14%, transparent)"
    : isNo
      ? "color-mix(in srgb, var(--no) 14%, transparent)"
      : "var(--fill-subtle)";
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        justifySelf: "start",
        minWidth: 34,
        padding: "1px 7px",
        background: bg,
        color,
        borderRadius: 3,
        fontFamily: "var(--font-mono)",
        fontSize: 10,
        fontWeight: 600,
        letterSpacing: "var(--track-wide)",
      }}
    >
      {upper}
    </span>
  );
}
