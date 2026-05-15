"use client";

/**
 * Order-outcome donut: filled arc = matched share, hollow arc = unmatched.
 * Pure SVG; sized for the batch-detail sidebar (68x68).
 */

export function DonutOutcome({
  matched,
  unmatched,
}: {
  matched: number;
  unmatched: number;
}) {
  const total = matched + unmatched || 1;
  const matchedPct = matched / total;
  const C = 2 * Math.PI * 28; // circumference at r=28

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
      <svg width="68" height="68" viewBox="0 0 68 68">
        <circle
          cx="34"
          cy="34"
          r="28"
          fill="none"
          stroke="var(--no-soft)"
          strokeWidth="8"
        />
        <circle
          cx="34"
          cy="34"
          r="28"
          fill="none"
          stroke="var(--yes)"
          strokeWidth="8"
          strokeDasharray={`${matchedPct * C} ${C}`}
          strokeDashoffset={C / 4}
          transform="rotate(-90 34 34)"
          strokeLinecap="butt"
        />
        <text
          x="34"
          y="32"
          textAnchor="middle"
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 13,
            fill: "var(--fg-1)",
            fontVariantNumeric: "tabular-nums",
          }}
        >
          {Math.round(matchedPct * 100)}%
        </text>
        <text
          x="34"
          y="44"
          textAnchor="middle"
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 8,
            fill: "var(--fg-3)",
            textTransform: "uppercase",
            letterSpacing: "0.04em",
          }}
        >
          matched
        </text>
      </svg>
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          gap: 6,
          flex: 1,
        }}
      >
        <LegendRow
          dotColor="var(--yes)"
          label="Matched"
          value={matched}
          valueColor="var(--yes)"
        />
        <LegendRow
          dotColor="var(--no)"
          label="Unmatched"
          value={unmatched}
          valueColor="var(--no)"
        />
      </div>
    </div>
  );
}

function LegendRow({
  dotColor,
  label,
  value,
  valueColor,
}: {
  dotColor: string;
  label: string;
  value: number;
  valueColor: string;
}) {
  return (
    <div
      style={{
        display: "flex",
        justifyContent: "space-between",
        alignItems: "center",
      }}
    >
      <span
        style={{
          fontFamily: "var(--font-sans)",
          fontSize: 12,
          color: "var(--fg-3)",
        }}
      >
        <span
          style={{
            display: "inline-block",
            width: 6,
            height: 6,
            borderRadius: "50%",
            background: dotColor,
            marginRight: 6,
          }}
        />
        {label}
      </span>
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 12,
          color: valueColor,
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {value}
      </span>
    </div>
  );
}
