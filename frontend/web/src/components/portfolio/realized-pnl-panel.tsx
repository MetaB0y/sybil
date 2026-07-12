"use client";

/**
 * "P&L" tab — cumulative realized-PnL over time. The panel owns the toolbar
 * (shared tab strip) and a summary line; the curve itself is `RealizedPnlChart`.
 * Realized PnL is the backend's per-fill figure (WAC cost basis) summed
 * chronologically — see `lib/account/realized-pnl.ts`.
 */

import { useMemo } from "react";
import { cumulativeRealizedPnl, totalRealizedPnl } from "@/lib/account/realized-pnl";
import type { HistoryEvent } from "@/lib/account/use-account-history";
import { formatDollars } from "@/lib/format/nanos";
import { PortfolioToolbar } from "./portfolio-toolbar";
import { RealizedPnlChart } from "./realized-pnl-chart";

interface Props {
  tabs: React.ReactNode;
  events: HistoryEvent[];
  isLoading?: boolean;
}

export function RealizedPnlPanel({ tabs, events, isLoading = false }: Props) {
  const points = useMemo(() => cumulativeRealizedPnl(events), [events]);
  const total = totalRealizedPnl(points);
  const tone = total > 0n ? "var(--yes)" : total < 0n ? "var(--no)" : "var(--fg-2)";

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-3)" }}>
      <PortfolioToolbar tabs={tabs} />

      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          gap: 10,
          flexWrap: "wrap",
        }}
      >
        <span
          style={{
            fontSize: 11,
            letterSpacing: "0.04em",
            textTransform: "uppercase",
            color: "var(--fg-4)",
            fontFamily: "var(--font-mono)",
          }}
        >
          Realized P&L
        </span>
        <span
          className="tabular"
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 22,
            fontWeight: 600,
            color: tone,
            fontVariantNumeric: "tabular-nums",
          }}
        >
          {formatDollars(total, { sign: true })}
        </span>
      </div>

      <RealizedPnlChart points={points} isLoading={isLoading} />
    </div>
  );
}
