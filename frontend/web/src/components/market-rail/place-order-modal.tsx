"use client";

/**
 * Place-order modal (SYB-54) — a focused, full-screen dialog launched from the
 * market detail page. It reuses the same `BuyBox` order form the Pro rail
 * renders inline, so there is ONE order-entry surface: side (YES/NO), $ / share
 * size, limit price, time-in-force (GTC / IOC / GTD), the live clearing
 * estimate, the collateral guard, signed submission, and the accepted receipt
 * (order-id + clearing block) all live in `BuyBox`.
 *
 * Shell conventions mirror `components/auth/connect-modal.tsx`: a body portal,
 * an overlay that closes on backdrop click, Esc-to-close, and CSS-token styling.
 */

import { useEffect } from "react";
import { createPortal } from "react-dom";
import { useEventGroup } from "@/lib/market-detail/use-event-group";
import { BuyBox } from "./buy-box";

export function PlaceOrderModal({
  marketId,
  open,
  onClose,
}: {
  marketId: number;
  open: boolean;
  onClose: () => void;
}) {
  useEffect(() => {
    if (!open) return;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => {
      document.body.style.overflow = previousOverflow;
      window.removeEventListener("keydown", onKey);
    };
  }, [open, onClose]);

  if (!open || typeof document === "undefined") return null;

  return createPortal(
    <div
      className="place-order-overlay"
      role="dialog"
      aria-modal="true"
      aria-label="Place order"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      style={{
        position: "fixed",
        inset: 0,
        background: "var(--overlay)",
        backdropFilter: "blur(6px)",
        WebkitBackdropFilter: "blur(6px)",
        zIndex: 100,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        padding: "var(--space-5)",
      }}
    >
      <ModalBody marketId={marketId} onClose={onClose} />
    </div>,
    document.body,
  );
}

function ModalBody({
  marketId,
  onClose,
}: {
  marketId: number;
  onClose: () => void;
}) {
  const { group, isPending } = useEventGroup(marketId);
  const selected = group
    ? group.outcomes.find((o) => o.marketId === group.currentMarketId) ??
      group.outcomes[0]
    : undefined;
  const closed = selected?.closed === true;

  return (
    <div
      onClick={(e) => e.stopPropagation()}
      className="no-scrollbar place-order-sheet"
      style={{
        width: "100%",
        maxWidth: 420,
        maxHeight: "calc(100dvh - 2 * var(--space-5))",
        overflowY: "auto",
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: 12,
        boxShadow: "0 20px 60px rgba(0,0,0,0.4)",
        fontFamily: "var(--font-sans)",
      }}
    >
      <div
        className="place-order-sheet-header"
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          padding: "14px 18px",
          borderBottom: "1px solid var(--border-1)",
          position: "sticky",
          top: 0,
          background: "var(--surface-1)",
          zIndex: 1,
        }}
      >
        <div style={{ display: "flex", flexDirection: "column", gap: 2, minWidth: 0 }}>
          <div
            style={{
              fontFamily: "var(--font-display)",
              fontWeight: 700,
              fontSize: 16,
              color: "var(--fg-1)",
              letterSpacing: "var(--track-tight)",
              textTransform: "uppercase",
            }}
          >
            Place order
          </div>
          {selected && (
            <span
              style={{
                fontFamily: "var(--font-mono)",
                fontSize: 11,
                color: "var(--fg-3)",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              {selected.shortLabel}
            </span>
          )}
        </div>
        <button
          className="place-order-close"
          type="button"
          onClick={onClose}
          aria-label="Close"
          style={{
            background: "transparent",
            border: 0,
            color: "var(--fg-3)",
            fontSize: 20,
            cursor: "pointer",
            padding: 0,
            lineHeight: 1,
            flexShrink: 0,
          }}
        >
          ×
        </button>
      </div>

      <div className="place-order-sheet-body" style={{ padding: "16px 18px 18px" }}>
        {isPending && (
          <div
            style={{
              padding: "24px 12px",
              color: "var(--fg-3)",
              fontFamily: "var(--font-mono)",
              fontSize: 11,
              textAlign: "center",
            }}
          >
            loading market…
          </div>
        )}
        {selected && closed && (
          <div
            className="text-mono"
            style={{
              padding: "20px 16px",
              borderRadius: "var(--radius-md)",
              background: "var(--surface-2)",
              border: "1px solid var(--border-1)",
              color: "var(--fg-3)",
              fontSize: 12,
              textAlign: "center",
            }}
          >
            This market has closed. Trading is disabled.
          </div>
        )}
        {selected && !closed && (
          <BuyBox outcome={selected} requireConfirmation />
        )}
      </div>
    </div>
  );
}
