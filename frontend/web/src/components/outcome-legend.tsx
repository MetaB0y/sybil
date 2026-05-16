"use client";

/**
 * Per-outcome strip rendered above the price chart — colored swatch · label ·
 * YES cents, one item per outcome. Matches `OutcomeLegend` in
 * `frontend/handoff/data/fed-primitives.jsx:281`.
 *
 * Two things keep it compact for strike-ladder events (20+ outcomes):
 *  - labels are stripped to what differs between siblings ("(HIGH) $105", not
 *    the full "Will Crude Oil (CL) hit (HIGH) $105 by end of June?");
 *  - only the first `VISIBLE_LIMIT` outcomes (sorted favourite-first) render
 *    inline — the rest collapse into a "+N more" marker. The full labelled
 *    list lives in the rail's outcome picker.
 */

import { getCategoryColor } from "@/lib/categorize";
import type { EventOutcome } from "@/lib/market-detail/use-event-group";

/**
 * Outcomes shown inline before the rest collapse into "+N more". Kept low so
 * every visible item can render its full (un-truncated) short label on one row
 * at desktop widths.
 */
const VISIBLE_LIMIT = 4;

// Reuse the category palette for outcome accents — same pattern as the
// markets index legend. Binary YES/NO use the semantic --yes / --no tokens.
export function colorForOutcome(o: EventOutcome, index: number): string {
  if (o.label.toLowerCase() === "yes") return "var(--yes)";
  if (o.label.toLowerCase() === "no") return "var(--no)";
  const PALETTE = ["#6FCC8A", "#E8B447", "#E89D9F", "#7E9AE8", "#5BC4E0", "#9F8FE8"];
  return PALETTE[index % PALETTE.length] ?? getCategoryColor(null);
}

export function OutcomeLegend({ outcomes }: { outcomes: EventOutcome[] }) {
  const shortLabels = outcomes.map((o) => o.shortLabel);
  const hidden = Math.max(0, outcomes.length - VISIBLE_LIMIT);

  return (
    <div
      style={{
        display: "flex",
        flexWrap: "nowrap",
        alignItems: "center",
        gap: 16,
        minWidth: 0,
        overflow: "hidden",
      }}
    >
      {outcomes.slice(0, VISIBLE_LIMIT).map((o, i) => {
        const color = colorForOutcome(o, i);
        return (
          <span
            key={o.marketId}
            title={o.label}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 7,
              flexShrink: 0,
              fontFamily: "var(--font-sans)",
              fontSize: 12,
              color: "var(--fg-2)",
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
                maxWidth: 150,
              }}
            >
              {shortLabels[i] ?? o.label}
            </span>
            <span
              className="tabular"
              style={{
                fontFamily: "var(--font-mono)",
                color,
                flexShrink: 0,
              }}
            >
              {o.yesCents == null ? "—" : `${o.yesCents}¢`}
            </span>
          </span>
        );
      })}

      {hidden > 0 && (
        <span
          className="text-mono"
          title={`${hidden} more outcome${hidden === 1 ? "" : "s"} — full list in the outcome picker`}
          style={{
            flexShrink: 0,
            fontSize: 11,
            color: "var(--fg-3)",
            whiteSpace: "nowrap",
          }}
        >
          +{hidden} more
        </span>
      )}
    </div>
  );
}
