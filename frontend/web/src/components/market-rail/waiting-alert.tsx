"use client";

import { useEffect, useRef, useState } from "react";

/**
 * Compact "in-flight bet" alert shown below the Bet button while a degen bet is
 * being settled (tracker phase === "tracking"). A pulsing dot + short status
 * line "Finding the best quote for you…" with an ⓘ info affordance that toggles
 * a tiny popover explaining the FBA batch. The fuller "why am I waiting?" copy
 * now lives in that tooltip rather than an always-visible bottom link.
 */
export function WaitingAlert() {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    function onPointer(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") setOpen(false);
    }
    document.addEventListener("mousedown", onPointer);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onPointer);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <div
      ref={ref}
      role="status"
      style={{
        position: "relative",
        display: "flex",
        alignItems: "center",
        gap: "var(--space-3)",
        background: "var(--surface-2)",
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-lg)",
        padding: "var(--space-3) var(--space-4)",
      }}
    >
      <style>{`
        @keyframes waiting-alert-pulse {
          0% { transform: scale(1); opacity: 1; }
          50% { transform: scale(1.5); opacity: 0.4; }
          100% { transform: scale(1); opacity: 1; }
        }
      `}</style>
      <span
        aria-hidden
        style={{
          width: 8,
          height: 8,
          borderRadius: "50%",
          background: "var(--accent)",
          flexShrink: 0,
          animation: "waiting-alert-pulse 1.4s ease-in-out infinite",
        }}
      />
      <span
        style={{
          flex: 1,
          fontFamily: "var(--font-sans)",
          fontSize: 13,
          color: "var(--fg-2)",
        }}
      >
        Finding the best quote for you…
      </span>
      <button
        type="button"
        aria-label="Why am I waiting?"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        style={{
          width: 16,
          height: 16,
          borderRadius: "50%",
          border: "1px solid var(--border-3)",
          background: "transparent",
          color: "var(--fg-3)",
          fontFamily: "var(--font-mono)",
          fontSize: 10,
          lineHeight: "14px",
          padding: 0,
          cursor: "pointer",
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          flexShrink: 0,
        }}
      >
        ?
      </button>
      {open && (
        <div
          role="tooltip"
          style={{
            position: "absolute",
            top: "calc(100% + var(--space-2))",
            left: 0,
            right: 0,
            background: "var(--surface-3, var(--surface-2))",
            border: "1px solid var(--border-2)",
            borderRadius: "var(--radius-md)",
            padding: "var(--space-3) var(--space-4)",
            boxShadow: "var(--shadow-popover, 0 8px 24px rgba(0,0,0,0.4))",
            zIndex: 20,
            fontFamily: "var(--font-sans)",
            fontSize: 12,
            lineHeight: "17px",
            color: "var(--fg-2)",
          }}
        >
          Every few seconds, all bets clear at one price. Same price for
          everyone in the batch.
        </div>
      )}
    </div>
  );
}
