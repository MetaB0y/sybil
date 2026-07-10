"use client";

/**
 * Hero block — left side of the /portfolio top row. Big portfolio-value
 * number + delta + 2×2 stat grid. Matches handoff `VariantClassic` hero.
 */

import { formatDollars, parseNanos } from "@/lib/format/nanos";
import { selectBalances } from "@/lib/account/use-available-balance";
import type { PnlSplit } from "@/lib/account/use-pnl-split";
import type { Portfolio } from "@/lib/account/use-portfolio";
import type { EquityCurve } from "@/lib/account/use-equity-curve";

interface Props {
  portfolio: Portfolio | null;
  pnlSplit: PnlSplit | null;
  curve: EquityCurve | null;
  rangeLabel: string;
}

export function PortfolioHero({
  portfolio,
  pnlSplit,
  curve,
  rangeLabel,
}: Props) {
  const totalValue = portfolio
    ? parseNanos(portfolio.portfolio_value_nanos)
    : null;
  // "Cash" shows AVAILABLE (spendable) balance — total minus funds reserved by
  // open orders — so it matches what the trader can actually deploy. Reserved is
  // surfaced in the sub-line when non-zero.
  const { availableNanos: balance, reservedNanos } = selectBalances(portfolio);
  const positionsValue = portfolio
    ? parseNanos(portfolio.total_position_value_nanos)
    : null;
  const positionCount = portfolio?.positions.length ?? 0;

  // Delta line, all real. For the ALL range it's P&L since first deposit,
  // straight from the portfolio (`pnl_nanos` over `total_deposited`). For the
  // windowed ranges (24H/7D/30D) it's end−start over the real equity series.
  const isAllRange = curve?.range === "ALL";
  const pnlNanos = portfolio ? parseNanos(portfolio.pnl_nanos) : 0n;
  const depositedNanos = portfolio
    ? parseNanos(portfolio.total_deposited_nanos)
    : 0n;
  const realDeltaAbs = Number(pnlNanos) / 1e9;
  const realDeltaPct =
    depositedNanos === 0n ? 0 : (Number(pnlNanos) / Number(depositedNanos)) * 100;

  const deltaAbs = isAllRange ? realDeltaAbs : curve?.deltaAbs ?? 0;
  const deltaPct = isAllRange ? realDeltaPct : curve?.deltaPct ?? 0;
  const deltaPositive = deltaAbs >= 0;

  const deltaSpan = (
    <span
      style={{
        color: deltaPositive ? "var(--yes)" : "var(--no)",
        fontSize: 16,
      }}
    >
      {deltaPositive ? "▲" : "▼"} $
      {Math.abs(deltaAbs).toLocaleString(undefined, {
        minimumFractionDigits: 2,
        maximumFractionDigits: 2,
      })}{" "}
      {deltaPositive ? "+" : ""}
      {deltaPct.toFixed(2)}%
    </span>
  );

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-3)",
      }}
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <div
          className="tabular"
          style={{
            fontFamily: "var(--font-sans)",
            fontSize: "clamp(46px, 4.6vw, 64px)",
            fontWeight: 600,
            letterSpacing: "-0.02em",
            color: "var(--fg-1)",
            lineHeight: 0.95,
          }}
        >
          {totalValue == null ? "—" : formatDollars(totalValue, { decimals: 2 })}
        </div>
        <div
          style={{
            display: "flex",
            alignItems: "baseline",
            gap: 8,
            fontFamily: "var(--font-mono)",
            fontSize: 13,
          }}
        >
          {deltaSpan}
          <span
            style={{
              color: "var(--fg-4)",
              fontSize: 11,
              letterSpacing: "var(--track-wide)",
              textTransform: "uppercase",
            }}
          >
            {rangeLabel}
          </span>
        </div>
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "1fr 1fr",
          columnGap: 32,
          rowGap: 18,
          paddingTop: 22,
          marginTop: 6,
          borderTop: "1px solid var(--border-1)",
        }}
      >
        <Stat
          label="Positions value"
          primary={
            positionsValue == null
              ? "—"
              : formatDollars(positionsValue, { decimals: 2 })
          }
          sub={`${positionCount} open`}
        />
        <Stat
          label="Cash"
          primary={
            balance == null ? "—" : formatDollars(balance, { decimals: 2 })
          }
          sub={
            reservedNanos > 0n
              ? `${formatDollars(reservedNanos, { decimals: 2 })} reserved`
              : "available"
          }
        />
        <Stat
          label="Unrealized P&L"
          primary={
            pnlSplit == null
              ? "—"
              : formatDollars(pnlSplit.unrealizedNanos, {
                  decimals: 2,
                  sign: true,
                })
          }
          tone={
            pnlSplit == null
              ? "neutral"
              : pnlSplit.unrealizedNanos >= 0n
                ? "yes"
                : "no"
          }
        />
        <Stat
          label="Realized P&L"
          primary={
            pnlSplit == null
              ? "—"
              : formatDollars(pnlSplit.realizedNanos, {
                  decimals: 2,
                  sign: true,
                })
          }
          tone={
            pnlSplit == null
              ? "neutral"
              : pnlSplit.realizedNanos >= 0n
                ? "yes"
                : "no"
          }
        />
      </div>
    </div>
  );
}

function Stat({
  label,
  primary,
  sub,
  tone = "neutral",
}: {
  label: string;
  primary: React.ReactNode;
  /** Optional caption under the number. Realized P&L has none — a trade count
   *  told the reader nothing the P&L figure didn't already say. */
  sub?: string;
  tone?: "yes" | "no" | "neutral";
}) {
  const color =
    tone === "yes"
      ? "var(--yes)"
      : tone === "no"
        ? "var(--no)"
        : "var(--fg-1)";
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 4,
        minWidth: 0,
      }}
    >
      <Eyebrow>{label}</Eyebrow>
      <span
        className="tabular"
        style={{
          fontFamily: "var(--font-sans)",
          fontSize: 20,
          fontWeight: 600,
          letterSpacing: "-0.01em",
          lineHeight: 1,
          color,
        }}
      >
        {primary}
      </span>
      {sub && (
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 10,
            color: "var(--fg-3)",
            letterSpacing: "var(--track-wide)",
            textTransform: "uppercase",
          }}
        >
          {sub}
        </span>
      )}
    </div>
  );
}

function Eyebrow({ children }: { children: React.ReactNode }) {
  return (
    <span
      style={{
        fontFamily: "var(--font-mono)",
        fontSize: 10,
        color: "var(--fg-3)",
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
      }}
    >
      {children}
    </span>
  );
}
