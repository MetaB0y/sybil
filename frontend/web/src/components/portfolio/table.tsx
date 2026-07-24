"use client";

/**
 * Shared chrome for the four portfolio tabs (Positions / Orders / Trades /
 * History).
 *
 * Each tab used to carry its own copy of the card wrapper, `rowGrid`,
 * `SortHeader`, `RightCell` and `Empty`. They had drifted — 10px vs 14px column
 * gaps, 9px vs 10px row padding, one tab's numeric cells missing the `--fg-1`
 * colour, two different pager paddings — so switching tabs visibly shifted the
 * table. Everything except the column widths and the comparators now lives
 * here, which is what keeps the tabs aligned rather than a promise to remember.
 *
 * Rows are clickable (the market page is one tap away), so they also opt into
 * `.portfolio-row` — see `globals.css` for the hover treatment. Inline styles
 * beat a stylesheet `:hover`, hence the class rather than a style prop.
 */

import type { CSSProperties, ReactNode } from "react";

import { DataCardList } from "@/components/data-card";
import { useCompactLayout } from "@/lib/responsive/use-compact";
import type { Column, Sort } from "@/lib/table/sort";

// Re-exported so the portfolio tabs keep importing their whole table toolkit
// from one module; the implementations are shared with the market-detail lists.
export {
  cmpBig,
  cmpNullableBig,
  nextSort,
  type Column,
  type Sort,
  type SortDir,
} from "@/lib/table/sort";

/** Vertical rhythm shared by every portfolio table row, header included. */
export const ROW_PADDING = "10px 14px";
/** Column gutter shared by every portfolio table row. */
export const ROW_GAP = 12;

/**
 * One table row's grid. `columns` is the tab's own `grid-template-columns`;
 * everything else is fixed so the tabs line up.
 */
export function rowGrid(columns: string, color: string): CSSProperties {
  return {
    display: "grid",
    gridTemplateColumns: columns,
    gap: ROW_GAP,
    alignItems: "center",
    padding: ROW_PADDING,
    color,
    fontFamily: "var(--font-mono)",
    fontSize: 11,
    letterSpacing: "var(--track-wide)",
  };
}

/** Header row (`--fg-4`) — the tab passes its own column template. */
export function headerRowGrid(columns: string): CSSProperties {
  return rowGrid(columns, "var(--fg-4)");
}

/** Body row (`--fg-2`) with the divider above it. */
export function bodyRowGrid(columns: string): CSSProperties {
  return {
    ...rowGrid(columns, "var(--fg-2)"),
    borderTop: "1px solid var(--border-1)",
  };
}

/**
 * The bordered, horizontally scrollable card every tab's table sits in — or, on
 * a phone, a plain stack, because the rows inside are `DataCard`s that bring
 * their own border and no longer need an 860px scroll port.
 */
export function TableCard({ children }: { children: ReactNode }) {
  const compact = useCompactLayout();
  if (compact) return <DataCardList>{children}</DataCardList>;
  return (
    <div
      className="portfolio-grid-table"
      style={{
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: 6,
        overflowY: "hidden",
      }}
    >
      {children}
    </div>
  );
}

/**
 * The header row — desktop only. Every card below labels its own values, so on
 * a phone the header had nothing left to name: it read as the leftover header
 * of a table that isn't there.
 */
export function TableHead({
  columns,
  children,
}: {
  columns: string;
  children: ReactNode;
}) {
  const compact = useCompactLayout();
  if (compact) return null;
  return <div style={headerRowGrid(columns)}>{children}</div>;
}

/** Footer well holding a tab's `<Pager>`, padded identically across tabs. */
export function PagerFooter({ children }: { children: ReactNode }) {
  return <div style={{ padding: "0 14px 12px" }}>{children}</div>;
}

/**
 * The market cell's label. Carries `.portfolio-row-market` so hovering the row
 * tints it — the cue that the row navigates to the market page. Colour and type
 * live in the stylesheet, not here: an inline `color` outranks the `:hover`
 * rule and left the label inert (the same trap the settings controls hit).
 */
export function MarketLabel({ children }: { children: ReactNode }) {
  return <span className="portfolio-row-market">{children}</span>;
}

/** Right-aligned cell; `mono` is the numeric variant every tab uses. */
export function RightCell({
  children,
  mono,
}: {
  children: ReactNode;
  mono?: boolean;
}) {
  return (
    <span
      style={{
        textAlign: "right",
        fontFamily: mono ? "var(--font-mono)" : "inherit",
        fontSize: mono ? 12 : undefined,
        color: mono ? "var(--fg-1)" : undefined,
        whiteSpace: "nowrap",
      }}
    >
      {children}
    </span>
  );
}

/** An em dash in the muted tone, for cells a row has no value for. */
export function Muted({ children }: { children: ReactNode }) {
  return <span style={{ color: "var(--fg-4)" }}>{children}</span>;
}

/** BUY / SELL / — in the shared action colours. */
export function ActionCell({ side }: { side?: "BUY" | "SELL" | undefined }) {
  const isBuy = side === "BUY";
  const isSell = side === "SELL";
  return (
    <span
      style={{
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        fontWeight: 600,
        letterSpacing: "var(--track-wide)",
        color: isBuy ? "var(--accent)" : isSell ? "var(--no)" : "var(--fg-4)",
      }}
    >
      {isBuy ? "BUY" : isSell ? "SELL" : "—"}
    </span>
  );
}

/**
 * Wall-clock time then a faded short date, on one line. Shared by History's
 * Time column, Trades' Time column and the expanded partial-fill rows, which
 * had three separate copies of this formatter.
 */
export function TimeCell({ ms }: { ms: number }) {
  const d = new Date(ms);
  const date = d.toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });
  const time = d.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
  return (
    <span
      title={d.toLocaleString()}
      style={{
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        color: "var(--fg-2)",
        whiteSpace: "nowrap",
      }}
    >
      {time}
      <span style={{ color: "var(--fg-4)" }}>{` ${date}`}</span>
    </span>
  );
}

/** Empty / no-match state, sized and toned the same on every tab. */
export function Empty({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        padding: "32px 16px",
        background: "var(--surface-1)",
        border: "1px dashed var(--border-1)",
        borderRadius: 6,
        color: "var(--fg-4)",
        fontFamily: "var(--font-mono)",
        fontSize: 12,
        textAlign: "center",
      }}
    >
      {children}
    </div>
  );
}

/** Click-to-sort column header with the active-direction caret. */
export function SortHeader<K extends string>({
  col,
  sort,
  onSort,
  children,
}: {
  col: Column<K>;
  sort: Sort<K> | null;
  onSort: (key: K) => void;
  /** Rendered beside the button — the glossary badge, where a column has one. */
  children?: ReactNode;
}) {
  const active = sort?.key === col.key;
  const button = (
    <button
      type="button"
      onClick={() => onSort(col.key)}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 3,
        width: children ? "auto" : "100%",
        justifyContent: col.align === "right" ? "flex-end" : "flex-start",
        padding: 0,
        border: 0,
        background: "transparent",
        cursor: "pointer",
        font: "inherit",
        letterSpacing: "var(--track-wide)",
        color: active ? "var(--fg-2)" : "var(--fg-4)",
      }}
    >
      <span style={{ whiteSpace: "nowrap" }}>{col.label}</span>
      <span style={{ fontSize: 8, lineHeight: 1, opacity: active ? 1 : 0.3 }}>
        {active ? (sort!.dir === "asc" ? "▲" : "▼") : "↕"}
      </span>
    </button>
  );
  if (!children) return button;
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 3,
        width: "100%",
        justifyContent: col.align === "right" ? "flex-end" : "flex-start",
      }}
    >
      {button}
      {children}
    </span>
  );
}
