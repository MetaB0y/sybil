"use client";

/**
 * Pro-mode "pick an outcome" radio list. Matches the picker block in
 * `V2BatchTheater` ProRail (`fed-variations.jsx:154`).
 *
 * For binary single-market events the list collapses to one row — keep it
 * visible for consistency.
 */

import { useRouter } from "next/navigation";
import { colorForOutcome } from "@/components/outcome-legend";
import type { EventOutcome } from "@/lib/market-detail/use-event-group";

export function OutcomeRadioList({
  outcomes,
  currentMarketId,
}: {
  outcomes: EventOutcome[];
  currentMarketId: number;
}) {
  const router = useRouter();

  return (
    <div
      style={{
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: 8,
        padding: "14px 16px",
        display: "flex",
        flexDirection: "column",
        gap: 6,
      }}
    >
      <div
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 10,
          color: "var(--fg-3)",
          textTransform: "uppercase",
          letterSpacing: "0.04em",
          marginBottom: 6,
        }}
      >
        pick an outcome
      </div>
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          gap: 6,
          maxHeight: 320,
          overflowY: "auto",
          paddingRight: outcomes.length > 6 ? 2 : 0,
        }}
      >
        {outcomes.map((o, i) => {
          const active = o.marketId === currentMarketId;
          const color = colorForOutcome(o, i);
          return (
            <button
              key={o.marketId}
              type="button"
              onClick={() => {
                if (!active) router.push(`/m/${o.marketId}`);
              }}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 10,
                padding: "10px 12px",
                borderRadius: 4,
                background: active ? "var(--bg-2)" : "transparent",
                border: `1px solid ${active ? color : "var(--border-1)"}`,
                cursor: active ? "default" : "pointer",
                textAlign: "left",
                fontFamily: "var(--font-sans)",
                transition: "border-color 120ms, background 120ms",
              }}
            >
              <span
                aria-hidden
                style={{
                  width: 12,
                  height: 12,
                  borderRadius: "50%",
                  border: `1.5px solid ${active ? color : "var(--border-3)"}`,
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  flexShrink: 0,
                }}
              >
                {active && (
                  <span
                    style={{
                      width: 5,
                      height: 5,
                      borderRadius: "50%",
                      background: color,
                    }}
                  />
                )}
              </span>
              <span
                style={{
                  flex: 1,
                  minWidth: 0,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
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
                  fontSize: 14,
                  fontWeight: 600,
                  color,
                  fontVariantNumeric: "tabular-nums",
                  flexShrink: 0,
                }}
              >
                {o.yesCents == null ? "—" : `${o.yesCents}¢`}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
