"use client";

import { useEffect, useRef, useState } from "react";

type Variant = "waiting" | "failed";

const COPY: Record<Variant, { trigger: string; body: string }> = {
  waiting: {
    trigger: "why am I waiting?",
    body: "Every 10s, all bets settle together at one shared price — so nobody jumps the line or beats you to it.",
  },
  failed: {
    trigger: "why failed?",
    body: "Nobody took the other side in time, so nothing was charged. Tap Bet again to retry.",
  },
};

/**
 * Small hover/tap explainer at the bottom of the Degen rail. Two variants:
 *  - `waiting` — plain-language FBA explainer (the default, shown while the
 *    bet is pending or before placing one).
 *  - `failed`  — shown after a bet finds no taker.
 *
 * The popover opens *downward* (below the trigger, which is the last element
 * in the rail) so it never overlaps the Bet button above it.
 */
export function WhyWaiting({ variant = "waiting" }: { variant?: Variant }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);
  const { trigger, body } = COPY[variant];

  useEffect(() => {
    function close(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, []);

  return (
    <div
      ref={ref}
      style={{ position: "relative", display: "flex", justifyContent: "center" }}
    >
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        style={{
          background: "transparent",
          border: 0,
          color: "var(--fg-3)",
          fontFamily: "var(--font-sans)",
          fontSize: 11.5,
          cursor: "pointer",
          display: "inline-flex",
          alignItems: "center",
          gap: 6,
        }}
      >
        <span
          style={{
            width: 14,
            height: 14,
            borderRadius: "50%",
            border: "1px solid var(--border-3)",
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            fontFamily: "var(--font-mono)",
            fontSize: 9,
            color: "var(--fg-3)",
          }}
        >
          ?
        </span>
        <span
          style={{
            textDecoration: "underline",
            textUnderlineOffset: 2,
            textDecorationColor: "var(--border-3)",
          }}
        >
          {trigger}
        </span>
      </button>
      {open && (
        <div
          style={{
            position: "absolute",
            top: "calc(100% + 8px)",
            left: 0,
            right: 0,
            background: "var(--surface-3, var(--surface-2))",
            border: "1px solid var(--border-2)",
            borderRadius: 6,
            padding: "12px 14px",
            boxShadow: "var(--shadow-popover, 0 8px 24px rgba(0,0,0,0.4))",
            zIndex: 20,
            fontFamily: "var(--font-sans)",
            fontSize: 12,
            lineHeight: "17px",
            color: "var(--fg-2)",
          }}
        >
          {body}
        </div>
      )}
    </div>
  );
}
