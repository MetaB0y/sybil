"use client";

/**
 * Shared market-name search box for the portfolio tables (positions / open
 * orders / trades / history). A controlled pill input with a leading magnifier
 * and a clear (✕) button; the border goes accent while a query is active so a
 * set filter reads at a glance. Each list owns the query state and does the
 * actual substring match against its market label.
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
    <div
      style={{
        position: "relative",
        display: "inline-flex",
        alignItems: "center",
        width: 240,
        maxWidth: "100%",
      }}
    >
      <svg
        width="12"
        height="12"
        viewBox="0 0 16 16"
        fill="none"
        aria-hidden
        style={{
          position: "absolute",
          left: 11,
          color: "var(--fg-4)",
          pointerEvents: "none",
        }}
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
          width: "100%",
          padding: "6px 28px",
          background: "var(--bg-2)",
          border: `1px solid ${active ? "var(--accent)" : "var(--border-1)"}`,
          borderRadius: 999,
          color: "var(--fg-1)",
          fontFamily: "var(--font-mono)",
          fontSize: 11,
          letterSpacing: "var(--track-wide)",
          outline: "none",
        }}
      />
      {active && (
        <button
          type="button"
          aria-label="Clear search"
          onClick={() => onChange("")}
          style={{
            position: "absolute",
            right: 8,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
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
