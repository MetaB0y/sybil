"use client";

/**
 * Shared pill-style filter dropdown used across the portfolio tabs (History
 * type/market/side filters, Trades market filter). A borderless trigger pill
 * that turns accent when a non-"all" value is active, opening a click-outside /
 * Escape-dismissable listbox. Extracted from `history-feed` so every tab uses
 * one implementation.
 */

import { useEffect, useRef, useState } from "react";

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
  const current = options.find((o) => o.value === value) ?? options[0];
  const isActive = value !== "all";

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
    document.addEventListener("pointerdown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <div ref={wrapRef} style={{ position: "relative" }}>
      <button
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel}
        onClick={() => setOpen((o) => !o)}
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: 6,
          padding: "5px 11px",
          background: open ? "var(--surface-2)" : "var(--bg-2)",
          border: `1px solid ${isActive ? "var(--accent)" : "var(--border-1)"}`,
          borderRadius: 999,
          color: isActive ? "var(--fg-1)" : "var(--fg-2)",
          fontFamily: "var(--font-mono)",
          fontSize: 11,
          letterSpacing: "var(--track-wide)",
          cursor: "pointer",
          maxWidth: 220,
        }}
      >
        <span
          style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
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
            position: "absolute",
            top: "calc(100% + 6px)",
            right: 0,
            zIndex: 30,
            minWidth: 168,
            maxWidth: 280,
            maxHeight: 300,
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
      <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
        {label}
      </span>
      {selected && (
        <span aria-hidden style={{ color: "var(--accent)", fontSize: 10, flexShrink: 0 }}>
          ✓
        </span>
      )}
    </button>
  );
}
