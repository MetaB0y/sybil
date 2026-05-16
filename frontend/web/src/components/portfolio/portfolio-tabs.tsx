"use client";

export type PortfolioTab = "positions" | "orders" | "history" | "activity";

interface TabSpec {
  id: PortfolioTab;
  label: string;
}

const TABS: TabSpec[] = [
  { id: "positions", label: "Positions" },
  { id: "orders", label: "Open orders" },
  { id: "history", label: "History" },
  { id: "activity", label: "Activity" },
];

export function PortfolioTabs({
  value,
  onChange,
  counts,
}: {
  value: PortfolioTab;
  onChange: (id: PortfolioTab) => void;
  counts: Record<PortfolioTab, number>;
}) {
  return (
    <div
      role="tablist"
      style={{
        display: "flex",
        gap: "var(--space-4)",
        borderBottom: "1px solid var(--border-1)",
        alignItems: "stretch",
      }}
    >
      {TABS.map((t) => {
        const active = value === t.id;
        return (
          <button
            key={t.id}
            type="button"
            role="tab"
            aria-selected={active}
            onClick={() => onChange(t.id)}
            style={{
              background: "transparent",
              border: 0,
              padding: "10px 2px",
              borderBottom: active
                ? "2px solid var(--accent)"
                : "2px solid transparent",
              color: active ? "var(--fg-1)" : "var(--fg-3)",
              fontFamily: "var(--font-sans)",
              fontSize: 13,
              fontWeight: active ? 600 : 500,
              cursor: "pointer",
              display: "inline-flex",
              alignItems: "center",
              gap: 6,
            }}
          >
            <span>{t.label}</span>
            <span
              className="tabular"
              style={{
                fontFamily: "var(--font-mono)",
                fontSize: 11,
                color: "var(--fg-4)",
              }}
            >
              {counts[t.id]}
            </span>
          </button>
        );
      })}
    </div>
  );
}
