"use client";

/**
 * Interactive per-outcome legend above the price chart. The chips shown here
 * are exactly the lines/bands drawn on the chart. Tapping a chip's body
 * switches to that outcome (navigates to its /m/[id], same as the rail's
 * outcome picker); tapping its ✕ removes it from the chart. "+N more" opens a
 * dropdown to add hidden outcomes, up to `maxSelected` (8).
 *
 * Colors are keyed to each outcome's index in the full favourite-first
 * group, so a given outcome keeps its color whether shown or hidden — and
 * matches the chart, which uses the same `colorForOutcome`.
 */

import { useEffect, useRef, useState } from "react";
import { getCategoryColor } from "@/lib/categorize";
import { useSelectOutcome } from "@/lib/market-detail/active-outcome";
import type { EventOutcome } from "@/lib/market-detail/use-event-group";

// Reuse the category palette for outcome accents. Binary YES/NO use the
// semantic --yes / --no tokens.
// Ten hue-separated accents for multi-outcome lines. The first eight — the set
// normally on the chart — are spread across the wheel so no two read alike; the
// old palette collapsed to near-identical blues/purples. Ordered so the busiest
// (favourite-first) outcomes land on the most distinct colours.
const OUTCOME_PALETTE = [
  "#5FC98A", // green
  "#E7B84A", // amber
  "#5B8AF0", // blue
  "#E86FB0", // pink
  "#3FC6D6", // cyan
  "#EE7A3C", // orange
  "#9B6BEA", // purple
  "#B9C94F", // lime
  "#E85D6B", // coral
  "#37B49E", // teal
];

export function colorForOutcome(o: EventOutcome, index: number): string {
  if (o.label.toLowerCase() === "yes") return "var(--yes)";
  if (o.label.toLowerCase() === "no") return "var(--no)";
  return OUTCOME_PALETTE[index % OUTCOME_PALETTE.length] ?? getCategoryColor(null);
}

export function OutcomeLegend({
  outcomes,
  selectedIds,
  onChange,
  maxSelected = 8,
  highlightId,
  onRowHeight,
}: {
  /** Full favourite-first group. */
  outcomes: EventOutcome[];
  /** marketIds currently drawn on the chart. */
  selectedIds: number[];
  onChange: (next: number[]) => void;
  maxSelected?: number;
  /** The chosen outcome (the market in the URL). Its chip is tinted and
   *  colour-ringed in place — chips never reorder on a switch. */
  highlightId?: number | undefined;
  /** Reports the rendered height (px) of one chip row, so the parent can
   *  reserve two rows above the chart and keep it from jumping when the legend
   *  wraps onto a second row. */
  onRowHeight?: (px: number) => void;
}) {
  const selectOutcome = useSelectOutcome();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);
  const firstChipRef = useRef<HTMLSpanElement | null>(null);

  useEffect(() => {
    function close(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, []);

  const sel = new Set(selectedIds);
  const colorOf = (o: EventOutcome) => colorForOutcome(o, outcomes.indexOf(o));
  // Chips keep the group's favourite-first order, chosen or not. Floating the
  // chosen one to the front meant every switch reshuffled the whole strip under
  // the cursor — the chip you just tapped jumped left and the rest slid right.
  // The tint + colour-matched border below is what marks the chosen outcome.
  const shown = outcomes.filter((o) => sel.has(o.marketId));
  const hidden = outcomes.filter((o) => !sel.has(o.marketId));
  const atCap = shown.length >= maxSelected;
  const interactive = outcomes.length > 1;

  // Report one chip row's rendered height so the parent can reserve two rows
  // above the chart. Re-measures when the shown set changes (and so survives
  // font loads); the value is stable, so the parent's setState bails out.
  useEffect(() => {
    if (!interactive) return;
    const h = firstChipRef.current?.offsetHeight ?? 0;
    if (h > 0) onRowHeight?.(h);
  }, [shown.length, interactive, onRowHeight]);

  const remove = (id: number) => {
    if (shown.length <= 1) return;
    if (id === highlightId) {
      // Closing the outcome you're viewing: drop its line from the chart and
      // switch the page to the first remaining outcome in the list.
      const next = shown.find((o) => o.marketId !== id);
      onChange(selectedIds.filter((x) => x !== id));
      if (next) selectOutcome(next.marketId);
      return;
    }
    onChange(selectedIds.filter((x) => x !== id));
  };
  const add = (id: number) => {
    if (atCap) return;
    onChange([...selectedIds, id]);
    // Adding an outcome's line also makes it the selected/active outcome, so the
    // rail + page switch to what you just pulled onto the chart.
    selectOutcome(id);
  };

  return (
    <div
      ref={ref}
      style={{
        position: "relative",
        display: "flex",
        flexWrap: "wrap",
        alignItems: "center",
        gap: 10,
        minWidth: 0,
      }}
    >
      {shown.map((o, i) => {
        const color = colorOf(o);
        const isHighlight = o.marketId === highlightId;
        const isClosed = o.closed;
        // Every shown outcome — including the chosen one — can be closed while
        // more than one line remains. Closing the chosen one also switches the
        // page to the next outcome (see `remove`).
        const removable = interactive && shown.length > 1;
        return (
          // Chip = a styled container holding two sibling buttons (kept
          // separate so the ✕ isn't nested inside the navigate button):
          //   · body  → switch to this outcome (navigate to its /m/[id])
          //   · ✕     → remove this line from the chart
          <span
            key={o.marketId}
            ref={i === 0 ? firstChipRef : null}
            style={{
              display: "inline-flex",
              alignItems: "center",
              flexShrink: 0,
              overflow: "hidden",
              borderRadius: 4,
              // The chosen outcome gets a subtle tint + colour-matched border so
              // you can see which one you're viewing, without shouting.
              border: isHighlight
                ? `1px solid color-mix(in srgb, ${color} 60%, var(--border-1))`
                : "1px solid var(--border-1)",
              background: isHighlight
                ? `color-mix(in srgb, ${color} 12%, var(--bg-2))`
                : "var(--bg-2)",
              fontFamily: "var(--font-sans)",
              fontSize: 12,
              fontWeight: isHighlight ? 500 : 400,
              color: isHighlight ? "var(--fg-1)" : "var(--fg-2)",
              opacity: isClosed ? 0.5 : 1,
            }}
          >
            <button
              type="button"
              onClick={() => {
                if (!isHighlight) selectOutcome(o.marketId);
              }}
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 7,
                padding: removable ? "3px 4px 3px 7px" : "3px 7px",
                border: 0,
                background: "transparent",
                cursor: isHighlight ? "default" : "pointer",
                font: "inherit",
                fontWeight: "inherit",
                color: "inherit",
              }}
            >
              <span
                aria-hidden
                style={{ width: 8, height: 8, background: color, borderRadius: 1, flexShrink: 0 }}
              />
              <span
                style={{
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                  maxWidth: 150,
                }}
              >
                {o.shortLabel}
              </span>
              {/* Price now reads off each line's right-edge tag on the chart;
                  the chip only flags a resolved outcome. */}
              {isClosed && (
                <span
                  style={{
                    fontFamily: "var(--font-mono)",
                    fontSize: 11,
                    color: "var(--fg-4)",
                    flexShrink: 0,
                  }}
                >
                  closed
                </span>
              )}
            </button>
            {removable && (
              <button
                type="button"
                onClick={() => remove(o.marketId)}
                aria-label={
                  isHighlight
                    ? `Remove ${o.label} and switch outcome`
                    : `Remove ${o.label} from chart`
                }
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  padding: "3px 7px 3px 3px",
                  border: 0,
                  background: "transparent",
                  cursor: "pointer",
                  color: "var(--fg-4)",
                  fontSize: 11,
                  lineHeight: 1,
                }}
                onMouseEnter={(e) =>
                  (e.currentTarget.style.color = "var(--fg-2)")
                }
                onMouseLeave={(e) =>
                  (e.currentTarget.style.color = "var(--fg-4)")
                }
              >
                ✕
              </button>
            )}
          </span>
        );
      })}

      {interactive && hidden.length > 0 && (
        <>
          <button
            type="button"
            onClick={() => setOpen((o) => !o)}
            className="text-mono"
            style={{
              flexShrink: 0,
              padding: "4px 8px",
              borderRadius: 4,
              border: "1px dashed var(--border-2)",
              background: "transparent",
              color: "var(--fg-3)",
              fontSize: 11,
              cursor: "pointer",
            }}
          >
            + {hidden.length} more
          </button>
          {open && (
            <div
              style={{
                position: "absolute",
                top: "calc(100% + 6px)",
                left: 0,
                zIndex: 30,
                minWidth: 220,
                maxHeight: 280,
                overflowY: "auto",
                background: "var(--surface-2)",
                border: "1px solid var(--border-2)",
                borderRadius: 6,
                padding: 4,
                boxShadow: "var(--shadow-popover, 0 8px 24px rgba(0,0,0,0.4))",
              }}
            >
              <div
                className="text-mono"
                style={{
                  padding: "6px 10px 4px",
                  fontSize: 9,
                  color: "var(--fg-4)",
                  textTransform: "uppercase",
                  letterSpacing: "0.04em",
                }}
              >
                {atCap ? `max ${maxSelected} shown` : "add to chart"}
              </div>
              {hidden.map((o) => {
                const color = colorOf(o);
                const isClosed = o.closed;
                return (
                  <button
                    key={o.marketId}
                    type="button"
                    disabled={atCap}
                    onClick={() => add(o.marketId)}
                    style={{
                      width: "100%",
                      display: "flex",
                      alignItems: "center",
                      gap: 8,
                      padding: "8px 10px",
                      borderRadius: 4,
                      background: "transparent",
                      border: 0,
                      cursor: atCap ? "not-allowed" : "pointer",
                      opacity: atCap ? 0.4 : isClosed ? 0.5 : 1,
                      textAlign: "left",
                    }}
                    onMouseEnter={(e) => {
                      if (!atCap) e.currentTarget.style.background = "var(--bg-2)";
                    }}
                    onMouseLeave={(e) =>
                      (e.currentTarget.style.background = "transparent")
                    }
                  >
                    <span
                      aria-hidden
                      style={{ width: 8, height: 8, background: color, borderRadius: 1, flexShrink: 0 }}
                    />
                    <span
                      style={{
                        flex: 1,
                        minWidth: 0,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                        fontFamily: "var(--font-sans)",
                        fontSize: 13,
                        color: "var(--fg-1)",
                      }}
                    >
                      {o.shortLabel}
                    </span>
                    <span
                      style={{
                        fontFamily: "var(--font-mono)",
                        fontSize: 12,
                        color: isClosed ? "var(--fg-4)" : color,
                        flexShrink: 0,
                      }}
                    >
                      {isClosed
                        ? "closed"
                        : o.yesCents == null
                          ? "—"
                          : `${o.yesCents}¢`}
                    </span>
                  </button>
                );
              })}
            </div>
          )}
        </>
      )}
    </div>
  );
}
