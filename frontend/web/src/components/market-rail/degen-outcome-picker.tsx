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
import { useRouter } from "next/navigation";
import type { EventOutcome } from "@/lib/market-detail/use-event-group";

export function DegenOutcomePicker({
  outcomes,
  currentMarketId,
}: {
  outcomes: EventOutcome[];
  currentMarketId: number;
}) {
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    function close(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, []);

  const selected =
    outcomes.find((o) => o.marketId === currentMarketId) ?? outcomes[0];
  if (!selected) return null;

  const others = outcomes.filter((o) => o.marketId !== selected.marketId);
  const accent = "var(--yes)";

  return (
    <div ref={ref} style={{ position: "relative" }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 12,
          padding: "14px 16px",
          borderRadius: 6,
          background: "color-mix(in srgb, var(--yes) 10%, transparent)",
          border: `1px solid ${accent}`,
        }}
      >
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
          {selected.yesCents != null ? `${selected.yesCents}¢` : "—"}
        </span>
      </div>

      {others.length > 0 && (
        <>
          <button
            type="button"
            onClick={() => setOpen((o) => !o)}
            style={{
              marginTop: 6,
              width: "100%",
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              padding: "8px 14px",
              borderRadius: 4,
              background: "transparent",
              border: "1px solid var(--border-1)",
              color: "var(--fg-3)",
              fontFamily: "var(--font-sans)",
              fontSize: 11.5,
              cursor: "pointer",
            }}
          >
            <span>switch outcome ({others.length} more)</span>
            <svg
              width="10"
              height="10"
              viewBox="0 0 12 12"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
              style={{
                transform: open ? "rotate(180deg)" : "none",
                transition: "transform 120ms",
              }}
            >
              <path d="m3 4.5 3 3 3-3" />
            </svg>
          </button>
          {open && (
            <div
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
              {others.map((o) => (
                <button
                  key={o.marketId}
                  type="button"
                  onClick={() => {
                    setOpen(false);
                    router.push(`/m/${o.marketId}`);
                  }}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 10,
                    padding: "10px 12px",
                    borderRadius: 4,
                    background: "transparent",
                    border: 0,
                    cursor: "pointer",
                    textAlign: "left",
                  }}
                  onMouseEnter={(e) =>
                    (e.currentTarget.style.background = "var(--bg-2)")
                  }
                  onMouseLeave={(e) =>
                    (e.currentTarget.style.background = "transparent")
                  }
                >
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
                      color: "var(--yes)",
                      fontVariantNumeric: "tabular-nums",
                      flexShrink: 0,
                    }}
                  >
                    {o.yesCents != null ? `${o.yesCents}¢` : "—"}
                  </span>
                </button>
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}
