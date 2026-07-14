"use client";

import { useState, type CSSProperties } from "react";

import { DataTable, Td, Th } from "@/components/dev/primitives/data-table";
import { Panel, PanelBody, PanelHead } from "@/components/dev/primitives/panel";
import {
  accountAggregates,
  marketIndex,
  participantRoleIndex,
  participantRoleLabel,
  pendingByAccount,
  pendingIndex,
  topPendingMarkets,
} from "@/lib/dev/derive";
import {
  useDevAccountFills,
  useDevAccounts,
  useDevBots,
  useDevLiquidityHealth,
  useDevMarkets,
  useDevPendingOrders,
} from "@/lib/dev/fetchers";
import {
  dollars,
  fmtInt,
  fmtPrice,
  moneySigned,
} from "@/lib/dev/format";
import { Stat, StatGrid } from "@/components/dev/primitives/stat";

const mutedSpan: CSSProperties = { color: "var(--fg-3)", fontSize: 12 };

const emptyMsg: CSSProperties = {
  padding: "16px 4px",
  textAlign: "center",
  color: "var(--fg-4)",
  fontSize: 12,
};

const chipBase: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  alignItems: "flex-start",
  gap: 2,
  flex: "0 0 auto",
  padding: "7px 10px",
  borderRadius: 8,
  border: "1px solid var(--border-2)",
  background: "var(--surface-2)",
  color: "var(--fg-2)",
  fontFamily: "inherit",
  fontSize: 11,
  cursor: "pointer",
  textAlign: "left",
};

const truncCell: CSSProperties = {
  padding: "7px 9px",
  borderBottom: "1px solid var(--border-2)",
  verticalAlign: "top",
  maxWidth: 280,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
};

export function AccountsView() {
  const [selectedAccountId, setSelectedAccountId] = useState<number | null>(
    null,
  );
  const [outcomeFilter, setOutcomeFilter] = useState<"ALL" | "YES" | "NO">(
    "ALL",
  );

  const accounts = useDevAccounts().data ?? [];
  const liquidityHealth = useDevLiquidityHealth().data;
  const bots = useDevBots().data;
  const pendingOrders = useDevPendingOrders().data ?? [];
  const markets = useDevMarkets().data ?? [];

  const mIdx = marketIndex(markets);
  const pendIdx = pendingIndex(pendingOrders);
  const pendByAcct = pendingByAccount(pendingOrders);

  const roles = participantRoleIndex(liquidityHealth, bots);
  const agg = accountAggregates(accounts, selectedAccountId, pendByAcct);
  const topPositions = agg.positionsByValue
    .filter((position) => outcomeFilter === "ALL" || position.outcome === outcomeFilter)
    .slice(0, 25);

  const fills =
    useDevAccountFills(agg.activeTradingAccounts.map((a) => a.account_id))
      .data ?? {};

  const accountPendingCount = (id: number): number => pendByAcct.get(id) ?? 0;
  const marketName = (id: number): string => {
    const m = mIdx.get(Number(id));
    return m ? m.name : "#" + id;
  };
  const shares = (quantity: number): string =>
    (Number(quantity) / 1_000).toLocaleString(undefined, {
      maximumFractionDigits: 3,
    });
  const outcomeSummary = (account: (typeof accounts)[number], outcome: "YES" | "NO") => {
    const positions = (account.positions ?? []).filter((position) => position.outcome === outcome);
    return {
      count: positions.length,
      quantity: positions.reduce((sum, position) => sum + Number(position.quantity || 0), 0),
      valueNanos: positions.reduce(
        (sum, position) => sum + Number(position.value_nanos || 0),
        0,
      ),
    };
  };

  return (
    <div>
      {/* Two-column row */}
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "minmax(0,1.4fr) minmax(360px,0.8fr)",
          gap: 12,
        }}
      >
        {/* Left — Active Trading Accounts */}
        <Panel>
          <PanelHead
            title="Active Trading Accounts"
            actions={
              <span style={mutedSpan}>
                Canonical cash, portfolios, and PnL from Sybil
              </span>
            }
          />
          <PanelBody>
            <div
              style={{
                display: "flex",
                gap: 8,
                overflowX: "auto",
                paddingBottom: 4,
                marginBottom: 12,
              }}
            >
              <button
                type="button"
                onClick={() => setSelectedAccountId(null)}
                style={
                  selectedAccountId === null
                    ? {
                        ...chipBase,
                        border: "1px solid var(--accent)",
                        color: "var(--accent)",
                      }
                    : chipBase
                }
              >
                <strong>Aggregate</strong>
                <span>
                  {agg.activeTradingAccounts.length + " active accounts"}
                </span>
              </button>
              {agg.activeTradingAccounts.map((a) => {
                const active = selectedAccountId === a.account_id;
                return (
                  <button
                    key={a.account_id}
                    type="button"
                    onClick={() => setSelectedAccountId(a.account_id)}
                    style={
                      active
                        ? {
                            ...chipBase,
                            border: "1px solid var(--accent)",
                            color: "var(--accent)",
                          }
                        : chipBase
                    }
                  >
                    <strong>
                      {participantRoleLabel(a.account_id, roles) + " #" + a.account_id}
                    </strong>
                    <span>
                      {accountPendingCount(a.account_id) +
                        " pending, " +
                        (a.positions ?? []).length +
                        " pos"}
                    </span>
                  </button>
                );
              })}
            </div>

            <StatGrid columns={4}>
              <Stat
                label="Scope"
                value={agg.selectedTradingAccounts.length}
                sub={
                  selectedAccountId === null
                    ? "active trading accounts"
                    : "selected account #" + selectedAccountId
                }
              />
              <Stat
                label="Pending Orders"
                value={fmtInt(agg.pendingOrders)}
                tone="warn"
                sub="resting liquidity"
              />
              <Stat
                label="Cash"
                value={"$" + dollars(agg.cashNanos)}
                sub="available cash"
              />
              <Stat
                label="Sybil Portfolio"
                value={"$" + dollars(agg.portfolioValueNanos)}
                tone="cyan"
                sub="canonical Sybil mark"
              />
              <Stat
                label="Sybil PnL"
                value={moneySigned(agg.pnlNanos / 1e9)}
                tone={agg.pnlNanos >= 0 ? "yes" : "no"}
                sub="canonical account PnL"
              />
              <Stat
                label="Positions"
                value={fmtInt(agg.positionCount)}
                sub="all outcomes"
              />
              <Stat
                label="YES Positions"
                value={fmtInt(agg.yes.count)}
                tone="yes"
                sub={`${shares(agg.yes.quantity)} shares · $${dollars(agg.yes.valueNanos)}`}
              />
              <Stat
                label="NO Positions"
                value={fmtInt(agg.no.count)}
                tone="no"
                sub={`${shares(agg.no.quantity)} shares · $${dollars(agg.no.valueNanos)}`}
              />
            </StatGrid>

            <div
              style={{
                margin: "14px 0 8px",
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
              }}
            >
              <h3
                style={{
                  margin: 0,
                  fontSize: 10,
                  fontWeight: 650,
                  letterSpacing: 0.4,
                  textTransform: "uppercase",
                  color: "var(--fg-3)",
                }}
              >
                Top 25 Positions by Sybil Value
              </h3>
              <div style={{ display: "flex", gap: 5 }}>
                {(["ALL", "YES", "NO"] as const).map((outcome) => (
                  <button
                    key={outcome}
                    type="button"
                    onClick={() => setOutcomeFilter(outcome)}
                    style={
                      outcomeFilter === outcome
                        ? { ...chipBase, padding: "4px 8px", borderColor: "var(--accent)", color: "var(--accent)" }
                        : { ...chipBase, padding: "4px 8px" }
                    }
                  >
                    {outcome === "ALL" ? "All" : outcome}
                  </button>
                ))}
              </div>
            </div>
            <DataTable maxHeight={300}>
              <thead>
                <tr>
                  <Th>Account</Th>
                  <Th>Market</Th>
                  <Th>Outcome</Th>
                  <Th align="right">Qty</Th>
                  <Th align="right">Sybil Value</Th>
                </tr>
              </thead>
              <tbody>
                {topPositions.map((p) => (
                  <tr
                    key={p.account_id + "-" + p.market_id + "-" + p.outcome}
                  >
                    <Td mono tone="accent">
                      {"#" + p.account_id}
                    </Td>
                    <td style={truncCell} title={marketName(p.market_id)}>
                      {marketName(p.market_id)}
                    </td>
                    <Td tone={p.outcome === "YES" ? "yes" : "no"}>
                      {p.outcome}
                    </Td>
                    <Td mono align="right">
                      {shares(p.quantity)}
                    </Td>
                    <Td mono align="right">
                      {"$" + dollars(p.value_nanos)}
                    </Td>
                  </tr>
                ))}
                {topPositions.length === 0 ? (
                  <tr>
                    <td colSpan={5} style={emptyMsg}>
                      No positions for the selected scope.
                    </td>
                  </tr>
                ) : null}
              </tbody>
            </DataTable>
          </PanelBody>
        </Panel>

        {/* Right — Participants */}
        <Panel>
          <PanelHead title="Participants" />
          <PanelBody>
            <DataTable maxHeight={520} minWidth={920}>
              <thead>
                <tr>
                  <Th>Account</Th>
                  <Th>Role</Th>
                  <Th align="right">Cash</Th>
                  <Th align="right">Portfolio</Th>
                  <Th align="right">PnL</Th>
                  <Th align="right">YES Positions</Th>
                  <Th align="right">NO Positions</Th>
                  <Th align="right">Pending</Th>
                  <Th align="right">Recent Fills</Th>
                </tr>
              </thead>
              <tbody>
                {accounts.map((a) => {
                  const pnl = (Number(a.pnl_nanos) || 0) / 1e9;
                  const yes = outcomeSummary(a, "YES");
                  const no = outcomeSummary(a, "NO");
                  return (
                    <tr key={a.account_id}>
                      <Td mono tone="accent">
                        {"#" + a.account_id}
                      </Td>
                      <Td>{participantRoleLabel(a.account_id, roles)}</Td>
                      <Td mono align="right">
                        {"$" + dollars(a.balance_nanos)}
                      </Td>
                      <Td mono align="right">
                        {"$" + dollars(a.portfolio_value_nanos)}
                      </Td>
                      <Td mono tone={pnl >= 0 ? "yes" : "no"} align="right">
                        {moneySigned(pnl)}
                      </Td>
                      <Td mono tone="yes" align="right">
                        {`${yes.count} · ${shares(yes.quantity)} sh · $${dollars(yes.valueNanos)}`}
                      </Td>
                      <Td mono tone="no" align="right">
                        {`${no.count} · ${shares(no.quantity)} sh · $${dollars(no.valueNanos)}`}
                      </Td>
                      <Td mono tone="warn" align="right">
                        {accountPendingCount(a.account_id)}
                      </Td>
                      <Td mono tone="yes" align="right">
                        {fills[a.account_id]?.length ?? 0}
                      </Td>
                    </tr>
                  );
                })}
                {accounts.length === 0 ? (
                  <tr>
                    <td colSpan={9} style={emptyMsg}>
                      Loading account portfolios...
                    </td>
                  </tr>
                ) : null}
              </tbody>
            </DataTable>
          </PanelBody>
        </Panel>
      </div>

      {/* Full-width — Pending Order Concentration */}
      <Panel style={{ marginTop: 12 }}>
        <PanelHead title="Pending Order Concentration" />
        <PanelBody>
          <DataTable>
            <thead>
              <tr>
                <Th>Market</Th>
                <Th align="right">Pending</Th>
                <Th align="right">Buy Yes</Th>
                <Th align="right">Buy No</Th>
                <Th align="right">Sell Yes</Th>
                <Th align="right">Sell No</Th>
                <Th align="right">Clearing</Th>
                <Th align="right">Reference</Th>
              </tr>
            </thead>
            <tbody>
              {topPendingMarkets(pendIdx).map((row) => {
                const m = mIdx.get(row.market_id);
                return (
                  <tr key={row.market_id}>
                    <td
                      style={truncCell}
                      title={"#" + row.market_id + " " + marketName(row.market_id)}
                    >
                      {"#" + row.market_id + " " + marketName(row.market_id)}
                    </td>
                    <Td mono tone="warn" align="right">
                      {row.count}
                    </Td>
                    <Td mono align="right">
                      {row.BuyYes || 0}
                    </Td>
                    <Td mono align="right">
                      {row.BuyNo || 0}
                    </Td>
                    <Td mono align="right">
                      {row.SellYes || 0}
                    </Td>
                    <Td mono align="right">
                      {row.SellNo || 0}
                    </Td>
                    <Td mono tone="yes" align="right">
                      {fmtPrice(m?.yes_price_nanos)}
                    </Td>
                    <Td mono tone="accent" align="right">
                      {fmtPrice(m?.reference_price_nanos)}
                    </Td>
                  </tr>
                );
              })}
              {topPendingMarkets(pendIdx).length === 0 ? (
                <tr>
                  <td colSpan={8} style={emptyMsg}>
                    No pending orders are visible from /v1/orders/pending.
                  </td>
                </tr>
              ) : null}
            </tbody>
          </DataTable>
        </PanelBody>
      </Panel>
    </div>
  );
}
