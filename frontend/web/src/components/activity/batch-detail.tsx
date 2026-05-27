"use client";

/**
 * Expanded panel for a batch row. Top: meta strip (tx hash / block / sequencer
 * / clearing duration / algo — most mocked, see OPEN_QUESTIONS). Body: 2-col
 * grid with a market-row table on the left and a donut + composition KV on
 * the right.
 *
 * Data comes from `useBatchDetail(height)`. The per-market rows are real —
 * volume, welfare and placed/matched come from `BlockResponse.by_market`.
 */

import Link from "next/link";
import { useMemo, useState } from "react";
import { MockValue } from "@/components/mock-value";
import { getCategoryColor } from "@/lib/categorize";
import {
  formatCents,
  formatCompactDollars,
  formatInt,
} from "@/lib/format/nanos";
import { mockClearingMs, mockTxHash, MOCK_SEQUENCER } from "@/lib/activity/mocks";
import { useBatchDetail } from "@/lib/activity/use-batch-detail";
import type { BatchMarketRow, BatchRow } from "@/lib/activity/types";
import { DonutOutcome } from "./donut-outcome";

const ROWS_INITIAL = 6;
const ROWS_STEP = 10;

const GRID = "2fr 70px 60px 110px 100px 130px";
const GRID_GAP = 12;

type SortKey = "clear" | "delta" | "volume" | "welfare" | "orders";
type SortDir = "asc" | "desc";

export function BatchDetail({ row }: { row: BatchRow }) {
  const { rows, isPending } = useBatchDetail(row.height);
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
      style={{
        background: "var(--bg-1)",
        borderBottom: "1px solid var(--border-1)",
        padding: "18px 24px 24px 70px",
      }}
    >
      {/* Meta strip */}
      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          gap: 32,
          paddingBottom: 16,
          borderBottom: "1px solid var(--border-1)",
          marginBottom: 18,
        }}
      >
        <MetaPair
          label="Tx hash"
          value={mockTxHash(row.height)}
          mono
          link
          mocked
          mockHint="tx hash — mock; backend doesn't expose a per-block DA commit hash today"
        />
        <MetaPair label="Block" value={`#${formatInt(row.height)}`} mono />
        <MetaPair
          label="Sequencer"
          value={MOCK_SEQUENCER}
          mono
          mocked
          mockHint="sequencer identity — mock; backend doesn't track this"
        />
        <MetaPair
          label="Clearing duration"
          value={`${mockClearingMs(row.height)} ms`}
          mono
          mocked
          mockHint="clearing duration — not instrumented on backend; deterministic placeholder"
        />
        <MetaPair label="Algo" value="uniform clearing · pro-rata" />
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "1fr 280px", gap: 24 }}>
        {/* Left: market rows */}
        <div>
          <div
            style={{
              background: "var(--surface-1)",
              border: "1px solid var(--border-1)",
              borderRadius: 6,
              overflow: "hidden",
            }}
          >
            <MarketTableHeader sort={sort} onSort={onSort} />
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
              <MarketRow key={m.marketId} row={m} />
            ))}

            {(remaining > 0 || shown > ROWS_INITIAL) && (
              <button
                onClick={() =>
                  setShown((s) =>
                    remaining > 0
                      ? Math.min(rows.length, s + ROWS_STEP)
                      : ROWS_INITIAL
                  )
                }
                style={{
                  display: "block",
                  width: "100%",
                  background: "transparent",
                  border: 0,
                  borderTop: "1px solid var(--border-1)",
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

        {/* Right: donut + composition KV */}
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <SidebarPanel title="Order outcome">
            <DonutOutcome
              matched={row.ordersMatched}
              unmatched={row.ordersUnmatched}
            />
          </SidebarPanel>
          <SidebarPanel title="Batch composition">
            <KvRow label="Markets" value={row.marketsTouched} />
            <KvRow label="Unique traders" value={row.uniqueTraders} />
            <KvRow label="Placed orders" value={row.ordersPlaced} />
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
        label="Placed / Matched"
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
      title={`Sort by ${label}`}
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
    if (a.ordersMatched !== b.ordersMatched) return b.ordersMatched - a.ordersMatched;
    if (a.ordersPlaced !== b.ordersPlaced) return b.ordersPlaced - a.ordersPlaced;
    return a.marketId - b.marketId;
  });
}

function MarketRow({ row }: { row: BatchMarketRow }) {
  const deltaCents =
    row.deltaNanos == null ? null : Number(row.deltaNanos) / 1e7;
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
      <div style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 0 }}>
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
          title={row.title}
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
      <span style={cellNumber("var(--fg-1)", 13)}>
        {formatCents(row.clearPriceNanos)}
      </span>

      {/* Delta vs prev batch (real, signed cents) */}
      <span
        style={cellNumber(
          deltaCents == null
            ? "var(--fg-4)"
            : deltaCents > 0
            ? "var(--yes)"
            : deltaCents < 0
            ? "var(--no)"
            : "var(--fg-3)",
          11
        )}
      >
        {deltaCents == null
          ? "—"
          : `${deltaCents >= 0 ? "+" : ""}${deltaCents.toFixed(1)}`}
      </span>

      {/* Matched volume — real, per-market from by_market */}
      <span style={cellNumber("var(--fg-2)", 12)}>
        {formatCompactDollars(row.matchedVolumeNanos)}
      </span>

      {/* Welfare — real, per-market from by_market */}
      <span style={cellNumber("var(--yes)", 12)}>
        {row.welfareNanos >= 0n ? "+" : ""}
        {formatCompactDollars(row.welfareNanos)}
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
  title: string;
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
      <div className="eyebrow" style={{ color: "var(--fg-3)", paddingBottom: 10 }}>
        {title}
      </div>
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

// ── Meta strip helper ─────────────────────────────────────────────────────

function MetaPair({
  label,
  value,
  mono,
  link,
  color,
  mocked,
  mockHint,
}: {
  label: string;
  value: string;
  mono?: boolean;
  link?: boolean;
  color?: string;
  mocked?: boolean;
  mockHint?: string;
}) {
  // When mocked, drop the link underline — the pill is the indicator and a
  // mock string isn't really clickable anyway.
  const showLinkUnderline = link && !mocked;
  const valueEl = (
    <span
      style={{
        fontFamily: mono ? "var(--font-mono)" : "var(--font-sans)",
        fontSize: 12,
        color: link ? "var(--accent)" : color ?? "var(--fg-1)",
        fontVariantNumeric: "tabular-nums",
        textDecoration: showLinkUnderline ? "underline" : "none",
        textUnderlineOffset: 2,
      }}
    >
      {value}
    </span>
  );
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 3 }}>
      <span className="eyebrow" style={{ color: "var(--fg-3)" }}>
        {label}
      </span>
      {mocked ? (
        <MockValue hint={mockHint ?? ""} variant="pill">
          {valueEl}
        </MockValue>
      ) : (
        valueEl
      )}
    </div>
  );
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
