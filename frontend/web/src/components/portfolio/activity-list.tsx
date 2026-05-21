"use client";

/**
 * Activity tab. Merges:
 *  - REST `/fills` (FILLED rows)
 *  - localStorage cancels (CANCELLED rows; OPEN_QUESTIONS #15)
 *
 * Matches `PortfolioVariants.jsx:241-268` row shape:
 *   state badge · action (BUY/SELL) · side · shares · market · @price · amount · batch + ago
 */

import Link from "next/link";
import { useMemo } from "react";
import { MockValue } from "@/components/mock-value";
import type { AccountFill } from "@/lib/account/use-account-fills";
import type { TrackedCancel } from "@/lib/account/use-cancelled-orders";
import { formatCents, formatDollars, parseNanos } from "@/lib/format/nanos";
import type { components } from "@/lib/api/schema";
import { SidePill } from "./side-pill";

type Market = components["schemas"]["MarketResponse"];

type Row =
  | {
      kind: "filled";
      timestampMs: number;
      blockHeight: number;
      action: "BUY" | "SELL";
      outcome: "YES" | "NO" | "";
      marketId: number;
      qty: number;
      priceNanos: bigint;
      amountNanos: bigint;
    }
  | {
      kind: "cancelled";
      timestampMs: number;
      blockHeight: null;
      action: "BUY" | "SELL";
      outcome: "YES" | "NO" | "";
      marketId: number;
      qty: number;
      priceNanos: bigint;
      amountNanos: bigint;
    };

interface Props {
  fills: AccountFill[];
  cancels: TrackedCancel[];
  marketsById: Map<number, Market>;
}

export function ActivityList({ fills, cancels, marketsById }: Props) {
  const rows = useMemo<Row[]>(() => {
    const out: Row[] = [];

    for (const f of fills) {
      const delta = f.position_deltas[0];
      if (!delta) continue;
      const action: "BUY" | "SELL" = delta.delta >= 0 ? "BUY" : "SELL";
      const outcome = normalizeOutcome(delta.outcome);
      const priceNanos = parseNanos(f.fill_price_nanos);
      out.push({
        kind: "filled",
        timestampMs: f.timestamp_ms,
        blockHeight: f.block_height,
        action,
        outcome,
        marketId: delta.market_id,
        qty: Math.abs(delta.delta),
        priceNanos,
        amountNanos: BigInt(Math.abs(delta.delta)) * priceNanos,
      });
    }

    for (const c of cancels) {
      const sideLower = c.side.toLowerCase();
      const action: "BUY" | "SELL" = sideLower.includes("buy") ? "BUY" : "SELL";
      const outcome = sideLower.includes("yes")
        ? "YES"
        : sideLower.includes("no")
          ? "NO"
          : "";
      const priceNanos = parseNanos(c.limitPriceNanos);
      out.push({
        kind: "cancelled",
        timestampMs: c.timestampMs,
        blockHeight: null,
        action,
        outcome,
        marketId: c.marketId,
        qty: c.qty,
        priceNanos,
        amountNanos: BigInt(c.qty) * priceNanos,
      });
    }

    out.sort((a, b) => b.timestampMs - a.timestampMs);
    return out.slice(0, 100);
  }, [fills, cancels]);

  if (rows.length === 0) {
    return <Empty>No activity yet.</Empty>;
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
      {rows.map((row, i) => (
        <RowComponent
          key={`${row.kind}-${row.timestampMs}-${i}`}
          row={row}
          market={marketsById.get(row.marketId)}
        />
      ))}
    </div>
  );
}

function HeaderRow() {
  return (
    <div style={rowGrid("var(--fg-4)")}>
      <span>State</span>
      <span>Action</span>
      <span>Side</span>
      <RightCell>Shares</RightCell>
      <span>Market</span>
      <RightCell>@ Price</RightCell>
      <RightCell>Amount</RightCell>
      <RightCell>Batch</RightCell>
    </div>
  );
}

function RowComponent({
  row,
  market,
}: {
  row: Row;
  market: Market | undefined;
}) {
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
      <StateBadge kind={row.kind} />
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 11,
          fontWeight: 600,
          color: row.action === "BUY" ? "var(--accent)" : "var(--no)",
          letterSpacing: "var(--track-wide)",
        }}
      >
        {row.action}
      </span>
      <SidePill outcome={row.outcome || "?"} />
      <RightCell mono>{row.qty}</RightCell>
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
      <RightCell mono>{formatCents(row.priceNanos)}</RightCell>
      <RightCell mono>
        {formatDollars(row.amountNanos, { decimals: 2 })}
      </RightCell>
      <RightCell>
        <span
          style={{
            display: "inline-flex",
            flexDirection: "column",
            alignItems: "flex-end",
            gap: 1,
          }}
        >
          <span
            className="tabular"
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: 11,
              color: "var(--accent)",
            }}
          >
            {row.blockHeight == null
              ? "—"
              : `#${row.blockHeight.toLocaleString()}`}
          </span>
          <span
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: 9.5,
              color: "var(--fg-4)",
              letterSpacing: "var(--track-wide)",
            }}
          >
            {formatRelative(row.timestampMs)}
          </span>
        </span>
      </RightCell>
    </Link>
  );
}

function StateBadge({ kind }: { kind: Row["kind"] }) {
  if (kind === "filled") {
    return (
      <span
        style={{
          display: "inline-block",
          padding: "1px 7px",
          background: "color-mix(in srgb, var(--yes) 14%, transparent)",
          color: "var(--yes)",
          borderRadius: 3,
          fontFamily: "var(--font-mono)",
          fontSize: 9.5,
          fontWeight: 600,
          letterSpacing: "var(--track-wide)",
        }}
      >
        FILLED
      </span>
    );
  }
  return (
    <MockValue
      hint="cancels tracked in localStorage; backend has no OrderCancelled event (OPEN_QUESTIONS #15)"
      variant="underline"
    >
      <span
        style={{
          display: "inline-block",
          padding: "1px 7px",
          background: "rgba(255,255,255,0.04)",
          color: "var(--fg-3)",
          borderRadius: 3,
          fontFamily: "var(--font-mono)",
          fontSize: 9.5,
          fontWeight: 600,
          letterSpacing: "var(--track-wide)",
        }}
      >
        CANCELLED
      </span>
    </MockValue>
  );
}

function normalizeOutcome(s: string): "YES" | "NO" | "" {
  const u = s.toUpperCase();
  if (u === "YES") return "YES";
  if (u === "NO") return "NO";
  return "";
}

function rowGrid(color: string): React.CSSProperties {
  return {
    display: "grid",
    gridTemplateColumns:
      "80px 50px 50px 70px minmax(0, 1.6fr) 70px 90px 110px",
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
