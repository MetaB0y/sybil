"use client";

/**
 * Batches table — sticky-header table of recent batches (= blocks). One row
 * per block; select any row to expand. The expanded detail is rendered via
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
  backfillError = false,
  retrying = false,
  onRetry = () => {},
  renderDetail,
}: {
  rows: BatchRowData[];
  isBackfilling: boolean;
  backfillError?: boolean;
  retrying?: boolean;
  onRetry?: () => void;
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
  const backfillUnavailable = backfillError && rows.length === 0;
  const backfillStale = backfillError && rows.length > 0;
  const newWhileFrozen =
    !live && rows[0] && frozenRows[0]
      ? Math.max(0, rows[0].height - frozenRows[0].height)
      : 0;
  const toggleLive = () => {
    if (live) {
      if (rows.length === 0) return;
      setFrozenRows(rows); // freezing → snapshot what's on screen now
      setLive(false);
    } else {
      setLive(true);
    }
  };

  return (
    <section
      className="activity-batches-section"
      style={{ padding: "26px 24px 40px" }}
    >
      <div
        className="activity-table-head"
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
          {isBackfilling ? " · backfilling…" : ""} · select any row to expand
        </span>
        <span style={{ marginLeft: "auto" }}>
          <LiveToggle
            live={live}
            newWhileFrozen={newWhileFrozen}
            disabled={live && rows.length === 0}
            onToggle={toggleLive}
          />
        </span>
      </div>

      {backfillStale && (
        <BatchBackfillNotice
          stale
          retrying={retrying}
          onRetry={onRetry}
        />
      )}

      <div
        className="activity-grid-table"
        style={{
          background: "var(--surface-1)",
          border: "1px solid var(--border-1)",
          borderRadius: 6,
          overflowY: "hidden",
        }}
      >
        <Header />
        {isBackfilling && displayRows.length === 0 && (
          <div role="status" aria-live="polite" style={emptyStyle}>
            loading recent batches…
          </div>
        )}
        {backfillUnavailable && !isBackfilling && (
          <BatchBackfillNotice
            stale={false}
            retrying={retrying}
            onRetry={onRetry}
          />
        )}
        {!isBackfilling && !backfillUnavailable && displayRows.length === 0 && (
          <div
            style={emptyStyle}
          >
            no batches yet — waiting for the first committed batch
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
            {expanded === r.height && renderDetail && (
              <div
                id={detailId(r.height)}
                role="region"
                aria-labelledby={batchLabelId(r.height)}
              >
                {renderDetail(r)}
              </div>
            )}
          </Fragment>
        ))}
      </div>
    </section>
  );
}

export function BatchBackfillNotice({
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
      style={backfillNoticeStyle}
    >
      <span>
        {stale
          ? "batch history refresh failed · showing live and saved rows"
          : "recent batches unavailable · the failed request is not shown as an empty chain"}
      </span>
      <button
        type="button"
        disabled={retrying}
        onClick={onRetry}
        style={retryButtonStyle(retrying)}
      >
        {retrying ? "retrying…" : "retry"}
      </button>
    </div>
  );
}

const emptyStyle: React.CSSProperties = {
  padding: "20px 22px",
  color: "var(--fg-3)",
  fontFamily: "var(--font-mono)",
  fontSize: 12,
};

const backfillNoticeStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "var(--space-3)",
  padding: "var(--space-3) 22px",
  color: "var(--warn)",
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-12)",
};

function retryButtonStyle(disabled: boolean): React.CSSProperties {
  return {
    minHeight: 32,
    padding: "0 var(--space-3)",
    border: "1px solid var(--border-2)",
    borderRadius: "var(--radius-sm)",
    background: "var(--surface-2)",
    color: "var(--fg-1)",
    font: "inherit",
    cursor: disabled ? "wait" : "pointer",
  };
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
  disabled,
  onToggle,
}: {
  live: boolean;
  newWhileFrozen: number;
  disabled: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onToggle}
      disabled={disabled}
      aria-pressed={live}
      title={
        disabled
          ? "Waiting for the first committed batch"
          : live
          ? "Pause the live tail to inspect a batch — rows stop updating"
          : "Resume live updates"
      }
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        padding: "4px 10px",
        borderRadius: 999,
        cursor: disabled ? "not-allowed" : "pointer",
        border: `1px solid ${live ? "var(--border-2)" : "var(--accent)"}`,
        background: live
          ? "var(--surface-1)"
          : "color-mix(in srgb, var(--accent) 12%, transparent)",
        color: disabled
          ? "var(--fg-4)"
          : live
            ? "var(--fg-2)"
            : "var(--accent)",
        opacity: disabled ? 0.7 : 1,
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
    <button
      type="button"
      id={triggerId(row.height)}
      className="activity-batch-row"
      onClick={onToggle}
      aria-expanded={expanded}
      aria-controls={detailId(row.height)}
      data-expanded={expanded}
      style={{
        display: "grid",
        gridTemplateColumns: GRID,
        gap: GRID_GAP,
        alignItems: "center",
        padding: "0 22px",
        height: 64,
        width: "100%",
        border: 0,
        borderBottom: "1px solid var(--border-1)",
        cursor: "pointer",
        color: "inherit",
        textAlign: "left",
        touchAction: "manipulation",
        transition: "background var(--dur-fast) var(--ease-standard)",
      }}
    >
      {/* chevron */}
      <span
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: "var(--fg-3)",
        }}
      >
        <svg
          aria-hidden="true"
          focusable="false"
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
      </span>

      {/* batch # */}
      <span style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
        <span
          id={batchLabelId(row.height)}
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
      </span>

      {/* cleared timestamp + relative */}
      <span style={{ display: "flex", flexDirection: "column", gap: 2 }}>
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
      </span>

      {/* markets touched */}
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 14,
          color: "var(--fg-1)",
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {row.marketsTouched}
      </span>

      {/* matched volume */}
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 14,
          color: "var(--fg-1)",
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {formatCompactDollars(row.matchedVolumeNanos)}
      </span>

      {/* welfare */}
      <span
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
      </span>

      {/* traders */}
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 14,
          color: "var(--fg-2)",
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {row.uniqueTraders}
      </span>

      {/* orders cell */}
      <OrdersCell
        placed={row.ordersPlaced}
        matched={row.ordersMatched}
        unmatched={row.ordersUnmatched}
      />
    </button>
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
    <span
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
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 12,
          color: "var(--yes)",
        }}
      >
        {matched} <span style={subLabel}>matched</span>
      </span>
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 12,
          color: "var(--no)",
        }}
      >
        {unmatched} <span style={subLabel}>unmatched</span>
      </span>
    </span>
  );
}

function triggerId(height: number): string {
  return `activity-batch-${height}-trigger`;
}

function detailId(height: number): string {
  return `activity-batch-${height}-detail`;
}

function batchLabelId(height: number): string {
  return `activity-batch-${height}-label`;
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
