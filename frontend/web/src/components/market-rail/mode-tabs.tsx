"use client";

import type { RailMode } from "./use-rail-mode";

const MODES: { id: RailMode; label: string }[] = [
  { id: "degen", label: "Lite" },
  { id: "pro", label: "Pro" },
];

/** Segmented control at the top of the market detail right rail. */
export function ModeTabs({
  value,
  onChange,
}: {
  value: RailMode;
  onChange: (m: RailMode) => void;
}) {
  return (
    <div
      role="tablist"
      style={{
        display: "grid",
        gridTemplateColumns: "1fr 1fr",
        gap: 4,
        padding: 4,
        background: "var(--bg-2)",
        border: "1px solid var(--border-1)",
        borderRadius: 6,
      }}
    >
      {MODES.map((m) => {
        const active = value === m.id;
        return (
          <button
            key={m.id}
            type="button"
            role="tab"
            aria-selected={active}
            onClick={() => onChange(m.id)}
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              minHeight: 44,
              padding: "8px 6px",
              borderRadius: 4,
              border: 0,
              cursor: "pointer",
              background: active ? "var(--surface-2)" : "transparent",
              boxShadow: active ? "inset 0 0 0 1px var(--border-3)" : "none",
              color: active ? "var(--fg-1)" : "var(--fg-3)",
              transition: "background 120ms, color 120ms, box-shadow 120ms",
              fontFamily: "var(--font-sans)",
            }}
          >
            <span style={{ fontSize: 13, fontWeight: 600, lineHeight: 1 }}>
              {m.label}
            </span>
          </button>
        );
      })}
    </div>
  );
}
