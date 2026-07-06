"use client";

export type Side = "YES" | "NO";

/**
 * Big segmented YES/NO toggle. Matches `YesNoToggle` in
 * `fed-right-rail-modes.jsx:161`.
 */
export function YesNoToggle({
  value,
  onChange,
}: {
  value: Side;
  onChange: (s: Side) => void;
}) {
  return (
    <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 6 }}>
      {(
        [
          { id: "YES", label: "Yes", color: "var(--yes)" },
          { id: "NO", label: "No", color: "var(--no)" },
        ] as const
      ).map((s) => {
        const active = value === s.id;
        return (
          <button
            key={s.id}
            type="button"
            onClick={() => onChange(s.id)}
            style={{
              minHeight: 48,
              padding: "14px 0",
              borderRadius: 6,
              cursor: "pointer",
              background: active ? s.color : "var(--bg-2)",
              border: `1px solid ${active ? s.color : "var(--border-1)"}`,
              color: active ? "var(--fg-on-accent)" : "var(--fg-2)",
              fontFamily: "var(--font-sans)",
              fontSize: 17,
              fontWeight: 700,
              letterSpacing: "-0.005em",
              transition: "background 120ms, color 120ms, border-color 120ms",
            }}
          >
            {s.label}
          </button>
        );
      })}
    </div>
  );
}
