"use client";

/**
 * Time-window selector for the price chart. Matches `RangeBar` in
 * `frontend/handoff/data/fed-primitives.jsx:299`.
 *
 * The selected range is applied client-side by slicing `history` against
 * `Date.now() - rangeMs` before passing it to `PriceChart`.
 */

export type ChartRange = "1H" | "6H" | "1D" | "1W" | "1M" | "ALL";

const RANGES: ChartRange[] = ["1H", "6H", "1D", "1W", "1M", "ALL"];

/** Window length in ms for each range. `null` = no filter (ALL). */
export const RANGE_MS: Record<ChartRange, number | null> = {
  "1H": 60 * 60 * 1000,
  "6H": 6 * 60 * 60 * 1000,
  "1D": 24 * 60 * 60 * 1000,
  "1W": 7 * 24 * 60 * 60 * 1000,
  "1M": 30 * 24 * 60 * 60 * 1000,
  ALL: null,
};

export function ChartRangeBar({
  value,
  onChange,
}: {
  value: ChartRange;
  onChange: (r: ChartRange) => void;
}) {
  return (
    <div
      style={{
        display: "inline-flex",
        gap: 2,
        padding: 2,
        background: "var(--bg-2)",
        border: "1px solid var(--border-1)",
        borderRadius: 4,
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
              padding: "4px 9px",
              borderRadius: 3,
              border: 0,
              cursor: "pointer",
              background: active ? "var(--surface-2)" : "transparent",
              color: active ? "var(--fg-1)" : "var(--fg-3)",
              fontFamily: "var(--font-mono)",
              fontSize: 10,
              letterSpacing: "0.04em",
            }}
          >
            {r}
          </button>
        );
      })}
    </div>
  );
}
