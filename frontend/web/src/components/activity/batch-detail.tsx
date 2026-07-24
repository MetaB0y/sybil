"use client";

/**
 * Expanded panel for a batch row. The author-vetted two-column layout keeps
 * market rows on the left and batch identity/order composition on the right.
 *
 * Data comes from `useBatchDetail(height)`. The per-market rows are real —
 * volume, welfare and placed/matched come from `BlockResponse.by_market`.
 */

import Link from "next/link";
import { useMemo, useState } from "react";
import { getCategoryColor } from "@/lib/categorize";
import {
  formatCents,
  formatCompactDollars,
  formatCompactDollarsCents,
  formatInt,
} from "@/lib/format/nanos";
import { useBatchDetail } from "@/lib/activity/use-batch-detail";
import { useCompactLayout } from "@/lib/responsive/use-compact";
import type { BatchMarketRow, BatchRow } from "@/lib/activity/types";
import { DataCard } from "@/components/data-card";
import { DonutOutcome } from "./donut-outcome";

const ROWS_INITIAL = 8;
const ROWS_STEP = 10;

const GRID = "2fr 70px 60px 110px 100px 130px";
const GRID_GAP = 12;

type SortKey = "clear" | "delta" | "volume" | "welfare" | "orders";
type SortDir = "asc" | "desc";

export function BatchDetail({ row }: { row: BatchRow }) {
  const compact = useCompactLayout();
  const { block, rows, isPending } = useBatchDetail(row.height);
  const [shown, setShown] = useState(ROWS_INITIAL);
  // Default order: biggest matched volume first, then most matched, then most
  // placed (see sortMarketRows tiebreakers).
  const [sort, setSort] = useState<{ key: SortKey; dir: SortDir }>({
    key: "volume",
    dir: "desc",
  });

  const sortedRows = useMemo(
    () => sortMarketRows(rows, sort.key, sort.dir),
    [rows, sort],
  );
  const visible = sortedRows.slice(0, shown);
  const remaining = sortedRows.length - visible.length;

  // First click on a column sorts it descending; clicking the active column
  // again flips direction.
  const onSort = (key: SortKey) =>
    setSort((cur) =>
      cur.key === key
        ? { key, dir: cur.dir === "desc" ? "asc" : "desc" }
        : { key, dir: "desc" },
    );

  return (
    <div
      /* The 70px left indent lines the panel up under the desktop row's expand
         caret. On a phone it pushed the table clean off the right edge, so the
         compact panel sits flush with the card stack it belongs to. */
      className="activity-batch-detail"
      style={{
        background: "var(--bg-1)",
        borderBottom: "1px solid var(--border-1)",
      }}
    >
      <div
        className="activity-batch-detail-grid"
        style={{ display: "grid", gridTemplateColumns: "1fr 280px", gap: 24 }}
      >
        {/* Left: market rows */}
        <div style={{ display: "flex", flexDirection: "column", minWidth: 0 }}>
          <div
            style={{
              flex: 1,
              display: "flex",
              flexDirection: "column",
              // Card rows carry their own border; the table shell would draw a
              // second one around the stack.
              gap: compact ? "var(--space-2)" : 0,
              background: compact ? "transparent" : "var(--surface-1)",
              border: compact ? 0 : "1px solid var(--border-1)",
              borderRadius: 6,
              overflow: "hidden",
            }}
          >
            {!compact && <MarketTableHeader sort={sort} onSort={onSort} />}
            {isPending && (
              <div
                style={{
                  padding: "16px 14px",
                  color: "var(--fg-3)",
                  fontFamily: "var(--font-mono)",
                  fontSize: 12,
                }}
              >
                loading market rows…
              </div>
            )}
            {!isPending && sortedRows.length === 0 && (
              <div
                style={{
                  padding: "16px 14px",
                  color: "var(--fg-3)",
                  fontFamily: "var(--font-mono)",
                  fontSize: 12,
                }}
              >
                no markets cleared in this batch
              </div>
            )}
            {visible.map((m) => (
              <MarketRow key={m.marketId} row={m} compact={compact} />
            ))}

            {(remaining > 0 || shown > ROWS_INITIAL) && (
              <button
                type="button"
                onClick={() =>
                  setShown((s) =>
                    remaining > 0
                      ? Math.min(rows.length, s + ROWS_STEP)
                      : ROWS_INITIAL,
                  )
                }
                style={{
                  display: "block",
                  width: "100%",
                  marginTop: "auto",
                  background: "transparent",
                  border: 0,
                  padding: "10px 14px",
                  cursor: "pointer",
                  color: "var(--accent)",
                  fontFamily: "var(--font-mono)",
                  fontSize: 11,
                  textTransform: "uppercase",
                  letterSpacing: "0.04em",
                  textAlign: "left",
                }}
              >
                {remaining > 0
                  ? `Show ${Math.min(ROWS_STEP, remaining)} more · ${remaining} remaining`
                  : "Show less"}
              </button>
            )}
          </div>
        </div>

        {/* Right: truthful batch identity, then the order stats it describes. */}
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <SidebarPanel>
            <BatchIdentity
              height={row.height}
              stateRoot={block?.state_root ?? null}
              eventsRoot={block?.events_root ?? null}
            />
          </SidebarPanel>
          <SidebarPanel title="Order outcome">
            <DonutOutcome
              matched={row.ordersMatched}
              unmatched={row.ordersUnmatched}
            />
          </SidebarPanel>
          <SidebarPanel title="Batch composition">
            <KvRow label="Markets" value={row.marketsTouched} />
            <KvRow label="Unique traders" value={row.uniqueTraders} />
            <KvRow label="Orders processed" value={row.ordersPlaced} />
            <KvRow
              label="Matched orders"
              value={row.ordersMatched}
              color="var(--yes)"
            />
            <KvRow
              label="Unmatched orders"
              value={row.ordersUnmatched}
              color="var(--no)"
            />
          </SidebarPanel>
        </div>
      </div>
    </div>
  );
}

// ── Market row inside the detail table ────────────────────────────────────

function MarketTableHeader({
  sort,
  onSort,
}: {
  sort: { key: SortKey; dir: SortDir };
  onSort: (key: SortKey) => void;
}) {
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: GRID,
        gap: GRID_GAP,
        padding: "8px 14px",
        alignItems: "center",
        fontFamily: "var(--font-mono)",
        fontSize: 9.5,
        color: "var(--fg-3)",
        textTransform: "uppercase",
        letterSpacing: "0.04em",
        borderBottom: "1px solid var(--border-1)",
        background: "var(--surface-2)",
      }}
    >
      <span>Market</span>
      <SortTh label="Clear" col="clear" sort={sort} onSort={onSort} />
      <SortTh label="Δ" col="delta" sort={sort} onSort={onSort} />
      <SortTh label="Matched vol" col="volume" sort={sort} onSort={onSort} />
      <SortTh label="Welfare" col="welfare" sort={sort} onSort={onSort} />
      <SortTh
        label="Processed / Matched"
        col="orders"
        sort={sort}
        onSort={onSort}
      />
    </div>
  );
}

/** A right-aligned, clickable column header with an active-sort arrow. */
function SortTh({
  label,
  col,
  sort,
  onSort,
}: {
  label: string;
  col: SortKey;
  sort: { key: SortKey; dir: SortDir };
  onSort: (key: SortKey) => void;
}) {
  const active = sort.key === col;
  return (
    <button
      type="button"
      onClick={() => onSort(col)}
      style={{
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "flex-end",
        gap: 3,
        width: "100%",
        background: "transparent",
        border: 0,
        padding: 0,
        cursor: "pointer",
        font: "inherit",
        textTransform: "inherit",
        letterSpacing: "inherit",
        color: active ? "var(--fg-1)" : "var(--fg-3)",
      }}
    >
      {label}
      <span aria-hidden style={{ fontSize: 8, opacity: active ? 1 : 0.3 }}>
        {active ? (sort.dir === "desc" ? "▼" : "▲") : "↕"}
      </span>
    </button>
  );
}

/**
 * Sort the market rows by the active column/direction. Missing Δ values always
 * sink to the bottom. Equal-key rows fall back to volume → matched → placed
 * (all descending), which is also the default ordering when sorting by volume.
 */
function sortMarketRows(
  rows: BatchMarketRow[],
  key: SortKey,
  dir: SortDir,
): BatchMarketRow[] {
  const sign = dir === "asc" ? 1 : -1;
  const cmpBig = (x: bigint, y: bigint) => (x < y ? -1 : x > y ? 1 : 0);
  const primary = (a: BatchMarketRow, b: BatchMarketRow): number => {
    switch (key) {
      case "clear":
        return cmpBig(a.clearPriceNanos, b.clearPriceNanos) * sign;
      case "welfare":
        return cmpBig(a.welfareNanos, b.welfareNanos) * sign;
      case "volume":
        return cmpBig(a.matchedVolumeNanos, b.matchedVolumeNanos) * sign;
      case "orders": {
        const d = a.ordersMatched - b.ordersMatched;
        return (d !== 0 ? d : a.ordersPlaced - b.ordersPlaced) * sign;
      }
      case "delta": {
        if (a.deltaNanos == null && b.deltaNanos == null) return 0;
        if (a.deltaNanos == null) return 1; // nulls last, direction-independent
        if (b.deltaNanos == null) return -1;
        return cmpBig(a.deltaNanos, b.deltaNanos) * sign;
      }
      default:
        return 0;
    }
  };
  return [...rows].sort((a, b) => {
    const p = primary(a, b);
    if (p !== 0) return p;
    const v = cmpBig(a.matchedVolumeNanos, b.matchedVolumeNanos);
    if (v !== 0) return -v;
    if (a.ordersMatched !== b.ordersMatched)
      return b.ordersMatched - a.ordersMatched;
    if (a.ordersPlaced !== b.ordersPlaced)
      return b.ordersPlaced - a.ordersPlaced;
    return a.marketId - b.marketId;
  });
}

function MarketRow({
  row,
  compact,
}: {
  row: BatchMarketRow;
  compact: boolean;
}) {
  const deltaCents =
    row.deltaNanos == null ? null : Number(row.deltaNanos) / 1e7;
  const deltaColor =
    deltaCents == null
      ? "var(--fg-4)"
      : deltaCents > 0
        ? "var(--yes)"
        : deltaCents < 0
          ? "var(--no)"
          : "var(--fg-3)";
  const deltaText =
    deltaCents == null
      ? "—"
      : `${deltaCents >= 0 ? "+" : ""}${deltaCents.toFixed(1)}`;

  // Six columns of numbers behind a truncated question ("W…") is what the grid
  // came to on a phone. Same figures, one card, nothing cut off.
  if (compact) {
    return (
      <DataCard
        href={`/m/${row.marketId}`}
        thumb={
          <span
            aria-hidden
            style={{
              width: 6,
              height: 6,
              borderRadius: "50%",
              background: getCategoryColor(row.category),
              display: "inline-block",
            }}
          />
        }
        title={row.title}
        pairs={[
          { label: "Clear", value: formatCents(row.clearPriceNanos) },
          {
            label: "Δ",
            value: <span style={{ color: deltaColor }}>{deltaText}</span>,
          },
          {
            label: "Matched vol",
            value: formatCompactDollars(row.matchedVolumeNanos),
          },
          {
            label: "Welfare",
            value: (
              <span style={{ color: "var(--yes)" }}>
                {row.welfareNanos >= 0n ? "+" : ""}
                {formatCompactDollarsCents(row.welfareNanos)}
              </span>
            ),
          },
          {
            label: "Processed / matched",
            value: `${formatInt(row.ordersPlaced)} / ${formatInt(row.ordersMatched)}`,
            wide: true,
          },
        ]}
      />
    );
  }

  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: GRID,
        gap: GRID_GAP,
        padding: "10px 14px",
        alignItems: "center",
        borderBottom: "1px solid var(--border-1)",
      }}
    >
      {/* Title + category dot */}
      <div
        style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 0 }}
      >
        <span
          style={{
            width: 6,
            height: 6,
            borderRadius: "50%",
            background: getCategoryColor(row.category),
            display: "inline-block",
            flexShrink: 0,
          }}
        />
        <Link
          href={`/m/${row.marketId}`}
          className="market-link"
          style={{
            fontFamily: "var(--font-sans)",
            fontSize: 12,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
            minWidth: 0,
          }}
        >
          {row.title}
        </Link>
      </div>

      {/* Clear price (real) */}
      <span style={cellNumber("var(--fg-1)", 12)}>
        {formatCents(row.clearPriceNanos)}
      </span>

      {/* Delta vs prev batch (real, signed cents) */}
      <span style={cellNumber(deltaColor, 11)}>{deltaText}</span>

      {/* Matched volume — real, per-market from by_market */}
      <span style={cellNumber("var(--fg-2)", 12)}>
        {formatCompactDollars(row.matchedVolumeNanos)}
      </span>

      {/* Welfare — real, per-market from by_market */}
      <span style={cellNumber("var(--yes)", 12)}>
        {row.welfareNanos >= 0n ? "+" : ""}
        {formatCompactDollarsCents(row.welfareNanos)}
      </span>

      {/* Placed / Matched — real, per-market from by_market */}
      <span style={cellNumber("var(--fg-2)", 11)}>
        <span style={{ color: "var(--fg-2)" }}>{row.ordersPlaced}</span>
        <span style={{ color: "var(--fg-4)" }}> / </span>
        <span style={{ color: "var(--yes)" }}>{row.ordersMatched}</span>
      </span>
    </div>
  );
}

// ── Sidebar building blocks ───────────────────────────────────────────────

function SidebarPanel({
  title,
  children,
}: {
  title?: string;
  children: React.ReactNode;
}) {
  return (
    <div
      style={{
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: 6,
        padding: "12px 14px",
      }}
    >
      {title != null && (
        <div
          className="eyebrow"
          style={{ color: "var(--fg-3)", paddingBottom: 10 }}
        >
          {title}
        </div>
      )}
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        {children}
      </div>
    </div>
  );
}

function KvRow({
  label,
  value,
  color,
}: {
  label: string;
  value: number;
  color?: string;
}) {
  return (
    <div
      style={{
        display: "flex",
        justifyContent: "space-between",
        alignItems: "center",
      }}
    >
      <span
        style={{
          fontFamily: "var(--font-sans)",
          fontSize: 12,
          color: "var(--fg-3)",
        }}
      >
        {label}
      </span>
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 12,
          color: color ?? "var(--fg-1)",
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {formatInt(value)}
      </span>
    </div>
  );
}

// ── Batch identity ─────────────────────────────────────────────────────────

function BatchIdentity({
  height,
  stateRoot,
  eventsRoot,
}: {
  height: number;
  stateRoot: string | null;
  eventsRoot: string | null;
}) {
  return (
    <>
      <IdentityRow label="Batch">
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 15,
            color: "var(--fg-1)",
            fontVariantNumeric: "tabular-nums",
          }}
        >
          #{formatInt(height)}
        </span>
      </IdentityRow>
      <IdentityRow label="State root">
        <span title={stateRoot ?? undefined} style={digestStyle}>
          {stateRoot ? shortDigest(stateRoot) : "—"}
        </span>
      </IdentityRow>
      <IdentityRow label="Events root">
        <span title={eventsRoot ?? undefined} style={digestStyle}>
          {eventsRoot ? shortDigest(eventsRoot) : "—"}
        </span>
      </IdentityRow>
    </>
  );
}

function IdentityRow({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "baseline",
        justifyContent: "space-between",
        gap: 10,
      }}
    >
      <span
        className="eyebrow"
        style={{
          color: "var(--fg-3)",
          whiteSpace: "nowrap",
          flexShrink: 0,
        }}
      >
        {label}
      </span>
      {children}
    </div>
  );
}

const digestStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  fontFamily: "var(--font-mono)",
  fontSize: 10,
  color: "var(--fg-2)",
  fontVariantNumeric: "tabular-nums",
};

function shortDigest(value: string): string {
  const hex = value.startsWith("0x") ? value.slice(2) : value;
  if (hex.length <= 22) return value;
  return `0x${hex.slice(0, 12)}···${hex.slice(-8)}`;
}

function cellNumber(color: string, fontSize: number): React.CSSProperties {
  return {
    fontFamily: "var(--font-mono)",
    fontSize,
    color,
    textAlign: "right",
    fontVariantNumeric: "tabular-nums",
  };
}
