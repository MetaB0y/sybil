"use client";

/**
 * Leaderboard table (SYB-59). Sticky-header CSS-grid table mirroring the public
 * /activity surface (hand-rolled grid + design tokens, not the dev DataTable).
 * The connected user's own row is highlighted when present.
 *
 * Columns are client-sortable (the server only ranks by PnL) and the list is
 * paginated with the shared `usePaged` / `<Pager>`. `rank` stays each trader's
 * canonical PnL standing from the server — sorting by another column only
 * reorders the view, it does not renumber ranks.
 */

import { useMemo, useState } from "react";
import { Pager, usePaged } from "@/components/event-list-pager";
import { formatCompactDollars, formatInt } from "@/lib/format/nanos";
import {
  formatRoiBps,
  formatSignedDollars,
  signColor,
} from "@/lib/leaderboard/format";
import type { LeaderboardRow } from "@/lib/leaderboard/use-leaderboard";

const GRID = "56px 1.6fr 1.1fr 0.9fr 0.9fr 1.1fr";
const GRID_GAP = 28;
const PAGE_SIZE = 25;
// Rows carry a 2px left border (accent for "you", transparent otherwise). The
// header mirrors it with a transparent border so its labels line up with the
// column values — box-sizing: border-box would otherwise shift every cell 2px.
const ROW_BORDER_LEFT = "2px solid transparent";

type SortKey = "rank" | "trader" | "pnl" | "roi" | "markets" | "equity";
type SortDir = "asc" | "desc";
type Sort = { key: SortKey; dir: SortDir };

/** Rank/Trader read low→high first; money/roi/markets read high→low first. */
function nextSort(prev: Sort | null, key: SortKey): Sort {
  if (prev && prev.key === key) {
    return { key, dir: prev.dir === "asc" ? "desc" : "asc" };
  }
  const ascFirst = key === "rank" || key === "trader";
  return { key, dir: ascFirst ? "asc" : "desc" };
}

function cmpBig(a: bigint, b: bigint): number {
  return a > b ? 1 : a < b ? -1 : 0;
}

/** Ascending comparison; direction is applied by the caller. */
function compareBy(a: LeaderboardRow, b: LeaderboardRow, key: SortKey): number {
  switch (key) {
    case "rank":
      return a.rank - b.rank;
    case "trader":
      return a.accountId - b.accountId;
    case "pnl":
      return cmpBig(a.pnlNanos, b.pnlNanos);
    case "roi":
      return a.roiBps - b.roiBps;
    case "markets":
      return a.marketsTraded - b.marketsTraded;
    case "equity":
      return cmpBig(a.equityNanos, b.equityNanos);
  }
}

export function LeaderboardTable({
  rows,
  isLoading,
  isRetrying,
  readState,
  errorMessage,
  onRetry,
  myAccountId,
}: {
  rows: LeaderboardRow[];
  isLoading: boolean;
  isRetrying: boolean;
  readState: "ready" | "unavailable" | "stale";
  errorMessage?: string | null;
  onRetry: () => void;
  myAccountId: number | null;
}) {
  const [sort, setSort] = useState<Sort | null>(null);

  const sorted = useMemo(() => {
    if (!sort) return rows; // no sort = server order = rank order
    const factor = sort.dir === "asc" ? 1 : -1;
    return [...rows].sort((a, b) => compareBy(a, b, sort.key) * factor);
  }, [rows, sort]);

  const paged = usePaged(sorted, PAGE_SIZE);

  const onSort = (key: SortKey) => {
    setSort((s) => nextSort(s, key));
    paged.setPage(0);
  };

  return (
    <section style={{ padding: "26px 24px 40px" }}>
      {readState !== "ready" && (
        <LeaderboardReadNotice
          stale={readState === "stale"}
          message={errorMessage}
          retrying={isRetrying}
          onRetry={onRetry}
        />
      )}
      <div
        className="leaderboard-grid-table"
        style={{
          background: "var(--surface-1)",
          // border-2 is the "card outline" token — border-1 (hairline) is too
          // faint where the header (--bg-1) meets the identical page bg, so the
          // table's top edge disappears in light theme.
          border: "1px solid var(--border-2)",
          borderRadius: 6,
          overflowY: "hidden",
        }}
      >
        <Header sort={sort} onSort={onSort} />
        {sorted.length === 0 && readState !== "unavailable" && (
          <div
            style={{
              padding: "20px 22px",
              borderTop: "1px solid var(--border-1)",
              color: "var(--fg-3)",
              fontFamily: "var(--font-mono)",
              fontSize: 12,
            }}
          >
            {isLoading ? "loading leaderboard…" : "no ranked traders yet"}
          </div>
        )}
        {paged.visible.map((row) => (
          <Row
            key={row.accountId}
            row={row}
            isMe={row.accountId === myAccountId}
          />
        ))}
        {paged.pageCount > 1 && (
          <div style={{ padding: "0 22px 10px" }}>
            <Pager paged={paged} />
          </div>
        )}
      </div>
    </section>
  );
}

export function LeaderboardReadNotice({
  stale,
  message,
  retrying,
  onRetry,
}: {
  stale: boolean;
  message?: string | null | undefined;
  retrying: boolean;
  onRetry: () => void;
}) {
  return (
    <div
      role={stale ? "status" : "alert"}
      aria-live={stale ? "polite" : undefined}
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: "var(--space-3)",
        padding: "var(--space-3) 22px",
        marginBottom: "var(--space-3)",
        border:
          "1px solid color-mix(in srgb, var(--warn) 45%, var(--border-1))",
        borderRadius: "var(--radius-sm)",
        color: "var(--warn)",
        fontFamily: "var(--font-mono)",
        fontSize: "var(--fs-12)",
      }}
    >
      <span>
        {stale
          ? "leaderboard refresh failed · showing saved rankings"
          : `leaderboard unavailable · ${message ?? "rankings could not be loaded"}`}
      </span>
      <button
        type="button"
        disabled={retrying}
        onClick={onRetry}
        style={{
          minWidth: 44,
          minHeight: 44,
          padding: "0 var(--space-3)",
          border: "1px solid var(--border-2)",
          borderRadius: "var(--radius-sm)",
          background: "var(--surface-2)",
          color: "var(--fg-1)",
          font: "inherit",
          cursor: retrying ? "wait" : "pointer",
        }}
      >
        {retrying ? "retrying…" : "retry"}
      </button>
    </div>
  );
}

function Header({
  sort,
  onSort,
}: {
  sort: Sort | null;
  onSort: (key: SortKey) => void;
}) {
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: GRID,
        gap: GRID_GAP,
        alignItems: "center",
        padding: "0 22px",
        height: 36,
        fontFamily: "var(--font-mono)",
        fontSize: 10,
        textTransform: "uppercase",
        letterSpacing: "var(--track-wide)",
        color: "var(--fg-3)",
        borderLeft: ROW_BORDER_LEFT,
        background: "var(--bg-1)",
        position: "sticky",
        top: 0,
        zIndex: 1,
      }}
    >
      <SortHeader
        col="rank"
        label="Rank"
        align="left"
        sort={sort}
        onSort={onSort}
      />
      <SortHeader
        col="trader"
        label="Trader"
        align="left"
        sort={sort}
        onSort={onSort}
      />
      <SortHeader
        col="pnl"
        label="PnL"
        align="right"
        sort={sort}
        onSort={onSort}
      />
      <SortHeader
        col="roi"
        label="ROI"
        align="right"
        sort={sort}
        onSort={onSort}
      />
      <SortHeader
        col="markets"
        label="Markets"
        align="right"
        sort={sort}
        onSort={onSort}
      />
      <SortHeader
        col="equity"
        label="Equity"
        align="right"
        sort={sort}
        onSort={onSort}
      />
    </div>
  );
}

function SortHeader({
  col,
  label,
  align,
  sort,
  onSort,
}: {
  col: SortKey;
  label: string;
  align: "left" | "right";
  sort: Sort | null;
  onSort: (key: SortKey) => void;
}) {
  const active = sort?.key === col;
  return (
    <span
      role="columnheader"
      aria-sort={
        active ? (sort!.dir === "asc" ? "ascending" : "descending") : "none"
      }
      style={{ display: "flex", width: "100%" }}
    >
      <button
        type="button"
        onClick={() => onSort(col)}
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: 3,
          width: "100%",
          justifyContent: align === "right" ? "flex-end" : "flex-start",
          padding: 0,
          border: 0,
          background: "transparent",
          cursor: "pointer",
          font: "inherit",
          letterSpacing: "var(--track-wide)",
          textTransform: "uppercase",
          color: active ? "var(--fg-1)" : "var(--fg-3)",
          transition: "color var(--dur-fast) var(--ease-standard)",
        }}
      >
        <span style={{ whiteSpace: "nowrap" }}>{label}</span>
        <span
          aria-hidden
          style={{ fontSize: 8, lineHeight: 1, opacity: active ? 1 : 0.35 }}
        >
          {active ? (sort!.dir === "asc" ? "▲" : "▼") : "↕"}
        </span>
      </button>
    </span>
  );
}

const cell: React.CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 14,
  color: "var(--fg-1)",
  fontVariantNumeric: "tabular-nums",
  textAlign: "right",
};

function Row({ row, isMe }: { row: LeaderboardRow; isMe: boolean }) {
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: GRID,
        gap: GRID_GAP,
        alignItems: "center",
        padding: "0 22px",
        height: 56,
        // borderTop (not bottom) so the last row has no trailing divider that
        // would double up with the Pager's own top border — matches the
        // portfolio tables (positions-list et al.).
        borderTop: "1px solid var(--border-1)",
        borderLeft: isMe ? "2px solid var(--accent)" : ROW_BORDER_LEFT,
        background: isMe
          ? "color-mix(in srgb, var(--accent) 10%, transparent)"
          : "transparent",
        transition: "background var(--dur-fast) var(--ease-standard)",
      }}
      onMouseEnter={(e) => {
        if (!isMe) e.currentTarget.style.background = "var(--surface-2)";
      }}
      onMouseLeave={(e) => {
        if (!isMe) e.currentTarget.style.background = "transparent";
      }}
    >
      {/* rank */}
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 14,
          color: "var(--fg-3)",
          fontVariantNumeric: "tabular-nums",
        }}
      >
        #{row.rank}
      </span>

      {/* trader label */}
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 14,
          color: "var(--fg-1)",
          display: "inline-flex",
          alignItems: "baseline",
          gap: 8,
        }}
      >
        {row.label}
        {isMe && (
          <span
            style={{
              fontSize: 9,
              textTransform: "uppercase",
              letterSpacing: "0.05em",
              color: "var(--accent)",
            }}
          >
            you
          </span>
        )}
      </span>

      {/* pnl */}
      <span style={{ ...cell, color: signColor(row.pnlNanos) }}>
        {formatSignedDollars(row.pnlNanos)}
      </span>

      {/* roi */}
      <span style={{ ...cell, color: signColor(row.roiBps) }}>
        {formatRoiBps(row.roiBps)}
      </span>

      {/* markets traded */}
      <span style={{ ...cell, color: "var(--fg-2)" }}>
        {formatInt(row.marketsTraded)}
      </span>

      {/* equity */}
      <span style={cell}>{formatCompactDollars(row.equityNanos)}</span>
    </div>
  );
}
