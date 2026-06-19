"use client";

/**
 * FloatingTooltip — positions a tooltip with `position: fixed` in a portal to
 * `document.body`, so it can't be clipped by an ancestor's `overflow` (the
 * trade rail is its own scroll column, which used to chop the top/sides off
 * the pro-mode glossary + batch-bar tooltips).
 *
 * Anchors to a caller-measured `DOMRect` (the trigger's bounding box). Opens
 * above the anchor by default, flips below when there isn't room, and clamps
 * horizontally to the viewport so a near-edge trigger stays fully on screen.
 */

import type React from "react";
import { createPortal } from "react-dom";

const MARGIN = 8;

export function FloatingTooltip({
  anchor,
  width,
  align = "left",
  estHeight = 140,
  children,
  style,
}: {
  /** The trigger's bounding box (from `getBoundingClientRect()`). */
  anchor: DOMRect;
  /** Fixed tooltip width — used to clamp horizontally. */
  width: number;
  /** "left" aligns the tooltip's left edge to the anchor; "center" centers it. */
  align?: "left" | "center";
  /** Rough height, used only to decide above-vs-below placement. */
  estHeight?: number;
  children: React.ReactNode;
  style?: React.CSSProperties;
}) {
  // Only renders when a caller passes a rect, which only happens client-side on
  // hover/focus — but guard anyway so SSR never touches document/window.
  if (typeof document === "undefined") return null;

  const vw = window.innerWidth;
  const vh = window.innerHeight;

  let left =
    align === "center"
      ? anchor.left + anchor.width / 2 - width / 2
      : anchor.left;
  left = Math.max(MARGIN, Math.min(left, vw - width - MARGIN));

  // Prefer above the anchor; flip below when it would clip the viewport top.
  const above = anchor.top - estHeight - MARGIN >= 0;
  const vertical: React.CSSProperties = above
    ? { bottom: Math.max(MARGIN, vh - anchor.top + 6) }
    : { top: Math.min(vh - MARGIN, anchor.bottom + 6) };

  return createPortal(
    <div
      role="tooltip"
      style={{
        position: "fixed",
        left,
        width,
        zIndex: 1000,
        pointerEvents: "none",
        ...vertical,
        ...style,
      }}
    >
      {children}
    </div>,
    document.body,
  );
}
