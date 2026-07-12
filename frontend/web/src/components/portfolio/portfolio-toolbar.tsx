"use client";

/**
 * One-row portfolio toolbar: the tab strip on the left, and the controls —
 * filters first, then the search box on the far right — right-aligned on the
 * same line, all sharing the 1px divider under the row. The tab strip is passed
 * in by the page (it owns the active-tab state) and rendered here so tabs +
 * controls read as a single bar instead of two stacked rows. Items bottom-align
 * so each tab's active underline sits on the divider; controls float just above
 * it. Search sits to the right of the filters, in the same place on every tab.
 */

export function PortfolioToolbar({
  tabs,
  search,
  children,
}: {
  tabs: React.ReactNode;
  /** Rendered last in the right-aligned control group (to the right of filters). */
  search?: React.ReactNode;
  /** Filters etc., right-aligned before the search on the same line. */
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
      {(search || children) && (
        <div
          className="portfolio-toolbar-controls"
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
          {search && (
            <div
              className="portfolio-toolbar-search"
              style={{ display: "flex", alignItems: "center" }}
            >
              {search}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
