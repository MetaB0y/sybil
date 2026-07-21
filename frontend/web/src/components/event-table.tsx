"use client";

/**
 * Shared chrome for the three market-detail lists under the chart — Holdings,
 * Open orders, Closed orders — inside the "your positions & orders" section.
 *
 * Each list used to own a private `Row` / `HeaderCell` / `Right` / `Empty`, and
 * they had drifted: 10px vs 14px vs 18px column gutters, and Open orders sat
 * ~4px taller per row than the other two because its Cancel button was built at
 * a different scale (9.5px type, 3px padding, a border) than the YES/NO and
 * status chips that set the row height everywhere else. Switching tabs shifted
 * the rows under the cursor.
 *
 * Everything but each list's columns and comparators lives here now. `Cancel`
 * is styled as a sibling of the chips so it can't inflate its row, and a row
 * floor (`ROW_MIN_HEIGHT`) keeps a chip-less row from collapsing shorter.
 */

import type { CSSProperties, ReactNode } from "react";

import type { Column, Sort } from "@/lib/table/sort";
import { DataCardList } from "@/components/data-card";
import { Glossary } from "@/components/glossary";
import { useCompactLayout } from "@/lib/responsive/use-compact";

export {
  cmpBig,
  cmpNullableBig,
  nextSort,
  type Column,
  type Sort,
  type SortDir,
} from "@/lib/table/sort";

/** Column gutter shared by all three lists. */
export const ROW_GAP = 14;
/**
 * Row floor. 15px is the height of the tinted chips (YES/NO, status, welfare)
 * that are the tallest thing in a typical row; the padding either side makes
 * 33. Pinning it means a row of plain numbers matches a row with chips, and no
 * future control can quietly make one list taller than its neighbours.
 */
export const ROW_MIN_HEIGHT = 33;

/**
 * Rows bleed 8px into the section's padding so the hover tint reads as a band
 * rather than a floating rectangle. The table pulls the same 8px back out, so
 * the columns stay aligned with everything else in the section.
 */
const ROW_INSET = 8;

export function eventRowGrid(
  columns: string,
  header?: boolean,
): CSSProperties {
  return {
    display: "grid",
    gridTemplateColumns: columns,
    gap: ROW_GAP,
    alignItems: "center",
    minHeight: header ? undefined : ROW_MIN_HEIGHT,
    padding: `9px ${ROW_INSET}px`,
    borderTop: header ? undefined : "1px solid var(--border-1)",
    borderRadius: header ? undefined : 4,
    fontFamily: "var(--font-mono)",
    fontSize: header ? 10 : 11,
    letterSpacing: "var(--track-wide)",
    textTransform: header ? "uppercase" : undefined,
    color: header ? "var(--fg-4)" : "var(--fg-2)",
  };
}

/**
 * Wrapper that cancels the rows' bleed so columns line up with the section —
 * or, on a phone, the plain stack the `DataCard` rows sit in (there is no bleed
 * to cancel once the rows are cards).
 */
export function EventTable({ children }: { children: ReactNode }) {
  const compact = useCompactLayout();
  if (compact) return <DataCardList>{children}</DataCardList>;
  return <div style={{ margin: `0 ${-ROW_INSET}px` }}>{children}</div>;
}

/**
 * One row. Body rows carry `.event-row` for the hover tint (see globals.css).
 *
 * On a phone the body rows are rendered as cards by their lists, and only the
 * header comes through here — as a wrapped strip of sort buttons, since the
 * cards label their own values but have nowhere to put the sort controls.
 */
export function EventRow({
  columns,
  header,
  children,
}: {
  columns: string;
  header?: boolean;
  children: ReactNode;
}) {
  const compact = useCompactLayout();
  // The header names columns the card rows below don't have.
  if (header && compact) return null;
  return (
    <div
      {...(header ? {} : { className: "event-row" })}
      style={eventRowGrid(columns, header)}
    >
      {children}
    </div>
  );
}

/** The outcome cell — the left-hand identity column in all three lists. */
export function OutcomeLabel({ children }: { children: ReactNode }) {
  return (
    <span
      style={{
        overflow: "hidden",
        textOverflow: "ellipsis",
        whiteSpace: "nowrap",
        color: "var(--fg-1)",
        fontFamily: "var(--font-sans)",
        fontSize: 13,
      }}
    >
      {children}
    </span>
  );
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

/** Right-aligned cell. `dim` fades a value that never actually happened. */
export function Right({
  children,
  mono,
  dim,
}: {
  children: ReactNode;
  mono?: boolean;
  dim?: boolean;
}) {
  return (
    <span
      style={{
        textAlign: "right",
        whiteSpace: "nowrap",
        fontFamily: mono ? "var(--font-mono)" : "inherit",
        fontSize: mono ? 12 : undefined,
        color: dim ? "var(--fg-4)" : mono ? "var(--fg-1)" : undefined,
      }}
    >
      {children}
    </span>
  );
}

export function Muted({ children }: { children: ReactNode }) {
  return <span style={{ color: "var(--fg-4)" }}>{children}</span>;
}

/** Wall-clock time then a faded short date, on one line. */
export function EventTime({ ms }: { ms: number }) {
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

/**
 * Cancel control, built to the chips' scale rather than its own. The old button
 * (9.5px type, 3px padding, a 1px border) stood ~19px tall against the 15px
 * chips, which is what made every Open-orders row taller than a Holdings row.
 */
export function CancelButton({
  cancelling,
  title,
  onClick,
}: {
  cancelling: boolean;
  title?: string | undefined;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={cancelling}
      {...(title ? { title } : {})}
      style={{
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        justifySelf: "end",
        minWidth: 34,
        padding: "1px 7px",
        background: "color-mix(in srgb, var(--no) 12%, transparent)",
        border: 0,
        borderRadius: 3,
        color: "var(--no)",
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        fontWeight: 500,
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
        whiteSpace: "nowrap",
        cursor: cancelling ? "not-allowed" : "pointer",
        opacity: cancelling ? 0.6 : 1,
      }}
    >
      {cancelling ? "…" : "Cancel"}
    </button>
  );
}

/** Click-to-sort column header with the active-direction caret. */
export function HeaderCell<K extends string>({
  col,
  sort,
  onSort,
}: {
  col: Column<K>;
  sort: Sort<K> | null;
  onSort: () => void;
}) {
  const active = sort?.key === col.key;
  const button = (
    <button
      type="button"
      onClick={onSort}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 3,
        width: col.info ? "auto" : "100%",
        justifyContent: col.align === "right" ? "flex-end" : "flex-start",
        padding: 0,
        border: 0,
        background: "transparent",
        cursor: "pointer",
        font: "inherit",
        textTransform: "uppercase",
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
  // The `?` badge is a sibling of the button, not nested inside it.
  if (!col.info) return button;
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
      <Glossary term={col.info} />
    </span>
  );
}

export function Empty({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        padding: "24px 0",
        color: "var(--fg-4)",
        fontFamily: "var(--font-mono)",
        fontSize: 12,
        letterSpacing: "var(--track-wide)",
        textAlign: "center",
      }}
    >
      {children}
    </div>
  );
}
