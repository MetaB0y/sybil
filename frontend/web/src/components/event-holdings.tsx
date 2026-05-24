"use client";

/**
 * EventHoldings — the connected user's open positions across every market in
 * this event (all outcomes of a multi-outcome event, or just the one binary
 * market). Sits in the left column under the chart. Renders nothing when the
 * user is disconnected or holds nothing in this event, so it never shows an
 * empty shell.
 *
 * Units/PnL math mirror the portfolio `PositionsList` so the two never drift:
 * `value_nanos` for current value, `quantity * avg_entry` for cost basis,
 * value − cost for unrealized PnL.
 */

import { useAccountSession } from "@/lib/account/use-account";
import {
  useAccountFills,
  type AccountFill,
} from "@/lib/account/use-account-fills";
import { avgEntryPriceNanos } from "@/lib/account/positions";
import { usePortfolio, type Portfolio } from "@/lib/account/use-portfolio";
import { formatDollars, parseNanos } from "@/lib/format/nanos";
import { useEventGroup } from "@/lib/market-detail/use-event-group";
import { SidePill } from "@/components/portfolio/side-pill";

type Position = Portfolio["positions"][number];

export function EventHoldings({ marketId }: { marketId: number }) {
  const session = useAccountSession();
  const accountId = session?.accountId ?? null;
  const { group } = useEventGroup(marketId);
  const portfolio = usePortfolio(accountId);
  const fills = useAccountFills(accountId).data ?? [];

  // shortLabel per market, so each holding reads like the outcome picker.
  const labelByMarket = new Map(
    (group?.outcomes ?? []).map((o) => [o.marketId, o.shortLabel]),
  );
  const eventMarketIds = new Set(labelByMarket.keys());

  const rows = (portfolio.data?.positions ?? []).filter(
    (p) => eventMarketIds.has(p.market_id) && p.quantity !== 0,
  );

  // Disconnected or nothing held in this event → don't take up space.
  if (accountId === null || rows.length === 0) return null;

  return (
    <section
      style={{
        padding: "var(--space-5)",
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-lg)",
        boxShadow: "var(--shadow-inset-top)",
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-3)",
      }}
    >
      <div className="eyebrow">{"// your holdings"}</div>
      <div>
        <Row header>
          <span>Outcome</span>
          <span>Side</span>
          <Right>Shares</Right>
          <Right>Value</Right>
          <Right>P&amp;L</Right>
        </Row>
        {rows.map((p) => (
          <HoldingRow
            key={`${p.market_id}:${p.outcome}`}
            position={p}
            label={labelByMarket.get(p.market_id) ?? `#${p.market_id}`}
            fills={fills}
          />
        ))}
      </div>
    </section>
  );
}

function HoldingRow({
  position,
  label,
  fills,
}: {
  position: Position;
  label: string;
  fills: AccountFill[];
}) {
  const valueNanos = parseNanos(position.value_nanos);
  const avgNanos = avgEntryPriceNanos(
    fills,
    position.market_id,
    position.outcome,
    position,
  );
  const costNanos =
    avgNanos == null ? null : BigInt(position.quantity) * avgNanos;
  const pnlNanos = costNanos == null ? null : valueNanos - costNanos;

  return (
    <Row>
      <span
        title={label}
        style={{
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          color: "var(--fg-1)",
          fontFamily: "var(--font-sans)",
          fontSize: 13,
        }}
      >
        {label}
      </span>
      <span>
        <SidePill outcome={position.outcome} />
      </span>
      <Right mono>{position.quantity}</Right>
      <Right mono>{formatDollars(valueNanos, { decimals: 2 })}</Right>
      <Right>
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 12,
            color:
              pnlNanos == null
                ? "var(--fg-3)"
                : pnlNanos >= 0n
                  ? "var(--yes)"
                  : "var(--no)",
          }}
        >
          {pnlNanos == null
            ? "—"
            : formatDollars(pnlNanos, { decimals: 2, sign: true })}
        </span>
      </Right>
    </Row>
  );
}

function Row({
  children,
  header,
}: {
  children: React.ReactNode;
  header?: boolean;
}) {
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "minmax(0, 1fr) 52px 64px 80px 80px",
        gap: 10,
        alignItems: "center",
        padding: "9px 0",
        borderTop: header ? undefined : "1px solid var(--border-1)",
        fontFamily: "var(--font-mono)",
        fontSize: header ? 10 : 11,
        letterSpacing: "var(--track-wide)",
        textTransform: header ? "uppercase" : undefined,
        color: header ? "var(--fg-4)" : "var(--fg-2)",
      }}
    >
      {children}
    </div>
  );
}

function Right({
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
        whiteSpace: "nowrap",
        fontFamily: mono ? "var(--font-mono)" : "inherit",
        fontSize: mono ? 12 : undefined,
        color: mono ? "var(--fg-1)" : undefined,
      }}
    >
      {children}
    </span>
  );
}
