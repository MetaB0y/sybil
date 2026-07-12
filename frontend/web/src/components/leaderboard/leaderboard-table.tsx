"use client";

/**
 * Leaderboard table (SYB-59). Sticky-header CSS-grid table mirroring the public
 * /activity surface (hand-rolled grid + design tokens, not the dev DataTable).
 * The connected user's own row is highlighted when present.
 */

import { formatCompactDollars, formatInt } from "@/lib/format/nanos";
import {
  formatRoiBps,
  formatSignedDollars,
  signColor,
} from "@/lib/leaderboard/format";
import type { LeaderboardRow } from "@/lib/leaderboard/use-leaderboard";

const GRID = "56px 1.6fr 1.1fr 0.9fr 0.9fr 1.1fr";
const GRID_GAP = 28;

export function LeaderboardTable({
  rows,
  isLoading,
  isRetrying,
  readState,
  onRetry,
  myAccountId,
}: {
  rows: LeaderboardRow[];
  isLoading: boolean;
  isRetrying: boolean;
  readState: "ready" | "unavailable" | "stale";
  onRetry: () => void;
  myAccountId: number | null;
}) {
  return (
    <section style={{ padding: "26px 24px 40px" }}>
      {readState !== "ready" && (
        <LeaderboardReadNotice
          stale={readState === "stale"}
          retrying={isRetrying}
          onRetry={onRetry}
        />
      )}
      <div
        className="leaderboard-grid-table"
        style={{
          background: "var(--surface-1)",
          border: "1px solid var(--border-1)",
          borderRadius: 6,
          overflowY: "hidden",
        }}
      >
        <Header />
        {rows.length === 0 && readState !== "unavailable" && (
          <div
            style={{
              padding: "20px 22px",
              color: "var(--fg-3)",
              fontFamily: "var(--font-mono)",
              fontSize: 12,
            }}
          >
            {isLoading ? "loading leaderboard…" : "no ranked traders yet"}
          </div>
        )}
        {rows.map((row) => (
          <Row
            key={row.accountId}
            row={row}
            isMe={row.accountId === myAccountId}
          />
        ))}
      </div>
    </section>
  );
}

export function LeaderboardReadNotice({
  stale,
  retrying,
  onRetry,
}: {
  stale: boolean;
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
          : "leaderboard unavailable · rankings could not be loaded"}
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

function Header() {
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
        letterSpacing: "0.04em",
        color: "var(--fg-3)",
        borderBottom: "1px solid var(--border-1)",
        background: "var(--bg-1)",
        position: "sticky",
        top: 0,
        zIndex: 1,
      }}
    >
      <span>Rank</span>
      <span>Trader</span>
      <span style={{ textAlign: "right" }}>PnL</span>
      <span style={{ textAlign: "right" }}>ROI</span>
      <span style={{ textAlign: "right" }}>Markets</span>
      <span style={{ textAlign: "right" }}>Equity</span>
    </div>
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
        borderBottom: "1px solid var(--border-1)",
        borderLeft: isMe ? "2px solid var(--accent)" : "2px solid transparent",
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
