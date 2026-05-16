"use client";

/**
 * History tab. Closed-position rows derived client-side from /fills +
 * /portfolio (OPEN_QUESTIONS #17). Each row carries a MockValue underline.
 * Matches `PortfolioVariants.jsx:196-239`.
 */

import Link from "next/link";
import { MockValue } from "@/components/mock-value";
import type { ClosedPosition } from "@/lib/account/use-closed-positions";
import { formatCents, formatDollars } from "@/lib/format/nanos";
import type { components } from "@/lib/api/schema";
import { CategoryDot } from "./category-dot";
import { SidePill } from "./side-pill";

type Market = components["schemas"]["MarketResponse"];

interface Props {
  closed: ClosedPosition[];
  marketsById: Map<number, Market>;
}

export function HistoryList({ closed, marketsById }: Props) {
  if (closed.length === 0) {
    return <Empty>No closed positions yet.</Empty>;
  }
  return (
    <div
      style={{
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: 6,
        overflow: "hidden",
      }}
    >
      <HeaderRow />
      {closed.map((c) => (
        <ClosedRow
          key={`${c.marketId}:${c.outcome}:${c.lastFillTimestampMs}`}
          row={c}
          market={marketsById.get(c.marketId)}
        />
      ))}
    </div>
  );
}

function HeaderRow() {
  return (
    <div style={rowGrid("var(--fg-4)")}>
      <span />
      <span>Market</span>
      <span>Side</span>
      <RightCell>Shares</RightCell>
      <RightCell>Entry</RightCell>
      <RightCell>Exit</RightCell>
      <RightCell>Realized P&amp;L</RightCell>
      <RightCell>Outcome</RightCell>
      <RightCell>Closed</RightCell>
    </div>
  );
}

function ClosedRow({
  row,
  market,
}: {
  row: ClosedPosition;
  market: Market | undefined;
}) {
  const tradedQty = Math.min(row.buyQty, row.sellQty);
  const positive = row.realizedNanos >= 0n;
  return (
    <Link
      href={`/m/${row.marketId}`}
      style={{
        ...rowGrid("var(--fg-2)"),
        textDecoration: "none",
        color: "inherit",
        borderTop: "1px solid var(--border-1)",
      }}
    >
      <CategoryDot market={market} />
      <span
        style={{
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          color: "var(--fg-1)",
          fontFamily: "var(--font-sans)",
          fontSize: 13,
        }}
        title={market?.name ?? `#${row.marketId}`}
      >
        {market?.name ?? `#${row.marketId}`}
      </span>
      <SidePill outcome={row.outcome} />
      <RightCell mono>{tradedQty}</RightCell>
      <RightCell mono>{formatCents(row.avgEntryNanos)}</RightCell>
      <RightCell mono>{formatCents(row.avgExitNanos)}</RightCell>
      <RightCell>
        <span
          style={{
            display: "inline-flex",
            flexDirection: "column",
            alignItems: "flex-end",
            gap: 1,
            fontFamily: "var(--font-mono)",
            color: positive ? "var(--yes)" : "var(--no)",
          }}
        >
          <MockValue
            hint="closed positions reconstructed client-side (OPEN_QUESTIONS #17)"
            variant="underline"
          >
            <span style={{ fontSize: 12 }}>
              {formatDollars(row.realizedNanos, { decimals: 2, sign: true })}
            </span>
          </MockValue>
          {row.realizedPct != null && (
            <span style={{ fontSize: 10 }}>
              {row.realizedPct >= 0 ? "+" : ""}
              {row.realizedPct.toFixed(2)}%
            </span>
          )}
        </span>
      </RightCell>
      <RightCell mono>sold</RightCell>
      <RightCell mono>{formatRelative(row.lastFillTimestampMs)}</RightCell>
    </Link>
  );
}

function rowGrid(color: string): React.CSSProperties {
  return {
    display: "grid",
    gridTemplateColumns:
      "14px minmax(0, 1.6fr) 50px 60px 60px 60px 110px 70px 90px",
    gap: 10,
    alignItems: "center",
    padding: "10px 14px",
    color,
    fontFamily: "var(--font-mono)",
    fontSize: 11,
    letterSpacing: "var(--track-wide)",
  };
}

function RightCell({
  children,
  mono,
}: {
  children: React.ReactNode;
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

function Empty({ children }: { children: React.ReactNode }) {
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

function formatRelative(ms: number): string {
  const diff = Date.now() - ms;
  if (diff < 60_000) return "just now";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
  return `${Math.floor(diff / 86_400_000)}d ago`;
}
