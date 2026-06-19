"use client";

/**
 * Batches table — sticky-header table of recent batches (= blocks). One row
 * per block; click any row to expand. The expanded detail is rendered via
 * the `renderDetail` slot prop so this file stays focused on the table; the
 * detail UI lives in <BatchDetail>.
 *
 * Column layout adapts the handoff `activity.html` template: a fixed-width
 * chevron, then weighted `fr` columns so the row stretches edge-to-edge of
 * the table instead of stranding empty space in the last column.
 */

import { useEffect, useState, Fragment, type ReactNode } from "react";
import {
  formatCompactDollars,
  formatCompactDollarsCents,
  formatInt,
} from "@/lib/format/nanos";
import type { BatchRow as BatchRowData } from "@/lib/activity/types";

const GRID = "24px 1fr 1.2fr 0.7fr 1.1fr 1.1fr 0.7fr 1.9fr";
const GRID_GAP = 28;

export function BatchesTable({
  rows,
  isBackfilling,
  renderDetail,
}: {
  rows: BatchRowData[];
  isBackfilling: boolean;
  /** Slot for the expanded-row content; called with the row that's open. */
  renderDetail?: (row: BatchRowData) => ReactNode;
}) {
  const [expanded, setExpanded] = useState<number | null>(null);
  // Re-render every second so the "Xs ago" column ticks live — the table
  // otherwise only re-renders on a new batch (~10s) or on interaction.
  useRelativeTimeTick();

  // Live tail vs. frozen. Freezing snapshots the current rows so the user can
  // expand and inspect a batch without rows shifting as new batches arrive.
  // The 1s ticker keeps running either way, so "Xs ago" stays live even when
  // frozen.
  const [live, setLive] = useState(true);
  const [frozenRows, setFrozenRows] = useState<BatchRowData[]>([]);
  const displayRows = live ? rows : frozenRows;
  const newWhileFrozen =
    !live && rows[0] && frozenRows[0]
      ? Math.max(0, rows[0].height - frozenRows[0].height)
      : 0;
  const toggleLive = () => {
    if (live) {
      setFrozenRows(rows); // freezing → snapshot what's on screen now
      setLive(false);
    } else {
      setLive(true);
    }
  };

  return (
    <section style={{ padding: "26px 24px 40px" }}>
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          gap: 14,
          paddingBottom: 14,
        }}
      >
        <h3
          style={{
            fontFamily: "var(--font-sans)",
            fontSize: 13,
            fontWeight: 600,
            margin: 0,
            color: "var(--fg-2)",
            textTransform: "uppercase",
            letterSpacing: "0.06em",
          }}
        >
          Batches
        </h3>
        <span className="text-annotation" style={{ fontSize: 11 }}>
          showing last {displayRows.length}
          {isBackfilling ? " · backfilling…" : ""} · click any row to expand
        </span>
        <span style={{ marginLeft: "auto" }}>
          <LiveToggle
            live={live}
            newWhileFrozen={newWhileFrozen}
            onToggle={toggleLive}
          />
        </span>
      </div>

      <div
        style={{
          background: "var(--surface-1)",
          border: "1px solid var(--border-1)",
          borderRadius: 6,
          overflow: "hidden",
        }}
      >
        <Header />
        {displayRows.length === 0 && (
          <div
            style={{
              padding: "20px 22px",
              color: "var(--fg-3)",
              fontFamily: "var(--font-mono)",
              fontSize: 12,
            }}
          >
            no batches yet — waiting for hydration
          </div>
        )}
        {displayRows.map((r) => (
          <Fragment key={r.height}>
            <Row
              row={r}
              expanded={expanded === r.height}
              onToggle={() =>
                setExpanded((cur) => (cur === r.height ? null : r.height))
              }
            />
            {expanded === r.height && renderDetail && renderDetail(r)}
          </Fragment>
        ))}
      </div>
    </section>
  );
}

/**
 * Live ⇄ Frozen toggle. Live = table tails new batches; Frozen = rows are held
 * so the user can inspect a batch in peace (relative times still tick). While
 * frozen, shows how many batches have queued up so the jump on resume isn't a
 * surprise.
 */
function LiveToggle({
  live,
  newWhileFrozen,
  onToggle,
}: {
  live: boolean;
  newWhileFrozen: number;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onToggle}
      aria-pressed={live}
      title={
        live
          ? "Pause the live tail to inspect a batch — rows stop updating"
          : "Resume live updates"
      }
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        padding: "4px 10px",
        borderRadius: 999,
        cursor: "pointer",
        border: `1px solid ${live ? "var(--border-2)" : "var(--accent)"}`,
        background: live ? "var(--surface-1)" : "color-mix(in srgb, var(--accent) 12%, transparent)",
        color: live ? "var(--fg-2)" : "var(--accent)",
        fontFamily: "var(--font-mono)",
        fontSize: 10,
        textTransform: "uppercase",
        letterSpacing: "0.05em",
        lineHeight: 1,
      }}
    >
      <span
        aria-hidden
        style={{
          width: 6,
          height: 6,
          borderRadius: "50%",
          background: live ? "var(--yes)" : "var(--fg-4)",
          boxShadow: live
            ? "0 0 0 3px color-mix(in srgb, var(--yes) 25%, transparent)"
            : "none",
        }}
      />
      {live
        ? "Live"
        : newWhileFrozen > 0
          ? `Frozen · ${newWhileFrozen} new`
          : "Frozen"}
    </button>
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
      <span />
      <span>Batch</span>
      <span>Cleared</span>
      <span>Markets</span>
      <span>Matched volume</span>
      <span>Welfare</span>
      <span>Traders</span>
      <span style={{ textAlign: "right" }}>Orders</span>
    </div>
  );
}

function Row({
  row,
  expanded,
  onToggle,
}: {
  row: BatchRowData;
  expanded: boolean;
  onToggle: () => void;
}) {
  return (
    <div
      onClick={onToggle}
      style={{
        display: "grid",
        gridTemplateColumns: GRID,
        gap: GRID_GAP,
        alignItems: "center",
        padding: "0 22px",
        height: 64,
        borderBottom: "1px solid var(--border-1)",
        cursor: "pointer",
        background: expanded ? "var(--surface-2)" : "transparent",
        transition: "background var(--dur-fast) var(--ease-standard)",
      }}
      onMouseEnter={(e) => {
        if (!expanded) e.currentTarget.style.background = "var(--surface-2)";
      }}
      onMouseLeave={(e) => {
        if (!expanded) e.currentTarget.style.background = "transparent";
      }}
    >
      {/* chevron */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: "var(--fg-3)",
        }}
      >
        <svg
          width="10"
          height="10"
          viewBox="0 0 12 12"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          style={{
            transform: expanded ? "rotate(90deg)" : "rotate(0deg)",
            transition: "transform 120ms ease",
          }}
        >
          <path d="m4 3 3 3-3 3" />
        </svg>
      </div>

      {/* batch # */}
      <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 16,
            color: "var(--fg-1)",
            fontVariantNumeric: "tabular-nums",
            letterSpacing: "-0.005em",
          }}
        >
          #{formatInt(row.height)}
        </span>
      </div>

      {/* cleared timestamp + relative */}
      <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 13,
            color: "var(--fg-1)",
            fontVariantNumeric: "tabular-nums",
          }}
        >
          {fmtClock(row.timestampMs)}
        </span>
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 10,
            color: "var(--fg-4)",
            textTransform: "uppercase",
            letterSpacing: "0.04em",
          }}
        >
          {fmtRelTime(row.timestampMs)}
        </span>
      </div>

      {/* markets touched */}
      <div
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 14,
          color: "var(--fg-1)",
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {row.marketsTouched}
      </div>

      {/* matched volume */}
      <div
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 14,
          color: "var(--fg-1)",
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {formatCompactDollars(row.matchedVolumeNanos)}
      </div>

      {/* welfare */}
      <div
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 14,
          color: "var(--yes)",
          fontVariantNumeric: "tabular-nums",
          display: "flex",
          alignItems: "baseline",
          gap: 6,
        }}
      >
        {row.welfareNanos > 0n ? "+" : ""}
        {formatCompactDollarsCents(row.welfareNanos)}
        {row.welfareNanos !== 0n && (
          <span
            style={{
              fontSize: 10,
              color: "rgba(91,217,154,0.6)",
              textTransform: "uppercase",
              letterSpacing: "0.04em",
            }}
          >
            saved
          </span>
        )}
      </div>

      {/* traders */}
      <div
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 14,
          color: "var(--fg-2)",
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {row.uniqueTraders}
      </div>

      {/* orders cell */}
      <OrdersCell
        placed={row.ordersPlaced}
        matched={row.ordersMatched}
        unmatched={row.ordersUnmatched}
      />
    </div>
  );
}

function OrdersCell({
  placed,
  matched,
  unmatched,
}: {
  placed: number;
  matched: number;
  unmatched: number;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "flex-end",
        gap: 18,
        fontVariantNumeric: "tabular-nums",
      }}
    >
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 14,
          color: "var(--fg-1)",
          minWidth: 48,
        }}
      >
        {placed}
      </span>
      <span style={{ fontFamily: "var(--font-mono)", fontSize: 12, color: "var(--yes)" }}>
        {matched}{" "}
        <span style={subLabel}>matched</span>
      </span>
      <span style={{ fontFamily: "var(--font-mono)", fontSize: 12, color: "var(--no)" }}>
        {unmatched}{" "}
        <span style={subLabel}>unmatched</span>
      </span>
    </div>
  );
}

const subLabel: React.CSSProperties = {
  color: "var(--fg-4)",
  textTransform: "uppercase",
  letterSpacing: "0.04em",
  fontSize: 9,
  marginLeft: 2,
};

function fmtClock(ms: number): string {
  const d = new Date(ms);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  const ss = String(d.getSeconds()).padStart(2, "0");
  return `${hh}:${mm}:${ss}`;
}

function fmtRelTime(ms: number): string {
  const s = Math.max(0, Math.floor((Date.now() - ms) / 1000));
  if (s < 60) return `${s}s ago`;
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

/**
 * Force a re-render once a second so relative timestamps stay current.
 * Pauses while the tab is hidden — no point re-rendering an unseen table.
 */
function useRelativeTimeTick(): void {
  const [, setTick] = useState(0);
  useEffect(() => {
    let id: ReturnType<typeof setInterval> | null = null;
    const start = () => {
      if (id == null) id = setInterval(() => setTick((t) => t + 1), 1000);
    };
    const stop = () => {
      if (id != null) {
        clearInterval(id);
        id = null;
      }
    };
    const onVisibility = () => {
      if (document.visibilityState === "visible") start();
      else stop();
    };
    onVisibility();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      stop();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, []);
}
