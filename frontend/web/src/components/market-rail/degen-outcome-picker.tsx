"use client";

/**
 * Compact outcome picker for the Degen rail. The currently selected outcome
 * is shown big; the rest collapse into a dropdown. Matches
 * `DegenOutcomePicker` in `fed-right-rail-modes.jsx:90`.
 *
 * For binary markets (single outcome) the dropdown is hidden — there's
 * nothing to switch to.
 */

import { useEffect, useRef, useState } from "react";
import { colorForOutcome } from "@/components/outcome-legend";
import { formatCentsPrecise } from "@/lib/format/nanos";
import { useSelectOutcome } from "@/lib/market-detail/active-outcome";
import type { EventOutcome } from "@/lib/market-detail/use-event-group";

export function DegenOutcomePicker({
  outcomes,
  currentMarketId,
}: {
  outcomes: EventOutcome[];
  currentMarketId: number;
}) {
  const selectOutcome = useSelectOutcome();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    function close(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") setOpen(false);
    }
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", onKey);
    };
  }, []);

  const selectedIndex = Math.max(
    0,
    outcomes.findIndex((o) => o.marketId === currentMarketId),
  );
  const selected = outcomes[selectedIndex] ?? outcomes[0];
  if (!selected) return null;

  const others = outcomes
    .map((o, i) => ({ o, i }))
    .filter(({ o }) => o.marketId !== selected.marketId)
    // Open outcomes first, by price (highest first); closed outcomes last,
    // most-recently-closed first. `i` is kept so colors stay group-stable.
    .sort((a, b) => {
      if (a.o.closed !== b.o.closed) return a.o.closed ? 1 : -1;
      if (!a.o.closed) return (b.o.yesCents ?? -1) - (a.o.yesCents ?? -1);
      return (b.o.endDateMs ?? 0) - (a.o.endDateMs ?? 0);
    });
  const accent = colorForOutcome(selected, selectedIndex);
  const interactive = others.length > 0;

  const boxStyle: React.CSSProperties = {
    display: "flex",
    alignItems: "center",
    gap: 12,
    width: "100%",
    padding: "14px 16px",
    borderRadius: 6,
    background: `color-mix(in srgb, ${accent} 10%, transparent)`,
    border: `1px solid ${accent}`,
    textAlign: "left",
    cursor: interactive ? "pointer" : "default",
  };

  const boxContent = (
    <>
      <span
        aria-hidden
        style={{
          width: 14,
          height: 14,
          borderRadius: "50%",
          border: `2px solid ${accent}`,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          flexShrink: 0,
        }}
      >
        <span
          style={{
            width: 6,
            height: 6,
            borderRadius: "50%",
            background: accent,
          }}
        />
      </span>
      <span
        style={{
          flex: 1,
          minWidth: 0,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          fontFamily: "var(--font-sans)",
          fontSize: 15,
          fontWeight: 600,
          color: "var(--fg-1)",
        }}
        title={selected.label}
      >
        {selected.shortLabel}
      </span>
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 18,
          fontWeight: 600,
          color: accent,
          fontVariantNumeric: "tabular-nums",
          flexShrink: 0,
        }}
      >
        {selected.yesPriceNanos != null ? formatCentsPrecise(selected.yesPriceNanos) : "—"}
      </span>
      {interactive && (
        <svg
          aria-hidden
          width="12"
          height="12"
          viewBox="0 0 12 12"
          fill="none"
          stroke={accent}
          strokeWidth="1.5"
          style={{
            flexShrink: 0,
            transform: open ? "rotate(180deg)" : "none",
            transition: "transform 120ms",
          }}
        >
          <path d="m3 4.5 3 3 3-3" />
        </svg>
      )}
    </>
  );

  return (
    <div ref={ref} style={{ position: "relative" }}>
      {interactive ? (
        <button
          type="button"
          onClick={() => setOpen((o) => !o)}
          aria-haspopup="listbox"
          aria-expanded={open}
          style={{
            ...boxStyle,
            outlineOffset: 2,
          }}
          onFocus={(e) => {
            e.currentTarget.style.outline = `2px solid ${accent}`;
          }}
          onBlur={(e) => {
            e.currentTarget.style.outline = "none";
          }}
        >
          {boxContent}
        </button>
      ) : (
        <div style={boxStyle}>{boxContent}</div>
      )}

      {interactive && (
        <>
          {open && (
            <div
              role="listbox"
              style={{
                position: "absolute",
                top: "calc(100% + 4px)",
                left: 0,
                right: 0,
                zIndex: 30,
                background: "var(--surface-2)",
                border: "1px solid var(--border-2)",
                borderRadius: 6,
                padding: 4,
                boxShadow: "var(--shadow-popover, 0 8px 24px rgba(0,0,0,0.4))",
                display: "flex",
                flexDirection: "column",
                gap: 2,
                maxHeight: 280,
                overflowY: "auto",
              }}
            >
              {others.map(({ o, i }) => {
                const color = colorForOutcome(o, i);
                const isClosed = o.closed;
                return (
                <button
                  key={o.marketId}
                  type="button"
                  role="option"
                  aria-selected={false}
                  disabled={isClosed}
                  onClick={() => {
                    if (isClosed) return;
                    setOpen(false);
                    selectOutcome(o.marketId);
                  }}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 10,
                    padding: "10px 12px",
                    borderRadius: 4,
                    background: "transparent",
                    border: 0,
                    cursor: isClosed ? "not-allowed" : "pointer",
                    opacity: isClosed ? 0.5 : 1,
                    textAlign: "left",
                  }}
                  onMouseEnter={(e) => {
                    if (!isClosed) e.currentTarget.style.background = "var(--bg-2)";
                  }}
                  onMouseLeave={(e) =>
                    (e.currentTarget.style.background = "transparent")
                  }
                >
                  <span
                    aria-hidden
                    style={{
                      width: 8,
                      height: 8,
                      borderRadius: "50%",
                      background: color,
                      flexShrink: 0,
                    }}
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
                      fontFamily: "var(--font-sans)",
                      fontSize: 13,
                      fontWeight: 600,
                      color: isClosed ? "var(--fg-4)" : color,
                      fontVariantNumeric: "tabular-nums",
                      flexShrink: 0,
                    }}
                  >
                    {isClosed
                      ? "closed"
                      : o.yesCents != null
                        ? `${o.yesCents}¢`
                        : "—"}
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
