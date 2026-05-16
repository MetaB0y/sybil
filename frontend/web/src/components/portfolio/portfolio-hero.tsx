"use client";

/**
 * Hero block — left side of the /portfolio top row. Big portfolio-value
 * number + delta + 2×2 stat grid. Matches handoff `VariantClassic` hero.
 */

import { MockValue } from "@/components/mock-value";
import { formatDollars, parseNanos } from "@/lib/format/nanos";
import type { PnlSplit } from "@/lib/account/use-pnl-split";
import type { Portfolio } from "@/lib/account/use-portfolio";
import type { EquityCurve } from "@/lib/account/use-equity-curve";

interface Props {
  portfolio: Portfolio | null;
  pnlSplit: PnlSplit | null;
  curve: EquityCurve | null;
  tradeCount: number;
  tradeCountCapped: boolean;
  rangeLabel: string;
}

export function PortfolioHero({
  portfolio,
  pnlSplit,
  curve,
  tradeCount,
  tradeCountCapped,
  rangeLabel,
}: Props) {
  const totalValue = portfolio
    ? parseNanos(portfolio.portfolio_value_nanos)
    : null;
  const balance = portfolio ? parseNanos(portfolio.balance_nanos) : null;
  const positionsValue = portfolio
    ? parseNanos(portfolio.total_position_value_nanos)
    : null;
  const positionCount = portfolio?.positions.length ?? 0;

  // Delta line: use mocked equity curve for ranges; for ALL we still get
  // a real-ish number because curve.deltaAbs is anchored on total_deposited.
  const deltaAbs = curve?.deltaAbs ?? 0;
  const deltaPct = curve?.deltaPct ?? 0;
  const deltaPositive = deltaAbs >= 0;

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-3)",
      }}
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <Eyebrow>Portfolio value</Eyebrow>
        <div
          className="tabular"
          style={{
            fontFamily: "var(--font-display)",
            fontSize: "clamp(40px, 6vw, 56px)",
            fontWeight: 600,
            letterSpacing: "var(--track-tight)",
            color: "var(--fg-1)",
            lineHeight: 1,
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
          <MockValue
            hint="NOT NOW — delta is computed against a mocked equity curve (OPEN_QUESTIONS #12)"
            variant="underline"
          >
            <span
              style={{
                color: deltaPositive ? "var(--yes)" : "var(--no)",
              }}
            >
              {deltaPositive ? "▲" : "▼"} ${Math.abs(deltaAbs).toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}{" "}
              {deltaPositive ? "+" : ""}
              {deltaPct.toFixed(2)}%
            </span>
          </MockValue>
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
          gap: 1,
          background: "var(--border-1)",
          border: "1px solid var(--border-1)",
          borderRadius: 6,
          overflow: "hidden",
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
          sub="available"
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
          sub="open positions"
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
          sub={`${tradeCount}${tradeCountCapped ? "+" : ""} trades`}
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
  sub: string;
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
        background: "var(--surface-1)",
        padding: "12px 14px",
        display: "flex",
        flexDirection: "column",
        gap: 4,
      }}
    >
      <Eyebrow>{label}</Eyebrow>
      <span
        className="tabular"
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 18,
          fontWeight: 500,
          color,
        }}
      >
        {primary}
      </span>
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 10,
          color: "var(--fg-4)",
          letterSpacing: "var(--track-wide)",
          textTransform: "uppercase",
        }}
      >
        {sub}
      </span>
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
