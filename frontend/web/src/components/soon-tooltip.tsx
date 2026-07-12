"use client";

import { useState, type MouseEvent } from "react";
import { createPortal } from "react-dom";

/**
 * Floating "soon" hint anchored just above the cursor — the shared "coming
 * soon" affordance for controls that aren't wired to the backend yet (the
 * Comments tab on the event Activity switcher, the Propose-resolution button).
 * Kept in one place so every one of them animates in identically.
 */
export function SoonTooltip({ x, y }: { x: number; y: number }) {
  return (
    <div
      role="tooltip"
      aria-hidden
      style={{
        position: "fixed",
        left: x,
        top: y - 14,
        transform: "translate(-50%, -100%)",
        pointerEvents: "none",
        zIndex: 100,
        padding: "3px 7px",
        background: "var(--surface-2)",
        border: "1px solid var(--border-2)",
        borderRadius: "var(--radius-sm)",
        boxShadow: "0 6px 18px rgba(0,0,0,0.35)",
        fontFamily: "var(--font-mono)",
        fontSize: "9px",
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
        color: "var(--accent)",
        whiteSpace: "nowrap",
        animation: "sybil-tooltip-in var(--dur-fast) var(--ease-standard)",
      }}
    >
      soon
    </div>
  );
}

/**
 * Hover wiring for `SoonTooltip`: spread `handlers` onto the disabled trigger
 * and render `tooltip` alongside it. The hint is portaled to <body> so it
 * escapes any overflow/stacking context, and follows the cursor while hovered.
 * `hovered` is returned so the trigger can also shift its own colour/opacity.
 */
export function useSoonTooltip() {
  const [hovered, setHovered] = useState(false);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  const trackCursor = (e: MouseEvent) => setPos({ x: e.clientX, y: e.clientY });

  const handlers = {
    onMouseEnter: (e: MouseEvent) => {
      setHovered(true);
      trackCursor(e);
    },
    onMouseMove: trackCursor,
    onMouseLeave: () => {
      setHovered(false);
      setPos(null);
    },
  };

  const tooltip =
    hovered && pos && typeof document !== "undefined"
      ? createPortal(<SoonTooltip x={pos.x} y={pos.y} />, document.body)
      : null;

  return { hovered, handlers, tooltip };
}
