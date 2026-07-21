"use client";

import type { EquityRange } from "@/lib/account/use-equity-curve";

const RANGES: EquityRange[] = ["24H", "7D", "30D", "ALL"];

export function RangeTabs({
  value,
  onChange,
}: {
  value: EquityRange;
  onChange: (r: EquityRange) => void;
}) {
  return (
    <div
      /* Four 11px labels in one track — see `.hit-target-group`. */
      className="hit-target-group"
      style={{
        display: "inline-flex",
        background: "var(--bg-2)",
        border: "1px solid var(--border-1)",
        borderRadius: 4,
        padding: 2,
        gap: 2,
      }}
    >
      {RANGES.map((r) => {
        const active = value === r;
        return (
          <button
            key={r}
            type="button"
            onClick={() => onChange(r)}
            style={{
              padding: "4px 12px",
              border: 0,
              borderRadius: 3,
              background: active ? "var(--surface-2)" : "transparent",
              color: active ? "var(--fg-1)" : "var(--fg-3)",
              fontFamily: "var(--font-mono)",
              fontSize: 11,
              letterSpacing: "var(--track-wide)",
              cursor: "pointer",
            }}
          >
            {r}
          </button>
        );
      })}
    </div>
  );
}
