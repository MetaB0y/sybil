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
 *
 * The tail is FROZEN by default. A table that reorders itself every 10s is
 * hostile to the thing this page is for — reading a batch — and it makes
 * pagination meaningless, since page 2 would slide by a row per block. Freezing
 * pins a `head` height; page N is then a pure function of (head, pageSize), so
 * nothing moves under the reader. Going Live re-glues `head` to the chain tip.
 */

import { useEffect, useState, Fragment, type ReactNode } from "react";
import { DropdownMenu } from "radix-ui";
import { ChevronDown } from "lucide-react";
import {
  formatCompactDollars,
  formatCompactDollarsCents,
  formatInt,
} from "@/lib/format/nanos";
import { useBatchPage } from "@/lib/activity/use-batches";
import { selectLatestBlock, useStore } from "@/lib/store";
import type { BatchRow as BatchRowData } from "@/lib/activity/types";

const GRID = "24px 1fr 1.2fr 0.7fr 1.1fr 1.1fr 0.7fr 1.9fr";
const GRID_GAP = 28;

// Every option stays under the store's RECENT_BLOCKS_CAP (80) so page 0 is
// always satisfiable from the store. A window larger than the cap could never
// be complete, and Live mode would then re-fetch it from the network on every
// block, since `head` — and with it the `before_height` cursor — moves each
// time the tip advances.
const PAGE_SIZES = [30, 60] as const;
const DEFAULT_PAGE_SIZE = 30;

export function BatchesTable({
  renderDetail,
}: {
  /** Slot for the expanded-row content; called with the row that's open. */
  renderDetail?: (row: BatchRowData) => ReactNode;
}) {
  const [expanded, setExpanded] = useState<number | null>(null);
  // Re-render every second so the "Xs ago" column ticks live — the table
  // otherwise only re-renders on a new batch (~10s) or on interaction.
  useRelativeTimeTick();

  const latestHeight = useStore(selectLatestBlock)?.height ?? null;
  const [live, setLive] = useState(false);
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState<number>(DEFAULT_PAGE_SIZE);

  // Frozen is the default, so the tip at first sight becomes the pin. On a
  // client-side nav the store usually already holds a block, which the lazy
  // initializer catches; on a cold load it's empty, and the subscription below
  // latches the first block to arrive.
  const [pinnedHead, setPinnedHead] = useState<number | null>(
    () => useStore.getState().latestBlock?.height ?? null,
  );
  useEffect(() => {
    if (live || pinnedHead != null) return;
    return useStore.subscribe((s) => {
      const height = s.latestBlock?.height;
      if (height != null) setPinnedHead(height);
    });
  }, [live, pinnedHead]);

  const head = live ? latestHeight : pinnedHead;
  const { rows, isLoading, hasOlder, error, isRetrying, retry } = useBatchPage({
    head,
    page,
    pageSize,
  });

  const newWhileFrozen =
    !live && pinnedHead != null && latestHeight != null
      ? Math.max(0, latestHeight - pinnedHead)
      : 0;

  const goLive = () => {
    setLive(true);
    setPage(0);
    setExpanded(null);
  };
  const goFrozen = () => {
    setPinnedHead(latestHeight);
    setLive(false);
    setPage(0);
    setExpanded(null);
  };
  // Paging implies freezing: an older page computed off a moving tip would
  // slide by one row per block.
  const goOlder = () => {
    if (live) {
      setPinnedHead(latestHeight);
      setLive(false);
    }
    setPage((p) => p + 1);
    setExpanded(null);
  };
  const goNewer = () => {
    setPage((p) => Math.max(0, p - 1));
    setExpanded(null);
  };
  const changePageSize = (n: number) => {
    setPageSize(n);
    setPage(0);
    setExpanded(null);
  };

  const newest = rows[0]?.height ?? null;
  const oldest =
    rows.length > 0 ? (rows[rows.length - 1]?.height ?? null) : null;

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
          {isLoading ? "loading…" : "select any row to expand"}
        </span>
        <span style={{ marginLeft: "auto" }}>
          <TailSwitch
            live={live}
            newWhileFrozen={newWhileFrozen}
            onLive={goLive}
            onFrozen={goFrozen}
          />
        </span>
      </div>

      {error && (
        <BatchPageNotice
          stale={rows.length > 0}
          retrying={isRetrying}
          onRetry={retry}
        />
      )}

      <div
        className="activity-grid-table"
        style={{
          background: "var(--surface-1)",
          // border-2 is the "card outline" token — border-1 (hairline) is too
          // faint in light theme, leaving the batch table (and its expanded
          // detail) floating with no visible left/right edge. Matches the
          // leaderboard fix (aa78ea2).
          border: "1px solid var(--border-2)",
          borderRadius: 6,
          overflowY: "hidden",
        }}
      >
        <Header />
        {rows.length === 0 && (
          <div
            role={isLoading ? "status" : error ? "alert" : undefined}
            style={{
              padding: "20px 22px",
              color: "var(--fg-3)",
              fontFamily: "var(--font-mono)",
              fontSize: 12,
            }}
          >
            {isLoading
              ? "loading recent batches…"
              : error
                ? "batch history unavailable — not shown as an empty chain"
                : page === 0
                  ? "no batches yet — waiting for the first committed batch"
                  : "no batches on this page"}
          </div>
        )}
        {rows.map((r) => (
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

      <Pager
        page={page}
        pageSize={pageSize}
        newest={newest}
        oldest={oldest}
        hasOlder={hasOlder}
        onNewer={goNewer}
        onOlder={goOlder}
        onPageSize={changePageSize}
      />
    </section>
  );
}

function BatchPageNotice({
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
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: "var(--space-3)",
        marginBottom: "var(--space-3)",
        padding: "var(--space-3) var(--space-4)",
        border: "1px solid color-mix(in srgb, var(--warn) 35%, transparent)",
        borderRadius: "var(--radius-sm)",
        color: "var(--warn)",
        fontFamily: "var(--font-mono)",
        fontSize: 11,
      }}
    >
      <span>
        {stale
          ? "batch history refresh failed · showing live and saved rows"
          : "batch history request failed"}
      </span>
      <button
        type="button"
        disabled={retrying}
        onClick={onRetry}
        style={{
          minHeight: 30,
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

/**
 * Frozen ⇄ Live segmented switch. Both states are always on screen with the
 * active one filled, so the control reads as a switch rather than a status
 * label — the old single pill toggled on click but looked like a badge.
 *
 * Frozen holds the rows still so a batch can be inspected in peace (relative
 * times keep ticking either way). The count of batches that piled up while
 * frozen rides on the Live half, which is both the invitation and the target.
 */
function TailSwitch({
  live,
  newWhileFrozen,
  onLive,
  onFrozen,
}: {
  live: boolean;
  newWhileFrozen: number;
  onLive: () => void;
  onFrozen: () => void;
}) {
  return (
    <SegmentedGroup label="Batch tail">
      <Segment active={!live} onClick={onFrozen}>
        <span
          aria-hidden
          style={{
            width: 6,
            height: 6,
            borderRadius: "50%",
            background: live ? "var(--fg-4)" : "var(--fg-2)",
          }}
        />
        Frozen
      </Segment>
      <Segment
        active={live}
        onClick={onLive}
        accent={!live && newWhileFrozen > 0}
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
        Live
        {!live && newWhileFrozen > 0 && (
          <span
            style={{
              padding: "1px 5px",
              borderRadius: "var(--radius-sm)",
              background: "color-mix(in srgb, var(--accent) 18%, transparent)",
              color: "var(--accent)",
              fontSize: 9,
            }}
          >
            +{formatInt(newWhileFrozen)}
          </span>
        )}
      </Segment>
    </SegmentedGroup>
  );
}

/**
 * The pill shell both segmented controls sit in. `--bg-0` is the "deeper than
 * the page" token, so the shell reads as an inset track under its raised active
 * segment, in both themes. (Not `--bg-2`, which ModeTabs asks for and silently
 * gets `transparent` — no such token exists.)
 */
function SegmentedGroup({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <span
      role="group"
      aria-label={label}
      style={{
        display: "inline-flex",
        gap: 4,
        padding: 3,
        background: "var(--bg-0)",
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-lg)",
      }}
    >
      {children}
    </span>
  );
}

function Segment({
  active,
  accent,
  onClick,
  children,
}: {
  active: boolean;
  accent?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      disabled={active}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        padding: "4px 10px",
        borderRadius: "var(--radius-md)",
        cursor: active ? "default" : "pointer",
        border: 0,
        background: active ? "var(--surface-2)" : "transparent",
        boxShadow: active ? "inset 0 0 0 1px var(--border-2)" : "none",
        color: active
          ? "var(--fg-1)"
          : accent
            ? "var(--accent)"
            : "var(--fg-3)",
        fontFamily: "var(--font-mono)",
        fontSize: 10,
        textTransform: "uppercase",
        letterSpacing: "0.05em",
        lineHeight: 1,
        transition:
          "background var(--dur-fast) var(--ease-standard), color var(--dur-fast) var(--ease-standard)",
      }}
    >
      {children}
    </button>
  );
}

/**
 * Height-range caption + page controls. There's no total to count against —
 * history runs back to genesis — so the caption names the heights on screen
 * instead of a page-of-N that we'd have to fabricate.
 */
function Pager({
  page,
  pageSize,
  newest,
  oldest,
  hasOlder,
  onNewer,
  onOlder,
  onPageSize,
}: {
  page: number;
  pageSize: number;
  newest: number | null;
  oldest: number | null;
  hasOlder: boolean;
  onNewer: () => void;
  onOlder: () => void;
  onPageSize: (n: number) => void;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 14,
        paddingTop: 12,
        flexWrap: "wrap",
      }}
    >
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 11,
          color: "var(--fg-3)",
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {oldest != null && newest != null
          ? `batches #${formatInt(oldest)}–#${formatInt(newest)}`
          : "—"}
        <span style={{ color: "var(--fg-4)" }}> · page {page + 1}</span>
      </span>

      <span
        style={{
          marginLeft: "auto",
          display: "inline-flex",
          gap: 14,
          alignItems: "center",
        }}
      >
        <span
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 8,
            fontFamily: "var(--font-mono)",
            fontSize: 10,
            textTransform: "uppercase",
            letterSpacing: "0.05em",
            color: "var(--fg-3)",
          }}
        >
          Rows
          <RowsSelect value={pageSize} onChange={onPageSize} />
        </span>

        <span style={{ display: "inline-flex", gap: 6 }}>
          <PageButton onClick={onNewer} disabled={page === 0}>
            ‹ Newer
          </PageButton>
          <PageButton onClick={onOlder} disabled={!hasOlder}>
            Older ›
          </PageButton>
        </span>
      </span>
    </div>
  );
}

/**
 * Rows-per-page dropdown, built on the same Radix menu as the Dev Zone nav
 * dropdown and styled to match it (see `.activity-rows-*` in globals.css).
 * A native `<select>` would drop unstyled OS chrome over the theme — the same
 * reason the portfolio filters aren't one either.
 */
function RowsSelect({
  value,
  onChange,
}: {
  value: number;
  onChange: (n: number) => void;
}) {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button
          type="button"
          className="activity-rows-trigger"
          aria-label="Batches per page"
        >
          {value}
          <ChevronDown size={12} aria-hidden />
        </button>
      </DropdownMenu.Trigger>

      <DropdownMenu.Portal>
        <DropdownMenu.Content
          sideOffset={6}
          align="end"
          style={{
            background: "var(--surface-3)",
            border: "1px solid var(--border-2)",
            borderRadius: "var(--radius-md)",
            padding: "var(--space-2)",
            minWidth: 76,
            zIndex: 60,
            boxShadow: "0 8px 24px rgba(0,0,0,0.32)",
          }}
        >
          <DropdownMenu.RadioGroup
            value={String(value)}
            onValueChange={(v) => onChange(Number(v))}
          >
            {PAGE_SIZES.map((n) => (
              <DropdownMenu.RadioItem
                key={n}
                value={String(n)}
                className="activity-rows-item"
              >
                {n}
              </DropdownMenu.RadioItem>
            ))}
          </DropdownMenu.RadioGroup>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

function PageButton({
  onClick,
  disabled,
  children,
}: {
  onClick: () => void;
  disabled: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      style={{
        padding: "5px 12px",
        borderRadius: 4,
        border: "1px solid var(--border-1)",
        background: "var(--surface-1)",
        color: disabled ? "var(--fg-4)" : "var(--fg-1)",
        cursor: disabled ? "not-allowed" : "pointer",
        opacity: disabled ? 0.5 : 1,
        fontFamily: "var(--font-mono)",
        fontSize: 10,
        textTransform: "uppercase",
        letterSpacing: "0.05em",
        lineHeight: 1.4,
      }}
    >
      {children}
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
      className="activity-batch-row"
      id={batchLabelId(row.height)}
      aria-expanded={expanded}
      aria-controls={detailId(row.height)}
      onClick={onToggle}
      style={{
        display: "grid",
        width: "100%",
        gridTemplateColumns: GRID,
        gap: GRID_GAP,
        alignItems: "center",
        padding: "0 22px",
        height: 64,
        borderBottom: "1px solid var(--border-1)",
        borderTop: 0,
        borderLeft: 0,
        borderRight: 0,
        cursor: "pointer",
        color: "inherit",
        font: "inherit",
        textAlign: "left",
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
      <span
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
      </span>

      {/* batch # */}
      <span style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
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

function batchLabelId(height: number): string {
  return `activity-batch-${height}-trigger`;
}

function detailId(height: number): string {
  return `activity-batch-${height}-detail`;
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
