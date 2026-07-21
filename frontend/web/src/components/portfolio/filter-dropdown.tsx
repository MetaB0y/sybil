"use client";

/**
 * Shared pill-style filter dropdown used across the portfolio tabs (History
 * type/market/side filters, Trades market filter). A borderless trigger pill
 * that turns accent when a non-"all" value is active, opening a click-outside /
 * Escape-dismissable listbox. Extracted from `history-feed` so every tab uses
 * one implementation.
 *
 * On a phone the list is pinned to the viewport rather than to the trigger:
 * three of these sit in one row, and a 168px panel hung off the left-most
 * trigger ran clean off the side of the screen.
 */

import { useEffect, useRef, useState } from "react";
import { useCompactLayout } from "@/lib/responsive/use-compact";

export function FilterDropdown({
  value,
  onChange,
  options,
  ariaLabel,
}: {
  value: string;
  onChange: (v: string) => void;
  options: { value: string; label: string }[];
  ariaLabel: string;
}) {
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const compact = useCompactLayout();
  // Where the viewport-pinned panel hangs from, captured when it opens.
  const [anchorBottom, setAnchorBottom] = useState(0);
  const current = options.find((o) => o.value === value) ?? options[0];
  const isActive = value !== "all";

  function toggle() {
    const next = !open;
    if (next && buttonRef.current) {
      setAnchorBottom(buttonRef.current.getBoundingClientRect().bottom);
    }
    setOpen(next);
  }

  useEffect(() => {
    if (!open) return;
    function onDown(e: PointerEvent) {
      if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") setOpen(false);
    }
    // A pinned panel does not travel with its trigger, so scrolling dismisses
    // it rather than leaving it stranded mid-page.
    function onScroll() {
      setOpen(false);
    }
    document.addEventListener("pointerdown", onDown);
    document.addEventListener("keydown", onKey);
    if (compact) window.addEventListener("scroll", onScroll, true);
    return () => {
      document.removeEventListener("pointerdown", onDown);
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("scroll", onScroll, true);
    };
  }, [open, compact]);

  return (
    <div ref={wrapRef} style={{ position: "relative" }}>
      <button
        ref={buttonRef}
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel}
        onClick={toggle}
        /* An 11px mono chip. Three of them side by side on a phone, each blown
           up to the coarse-pointer 44px, made a band of boxes taller than the
           tab strip above them. See `.hit-target`. */
        className="hit-target"
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: 6,
          padding: "5px 11px",
          background: open ? "var(--surface-2)" : "var(--bg-2)",
          border: `1px solid ${isActive ? "var(--accent)" : "var(--border-1)"}`,
          // Rectangular (radius-sm) to match the markets filter buttons, not a pill.
          borderRadius: "var(--radius-sm)",
          color: isActive ? "var(--fg-1)" : "var(--fg-2)",
          fontFamily: "var(--font-mono)",
          fontSize: 11,
          letterSpacing: "var(--track-wide)",
          cursor: "pointer",
          maxWidth: 220,
        }}
      >
        <span
          style={{
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {current?.label}
        </span>
        <span
          aria-hidden
          style={{
            fontSize: 8,
            opacity: 0.7,
            transform: open ? "rotate(180deg)" : "none",
            transition: "transform var(--dur-fast) var(--ease-standard)",
          }}
        >
          ▾
        </span>
      </button>
      {open && (
        <div
          role="listbox"
          aria-label={ariaLabel}
          style={{
            ...(compact
              ? {
                  position: "fixed" as const,
                  top: anchorBottom + 6,
                  left: "var(--space-4)",
                  right: "var(--space-4)",
                  maxHeight: `calc(100dvh - ${Math.round(anchorBottom + 24)}px)`,
                }
              : {
                  position: "absolute" as const,
                  top: "calc(100% + 6px)",
                  right: 0,
                  minWidth: 168,
                  maxWidth: 280,
                  maxHeight: 300,
                }),
            zIndex: 30,
            overflowY: "auto",
            background: "var(--surface-3)",
            border: "1px solid var(--border-2)",
            borderRadius: 8,
            padding: 4,
            boxShadow: "var(--shadow-popover, 0 8px 24px rgba(0,0,0,0.4))",
          }}
        >
          {options.map((o) => (
            <DropdownOption
              key={o.value}
              label={o.label}
              selected={o.value === value}
              onClick={() => {
                onChange(o.value);
                setOpen(false);
              }}
            />
          ))}
        </div>
      )}
    </div>
  );
}

/** One option row in `FilterDropdown` — hover/selected states via local state
 *  (avoids global CSS for this scoped menu). */
function DropdownOption({
  label,
  selected,
  onClick,
}: {
  label: string;
  selected: boolean;
  onClick: () => void;
}) {
  const [hover, setHover] = useState(false);
  return (
    <button
      type="button"
      role="option"
      aria-selected={selected}
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: 8,
        width: "100%",
        padding: "6px 9px",
        background: selected
          ? "color-mix(in srgb, var(--accent) 16%, transparent)"
          : hover
            ? "var(--surface-2)"
            : "transparent",
        border: 0,
        borderRadius: 5,
        color: selected ? "var(--fg-1)" : "var(--fg-2)",
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        letterSpacing: "var(--track-wide)",
        textAlign: "left",
        cursor: "pointer",
      }}
    >
      <span
        style={{
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
      >
        {label}
      </span>
      {selected && (
        <span
          aria-hidden
          style={{ color: "var(--accent)", fontSize: 10, flexShrink: 0 }}
        >
          ✓
        </span>
      )}
    </button>
  );
}
