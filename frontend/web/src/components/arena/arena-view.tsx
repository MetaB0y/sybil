"use client";

import { useMemo, useState, type CSSProperties, type ReactNode } from "react";
import { BlockBarChart } from "@/components/dev/block-bar-chart";
import type { Tone } from "@/components/dev/primitives/color-text";
import { DataTable, Td, Th } from "@/components/dev/primitives/data-table";
import { Panel, PanelBody, PanelHead } from "@/components/dev/primitives/panel";
import { Pill } from "@/components/dev/primitives/pill";
import { Stat, StatGrid } from "@/components/dev/primitives/stat";
import { PageHeader } from "@/components/page-header";
import {
  articleLabel,
  articleList,
  articleUrl,
  estimateTokenCost,
  extractStrategy,
  formatDecisionOrder,
  orderList,
  orderSideTone,
  strategyRows,
  summarizeBots,
  totalTokenCalls,
  totalTokens,
} from "@/lib/arena/derive";
import {
  useArenaFeed,
  type ArenaBotSummary,
  type ArenaDecision,
  type ArenaTokenUsage,
} from "@/lib/arena/use-arena-feed";
import { useActivityOverview } from "@/lib/activity/use-activity-overview";
import { formatCompactDollars, formatCompactDollarsCents } from "@/lib/format/nanos";
import { useDevRecentBlocks } from "@/lib/dev/use-recent-blocks";

const controlStyle: CSSProperties = {
  border: "1px solid var(--border-2)",
  background: "var(--surface-1)",
  color: "var(--fg-1)",
  borderRadius: 6,
  padding: "7px 9px",
  fontFamily: "inherit",
  fontSize: 12,
  minWidth: 180,
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

const muted: CSSProperties = { color: "var(--fg-3)", fontSize: 12 };

const emptyMsg: CSSProperties = {
  padding: "16px 4px",
  textAlign: "center",
  color: "var(--fg-4)",
  fontSize: 12,
};

const EMPTY_SUMMARIES: ArenaBotSummary[] = [];
const EMPTY_DECISIONS: ArenaDecision[] = [];
const EMPTY_TOKEN_USAGE: ArenaTokenUsage[] = [];

export function ArenaView() {
  const [selectedTrader, setSelectedTrader] = useState("");
  const feed = useArenaFeed({
    limit: 140,
    trader: selectedTrader || undefined,
  });
  const activity = useActivityOverview();
  const { blocks, latestBlock, isBackfilling } = useDevRecentBlocks(36);

  const data = feed.data;
  const dbAvailable = data?.db_available === true;
  const stats = data?.stats;
  const summaries = data?.summaries ?? EMPTY_SUMMARIES;
  const decisions = data?.decisions ?? EMPTY_DECISIONS;
  const tokenUsage = data?.token_usage ?? EMPTY_TOKEN_USAGE;
  const traderNames = summaries.map((bot) => bot.trader_name);
  const totals = useMemo(() => summarizeBots(summaries), [summaries]);
  const strategies = useMemo(() => strategyRows(summaries), [summaries]);
  const tokenCost = estimateTokenCost(tokenUsage);
  const latestDecision = stats?.latest_decision_timestamp
    ? shortTime(stats.latest_decision_timestamp)
    : "none yet";

  return (
    <main
      style={{
        minHeight: "100vh",
        background: "var(--bg-1)",
        color: "var(--fg-1)",
        fontFamily: "var(--font-sans)",
        padding: "var(--space-6) var(--space-5) var(--space-9)",
      }}
    >
      <PageHeader
        title="Bot Arena"
        meta={
          dbAvailable
            ? "live arena bot feed · decisions, portfolios, reasoning, platform activity"
            : "arena database unavailable"
        }
        action={
          <Pill tone={dbAvailable ? "yes" : "warn"}>
            {dbAvailable ? "SQLite mounted" : "DB unavailable"}
          </Pill>
        }
      />

      {!dbAvailable && data?.error ? (
        <Panel style={{ marginTop: 18 }}>
          <PanelBody>
            <span style={{ color: "var(--warn)" }}>{data.error}</span>
          </PanelBody>
        </Panel>
      ) : null}

      <StatGrid
        columns={6}
        style={{
          marginTop: 18,
          gridTemplateColumns: "repeat(auto-fit, minmax(148px, 1fr))",
        }}
      >
        <Stat
          label="Decisions"
          value={fmtInt(stats?.decisions)}
          tone="accent"
          sub={"latest " + latestDecision}
        />
        <Stat
          label="Bots"
          value={fmtInt(stats?.traders)}
          sub={fmtInt(stats?.snapshots) + " portfolio snapshots"}
        />
        <Stat
          label="Arena PnL"
          value={money(totals.pnl, true)}
          tone={totals.pnl >= 0 ? "yes" : "no"}
          sub={"portfolio " + money(totals.portfolioValue)}
        />
        <Stat
          label="Orders / Fills"
          value={fmtInt(totals.orders) + " / " + fmtInt(totals.fills)}
          sub="latest bot snapshots"
        />
        <Stat
          label="LLM Calls"
          value={fmtInt(totalTokenCalls(tokenUsage))}
          tone="warn"
          sub={fmtCompact(totalTokens(tokenUsage)) + " tokens"}
        />
        <Stat
          label="Est. LLM Cost"
          value={"$" + tokenCost.toFixed(tokenCost >= 1 ? 2 : 4)}
          sub="$0.70 / 1M tokens"
        />
      </StatGrid>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(340px, 1fr))",
          gap: 12,
          marginTop: 12,
        }}
      >
        <StrategyPanel rows={strategies} />
        <LlmUsagePanel rows={tokenUsage} />
      </div>

      <BotRosterPanel
        summaries={summaries}
        selectedTrader={selectedTrader}
        onSelectTrader={setSelectedTrader}
      />

      <ActivityPanel
        blocks={blocks}
        latestBlockHeight={latestBlock?.height ?? null}
        isBackfilling={isBackfilling}
        allTimeVolume={activity.allTime.matchedVolume}
        allTimeWelfare={activity.allTime.welfare}
        last24hVolume={activity.last24h.matchedVolume}
        last24hWelfare={activity.last24h.welfare}
      />

      <DecisionsPanel
        decisions={decisions}
        selectedTrader={selectedTrader}
        traderNames={traderNames}
        onSelectTrader={setSelectedTrader}
        isLoading={feed.isPending}
      />
    </main>
  );
}

function StrategyPanel({ rows }: { rows: ReturnType<typeof strategyRows> }) {
  return (
    <Panel>
      <PanelHead title="Strategy Snapshot" />
      <PanelBody>
        <DataTable minWidth={560}>
          <thead>
            <tr>
              <Th>Strategy</Th>
              <Th align="right">Bots</Th>
              <Th align="right">Total PnL</Th>
              <Th align="right">Avg PnL</Th>
              <Th align="right">Avg Edge</Th>
              <Th align="right">Orders/Fills</Th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.strategy}>
                <Td>
                  <TonePill tone={strategyTone(row.strategy)}>
                    {row.strategy}
                  </TonePill>
                </Td>
                <Td mono align="right">
                  {fmtInt(row.traders)}
                </Td>
                <Td
                  mono
                  align="right"
                  tone={row.totalPnl >= 0 ? "yes" : "no"}
                >
                  {money(row.totalPnl, true)}
                </Td>
                <Td mono align="right" tone={row.avgPnl >= 0 ? "yes" : "no"}>
                  {money(row.avgPnl, true)}
                </Td>
                <Td mono align="right">
                  {pct(row.avgEdge)}
                </Td>
                <Td mono align="right">
                  {fmtInt(row.totalOrders) + " / " + fmtInt(row.totalFills)}
                </Td>
              </tr>
            ))}
            {rows.length === 0 ? (
              <tr>
                <td colSpan={6} style={emptyMsg}>
                  No bot summaries yet.
                </td>
              </tr>
            ) : null}
          </tbody>
        </DataTable>
      </PanelBody>
    </Panel>
  );
}

function LlmUsagePanel({ rows }: { rows: ArenaTokenUsage[] }) {
  return (
    <Panel>
      <PanelHead
        title="LLM Usage"
        actions={<span style={muted}>estimated cost, budget not exposed</span>}
      />
      <PanelBody>
        <DataTable maxHeight={280} minWidth={620}>
          <thead>
            <tr>
              <Th>Trader</Th>
              <Th align="right">Calls</Th>
              <Th align="right">Tokens</Th>
              <Th align="right">Avg Latency</Th>
              <Th align="right">Est. Cost</Th>
              <Th>Model</Th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => {
              const tokens = row.prompt_tokens + row.completion_tokens;
              const cost = estimateTokenCost([row]);
              return (
                <tr key={row.trader_name}>
                  <td style={truncCell} title={row.trader_name}>
                    {row.trader_name}
                  </td>
                  <Td mono align="right">
                    {fmtInt(row.calls)}
                  </Td>
                  <Td mono align="right">
                    {fmtCompact(tokens)}
                  </Td>
                  <Td mono align="right">
                    {row.avg_latency_s == null
                      ? "-"
                      : row.avg_latency_s.toFixed(1) + "s"}
                  </Td>
                  <Td mono align="right">
                    {"$" + cost.toFixed(cost >= 1 ? 2 : 4)}
                  </Td>
                  <td style={truncCell} title={row.latest_model ?? ""}>
                    {row.latest_model ?? "-"}
                  </td>
                </tr>
              );
            })}
            {rows.length === 0 ? (
              <tr>
                <td colSpan={6} style={emptyMsg}>
                  No token usage rows exposed yet.
                </td>
              </tr>
            ) : null}
          </tbody>
        </DataTable>
      </PanelBody>
    </Panel>
  );
}

function BotRosterPanel({
  summaries,
  selectedTrader,
  onSelectTrader,
}: {
  summaries: ArenaBotSummary[];
  selectedTrader: string;
  onSelectTrader: (trader: string) => void;
}) {
  return (
    <Panel style={{ marginTop: 12 }}>
      <PanelHead
        title="Bot Roster"
        actions={<span style={muted}>current snapshots; historical curves need an API</span>}
      />
      <PanelBody>
        <DataTable maxHeight={430} minWidth={1120}>
          <thead>
            <tr>
              <Th>Bot</Th>
              <Th>Strategy</Th>
              <Th align="right">Decisions</Th>
              <Th align="right">Portfolio</Th>
              <Th align="right">PnL</Th>
              <Th>Equity Snapshot</Th>
              <Th>Latest Market</Th>
              <Th align="right">FV</Th>
              <Th align="right">Mkt</Th>
              <Th align="right">Edge</Th>
              <Th align="right">Orders/Fills</Th>
            </tr>
          </thead>
          <tbody>
            {summaries.map((bot) => {
              const active = selectedTrader === bot.trader_name;
              return (
                <tr
                  key={bot.trader_name}
                  onClick={() =>
                    onSelectTrader(active ? "" : bot.trader_name)
                  }
                  style={{
                    cursor: "pointer",
                    background: active ? "var(--surface-2)" : "transparent",
                  }}
                >
                  <td style={truncCell} title={bot.trader_name}>
                    {bot.trader_name}
                  </td>
                  <Td>
                    <TonePill tone={strategyTone(extractStrategy(bot.trader_name))}>
                      {extractStrategy(bot.trader_name)}
                    </TonePill>
                  </Td>
                  <Td mono align="right">
                    {fmtInt(bot.decision_count)}
                  </Td>
                  <Td mono align="right">
                    {money(bot.portfolio_value)}
                  </Td>
                  <Td
                    mono
                    align="right"
                    tone={(bot.pnl ?? 0) >= 0 ? "yes" : "no"}
                  >
                    {money(bot.pnl, true)}
                  </Td>
                  <td style={{ ...truncCell, minWidth: 136 }}>
                    <EquitySnapshot bot={bot} />
                  </td>
                  <td style={truncCell} title={bot.latest_market_name ?? ""}>
                    {bot.latest_market_name ?? "-"}
                  </td>
                  <Td mono align="right" tone="yes">
                    {pct(bot.latest_fair_value)}
                  </Td>
                  <Td mono align="right" tone="accent">
                    {pct(bot.latest_market_price)}
                  </Td>
                  <ToneTd mono align="right" tone={edgeTone(bot.latest_edge)}>
                    {pct(bot.latest_edge)}
                  </ToneTd>
                  <Td mono align="right">
                    {fmtInt(bot.total_orders) + " / " + fmtInt(bot.total_fills)}
                  </Td>
                </tr>
              );
            })}
            {summaries.length === 0 ? (
              <tr>
                <td colSpan={11} style={emptyMsg}>
                  No bot roster rows yet.
                </td>
              </tr>
            ) : null}
          </tbody>
        </DataTable>
      </PanelBody>
    </Panel>
  );
}

function EquitySnapshot({ bot }: { bot: ArenaBotSummary }) {
  if (bot.portfolio_value == null || bot.pnl == null) {
    return <span style={{ color: "var(--fg-4)", fontSize: 12 }}>no snapshot</span>;
  }
  const current = bot.portfolio_value;
  const baseline = current - bot.pnl;
  const min = Math.min(current, baseline);
  const max = Math.max(current, baseline);
  const span = Math.max(1, max - min);
  const y = (v: number) => 22 - ((v - min) / span) * 18;
  const stroke = bot.pnl >= 0 ? "var(--yes)" : "var(--no)";

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
      <svg width="84" height="26" viewBox="0 0 84 26" aria-hidden>
        <line x1="0" x2="84" y1="22" y2="22" stroke="var(--chart-grid)" />
        <path
          d={`M 4 ${y(baseline).toFixed(2)} L 80 ${y(current).toFixed(2)}`}
          fill="none"
          stroke={stroke}
          strokeWidth="1.8"
          strokeLinecap="round"
        />
        <circle cx="4" cy={y(baseline)} r="2.5" fill="var(--fg-4)" />
        <circle cx="80" cy={y(current)} r="3" fill={stroke} />
      </svg>
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 11,
          color: "var(--fg-3)",
          whiteSpace: "nowrap",
        }}
      >
        {money(current)}
      </span>
    </div>
  );
}

function ActivityPanel({
  blocks,
  latestBlockHeight,
  isBackfilling,
  allTimeVolume,
  allTimeWelfare,
  last24hVolume,
  last24hWelfare,
}: {
  blocks: Parameters<typeof BlockBarChart>[0]["blocks"];
  latestBlockHeight: number | null;
  isBackfilling: boolean;
  allTimeVolume: string;
  allTimeWelfare: string;
  last24hVolume: string;
  last24hWelfare: string;
}) {
  const recent = blocks.slice(-8).reverse();
  return (
    <Panel style={{ marginTop: 12 }}>
      <PanelHead
        title="Market Activity & Welfare"
        actions={
          <span style={muted}>
            {latestBlockHeight == null
              ? isBackfilling
                ? "backfilling blocks"
                : "no blocks yet"
              : "latest #" + latestBlockHeight}
          </span>
        }
      />
      <PanelBody>
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fit, minmax(190px, 1fr))",
            gap: 10,
            marginBottom: 12,
          }}
        >
          <MiniStat label="All-time Volume" value={allTimeVolume} />
          <MiniStat label="All-time Welfare" value={allTimeWelfare} tone="yes" />
          <MiniStat label="24h Volume" value={last24hVolume} />
          <MiniStat label="24h Welfare" value={last24hWelfare} tone="yes" />
        </div>
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fit, minmax(300px, 1fr))",
            gap: 12,
          }}
        >
          <Panel>
            <PanelHead title="Recent Volume" />
            <BlockBarChart blocks={blocks} metric="volume" height={220} />
          </Panel>
          <Panel>
            <PanelHead title="Recent Fills" />
            <BlockBarChart blocks={blocks} metric="fills" height={220} />
          </Panel>
        </div>
        <DataTable maxHeight={260} minWidth={760}>
          <thead>
            <tr>
              <Th>Batch</Th>
              <Th align="right">Volume</Th>
              <Th align="right">Welfare</Th>
              <Th align="right">Orders</Th>
              <Th align="right">Fills</Th>
              <Th align="right">Markets</Th>
            </tr>
          </thead>
          <tbody>
            {recent.map((block) => (
              <tr key={block.height}>
                <Td mono>{"#" + block.height}</Td>
                <Td mono align="right" tone="accent">
                  {formatCompactDollars(block.total_volume_nanos ?? 0)}
                </Td>
                <Td mono align="right" tone="yes">
                  {formatCompactDollarsCents(block.total_welfare_nanos ?? 0)}
                </Td>
                <Td mono align="right">
                  {fmtInt(block.order_count)}
                </Td>
                <Td mono align="right" tone="yes">
                  {fmtInt(block.fill_count)}
                </Td>
                <Td mono align="right">
                  {fmtInt(Object.keys(block.by_market ?? {}).length)}
                </Td>
              </tr>
            ))}
            {recent.length === 0 ? (
              <tr>
                <td colSpan={6} style={emptyMsg}>
                  No recent blocks yet.
                </td>
              </tr>
            ) : null}
          </tbody>
        </DataTable>
      </PanelBody>
    </Panel>
  );
}

function MiniStat({
  label,
  value,
  tone,
}: {
  label: string;
  value: string;
  tone?: "yes" | "no" | "accent";
}) {
  const color =
    tone === "yes"
      ? "var(--yes)"
      : tone === "no"
        ? "var(--no)"
        : tone === "accent"
          ? "var(--accent)"
          : "var(--fg-1)";
  return (
    <div
      style={{
        border: "1px solid var(--border-2)",
        borderRadius: 8,
        padding: 10,
        background: "var(--surface-2)",
      }}
    >
      <div className="eyebrow">{label}</div>
      <div
        style={{
          marginTop: 6,
          fontFamily: "var(--font-mono)",
          fontSize: 18,
          color,
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {value}
      </div>
    </div>
  );
}

function DecisionsPanel({
  decisions,
  selectedTrader,
  traderNames,
  onSelectTrader,
  isLoading,
}: {
  decisions: ArenaDecision[];
  selectedTrader: string;
  traderNames: string[];
  onSelectTrader: (trader: string) => void;
  isLoading: boolean;
}) {
  return (
    <Panel style={{ marginTop: 12 }}>
      <PanelHead
        title="Recent Decisions"
        actions={
          <select
            value={selectedTrader}
            onChange={(event) => onSelectTrader(event.target.value)}
            style={controlStyle}
          >
            <option value="">All bots</option>
            {traderNames.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </select>
        }
      />
      <PanelBody>
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fit, minmax(360px, 1fr))",
            gap: 10,
          }}
        >
          {decisions.map((decision) => (
            <DecisionCard key={decision.id} decision={decision} />
          ))}
        </div>
        {decisions.length === 0 ? (
          <div style={emptyMsg}>
            {isLoading ? "Loading decisions..." : "No matching bot decisions."}
          </div>
        ) : null}
      </PanelBody>
    </Panel>
  );
}

function DecisionCard({ decision }: { decision: ArenaDecision }) {
  const orders = orderList(decision);
  const articles = articleList(decision);
  return (
    <article
      style={{
        border: "1px solid var(--border-2)",
        background: "var(--surface-2)",
        borderRadius: 8,
        padding: 12,
        minWidth: 0,
      }}
    >
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          gap: 10,
          alignItems: "flex-start",
        }}
      >
        <div style={{ minWidth: 0 }}>
          <div
            title={decision.market_name ?? ""}
            style={{
              fontWeight: 650,
              fontSize: 13,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              color: "var(--fg-1)",
            }}
          >
            {decision.market_name || "Market #" + decision.market_id}
          </div>
          <div style={{ ...muted, marginTop: 3 }}>
            {decision.trader_name + " / " + shortTime(decision.timestamp)}
          </div>
        </div>
        <TonePill tone={edgeTone(decision.edge)}>
          {"edge " + pct(decision.edge)}
        </TonePill>
      </div>

      <div style={chipRow}>
        <Pill tone="yes">{"FV " + pct(decision.fair_value)}</Pill>
        <Pill tone="accent">{"market " + pct(decision.market_price)}</Pill>
        <Pill>{"cash " + money(decision.balance)}</Pill>
        <Pill>
          {fmtInt(decision.yes_pos) + " YES / " + fmtInt(decision.no_pos) + " NO"}
        </Pill>
        <Pill>
          {decision.llm_duration_s == null
            ? "LLM -"
            : decision.llm_duration_s.toFixed(1) + "s LLM"}
        </Pill>
      </div>

      {decision.motivation ? (
        <p
          style={{
            margin: "9px 0 0",
            fontSize: 12,
            lineHeight: 1.45,
            color: "var(--fg-3)",
          }}
        >
          {decision.motivation}
        </p>
      ) : null}
      {decision.analysis ? (
        <p
          style={{
            margin: "9px 0 0",
            fontSize: 12,
            lineHeight: 1.5,
            color: "var(--fg-2)",
            whiteSpace: "pre-wrap",
            overflowWrap: "anywhere",
          }}
        >
          {decision.analysis}
        </p>
      ) : null}

      {orders.length ? (
        <div style={chipRow}>
          {orders.map((order, index) => (
            <Pill key={index} tone={orderSideTone(order)}>
              {formatDecisionOrder(order)}
            </Pill>
          ))}
        </div>
      ) : (
        <div style={chipRow}>
          <Pill>HOLD</Pill>
        </div>
      )}

      {articles.length ? (
        <div style={chipRow}>
          {articles.map((article, index) => (
            <a
              key={index}
              href={articleUrl(article)}
              target="_blank"
              rel="noreferrer"
              title={articleLabel(article)}
              style={{
                display: "inline-flex",
                alignItems: "center",
                padding: "2px 6px",
                borderRadius: 999,
                fontSize: 10,
                whiteSpace: "nowrap",
                overflow: "hidden",
                textOverflow: "ellipsis",
                maxWidth: 320,
                border: "1px solid var(--border-2)",
                color: "var(--accent)",
                textDecoration: "none",
              }}
            >
              {articleLabel(article)}
            </a>
          ))}
        </div>
      ) : null}
    </article>
  );
}

const chipRow: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  gap: 6,
  marginTop: 9,
};

function fmtInt(value: number | string | null | undefined): string {
  const n = Number(value);
  return Number.isFinite(n) ? n.toLocaleString() : "0";
}

function money(value: number | null | undefined, sign = false): string {
  const n = Number(value);
  if (!Number.isFinite(n)) return "-";
  const prefix = sign && n >= 0 ? "+" : "";
  return (
    prefix +
    n.toLocaleString(undefined, {
      style: "currency",
      currency: "USD",
      maximumFractionDigits: Math.abs(n) >= 100 ? 0 : 2,
    })
  );
}

function pct(value: number | null | undefined): string {
  const n = Number(value);
  if (!Number.isFinite(n)) return "-";
  return (n * 100).toFixed(1) + "%";
}

function fmtCompact(value: number): string {
  if (!Number.isFinite(value)) return "0";
  const abs = Math.abs(value);
  if (abs >= 1_000_000_000) return (value / 1_000_000_000).toFixed(1) + "B";
  if (abs >= 1_000_000) return (value / 1_000_000).toFixed(1) + "M";
  if (abs >= 1_000) return (value / 1_000).toFixed(1) + "K";
  return value.toLocaleString();
}

function shortTime(value: number | string | null | undefined): string {
  if (!value) return "-";
  const d = new Date(value);
  if (Number.isNaN(d.getTime())) return String(value);
  return d.toLocaleString(undefined, {
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function TonePill({
  tone,
  children,
}: {
  tone: Tone | undefined;
  children: ReactNode;
}) {
  return tone ? <Pill tone={tone}>{children}</Pill> : <Pill>{children}</Pill>;
}

function ToneTd({
  tone,
  children,
  mono,
  align,
}: {
  tone: Tone | undefined;
  children: ReactNode;
  mono?: boolean;
  align?: "left" | "right";
}) {
  const props = {
    ...(tone ? { tone } : {}),
    ...(mono == null ? {} : { mono }),
    ...(align == null ? {} : { align }),
  };
  return <Td {...props}>{children}</Td>;
}

function strategyTone(strategy: ReturnType<typeof extractStrategy>): Tone | undefined {
  if (strategy === "Kelly") return "warn";
  if (strategy === "Flat") return "accent";
  if (strategy === "Noise") return "dim";
  return undefined;
}

function edgeTone(edge: number | null | undefined): Tone | undefined {
  const n = Number(edge);
  if (!Number.isFinite(n)) return undefined;
  if (n >= 0.1) return "warn";
  if (n >= 0.03) return "accent";
  return "dim";
}
