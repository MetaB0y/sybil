"use client";

/**
 * CategoryTabs — handoff's 12 text-link tabs. Active state is the cyan
 * underline under "All".
 *
 * Backend `category` is always null today, so only "All" is functional;
 * the other 11 tabs render dimmed + non-interactive with a tooltip. When
 * backend exposes the field, drop the `disabled` flag and wire onClick
 * to a `?category=` URL param.
 */

const CATEGORIES = [
  "All",
  "Politics",
  "Geopolitics",
  "AI",
  "Tech",
  "Economy",
  "Culture",
  "Science",
  "World",
  "Finance",
  "Business",
  "Weather",
  "Mentions",
  "Sports",
  "Crypto",
  "Commodities",
] as const;

export function CategoryTabs() {
  return (
    <nav
      aria-label="market categories"
      style={{
        display: "flex",
        alignItems: "center",
        gap: "var(--space-4)",
        minWidth: 0,
        flex: "1 1 auto",
        overflowX: "auto",
        scrollbarWidth: "none",
      }}
    >
      {CATEGORIES.map((cat) => {
        const isAll = cat === "All";
        return (
          <button
            key={cat}
            type="button"
            disabled={!isAll}
            title={
              isAll
                ? undefined
                : "category filter — wired when backend populates the field"
            }
            style={{
              position: "relative",
              flexShrink: 0,
              background: "transparent",
              border: 0,
              padding: "var(--space-2) 0",
              fontFamily: "var(--font-sans)",
              fontSize: "var(--fs-14)",
              fontWeight: 500,
              color: isAll ? "var(--fg-1)" : "var(--fg-4)",
              cursor: isAll ? "default" : "not-allowed",
            }}
          >
            {cat}
            {isAll && (
              <span
                aria-hidden
                style={{
                  position: "absolute",
                  bottom: 0,
                  left: 0,
                  right: 0,
                  height: 2,
                  background: "var(--accent)",
                  borderRadius: 1,
                }}
              />
            )}
          </button>
        );
      })}
    </nav>
  );
}
