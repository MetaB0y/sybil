"use client";

/**
 * Standing notice that this deployment is a devnet.
 *
 * Visibility is CSS-driven, not React state: the strip is always rendered, and
 * a `data-devnet="dismissed"` attribute on <html> hides it and zeroes the
 * layout offset it reserves (`--devnet-offset`). The attribute is set before
 * first paint by the init script in `layout.tsx`, the same way the theme is, so
 * someone who has dismissed it never sees it flash and the page never jumps
 * once React hydrates.
 */

import { X } from "lucide-react";
import { DEVNET_DISMISSED_KEY } from "@/lib/devnet";

export function DevnetNotice() {
  return (
    <div className="devnet-notice" role="status">
      {/* Two wordings, swapped by width in CSS rather than by a media query in
          JS: the strip's height is reserved by the layout, and the long text
          runs to three lines on a phone. */}
      <p className="devnet-notice-text">
        <strong>Devnet.</strong>{" "}
        <span className="devnet-notice-long">
          The chain can be restarted and balances, positions and history reset
          without notice. Functionality is limited — nothing here is real money.
        </span>
        <span className="devnet-notice-short">
          Restarts and data loss are possible; functionality is limited.
        </span>
      </p>
      <button
        type="button"
        className="devnet-notice-close hit-target"
        aria-label="Dismiss devnet notice"
        onClick={() => {
          try {
            localStorage.setItem(DEVNET_DISMISSED_KEY, "1");
          } catch {
            // Private mode / storage disabled: dismissing for this page view
            // still works, it just won't be remembered.
          }
          document.documentElement.setAttribute("data-devnet", "dismissed");
        }}
      >
        <X size={14} aria-hidden />
      </button>
    </div>
  );
}
