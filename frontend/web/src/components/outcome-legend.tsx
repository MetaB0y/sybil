"use client";

/**
 * Per-outcome legend strip rendered above the price chart. For each outcome:
 * colored swatch · label · YES cents · 24h Δ%. Matches `OutcomeLegend` in
 * `frontend/handoff/data/fed-primitives.jsx:281`.
 *
 * The 24h Δ% is MOCKED via `mockDelta` (`lib/mock.ts:33`) — OPEN_QUESTIONS #3
 * tracks the real backend rollup. The current YES price is real.
 */

import { MockValue } from "@/components/mock-value";
import { getCategoryColor } from "@/lib/categorize";
import type { EventOutcome } from "@/lib/market-detail/use-event-group";

// Reuse the category palette for outcome accents — same pattern as the
// markets index legend. For binary markets the YES swatch uses --yes,
// NO uses --no.
function colorForOutcome(o: EventOutcome, index: number): string {
  if (o.label.toLowerCase() === "yes") return "var(--yes)";
  if (o.label.toLowerCase() === "no") return "var(--no)";
  // Cycle through the category palette deterministically by index.
  const PALETTE = ["#6FCC8A", "#E8B447", "#E89D9F", "#7E9AE8", "#5BC4E0", "#9F8FE8"];
  return PALETTE[index % PALETTE.length] ?? getCategoryColor(null);
}

export function OutcomeLegend({ outcomes }: { outcomes: EventOutcome[] }) {
  return (
    <div
      style={{
        display: "flex",
        flexWrap: "wrap",
        gap: 18,
        alignItems: "center",
        rowGap: 6,
      }}
    >
      {outcomes.map((o, i) => {
        const color = colorForOutcome(o, i);
        const deltaPositive = o.delta24Cents >= 0;
        return (
          <span
            key={o.marketId}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 8,
              fontFamily: "var(--font-sans)",
              fontSize: 12,
              color: "var(--fg-2)",
              minWidth: 0,
            }}
          >
            <span
              aria-hidden
              style={{
                width: 8,
                height: 8,
                background: color,
                borderRadius: 1,
                flexShrink: 0,
              }}
            />
            <span
              style={{
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
                maxWidth: 160,
              }}
              title={o.label}
            >
              {o.label}
            </span>
            <span
              className="tabular"
              style={{
                fontFamily: "var(--font-mono)",
                color,
              }}
            >
              {o.yesCents == null ? "—" : `${o.yesCents}¢`}
            </span>
            <MockValue hint="24h delta — no backend rollup (OPEN_QUESTIONS #3)">
              <span
                style={{
                  fontFamily: "var(--font-mono)",
                  fontSize: 10,
                  color: deltaPositive ? "var(--yes)" : "var(--no)",
                }}
              >
                {deltaPositive ? "+" : ""}
                {o.delta24Cents}¢
              </span>
            </MockValue>
          </span>
        );
      })}
    </div>
  );
}
