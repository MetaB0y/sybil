"use client";

/**
 * Positions tab. Grid rows matching `PortfolioVariants.jsx:11-60`:
 *   dot · market · side · shares · entry¢ · mark¢ · spark · value · pnl ($+%) · resolves
 */

import Link from "next/link";
import { CategoryDot } from "./category-dot";
import { PositionSparkline } from "./position-sparkline";
import { SidePill } from "./side-pill";
import { avgEntryPriceNanos } from "@/lib/account/positions";
import type { AccountFill } from "@/lib/account/use-account-fills";
import type { Portfolio } from "@/lib/account/use-portfolio";
import { formatCents, formatDollars, parseNanos } from "@/lib/format/nanos";
import type { components } from "@/lib/api/schema";

type Market = components["schemas"]["MarketResponse"];
type Position = Portfolio["positions"][number];

interface Props {
  positions: Position[];
  fills: AccountFill[];
  marketsById: Map<number, Market>;
}

export function PositionsList({ positions, fills, marketsById }: Props) {
  if (positions.length === 0) {
    return <Empty>No open positions.</Empty>;
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
      {positions.map((p) => (
        <PositionRow
          key={`${p.market_id}:${p.outcome}`}
          position={p}
          market={marketsById.get(p.market_id)}
          fills={fills}
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
      <RightCell>Mark</RightCell>
      <span style={{ textAlign: "center" }}>7d</span>
      <RightCell>Value</RightCell>
      <RightCell>P&amp;L</RightCell>
      <RightCell>Resolves</RightCell>
    </div>
  );
}

function PositionRow({
  position,
  market,
  fills,
}: {
  position: Position;
  market: Market | undefined;
  fills: AccountFill[];
}) {
  const avgNanos = avgEntryPriceNanos(
    fills,
    position.market_id,
    position.outcome,
    position,
  );
  const valueNanos = parseNanos(position.value_nanos);
  const markNanos = parseNanos(position.current_price_nanos);

  const costNanos =
    avgNanos == null ? null : (BigInt(position.quantity) * avgNanos) / 1_000_000_000n;
  const pnlNanos = costNanos == null ? null : valueNanos - costNanos;
  const pnlPct =
    costNanos == null || costNanos === 0n
      ? null
      : (Number(pnlNanos! * 10000n / costNanos) / 100);
  const resolvesText = formatResolves(market);

  return (
    <Link
      href={`/m/${position.market_id}`}
      style={{
        ...rowGrid("var(--fg-2)"),
        textDecoration: "none",
        color: "inherit",
        borderTop: "1px solid var(--border-1)",
        transition: "background var(--dur-fast) var(--ease-standard)",
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
        title={market?.name ?? `#${position.market_id}`}
      >
        {market?.name ?? `#${position.market_id}`}
      </span>
      <SidePill outcome={position.outcome} />
      <RightCell mono>{position.quantity}</RightCell>
      <RightCell mono>{avgNanos == null ? "—" : formatCents(avgNanos)}</RightCell>
      <RightCell mono>{formatCents(markNanos)}</RightCell>
      <span style={{ display: "flex", justifyContent: "center" }}>
        <PositionSparkline marketId={position.market_id} outcome={position.outcome} />
      </span>
      <RightCell mono>{formatDollars(valueNanos, { decimals: 2 })}</RightCell>
      <RightCell>
        <span
          style={{
            display: "inline-flex",
            flexDirection: "column",
            alignItems: "flex-end",
            gap: 1,
            fontFamily: "var(--font-mono)",
            color:
              pnlNanos == null
                ? "var(--fg-3)"
                : pnlNanos >= 0n
                  ? "var(--yes)"
                  : "var(--no)",
          }}
        >
          <span style={{ fontSize: 12 }}>
            {pnlNanos == null
              ? "—"
              : formatDollars(pnlNanos, { decimals: 2, sign: true })}
          </span>
          <span style={{ fontSize: 10 }}>
            {pnlPct == null
              ? ""
              : `${pnlPct >= 0 ? "+" : ""}${pnlPct.toFixed(2)}%`}
          </span>
        </span>
      </RightCell>
      <RightCell mono>{resolvesText}</RightCell>
    </Link>
  );
}

function rowGrid(color: string): React.CSSProperties {
  return {
    display: "grid",
    gridTemplateColumns:
      "14px minmax(0, 1.4fr) 50px 70px 60px 60px 96px 80px 90px 110px",
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

function formatResolves(market: Market | undefined): string {
  const ms =
    market?.market_end_date_ms ?? market?.event_end_date_ms ?? null;
  if (!ms) return "—";
  const date = new Date(ms);
  const days = Math.round((ms - Date.now()) / 86400000);
  const dateStr = `${date.toLocaleString(undefined, { month: "short" })} ${date.getDate()}`;
  if (days < 0) return `${dateStr} · past`;
  if (days === 0) return `${dateStr} · today`;
  return `${dateStr} · ${days}d`;
}
