"use client";

/**
 * The phone-width form of one table row.
 *
 * Every grid table on the site is at least 860px wide, so on a phone a row was
 * a horizontal scroll with its identity column cut off mid-word. Below
 * `COMPACT_BREAKPOINT_PX` each row renders through here instead: the thing the
 * row is *about* on its own line, then the numbers as labelled pairs, two to a
 * line. Nothing is dropped and nothing scrolls sideways.
 *
 *   ┌──────────────────────────────┐
 *   │ [img] U.S. enacts AI safety  │  thumb + title
 *   │       bill before 2027?      │
 *   │ YES · 56.672 shares          │  chips
 *   │ ENTRY  17.6¢   MARK     16¢  │  pairs
 *   │ VALUE  $9.07   P&L   −$0.92  │
 *   │ resolves Dec 31 · 162d       │  footer
 *   └──────────────────────────────┘
 *
 * Cells are passed explicitly rather than inferred from the desktop row's
 * children: the lists disagree about which column is the identity one, two of
 * them span a `<Link>` across the first two columns, and one carries a trailing
 * error slot. Positional mapping would have coupled the card silently to that
 * layout. Presentation lives in `globals.css` under `.data-card`.
 */

import Link from "next/link";
import type { ReactNode } from "react";

export interface DataCardPair {
  /** Column label, e.g. "Entry". Rendered uppercase and dimmed. */
  label: string;
  value: ReactNode;
  /** Give the pair the full row — for values too wide to share a line. */
  wide?: boolean;
}

export interface DataCardProps {
  /** Makes the whole card a link, matching the desktop row's click target. */
  href?: string;
  /** Market/outcome thumbnail, shown beside the title. */
  thumb?: ReactNode;
  /** What the row is about — the identity column on desktop. */
  title: ReactNode;
  /** Status chips (side, order state) shown under the title. */
  chips?: ReactNode;
  pairs: DataCardPair[];
  /** Full-width line below the pairs — a resolve date, a Cancel button. */
  footer?: ReactNode;
  /** Marks the viewer's own row, as the desktop tables do. */
  highlighted?: boolean;
}

export function DataCard({
  href,
  thumb,
  title,
  chips,
  pairs,
  footer,
  highlighted,
}: DataCardProps) {
  const body = (
    <>
      <div className="data-card-head">
        {thumb != null && <span className="data-card-thumb">{thumb}</span>}
        <span className="data-card-title">{title}</span>
      </div>
      {chips != null && <div className="data-card-chips">{chips}</div>}
      {pairs.length > 0 && (
        <div className="data-card-pairs">
          {pairs.map((pair) => (
            <div
              key={pair.label}
              className="data-card-pair"
              data-wide={pair.wide ? "true" : undefined}
            >
              <span className="data-card-label">{pair.label}</span>
              <span className="data-card-value">{pair.value}</span>
            </div>
          ))}
        </div>
      )}
      {footer != null && <div className="data-card-footer">{footer}</div>}
    </>
  );

  if (href) {
    return (
      <Link className="data-card" href={href} data-highlighted={highlighted}>
        {body}
      </Link>
    );
  }
  return (
    <div className="data-card" data-highlighted={highlighted}>
      {body}
    </div>
  );
}

/**
 * Card-mode wrapper for a list of `DataCard`s. Replaces the bordered,
 * side-scrolling `TableCard` / `EventTable` shell, which has nothing left to do
 * once the rows carry their own borders.
 */
export function DataCardList({ children }: { children: ReactNode }) {
  return <div className="data-card-list">{children}</div>;
}
