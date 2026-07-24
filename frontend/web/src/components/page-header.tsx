import type { ReactNode } from "react";

/**
 * Canonical page header — one source of truth for the title + metadata block
 * at the top of every page (/, /activity, /portfolio). Previously each page
 * hand-rolled its own <h1> and subtitle, which drifted in font size, color,
 * gap, and layout.
 *
 * - Title uses the `.h-display` token style (56px Syne / 600 / --fg-1).
 * - `meta` renders through `.text-annotation` (13px mono / --fg-3).
 * - `action` (e.g. a status chip) sits flush-right on the title's baseline.
 */
export function PageHeader({
  title,
  meta,
  action,
}: {
  title: ReactNode;
  meta?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <header
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-2)",
      }}
    >
      {/* The row wraps below 640px (see globals.css): a 40px display title plus
          a range bar does not fit on a phone, and an unwrapped row pushed the
          whole page 34px wider than the viewport on /leaderboard. */}
      <div
        className="page-header-title-row"
        style={{
          display: "flex",
          alignItems: "baseline",
          gap: "var(--space-3)",
        }}
      >
        <h1 className="h-display" style={{ minWidth: 0 }}>
          {title}
        </h1>
        {action != null && (
          <div className="page-header-action" style={{ marginLeft: "auto" }}>
            {action}
          </div>
        )}
      </div>
      {meta != null && <p className="text-annotation">{meta}</p>}
    </header>
  );
}
