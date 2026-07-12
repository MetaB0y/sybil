"use client";

/**
 * Shared market-name search box for the portfolio tables (positions / open
 * orders / trades / history).
 *
 * Styled as a squared shell to match the global nav search (`.nav-search-shell`)
 * rather than as a pill: same 32px height, same `--radius-md`, same surface and
 * border, and the same quiet focus-brighten instead of the app-wide cyan
 * `:focus-visible` ring. Each list owns the query state and does the actual
 * substring match against its market label; the ✕ clears it.
 */

import { useId } from "react";

export function SearchField({
  value,
  onChange,
  placeholder = "Search by market…",
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
}) {
  const id = useId();
  const active = value.length > 0;
  return (
    <div className="portfolio-search-shell">
      <svg
        width="12"
        height="12"
        viewBox="0 0 16 16"
        fill="none"
        aria-hidden
        style={{ color: "var(--fg-4)", flexShrink: 0 }}
      >
        <circle cx="7" cy="7" r="4.5" stroke="currentColor" strokeWidth="1.5" />
        <line
          x1="10.6"
          y1="10.6"
          x2="14"
          y2="14"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
        />
      </svg>
      <input
        id={id}
        type="text"
        inputMode="search"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        aria-label={placeholder}
        style={{
          flex: 1,
          minWidth: 0,
          padding: "0 var(--space-2)",
          background: "transparent",
          border: 0,
          outline: "none",
          color: "var(--fg-1)",
          fontFamily: "var(--font-mono)",
          fontSize: 11,
          letterSpacing: "var(--track-wide)",
        }}
      />
      {active && (
        <button
          type="button"
          aria-label="Clear search"
          onClick={() => onChange("")}
          style={{
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
            width: 16,
            height: 16,
            padding: 0,
            border: 0,
            borderRadius: 999,
            background: "var(--fill-subtle)",
            color: "var(--fg-3)",
            fontSize: 9,
            lineHeight: 1,
            cursor: "pointer",
          }}
        >
          ✕
        </button>
      )}
    </div>
  );
}
