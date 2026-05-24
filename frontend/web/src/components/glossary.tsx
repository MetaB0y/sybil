"use client";

/**
 * Inline glossary badge — a small "?" next to a term that reveals a short
 * definition on hover/focus (or tap). Ported from the handoff `Glossary`
 * (`frontend/handoff/data/fed-primitives.jsx:22-50`).
 *
 * Used in the Pro rail to explain Sybil-specific FBA jargon (indicative
 * price, IEV, imbalance, batch) without cluttering the dense layout.
 */

import { useState } from "react";

/** Term → definition. Keep copy in sync with the handoff GLOSSARY. */
export const GLOSSARY: Record<string, string> = {
  "Indicative price":
    "The price the current batch would clear at if it ran right now. Updates as new orders come in. Not final until the batch closes.",
  IEV: "Indicative Executable Volume — how much $ would actually trade at the indicative price. High IEV = a thick batch; low = thin.",
  Imbalance:
    "Net direction of unmatched orders. Buy = more demand than supply at current price; sell = the reverse. Tells you which side is leaning.",
  Batch:
    "Sybil clears all orders in fixed time windows at one uniform price. No order has time priority within a batch.",
  "Uniform clearing":
    "Every order in a batch trades at the same single price. Eliminates the “sniper’s tax” continuous order books leak to fast actors.",
  FBA: "Frequent Batch Auction. The market mechanism Sybil uses instead of a continuous limit order book.",
};

export function Glossary({
  term,
  children,
  side = "top",
}: {
  term: keyof typeof GLOSSARY | string;
  children: React.ReactNode;
  side?: "top" | "bottom";
}) {
  const [open, setOpen] = useState(false);
  const content = GLOSSARY[term] ?? "";

  return (
    <span
      style={{
        position: "relative",
        display: "inline-flex",
        alignItems: "center",
        gap: 4,
      }}
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
      onFocus={() => setOpen(true)}
      onBlur={() => setOpen(false)}
    >
      {children}
      <button
        type="button"
        tabIndex={0}
        aria-label={`What is ${term}?`}
        onClick={(e) => {
          e.preventDefault();
          setOpen((o) => !o);
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
      {open && content && (
        <span
          role="tooltip"
          style={{
            position: "absolute",
            zIndex: 50,
            [side === "top" ? "bottom" : "top"]: "calc(100% + 6px)",
            left: 0,
            width: 240,
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
      )}
    </span>
  );
}
