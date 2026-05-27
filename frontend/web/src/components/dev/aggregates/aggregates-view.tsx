"use client";

import { useState, type CSSProperties } from "react";

import { DataTable, Td, Th } from "@/components/dev/primitives/data-table";
import { Panel, PanelBody, PanelHead } from "@/components/dev/primitives/panel";
import { Pill } from "@/components/dev/primitives/pill";
import { Stat, StatGrid } from "@/components/dev/primitives/stat";
import {
  cancelMarketLabel,
  cancelMarketTitle,
  cancelSideClass,
  fmtLiquidity,
  fmtYesDelta24h,
  latestBlockByMarketRows,
  marketIndex,
  orderStatsSub,
  recentCancellations,
  topMarketsByVolume24h,
  yesDelta24hClass,
} from "@/lib/dev/derive";
import {
  useDevActivityOverview,
  useDevMarkets,
  useDevOpenBatch,
  useDevPortfolio,
} from "@/lib/dev/fetchers";
import {
  dollars,
  fmtInt,
  fmtPrice,
  moneySigned,
  shortTime,
} from "@/lib/dev/format";
import { useDevRecentBlocks } from "@/lib/dev/use-recent-blocks";

const controlStyle: CSSProperties = {
  border: "1px solid var(--border-2)",
  background: "var(--surface-1)",
  color: "var(--fg-1)",
  borderRadius: 6,
  padding: "7px 9px",
  fontFamily: "inherit",
  fontSize: 12,
};

const truncCell: CSSProperties = {
  padding: "7px 9px",
  borderBottom: "1px solid var(--border-2)",
  verticalAlign: "top",
  maxWidth: 320,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
};

const mutedSpan: CSSProperties = { color: "var(--fg-3)", fontSize: 12 };

const emptyMsg: CSSProperties = {
  padding: "16px 4px",
  textAlign: "center",
  color: "var(--fg-4)",
  fontSize: 12,
};

export function AggregatesView() {
  const [openBatchMarketId, setOpenBatchMarketId] = useState<number>(0);
  const [accountInput, setAccountInput] = useState<string>("");
  const [accountId, setAccountId] = useState<number>(0);

  const markets = useDevMarkets().data ?? [];
  const activityOverview = useDevActivityOverview().data ?? {};
  const { blocks, latestBlock } = useDevRecentBlocks();
  const openBatch = useDevOpenBatch(openBatchMarketId).data ?? {};
  const portfolio = useDevPortfolio(accountId).data ?? null;

  const allTime = activityOverview.all_time;
  const last24h = activityOverview.last_24h;

  const mIdx = marketIndex(markets);
  const topMarkets = topMarketsByVolume24h(markets);
  const cancellations = recentCancellations(blocks);
  const blockRows = latestBlockByMarketRows(latestBlock, mIdx);

  const openBatchOptions = markets
    .slice()
    .sort((a, b) => (Number(b.volume_24h_nanos) || 0) - (Number(a.volume_24h_nanos) || 0))
    .slice(0, 60);

  function loadPortfolio() {
    const id = Number(accountInput);
    setAccountId(Number.isFinite(id) ? id : 0);
  }

  return (
    <div>
      {/* (a) Top StatGrid */}
      <StatGrid columns={4}>
        <Stat
          label="Unique Traders (all-time)"
          value={fmtInt(allTime?.unique_traders) || "—"}
          tone="accent"
          sub={
            <>
              {"24h: " + fmtInt(last24h?.unique_traders)} ·{" "}
              <span style={{ color: "var(--fg-4)" }}>since last restart</span>
            </>
          }
        />
        <Stat
          label="Platform Volume (all-time)"
          value={"$" + dollars(allTime?.total_volume_nanos)}
          tone="accent"
          sub={"24h: $" + dollars(last24h?.total_volume_nanos)}
        />
        <Stat
          label="Orders Matched (all-time)"
          value={fmtInt(allTime?.orders?.matched)}
          tone="yes"
          sub={orderStatsSub(allTime)}
        />
        <Stat
          label="Cancels (recent window)"
          value={fmtInt(cancellations.length)}
          tone="warn"
          sub={
            <>
              {blocks.length + " blocks scanned"} ·{" "}
              <span style={{ color: "var(--fg-4)" }}>D1 OrderCancelled</span>
            </>
          }
        />
      </StatGrid>

      {/* (b) Per-Market Aggregates */}
      <Panel style={{ marginTop: 12 }}>
        <PanelHead
          title="Per-Market Aggregates"
          actions={
            <span style={mutedSpan}>
              {topMarkets.length + " markets, sorted by 24h volume"}
            </span>
          }
        />
        <PanelBody>
          <DataTable maxHeight={460}>
            <thead>
              <tr>
                <Th>ID</Th>
                <Th>Market</Th>
                <Th align="right">Traders</Th>
                <Th align="right">24h Vol</Th>
                <Th align="right">Lifetime Vol</Th>
                <Th align="right">Liquidity ±band</Th>
                <Th align="right">Placed</Th>
                <Th align="right">Matched</Th>
                <Th align="right">Unmatched</Th>
                <Th align="right">Δ24h YES</Th>
              </tr>
            </thead>
            <tbody>
              {topMarkets.map((m) => (
                <tr key={m.market_id}>
                  <Td mono tone="dim">
                    {m.market_id}
                  </Td>
                  <td style={truncCell} title={m.name}>
                    {m.name}
                  </td>
                  <Td mono align="right">
                    {fmtInt(m.trader_count)}
                  </Td>
                  <Td mono tone="accent" align="right">
                    {"$" + dollars(m.volume_24h_nanos)}
                  </Td>
                  <Td mono tone="dim" align="right">
                    {"$" + dollars(m.volume_nanos)}
                  </Td>
                  <Td mono align="right">
                    {fmtLiquidity(m)}
                  </Td>
                  <Td mono align="right">
                    {fmtInt(m.orders_placed_total)}
                  </Td>
                  <Td mono tone="yes" align="right">
                    {fmtInt(m.orders_matched_total)}
                  </Td>
                  <Td mono tone="no" align="right">
                    {fmtInt(m.orders_unmatched_total)}
                  </Td>
                  <Td mono tone={yesDelta24hClass(m)} align="right">
                    {fmtYesDelta24h(m)}
                  </Td>
                </tr>
              ))}
              {topMarkets.length === 0 ? (
                <tr>
                  <td colSpan={10} style={emptyMsg}>
                    No markets with 24h activity yet.
                  </td>
                </tr>
              ) : null}
            </tbody>
          </DataTable>
        </PanelBody>
      </Panel>

      {/* (c) Two-column row */}
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "minmax(0,1fr) minmax(0,1fr)",
          gap: 12,
          marginTop: 12,
        }}
      >
        <Panel>
          <PanelHead
            title="Latest Block · Per-Market Sidecar"
            actions={
              <span style={mutedSpan}>
                {latestBlock ? "#" + latestBlock.height : "no block yet"}
              </span>
            }
          />
          <PanelBody>
            <DataTable maxHeight={320}>
              <thead>
                <tr>
                  <Th>Market</Th>
                  <Th align="right">Placers</Th>
                  <Th align="right">Volume</Th>
                  <Th align="right">Placed</Th>
                  <Th align="right">Matched</Th>
                  <Th align="right">Unmatched</Th>
                  <Th align="right">Welfare</Th>
                </tr>
              </thead>
              <tbody>
                {blockRows.map((row) => (
                  <tr key={row.market_id}>
                    <td style={truncCell} title={row.name}>
                      {row.label}
                    </td>
                    <Td mono align="right">
                      {fmtInt(row.placers)}
                    </Td>
                    <Td mono tone="accent" align="right">
                      {"$" + dollars(row.volume_nanos)}
                    </Td>
                    <Td mono align="right">
                      {fmtInt(row.placed)}
                    </Td>
                    <Td mono tone="yes" align="right">
                      {fmtInt(row.matched)}
                    </Td>
                    <Td mono tone="no" align="right">
                      {fmtInt(row.unmatched)}
                    </Td>
                    <Td
                      mono
                      tone={row.welfare_nanos >= 0 ? "yes" : "no"}
                      align="right"
                    >
                      {moneySigned(row.welfare_nanos / 1e9)}
                    </Td>
                  </tr>
                ))}
                {blockRows.length === 0 ? (
                  <tr>
                    <td colSpan={7} style={emptyMsg}>
                      No per-market activity in the latest block.
                    </td>
                  </tr>
                ) : null}
              </tbody>
            </DataTable>
            {latestBlock ? (
              <div
                style={{
                  marginTop: 8,
                  fontSize: 11,
                  color: "var(--fg-4)",
                }}
              >
                {"Unique placers (platform): " +
                  fmtInt(latestBlock.unique_placers)}{" "}
                ·{" "}
                {"Block welfare: " +
                  moneySigned((Number(latestBlock.total_welfare_nanos) || 0) / 1e9)}
              </div>
            ) : null}
          </PanelBody>
        </Panel>

        <Panel>
          <PanelHead
            title="Recent Cancellations"
            actions={
              <span style={mutedSpan}>
                {cancellations.length + " in last " + blocks.length + " blocks"}
              </span>
            }
          />
          <PanelBody>
            <DataTable maxHeight={320}>
              <thead>
                <tr>
                  <Th align="right">Block</Th>
                  <Th align="right">Acct</Th>
                  <Th align="right">Order</Th>
                  <Th>Market(s)</Th>
                  <Th>Side</Th>
                  <Th align="right">Remaining</Th>
                </tr>
              </thead>
              <tbody>
                {cancellations.slice(0, 50).map((evt) => {
                  const sideTone = cancelSideClass(evt.side);
                  return (
                  <tr key={evt.row_key}>
                    <Td mono tone="dim" align="right">
                      {"#" + evt.block_height}
                    </Td>
                    <Td mono align="right">
                      {evt.account_id}
                    </Td>
                    <Td mono tone="dim" align="right">
                      {evt.order_id}
                    </Td>
                    <td
                      style={truncCell}
                      title={cancelMarketTitle(evt, mIdx)}
                    >
                      {cancelMarketLabel(evt, mIdx)}
                    </td>
                    <Td>
                      {sideTone ? (
                        <Pill tone={sideTone}>{evt.side}</Pill>
                      ) : (
                        <Pill>{evt.side}</Pill>
                      )}
                    </Td>
                    <Td mono align="right">
                      {fmtInt(evt.remaining_quantity)}
                    </Td>
                  </tr>
                  );
                })}
                {cancellations.length === 0 ? (
                  <tr>
                    <td colSpan={6} style={emptyMsg}>
                      No OrderCancelled events in the recent block window.
                    </td>
                  </tr>
                ) : null}
              </tbody>
            </DataTable>
          </PanelBody>
        </Panel>
      </div>

      {/* (d) Open-Batch Indicative */}
      <Panel style={{ marginTop: 12 }}>
        <PanelHead
          title="Open-Batch Indicative"
          actions={
            <select
              value={openBatchMarketId}
              onChange={(e) => setOpenBatchMarketId(Number(e.target.value))}
              style={{ ...controlStyle, minWidth: 280 }}
            >
              <option value={0}>Pick a market…</option>
              {openBatchOptions.map((m) => (
                <option key={m.market_id} value={m.market_id}>
                  {"#" + m.market_id + " · " + m.name}
                </option>
              ))}
            </select>
          }
        />
        <PanelBody>
          {openBatchMarketId > 0 ? (
            <StatGrid columns={4}>
              <Stat
                label="Unique Placers (open batch)"
                value={fmtInt(openBatch.unique_placers)}
                tone="warn"
                sub="non-MM admits"
              />
              <Stat
                label="Indicative YES"
                value={fmtPrice(openBatch.indicative_yes_price_nanos)}
                tone="yes"
                sub="shadow solve"
              />
              <Stat
                label="Indicative NO"
                value={fmtPrice(openBatch.indicative_no_price_nanos)}
                tone="no"
                sub="shadow solve"
              />
              <Stat
                label="Indicative Volume"
                value={"$" + dollars(openBatch.indicative_volume_nanos)}
                tone="accent"
                sub={
                  openBatch.indicative_computed_at_ms
                    ? "computed " + shortTime(openBatch.indicative_computed_at_ms)
                    : "no data"
                }
              />
            </StatGrid>
          ) : (
            <div style={emptyMsg}>
              Pick a market above to see its open-batch indicative snapshot (C2).
            </div>
          )}
        </PanelBody>
      </Panel>

      {/* (e) Cost Basis · Portfolio Mark */}
      <Panel style={{ marginTop: 12 }}>
        <PanelHead
          title="Cost Basis · Portfolio Mark"
          actions={
            <div style={{ display: "flex", gap: 6 }}>
              <input
                type="number"
                value={accountInput}
                onChange={(e) => setAccountInput(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") loadPortfolio();
                }}
                placeholder="Account ID"
                style={{ ...controlStyle, width: 140 }}
              />
              <button
                type="button"
                onClick={loadPortfolio}
                disabled={!accountInput}
                style={{ ...controlStyle, cursor: "pointer" }}
              >
                Load
              </button>
            </div>
          }
        />
        <PanelBody>
          {portfolio && portfolio.account_id != null ? (
            <>
              <StatGrid columns={4}>
                <Stat
                  label="Balance"
                  value={"$" + dollars(portfolio.balance_nanos)}
                  sub={"Deposited $" + dollars(portfolio.total_deposited_nanos)}
                />
                <Stat
                  label="Realized PnL (C1)"
                  value={moneySigned(
                    (Number(portfolio.realized_pnl_nanos) || 0) / 1e9,
                  )}
                  tone={
                    (Number(portfolio.realized_pnl_nanos) || 0) >= 0
                      ? "yes"
                      : "no"
                  }
                  sub="across closed positions"
                />
                <Stat
                  label="Unrealized PnL (C1)"
                  value={moneySigned(
                    (Number(portfolio.unrealized_pnl_nanos) || 0) / 1e9,
                  )}
                  tone={
                    (Number(portfolio.unrealized_pnl_nanos) || 0) >= 0
                      ? "yes"
                      : "no"
                  }
                  sub="mark-to-market"
                />
                <Stat
                  label="Lifetime Fills (B8)"
                  value={fmtInt(portfolio.total_fill_count)}
                  tone="accent"
                  sub={
                    portfolio.first_deposit_ms
                      ? "first deposit " + shortTime(portfolio.first_deposit_ms)
                      : "no deposit"
                  }
                />
              </StatGrid>
              {(portfolio.positions ?? []).length > 0 ? (
                <div style={{ marginTop: 10 }}>
                  <DataTable maxHeight={280}>
                    <thead>
                      <tr>
                        <Th>Market</Th>
                        <Th>Side</Th>
                        <Th align="right">Qty</Th>
                        <Th align="right">Avg Entry (C1)</Th>
                        <Th align="right">Mark</Th>
                        <Th align="right">Value</Th>
                      </tr>
                    </thead>
                    <tbody>
                      {(portfolio.positions ?? []).map((p) => {
                        const m = mIdx.get(Number(p.market_id));
                        const name = m ? m.name : "#" + p.market_id;
                        return (
                          <tr key={p.market_id + ":" + p.outcome}>
                            <td style={truncCell} title={name}>
                              {name}
                            </td>
                            <Td>
                              <Pill tone={p.outcome === "YES" ? "yes" : "no"}>
                                {p.outcome}
                              </Pill>
                            </Td>
                            <Td mono align="right">
                              {fmtInt(p.quantity)}
                            </Td>
                            <Td mono align="right">
                              {p.avg_entry_price_nanos
                                ? fmtPrice(p.avg_entry_price_nanos)
                                : "—"}
                            </Td>
                            <Td mono align="right">
                              {fmtPrice(p.current_price_nanos)}
                            </Td>
                            <Td mono tone="accent" align="right">
                              {"$" + dollars(p.value_nanos)}
                            </Td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </DataTable>
                </div>
              ) : null}
            </>
          ) : (
            <div style={emptyMsg}>
              Load an account to see realized/unrealized PnL and per-position
              cost basis.
            </div>
          )}
        </PanelBody>
      </Panel>
    </div>
  );
}
