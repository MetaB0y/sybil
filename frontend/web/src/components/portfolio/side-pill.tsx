"use client";

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
      : "var(--bg-2)";
  return (
    <span
      style={{
        display: "inline-block",
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
