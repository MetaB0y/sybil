"use client";

export type PortfolioTab = "positions" | "orders" | "trades" | "history";

interface TabSpec {
  id: PortfolioTab;
  label: string;
}

const TABS: TabSpec[] = [
  { id: "positions", label: "Positions" },
  { id: "orders", label: "Open orders" },
  { id: "trades", label: "Trades" },
  { id: "history", label: "History" },
];

export function PortfolioTabs({
  value,
  onChange,
  counts,
  retentionLimited = false,
}: {
  value: PortfolioTab;
  onChange: (id: PortfolioTab) => void;
  counts: Record<PortfolioTab, number>;
  retentionLimited?: boolean;
}) {
  return (
    <div
      role="tablist"
      aria-label="Portfolio sections"
      className="portfolio-tabs"
      style={{
        display: "flex",
        gap: "var(--space-4)",
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
              // "Open orders" broke over two lines on a phone and made the
              // whole strip twice as tall. The strip scrolls sideways instead.
              whiteSpace: "nowrap",
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
              {retentionLimited && (t.id === "trades" || t.id === "history")
                ? `≥${counts[t.id]}`
                : counts[t.id]}
            </span>
          </button>
        );
      })}
    </div>
  );
}
