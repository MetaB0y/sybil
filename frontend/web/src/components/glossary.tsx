"use client";

/**
 * Inline glossary badge — a small "?" next to a term that reveals a short
 * definition on hover/focus (or tap).
 *
 * Used in the Pro rail to explain Sybil-specific FBA jargon (indicative
 * price, IEV, imbalance, batch) without cluttering the dense layout.
 */

import { useRef, useState } from "react";
import { FloatingTooltip } from "./floating-tooltip";

/** Term → definition. */
export const GLOSSARY: Record<string, string> = {
  "Indicative price":
    "The price the current batch would clear at if it ran right now. Updates as new orders come in. Not final until the batch closes.",
  "Last price":
    "The price this outcome last traded at. If it's been quiet, the most recent midpoint (mark) price.",
  // The band is a server setting (`liquidity_band_nanos`, 5¢ by default), so
  // naming a figure here would go stale the moment it's retuned.
  Liquidity:
    "Resting orders close to the current price — the depth you could trade against. Averaged over the last few batches.",
  IEV: "Indicative Executable Volume — the $ that would trade when this batch clears.",
  Imbalance:
    "Net direction of unmatched orders. Buy = more demand than supply at current price; sell = the reverse. Tells you which side is leaning.",
  Batch:
    "Sybil clears all orders in fixed time windows at one uniform price. No order has time priority within a batch.",
  "Uniform clearing":
    "Every order in a batch trades at the same single price. Eliminates the “sniper’s tax” continuous order books leak to fast actors.",
  FBA: "Frequent Batch Auction. The market mechanism Sybil uses instead of a continuous limit order book.",
  Welfare:
    "Your surplus on the fills: how much better than your limit price you traded, times quantity filled. A buy that filled below your limit — or a sell that filled above it — earns positive welfare.",
  "All-time welfare":
    "Total trader surplus across every fill since launch — for each trade, how much better than its limit price it cleared, times quantity. The cumulative value Sybil's uniform-price batches have created for traders.",
  "Portfolio value":
    "Your cash balance plus the current value of every open position, marked at the latest batch clearing price.",
  "Net deposits":
    "Everything you've deposited minus what you've withdrawn. When your portfolio line sits above it you're in profit; below it, at a loss.",
};

export function Glossary({
  term,
  children,
}: {
  term: keyof typeof GLOSSARY | string;
  /** Optional inline content the badge sits beside; omit for a standalone "?". */
  children?: React.ReactNode;
}) {
  // The trigger's bounding box, captured in the open handlers (reading layout
  // in render is disallowed). `null` = closed; a rect = open + anchored.
  const [rect, setRect] = useState<DOMRect | null>(null);
  const ref = useRef<HTMLSpanElement>(null);
  const content = GLOSSARY[term] ?? "";
  const openAt = (el: HTMLElement | null) =>
    setRect(el ? el.getBoundingClientRect() : null);

  return (
    <span
      ref={ref}
      style={{
        position: "relative",
        display: "inline-flex",
        alignItems: "center",
        gap: 4,
      }}
      onMouseEnter={(e) => openAt(e.currentTarget)}
      onMouseLeave={() => setRect(null)}
      onFocus={(e) => openAt(e.currentTarget)}
      onBlur={() => setRect(null)}
    >
      {children}
      <button
        type="button"
        tabIndex={0}
        aria-label={`What is ${term}?`}
        onClick={(e) => {
          e.preventDefault();
          setRect((r) => (r ? null : ref.current?.getBoundingClientRect() ?? null));
        }}
        style={{
          width: 13,
          height: 13,
          borderRadius: "50%",
          border: "1px solid var(--border-3)",
          background: "transparent",
          color: "var(--fg-3)",
          fontFamily: "var(--font-mono)",
          fontSize: 9,
          lineHeight: "11px",
          padding: 0,
          cursor: "help",
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          flexShrink: 0,
        }}
      >
        ?
      </button>
      {rect && content && (
        <FloatingTooltip anchor={rect} width={240} estHeight={150}>
          <span
            style={{
              display: "block",
              background: "var(--surface-3)",
              border: "1px solid var(--border-2)",
              borderRadius: 4,
              padding: "10px 12px",
              fontFamily: "var(--font-sans)",
              fontSize: 12,
              lineHeight: "17px",
              color: "var(--fg-2)",
              textTransform: "none",
              letterSpacing: "normal",
              boxShadow: "var(--shadow-popover, 0 8px 24px rgba(0,0,0,0.4))",
            }}
          >
            <span
              style={{
                display: "block",
                fontFamily: "var(--font-mono)",
                fontSize: 10,
                color: "var(--fg-3)",
                textTransform: "uppercase",
                letterSpacing: "0.04em",
                marginBottom: 4,
              }}
            >
              {term}
            </span>
            {content}
          </span>
        </FloatingTooltip>
      )}
    </span>
  );
}
