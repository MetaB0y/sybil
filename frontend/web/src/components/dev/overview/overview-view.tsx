"use client";

import { useState, type CSSProperties } from "react";

import { BlockBarChart } from "@/components/dev/block-bar-chart";
import { Panel, PanelBody, PanelHead } from "@/components/dev/primitives/panel";
import { Pill } from "@/components/dev/primitives/pill";
import { Stat, StatGrid } from "@/components/dev/primitives/stat";
import {
  accountAggregates,
  buildInsights,
  buildQuickAnswer,
  marketIndex,
  pendingByAccount,
} from "@/lib/dev/derive";
import {
  useDevAccounts,
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
  borderColor: "var(--accent)",
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
  const accounts = useDevAccounts().data ?? [];
  const { blocks } = useDevRecentBlocks();

  // ── trivial aggregates (computed inline per the console getters) ──────
  const pricedCount = markets.filter((m) => present(m.yes_price_nanos)).length;
  const noClearCount = markets.length - pricedCount;
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
  const aggregates = accountAggregates(
    accounts,
    mIdx,
    null,
    pendingByAccount(pendingOrders),
  );

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
      <StatGrid columns={6}>
        <Stat
          label="Markets"
          value={markets.length}
          sub={`${pricedCount} cleared / ${noClearCount} no clears`}
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
        <Stat
          label="MM Ref PnL"
          value={moneySigned(aggregates.mmReferencePnl)}
          tone={aggregates.mmReferencePnl >= 0 ? "yes" : "no"}
          sub={`${aggregates.mmPositionCount} positions, ${aggregates.activeTradingAccounts.length} active accounts`}
        />
      </StatGrid>

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
