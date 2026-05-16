"use client";

/**
 * Interactive per-outcome legend above the price chart. Each chip is a
 * toggle: the chips shown here are exactly the lines/bands drawn on the
 * chart. Clicking a chip removes its outcome from the chart; "+N more"
 * opens a dropdown to add hidden outcomes, up to `maxSelected` (8).
 *
 * Colors are keyed to each outcome's index in the full favourite-first
 * group, so a given outcome keeps its color whether shown or hidden — and
 * matches the chart, which uses the same `colorForOutcome`.
 */

import { useEffect, useRef, useState } from "react";
import { getCategoryColor } from "@/lib/categorize";
import type { EventOutcome } from "@/lib/market-detail/use-event-group";

// Reuse the category palette for outcome accents. Binary YES/NO use the
// semantic --yes / --no tokens.
export function colorForOutcome(o: EventOutcome, index: number): string {
  if (o.label.toLowerCase() === "yes") return "var(--yes)";
  if (o.label.toLowerCase() === "no") return "var(--no)";
  const PALETTE = ["#6FCC8A", "#E8B447", "#E89D9F", "#7E9AE8", "#5BC4E0", "#9F8FE8"];
  return PALETTE[index % PALETTE.length] ?? getCategoryColor(null);
}

export function OutcomeLegend({
  outcomes,
  selectedIds,
  onChange,
  maxSelected = 8,
}: {
  /** Full favourite-first group. */
  outcomes: EventOutcome[];
  /** marketIds currently drawn on the chart. */
  selectedIds: number[];
  onChange: (next: number[]) => void;
  maxSelected?: number;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    function close(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, []);

  const sel = new Set(selectedIds);
  const colorOf = (o: EventOutcome) => colorForOutcome(o, outcomes.indexOf(o));
  const shown = outcomes.filter((o) => sel.has(o.marketId));
  const hidden = outcomes.filter((o) => !sel.has(o.marketId));
  const atCap = shown.length >= maxSelected;
  const interactive = outcomes.length > 1;

  const remove = (id: number) => {
    if (shown.length > 1) onChange(selectedIds.filter((x) => x !== id));
  };
  const add = (id: number) => {
    if (!atCap) onChange([...selectedIds, id]);
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
      {shown.map((o) => {
        const color = colorOf(o);
        const removable = interactive && shown.length > 1;
        return (
          <button
            key={o.marketId}
            type="button"
            disabled={!removable}
            onClick={() => remove(o.marketId)}
            title={
              removable ? `${o.label} — click to hide` : o.label
            }
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 7,
              flexShrink: 0,
              padding: "3px 7px",
              borderRadius: 4,
              border: "1px solid var(--border-1)",
              background: "var(--bg-2)",
              cursor: removable ? "pointer" : "default",
              fontFamily: "var(--font-sans)",
              fontSize: 12,
              color: "var(--fg-2)",
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
            <span
              style={{ fontFamily: "var(--font-mono)", color, flexShrink: 0 }}
            >
              {o.yesCents == null ? "—" : `${o.yesCents}¢`}
            </span>
            {removable && (
              <span aria-hidden style={{ color: "var(--fg-4)", fontSize: 11, marginLeft: 1 }}>
                ✕
              </span>
            )}
          </button>
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
                      opacity: atCap ? 0.4 : 1,
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
                      title={o.label}
                    >
                      {o.shortLabel}
                    </span>
                    <span
                      style={{
                        fontFamily: "var(--font-mono)",
                        fontSize: 12,
                        color,
                        flexShrink: 0,
                      }}
                    >
                      {o.yesCents == null ? "—" : `${o.yesCents}¢`}
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
