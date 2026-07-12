"use client";

/**
 * One-row portfolio toolbar: the tab strip on the left, the search box right
 * after it, and any filters right-aligned on the same line — all sharing the
 * 1px divider under the row. The tab strip is passed in by the page (it owns
 * the active-tab state) and rendered here so tabs + controls read as a single
 * bar instead of two stacked rows. Items bottom-align so each tab's active
 * underline sits on the divider; controls float just above it.
 */

export function PortfolioToolbar({
  tabs,
  search,
  children,
}: {
  tabs: React.ReactNode;
  /** Rendered immediately after the tabs, on the left. */
  search?: React.ReactNode;
  /** Filters etc., right-aligned at the far end of the row. */
  children?: React.ReactNode;
}) {
  return (
    <div
      className="portfolio-toolbar"
      style={{
        display: "flex",
        alignItems: "flex-end",
        flexWrap: "wrap",
        gap: 16,
        borderBottom: "1px solid var(--border-1)",
      }}
    >
      {tabs}
      {search && (
        <div
          className="portfolio-toolbar-search"
          style={{ display: "flex", alignItems: "center", paddingBottom: 8 }}
        >
          {search}
        </div>
      )}
      {children && (
        <div
          className="portfolio-toolbar-actions"
          style={{
            display: "flex",
            alignItems: "center",
            flexWrap: "wrap",
            gap: 10,
            justifyContent: "flex-end",
            marginLeft: "auto",
            paddingBottom: 8,
          }}
        >
          {children}
        </div>
      )}
    </div>
  );
}
