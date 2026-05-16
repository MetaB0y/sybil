"use client";

import { useEffect, useRef, useState } from "react";

/**
 * "Why am I waiting?" — small hover/tap popover explaining FBA in plain
 * language. Matches `WhyWaiting` in `fed-right-rail-modes.jsx:250`.
 *
 * The handoff prototype mentions "every 60 seconds"; Sybil's actual cadence
 * is 2s. Copy reflects the real cadence.
 */
export function WhyWaiting() {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);

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
          why am I waiting?
        </span>
      </button>
      {open && (
        <div
          style={{
            position: "absolute",
            bottom: "calc(100% + 8px)",
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
          <div
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: 9.5,
              color: "var(--fg-3)",
              textTransform: "uppercase",
              letterSpacing: "0.06em",
              marginBottom: 6,
            }}
          >
            frequent batch auction
          </div>
          Every 2 seconds, every pending order settles together at one fair
          price — a whale can&apos;t jump the queue and bots can&apos;t snipe
          you. Your order joins the next batch and clears with everyone
          else&apos;s.
          <div
            style={{
              marginTop: 8,
              paddingTop: 8,
              borderTop: "1px solid var(--border-1)",
              fontSize: 11,
              color: "var(--fg-3)",
            }}
          >
            tl;dr — same price for everyone, no front-running.
          </div>
        </div>
      )}
    </div>
  );
}
