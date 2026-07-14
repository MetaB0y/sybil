"use client";

import { useState, type CSSProperties } from "react";

import { BlockBarChart } from "@/components/dev/block-bar-chart";
import { Panel, PanelBody, PanelHead } from "@/components/dev/primitives/panel";
import { Pill } from "@/components/dev/primitives/pill";
import { Stat, StatGrid } from "@/components/dev/primitives/stat";
import {
  accountAggregates,
  actorPnlCohorts,
  buildInsights,
  buildQuickAnswer,
  marketIndex,
  participantRoleIndex,
  pendingByAccount,
} from "@/lib/dev/derive";
import {
  useDevAccounts,
  useDevBots,
  useDevLiquidityHealth,
  useDevMarkets,
  useDevPendingOrders,
} from "@/lib/dev/fetchers";
import { dollars, fmtInt, moneySigned } from "@/lib/dev/format";
import { useDevRecentBlocks } from "@/lib/dev/use-recent-blocks";

type ChartMetric = "volume" | "fills" | "orders";
type QuestionKind = "prices" | "chain" | "liquidity" | "mm";

const buttonStyle: CSSProperties = {
  border: "1px solid var(--border-2)",
  background: "var(--surface-2)",
  borderRadius: 6,
  padding: "7px 10px",
  color: "var(--fg-1)",
  cursor: "pointer",
  fontFamily: "inherit",
  fontSize: 12,
};

const activeButtonStyle: CSSProperties = {
  ...buttonStyle,
  border: "1px solid var(--accent)",
  color: "var(--accent)",
};

function present(v: unknown): boolean {
  return v != null;
}

export function OverviewView() {
  const [metric, setMetric] = useState<ChartMetric>("volume");
  const [answer, setAnswer] = useState<string>("");

  const markets = useDevMarkets().data ?? [];
  const pendingOrders = useDevPendingOrders().data ?? [];
  const accountsQuery = useDevAccounts();
  const accounts = accountsQuery.data ?? [];
  const botsQuery = useDevBots();
  const bots = botsQuery.data;
  const { blocks } = useDevRecentBlocks();
  const liquidityHealth = useDevLiquidityHealth().data;
  const liquidityExceptions = (liquidityHealth?.markets ?? [])
    .filter((market) => market.mm_orders === 0)
    .slice(0, 12);

  // ── trivial aggregates (computed inline per the console getters) ──────
  const pricedCount = markets.filter((m) => present(m.yes_price_nanos)).length;
  const unpricedCount = markets.length - pricedCount;
  const refCount = markets.filter((m) => present(m.reference_price_nanos)).length;
  const refOnlyCount = markets.filter(
    (m) => !present(m.yes_price_nanos) && present(m.reference_price_nanos),
  ).length;
  const marketsWithPending = new Set(pendingOrders.map((o) => o.market_id)).size;

  const recentVolumeNanos = blocks.reduce(
    (sum, b) => sum + (Number(b.total_volume_nanos) || 0),
    0,
  );
  const recentFills = blocks.reduce((sum, b) => sum + (b.fill_count || 0), 0);
  const recentOrders = blocks.reduce((sum, b) => sum + (b.order_count || 0), 0);
  const uniqueStateRoots = new Set(
    blocks.map((b) => b.state_root).filter(Boolean),
  ).size;

  const mIdx = marketIndex(markets);
  const aggregates = accountAggregates(accounts, null, pendingByAccount(pendingOrders));
  const roles = participantRoleIndex(liquidityHealth, bots);
  const pnl = actorPnlCohorts(accounts, roles);
  const actorMetadataReady = (liquidityHealth?.actors?.length ?? 0) > 0;
  const llmMetadataReady = bots?.db_available === true;
  const accountDataReady = accountsQuery.isSuccess;
  const mmNoiseReady = actorMetadataReady && accountDataReady;
  const allActorsReady = mmNoiseReady && llmMetadataReady;

  const insights = buildInsights({ markets, blocks, pendingOrders });

  const firstHeight = blocks.length ? blocks[0]!.height : null;
  const lastHeight = blocks.length ? blocks[blocks.length - 1]!.height : null;
  const blockRangeLabel =
    firstHeight !== null && lastHeight !== null
      ? "blocks #" + firstHeight + "..#" + lastHeight
      : "blocks none";

  function answerQuestion(kind: QuestionKind) {
    setAnswer(
      buildQuickAnswer(kind, {
        markets,
        blocks,
        pendingOrders,
        accounts,
        marketIndex: mIdx,
        aggregates,
      }),
    );
  }

  return (
    <div>
      <StatGrid columns={5}>
        <Stat
          label="Markets"
          value={markets.length}
          sub={`${pricedCount} marked / ${unpricedCount} unpriced`}
        />
        <Stat
          label="Reference Prices"
          value={refCount}
          tone="accent"
          sub={`${refOnlyCount} ref-only markets`}
        />
        <Stat
          label="Pending Orders"
          value={fmtInt(pendingOrders.length)}
          tone="warn"
          sub={`${marketsWithPending} markets with resting orders`}
        />
        <Stat
          label="Recent Volume"
          value={"$" + dollars(recentVolumeNanos)}
          tone="accent"
          sub={`${blocks.length} blocks in window`}
        />
        <Stat
          label="Recent Fills"
          value={fmtInt(recentFills)}
          tone={recentFills > 0 ? "yes" : "no"}
          sub={`${fmtInt(recentOrders)} orders seen`}
        />
      </StatGrid>

      <StatGrid columns={4} style={{ marginTop: 12 }}>
        <Stat
          label="MM PnL · Sybil Mark"
          value={mmNoiseReady ? moneySigned(pnl.mm.pnlNanos / 1e9) : "—"}
          tone={!mmNoiseReady ? "warn" : pnl.mm.pnlNanos >= 0 ? "yes" : "no"}
          sub={mmNoiseReady ? `$${dollars(pnl.mm.portfolioValueNanos)} portfolio · ${pnl.mm.accountCount} account` : "runtime actor metadata unavailable"}
        />
        <Stat
          label="Noise PnL · Sybil Mark"
          value={mmNoiseReady ? moneySigned(pnl.noise.pnlNanos / 1e9) : "—"}
          tone={!mmNoiseReady ? "warn" : pnl.noise.pnlNanos >= 0 ? "yes" : "no"}
          sub={mmNoiseReady ? `$${dollars(pnl.noise.portfolioValueNanos)} portfolio · ${pnl.noise.accountCount} accounts` : "runtime actor metadata unavailable"}
        />
        <Stat
          label="LLM PnL · Sybil Mark"
          value={llmMetadataReady && accountDataReady ? moneySigned(pnl.llm.pnlNanos / 1e9) : "—"}
          tone={!llmMetadataReady || !accountDataReady ? "warn" : pnl.llm.pnlNanos >= 0 ? "yes" : "no"}
          sub={llmMetadataReady && accountDataReady ? `$${dollars(pnl.llm.portfolioValueNanos)} portfolio · ${pnl.llm.accountCount} accounts` : "Arena account metadata unavailable"}
        />
        <Stat
          label="All Actors PnL · Sybil Mark"
          value={allActorsReady ? moneySigned(pnl.all.pnlNanos / 1e9) : "—"}
          tone={!allActorsReady ? "warn" : pnl.all.pnlNanos >= 0 ? "yes" : "no"}
          sub={allActorsReady ? `$${dollars(pnl.all.portfolioValueNanos)} portfolio · ${pnl.all.accountCount} actors · ${pnl.otherAccountCount} other` : "complete role metadata unavailable"}
        />
      </StatGrid>

      <Panel style={{ marginTop: 12 }}>
        <PanelHead title="Actor Liquidity · Latest Block" />
        <PanelBody>
          <StatGrid columns={6}>
            <Stat
              label="Universe"
              value={liquidityHealth?.active_markets ?? "—"}
              sub={liquidityHealth ? `generation ${liquidityHealth.universe_generation} · block #${liquidityHealth.height}` : "health feed unavailable"}
            />
            <Stat
              label="MM Coverage"
              value={liquidityHealth ? `${(liquidityHealth.mm_coverage_bps / 100).toFixed(1)}%` : "—"}
              tone={(liquidityHealth?.mm_coverage_bps ?? 0) >= 8000 ? "yes" : "no"}
              sub={liquidityHealth ? `${liquidityHealth.mm_markets_quoted}/${liquidityHealth.active_markets} current · ${(liquidityHealth.rolling_mm_coverage_bps / 100).toFixed(1)}% over ${liquidityHealth.rolling_window_blocks}` : "target 100%"}
            />
            <Stat
              label="MM Two-Sided"
              value={liquidityHealth ? `${(liquidityHealth.rolling_mm_two_sided_coverage_bps / 100).toFixed(1)}%` : "—"}
              tone={(liquidityHealth?.rolling_mm_two_sided_coverage_bps ?? 0) >= 9800 ? "yes" : "warn"}
              sub={liquidityHealth ? `${liquidityHealth.mm_markets_two_sided}/${liquidityHealth.active_markets} current · ${liquidityHealth.rolling_window_blocks}-block window` : "healthy target ≥ 98%"}
            />
            <Stat
              label="Noise Actors"
              value={liquidityHealth ? `${liquidityHealth.observed_noise_actors}/${liquidityHealth.expected_noise_actors}` : "—"}
              tone={liquidityHealth && liquidityHealth.observed_noise_actors === liquidityHealth.expected_noise_actors ? "yes" : "warn"}
              sub="durable actor packages observed"
            />
            <Stat
              label="Noise Coverage"
              value={liquidityHealth ? `${(liquidityHealth.rolling_noise_coverage_bps / 100).toFixed(1)}%` : "—"}
              tone={liquidityHealth && liquidityHealth.rolling_noise_coverage_bps >= 2200 && liquidityHealth.rolling_noise_coverage_bps <= 2800 ? "yes" : "warn"}
              sub={liquidityHealth ? `${liquidityHealth.noise_markets_selected}/${liquidityHealth.active_markets} current · ${liquidityHealth.rolling_window_blocks}-block window` : "rolling target 22–28%"}
            />
            <Stat
              label="Noise Fill Markets"
              value={liquidityHealth ? `${(liquidityHealth.rolling_noise_fill_coverage_bps / 100).toFixed(1)}%` : "—"}
              tone={(liquidityHealth?.rolling_noise_fill_coverage_bps ?? 0) >= 1000 && (liquidityHealth?.rolling_noise_fill_coverage_bps ?? 0) <= 2000 ? "yes" : "warn"}
              sub={liquidityHealth ? `${liquidityHealth.markets_with_noise_fills}/${liquidityHealth.active_markets} current · ${(liquidityHealth.rolling_noise_crossing_coverage_bps / 100).toFixed(1)}% naturally marketable vs MM` : "rolling target 10–20%"}
            />
            <Stat
              label="Block Flow"
              value={liquidityHealth ? `${liquidityHealth.total_fills} fills` : "—"}
              tone={(liquidityHealth?.total_fills ?? 0) > 0 ? "yes" : "no"}
              sub={liquidityHealth ? `$${dollars(liquidityHealth.total_volume_nanos)} volume · ${liquidityHealth.total_rejections} rejected` : "latest committed block"}
            />
          </StatGrid>
          {liquidityExceptions.length > 0 ? (
            <div style={{ marginTop: 10, fontFamily: "var(--font-mono)", fontSize: 11, color: "var(--fg-2)" }}>
              Exceptions: {liquidityExceptions.map((market) =>
                `#${market.market_id} MM:${market.mm_orders || market.mm_skip_reason || "missing"}`
              ).join(" · ")}
            </div>
          ) : liquidityHealth ? (
            <div style={{ marginTop: 10, fontSize: 11, color: "var(--yes)" }}>
              Every active market received an MM order package.
            </div>
          ) : null}
        </PanelBody>
      </Panel>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "minmax(0,1.4fr) minmax(360px,0.8fr)",
          gap: 12,
          marginTop: 12,
        }}
      >
        <Panel>
          <PanelHead
            title="Block Activity"
            actions={
              <div style={{ display: "flex", gap: 6 }}>
                {(["volume", "fills", "orders"] as ChartMetric[]).map((m) => (
                  <button
                    key={m}
                    type="button"
                    style={metric === m ? activeButtonStyle : buttonStyle}
                    onClick={() => setMetric(m)}
                  >
                    {m === "volume" ? "Volume" : m === "fills" ? "Fills" : "Orders"}
                  </button>
                ))}
              </div>
            }
          />
          <PanelBody>
            <BlockBarChart blocks={blocks} metric={metric} />
            <div
              style={{
                display: "flex",
                flexWrap: "wrap",
                gap: 6,
                marginTop: 10,
              }}
            >
              <Pill>{blockRangeLabel}</Pill>
              <Pill tone="yes">{recentFills + " fills"}</Pill>
              <Pill tone="accent">{"$" + dollars(recentVolumeNanos) + " volume"}</Pill>
              <Pill>{uniqueStateRoots + " state roots"}</Pill>
            </div>
          </PanelBody>
        </Panel>

        <div style={{ display: "grid", gap: 12 }}>
          <Panel>
            <PanelHead title="What Is Going On?" />
            <PanelBody>
              <div style={{ display: "grid", gap: 8 }}>
                {insights.map((item) => (
                  <div
                    key={item.title}
                    style={{
                      border: "1px solid var(--border-2)",
                      borderRadius: 6,
                      padding: "8px 10px",
                    }}
                  >
                    <strong style={{ fontSize: 12, color: "var(--fg-1)" }}>
                      {item.title}
                    </strong>
                    <div
                      style={{
                        marginTop: 3,
                        fontSize: 11,
                        color: "var(--fg-3)",
                      }}
                    >
                      {item.body}
                    </div>
                  </div>
                ))}
              </div>
            </PanelBody>
          </Panel>

          <Panel>
            <PanelHead title="Quick Questions" />
            <PanelBody>
              <div
                style={{ display: "flex", flexWrap: "wrap", gap: 6 }}
              >
                <button
                  type="button"
                  style={buttonStyle}
                  onClick={() => answerQuestion("prices")}
                >
                  Why no prices?
                </button>
                <button
                  type="button"
                  style={buttonStyle}
                  onClick={() => answerQuestion("chain")}
                >
                  Is chain alive?
                </button>
                <button
                  type="button"
                  style={buttonStyle}
                  onClick={() => answerQuestion("liquidity")}
                >
                  Where is liquidity?
                </button>
                <button
                  type="button"
                  style={buttonStyle}
                  onClick={() => answerQuestion("mm")}
                >
                  How is MM doing?
                </button>
              </div>
              <div
                style={{
                  marginTop: 10,
                  whiteSpace: "pre-wrap",
                  fontFamily: "var(--font-mono)",
                  fontSize: 12,
                  color: "var(--fg-1)",
                  border: "1px solid var(--border-2)",
                  borderRadius: 6,
                  minHeight: 96,
                  padding: 10,
                }}
              >
                {answer ||
                  "Pick a question. These answers are computed from live API data on this page."}
              </div>
            </PanelBody>
          </Panel>
        </div>
      </div>
    </div>
  );
}
