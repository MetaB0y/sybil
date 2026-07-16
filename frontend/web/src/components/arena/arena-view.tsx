"use client";

import {
  useMemo,
  useState,
  type CSSProperties,
  type ReactNode,
  type SelectHTMLAttributes,
} from "react";
import { BlockBarChart } from "@/components/dev/block-bar-chart";
import { PageHeader } from "@/components/page-header";
import {
  articleLabel,
  articleList,
  articleUrl,
  driftPointsFromDecisions,
  estimateTokenCost,
  extractStrategy,
  formatDecisionOrder,
  marketOptionsFromDecisions,
  orderList,
  orderSideTone,
  strategyRows,
  summarizeBots,
  totalTokenCalls,
  totalTokens,
} from "@/lib/arena/derive";
import {
  useArenaDecisionHistory,
  useArenaEquitySeries,
  useArenaFeed,
  type ArenaFeed,
  type ArenaBotSummary,
  type ArenaDecision,
  type ArenaEquityPoint,
  type ArenaTokenUsage,
} from "@/lib/arena/use-arena-feed";
import { useActivityOverview } from "@/lib/activity/use-activity-overview";
import {
  formatCompactDollars,
  formatCompactDollarsCents,
} from "@/lib/format/nanos";
import { useDevRecentBlocks } from "@/lib/dev/use-recent-blocks";

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

type Tone = "yes" | "no" | "warn" | "accent" | "dim";

export type ArenaFeedUiState = {
  kind: "loading" | "transport_error" | "server_error" | "ready";
  showDashboard: boolean;
  meta: string;
  badge: string;
  tone: Tone;
  title?: string;
  message?: string;
};

export function arenaFeedUiState({
  data,
  isPending,
  isError,
}: {
  data: ArenaFeed | undefined;
  isPending: boolean;
  isError: boolean;
}): ArenaFeedUiState {
  if (data?.db_available === false) {
    return {
      kind: "server_error",
      showDashboard: false,
      meta: "arena database unavailable",
      badge: "DB unavailable",
      tone: "warn",
      title: "Arena database unavailable",
      message:
        data.error ||
        "The API is reachable, but it cannot read the Arena decision database.",
    };
  }
  if (isError) {
    const hasSnapshot = data?.db_available === true;
    return {
      kind: "transport_error",
      showDashboard: hasSnapshot,
      meta: hasSnapshot ? "arena refresh failed" : "arena feed unavailable",
      badge: hasSnapshot ? "Update failed" : "Feed unavailable",
      tone: "warn",
      title: hasSnapshot ? "Arena refresh failed" : "Arena feed unavailable",
      message: hasSnapshot
        ? "The latest successful snapshot remains visible, but Sybil could not refresh the Arena feed."
        : "Sybil could not load the Arena feed. No decision or portfolio totals are being shown as zero.",
    };
  }
  if (isPending || data == null) {
    return {
      kind: "loading",
      showDashboard: false,
      meta: "loading arena feed",
      badge: "Loading",
      tone: "dim",
      title: "Loading Arena",
      message: "Fetching bot decisions, portfolios, and usage totals…",
    };
  }
  return {
    kind: "ready",
    showDashboard: true,
    meta: "live arena bot feed · decisions, portfolios, reasoning, platform activity",
    badge: "SQLite mounted",
    tone: "yes",
  };
}

export type ArenaPanelDataState = {
  kind: "idle" | "loading" | "transport_error" | "server_error" | "ready";
  showData: boolean;
  title?: string;
  message?: string;
};

export function arenaPanelDataState({
  data,
  enabled,
  isPending,
  isError,
  label,
}: {
  data: { db_available: boolean; error?: string | null } | undefined;
  enabled: boolean;
  isPending: boolean;
  isError: boolean;
  label: string;
}): ArenaPanelDataState {
  if (!enabled) {
    return {
      kind: "idle",
      showData: false,
      title: label,
      message: "Select a bot to load this history.",
    };
  }
  if (data?.db_available === false) {
    return {
      kind: "server_error",
      showData: false,
      title: `${label} unavailable`,
      message:
        data.error ||
        "The API is reachable, but the Arena database cannot serve this history.",
    };
  }
  if (isError) {
    const hasSnapshot = data?.db_available === true;
    return {
      kind: "transport_error",
      showData: hasSnapshot,
      title: `${label} ${hasSnapshot ? "refresh failed" : "unavailable"}`,
      message: hasSnapshot
        ? "The latest successful history remains visible, but its refresh failed."
        : "This Arena history could not be loaded. Empty history is not being inferred.",
    };
  }
  if (isPending || data == null) {
    return {
      kind: "loading",
      showData: false,
      title: `Loading ${label.toLowerCase()}`,
      message: "Waiting for the Arena database response…",
    };
  }
  return { kind: "ready", showData: true };
}

export function combineArenaPanelDataStates(
  ...states: ArenaPanelDataState[]
): ArenaPanelDataState {
  return (
    states.find((state) => !state.showData) ??
    states.find((state) => state.kind !== "ready") ?? {
      kind: "ready",
      showData: true,
    }
  );
}

type ArenaFilterSelectProps = Omit<
  SelectHTMLAttributes<HTMLSelectElement>,
  "aria-label"
> & {
  label: string;
};

export function ArenaFilterSelect({
  label,
  className,
  ...props
}: ArenaFilterSelectProps) {
  return (
    <select
      aria-label={label}
      className={["arena-filter-select", className].filter(Boolean).join(" ")}
      {...props}
    />
  );
}

function toneColor(tone: Tone): string {
  if (tone === "yes") return "var(--yes)";
  if (tone === "no") return "var(--no)";
  if (tone === "warn") return "var(--warn)";
  if (tone === "accent") return "var(--accent)";
  return "var(--fg-3)";
}

function Panel({
  children,
  style,
}: {
  children?: ReactNode;
  style?: CSSProperties;
}) {
  return (
    <section
      style={{
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-lg)",
        boxShadow: "var(--shadow-inset-top)",
        overflow: "hidden",
        ...style,
      }}
    >
      {children}
    </section>
  );
}

function PanelHead({ title, actions }: { title: string; actions?: ReactNode }) {
  return (
    <div
      style={{
        padding: "14px 16px",
        borderBottom: "1px solid var(--border-1)",
        display: "flex",
        justifyContent: "space-between",
        alignItems: "center",
        gap: 12,
        flexWrap: "wrap",
      }}
    >
      <span className="eyebrow">{title}</span>
      {actions ? <div className="arena-panel-actions">{actions}</div> : null}
    </div>
  );
}

function PanelBody({
  children,
  style,
}: {
  children?: ReactNode;
  style?: CSSProperties;
}) {
  return <div style={{ padding: 16, ...style }}>{children}</div>;
}

export function ArenaFeedGate({
  state,
  retrying,
  onRetry,
  children,
}: {
  state: ArenaFeedUiState;
  retrying: boolean;
  onRetry: () => void;
  children?: ReactNode;
}) {
  const loading = state.kind === "loading";
  return (
    <>
      {state.kind !== "ready" ? (
        <Panel style={{ marginTop: 18 }}>
          <PanelBody>
            <div
              role={loading ? "status" : "alert"}
              aria-live={loading ? "polite" : "assertive"}
              aria-busy={loading || retrying}
              style={{
                display: "flex",
                flexDirection: "column",
                alignItems: "flex-start",
                gap: 10,
              }}
            >
              <strong
                style={{ color: loading ? "var(--fg-2)" : "var(--warn)" }}
              >
                {state.title}
              </strong>
              <p style={{ ...muted, margin: 0 }}>{state.message}</p>
              {!loading ? (
                <button
                  type="button"
                  onClick={onRetry}
                  disabled={retrying}
                  style={{
                    minHeight: 44,
                    padding: "8px 12px",
                    borderRadius: 6,
                    border: "1px solid var(--border-2)",
                    background: "var(--surface-2)",
                    color: "var(--fg-1)",
                    fontFamily: "var(--font-mono)",
                    fontSize: 11,
                    cursor: retrying ? "wait" : "pointer",
                  }}
                >
                  {retrying ? "Retrying…" : "Retry Arena feed"}
                </button>
              ) : null}
            </div>
          </PanelBody>
        </Panel>
      ) : null}
      {state.showDashboard ? children : null}
    </>
  );
}

export function ArenaPanelDataNotice({
  state,
  retrying,
  onRetry,
}: {
  state: ArenaPanelDataState;
  retrying: boolean;
  onRetry: () => void;
}) {
  if (state.kind === "ready") return null;
  const loading = state.kind === "loading";
  const failed =
    state.kind === "transport_error" || state.kind === "server_error";
  return (
    <div
      role={failed ? "alert" : "status"}
      aria-live={failed ? "assertive" : "polite"}
      aria-busy={loading || retrying}
      style={{
        padding: 10,
        marginBottom: state.showData ? 10 : 0,
        border: "1px solid var(--border-2)",
        borderRadius: 8,
        background: "var(--surface-2)",
      }}
    >
      <strong style={{ color: failed ? "var(--warn)" : "var(--fg-2)" }}>
        {state.title}
      </strong>
      <p style={{ ...muted, margin: "6px 0 0" }}>{state.message}</p>
      {failed ? (
        <button
          type="button"
          onClick={onRetry}
          disabled={retrying}
          style={{
            minHeight: 44,
            marginTop: 10,
            padding: "8px 12px",
            borderRadius: 6,
            border: "1px solid var(--border-2)",
            background: "var(--surface-1)",
            color: "var(--fg-1)",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
            cursor: retrying ? "wait" : "pointer",
          }}
        >
          {retrying ? "Retrying…" : "Retry history"}
        </button>
      ) : null}
    </div>
  );
}

function Pill({ children, tone }: { children?: ReactNode; tone?: Tone }) {
  const color = tone ? toneColor(tone) : "var(--fg-3)";
  const bg = tone
    ? `color-mix(in srgb, ${color} 14%, transparent)`
    : "var(--fill-subtle)";
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        minHeight: 24,
        padding: "3px 8px",
        borderRadius: "var(--radius-pill)",
        background: bg,
        color,
        fontFamily: "var(--font-mono)",
        fontSize: 10,
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
        whiteSpace: "nowrap",
      }}
    >
      {children}
    </span>
  );
}

function StatGrid({
  children,
  style,
}: {
  children?: ReactNode;
  style?: CSSProperties;
}) {
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "repeat(auto-fit, minmax(148px, 1fr))",
        gap: 10,
        ...style,
      }}
    >
      {children}
    </div>
  );
}

function Stat({
  label,
  value,
  sub,
  tone,
}: {
  label: string;
  value: string;
  sub?: string;
  tone?: Tone;
}) {
  return (
    <div
      style={{
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-lg)",
        background: "var(--surface-1)",
        padding: "12px 14px",
        boxShadow: "var(--shadow-inset-top)",
      }}
    >
      <div className="eyebrow">{label}</div>
      <div
        style={{
          marginTop: 8,
          fontFamily: "var(--font-mono)",
          fontSize: 20,
          color: tone ? toneColor(tone) : "var(--fg-1)",
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {value}
      </div>
      {sub ? <div style={{ ...muted, marginTop: 4 }}>{sub}</div> : null}
    </div>
  );
}

function DataTable({
  children,
  maxHeight,
  minWidth = 760,
}: {
  children?: ReactNode;
  maxHeight?: number | string;
  minWidth?: number;
}) {
  return (
    <div
      style={{
        overflow: "auto",
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-md)",
        ...(maxHeight !== undefined ? { maxHeight } : {}),
      }}
    >
      <table
        className="arena-data-table"
        style={{
          width: "100%",
          borderCollapse: "collapse",
          minWidth,
        }}
      >
        {children}
      </table>
    </div>
  );
}

function Th({
  children,
  align = "left",
}: {
  children?: ReactNode;
  align?: "left" | "right";
}) {
  return (
    <th
      style={{
        position: "sticky",
        top: 0,
        zIndex: 1,
        background: "var(--surface-2)",
        color: "var(--fg-3)",
        fontWeight: 600,
        fontSize: 10,
        textTransform: "uppercase",
        letterSpacing: "var(--track-wide)",
        padding: "9px 10px",
        borderBottom: "1px solid var(--border-1)",
        textAlign: align,
      }}
    >
      {children}
    </th>
  );
}

function Td({
  children,
  tone,
  align = "left",
  mono = false,
}: {
  children?: ReactNode;
  tone?: Tone;
  align?: "left" | "right";
  mono?: boolean;
}) {
  return (
    <td
      style={{
        padding: "9px 10px",
        borderBottom: "1px solid var(--border-1)",
        verticalAlign: "top",
        textAlign: align,
        color: tone ? toneColor(tone) : undefined,
        fontFamily: mono ? "var(--font-mono)" : undefined,
        fontVariantNumeric: mono ? "tabular-nums" : undefined,
        whiteSpace: mono ? "nowrap" : undefined,
      }}
    >
      {children}
    </td>
  );
}

export function ArenaView() {
  const [selectedTrader, setSelectedTrader] = useState("");
  const [selectedMarketId, setSelectedMarketId] = useState("");
  const feed = useArenaFeed({
    limit: 140,
    trader: selectedTrader || undefined,
  });
  const activity = useActivityOverview();
  const { blocks, latestBlock, isBackfilling } = useDevRecentBlocks(36);

  const data = feed.data;
  const feedState = arenaFeedUiState({
    data,
    isPending: feed.isPending,
    isError: feed.isError,
  });
  const stats = data?.stats;
  const summaries = data?.summaries ?? EMPTY_SUMMARIES;
  const decisions = data?.decisions ?? EMPTY_DECISIONS;
  const tokenUsage = data?.token_usage ?? EMPTY_TOKEN_USAGE;
  const traderNames = summaries.map((bot) => bot.trader_name);
  const activeTrader = selectedTrader || traderNames[0] || "";
  const equity = useArenaEquitySeries({
    trader: activeTrader || undefined,
    limit: 360,
  });
  const traderHistory = useArenaDecisionHistory({
    trader: activeTrader || undefined,
    limit: 500,
  });
  const marketOptions = useMemo(
    () =>
      marketOptionsFromDecisions(
        traderHistory.data?.decisions ?? EMPTY_DECISIONS,
      ),
    [traderHistory.data?.decisions],
  );
  const selectedMarketStillVisible = marketOptions.some(
    (option) => String(option.marketId) === selectedMarketId,
  );
  const effectiveMarketId =
    selectedMarketStillVisible || marketOptions.length === 0
      ? selectedMarketId
      : String(marketOptions[0]!.marketId);
  const numericMarketId =
    effectiveMarketId === "" ? undefined : Number(effectiveMarketId);
  const hasDriftMarket =
    numericMarketId !== undefined && Number.isFinite(numericMarketId);
  const driftFeed = useArenaDecisionHistory({
    trader: hasDriftMarket ? activeTrader || undefined : undefined,
    marketId: hasDriftMarket ? numericMarketId : undefined,
    limit: 500,
  });
  const equityState = arenaPanelDataState({
    data: equity.data,
    enabled: activeTrader !== "",
    isPending: equity.isPending,
    isError: equity.isError,
    label: "Equity history",
  });
  const traderHistoryState = arenaPanelDataState({
    data: traderHistory.data,
    enabled: activeTrader !== "",
    isPending: traderHistory.isPending,
    isError: traderHistory.isError,
    label: "Bot decision history",
  });
  const driftState = hasDriftMarket
    ? arenaPanelDataState({
        data: driftFeed.data,
        enabled: true,
        isPending: driftFeed.isPending,
        isError: driftFeed.isError,
        label: "Fair-value history",
      })
    : ({ kind: "ready", showData: true } satisfies ArenaPanelDataState);
  const fvState = combineArenaPanelDataStates(traderHistoryState, driftState);
  const totals = useMemo(() => summarizeBots(summaries), [summaries]);
  const strategies = useMemo(() => strategyRows(summaries), [summaries]);
  const tokenCost = estimateTokenCost(tokenUsage);
  const latestDecision = stats?.latest_decision_timestamp
    ? shortTime(stats.latest_decision_timestamp)
    : "none yet";
  const feedHeader = (
    <PageHeader
      title="Bot Arena"
      meta={feedState.meta}
      action={<Pill tone={feedState.tone}>{feedState.badge}</Pill>}
    />
  );
  const feedNotice = (
    <ArenaFeedGate
      state={feedState}
      retrying={feed.isFetching}
      onRetry={() => void feed.refetch()}
    />
  );

  if (!feedState.showDashboard) {
    return (
      <main className="arena-main" style={{}}>
        {feedHeader}
        {feedNotice}
      </main>
    );
  }

  return (
    <main className="arena-main" style={{}}>
      {feedHeader}
      {feedNotice}

      <StatGrid
        style={{
          marginTop: 18,
        }}
      >
        <Stat
          label="Decisions"
          value={fmtInt(stats?.decisions)}
          tone="accent"
          sub={"latest " + latestDecision}
        />
        <Stat
          label="Scored Traders"
          value={fmtInt(strategies.reduce((sum, row) => sum + row.traders, 0))}
          sub={fmtInt(stats?.snapshots) + " portfolio snapshots"}
        />
        <Stat
          label="Scored Arena PnL"
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

      <div className="arena-panel-grid" style={{}}>
        <StrategyPanel rows={strategies} />
        <LlmUsagePanel rows={tokenUsage} />
      </div>

      <div className="arena-panel-grid" style={{}}>
        <EquityCurvePanel
          trader={activeTrader}
          points={equity.data?.points ?? []}
          state={equityState}
          retrying={equity.isFetching}
          onRetry={() => void equity.refetch()}
          downsampled={equity.data?.downsampled === true}
          stride={equity.data?.stride ?? 1}
          sourceRows={equity.data?.source_rows ?? 0}
        />
        <FvDriftPanel
          trader={activeTrader}
          traderNames={traderNames}
          onSelectTrader={setSelectedTrader}
          marketOptions={marketOptions}
          selectedMarketId={effectiveMarketId}
          onSelectMarketId={setSelectedMarketId}
          decisions={driftFeed.data?.decisions ?? EMPTY_DECISIONS}
          state={fvState}
          retrying={traderHistory.isFetching || driftFeed.isFetching}
          onRetry={() => {
            void traderHistory.refetch();
            if (hasDriftMarket) void driftFeed.refetch();
          }}
        />
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
                <Td mono align="right" tone={row.totalPnl >= 0 ? "yes" : "no"}>
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

function EquityCurvePanel({
  trader,
  points,
  state,
  retrying,
  onRetry,
  downsampled,
  stride,
  sourceRows,
}: {
  trader: string;
  points: ArenaEquityPoint[];
  state: ArenaPanelDataState;
  retrying: boolean;
  onRetry: () => void;
  downsampled: boolean;
  stride: number;
  sourceRows: number;
}) {
  const chartPoints = useMemo(() => equityChartPoints(points), [points]);
  const latest = chartPoints[chartPoints.length - 1];
  return (
    <Panel>
      <PanelHead
        title="Equity Curve"
        actions={
          <span style={muted}>
            {state.showData && trader
              ? downsampled
                ? `${trader} · ${fmtInt(sourceRows)} rows · stride ${stride}`
                : trader
              : state.kind === "idle"
                ? "select a bot"
                : state.kind === "loading"
                  ? "loading"
                  : "unavailable"}
          </span>
        }
      />
      <PanelBody>
        <ArenaPanelDataNotice
          state={state}
          retrying={retrying}
          onRetry={onRetry}
        />
        {state.showData ? (
          <>
            <div
              style={{
                display: "grid",
                gridTemplateColumns: "repeat(auto-fit, minmax(130px, 1fr))",
                gap: 10,
                marginBottom: 10,
              }}
            >
              <MiniStat
                label="Latest Equity"
                value={latest ? money(latest.value) : "-"}
                tone="accent"
              />
              <MiniStat
                label="Range PnL"
                value={
                  chartPoints.length >= 2
                    ? money(
                        chartPoints[chartPoints.length - 1]!.value -
                          chartPoints[0]!.value,
                        true,
                      )
                    : "-"
                }
                tone={
                  chartPoints.length >= 2 &&
                  chartPoints[chartPoints.length - 1]!.value >=
                    chartPoints[0]!.value
                    ? "yes"
                    : "no"
                }
              />
            </div>
            <EquityLineChart points={chartPoints} isLoading={false} />
          </>
        ) : null}
      </PanelBody>
    </Panel>
  );
}

function FvDriftPanel({
  trader,
  traderNames,
  onSelectTrader,
  marketOptions,
  selectedMarketId,
  onSelectMarketId,
  decisions,
  state,
  retrying,
  onRetry,
}: {
  trader: string;
  traderNames: string[];
  onSelectTrader: (trader: string) => void;
  marketOptions: ReturnType<typeof marketOptionsFromDecisions>;
  selectedMarketId: string;
  onSelectMarketId: (marketId: string) => void;
  decisions: ArenaDecision[];
  state: ArenaPanelDataState;
  retrying: boolean;
  onRetry: () => void;
}) {
  const points = useMemo(
    () => driftPointsFromDecisions(decisions),
    [decisions],
  );
  const latest = points[points.length - 1];
  const selectedMarket = marketOptions.find(
    (option) => String(option.marketId) === selectedMarketId,
  );
  return (
    <Panel>
      <PanelHead
        title="FV Drift Monitor"
        actions={
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            <ArenaFilterSelect
              label="Filter fair value drift by bot"
              value={trader}
              onChange={(event) => onSelectTrader(event.target.value)}
              className="arena-select"
              style={{ minWidth: 160 }}
            >
              {traderNames.length === 0 ? (
                <option value="">No bots</option>
              ) : null}
              {traderNames.map((name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ))}
            </ArenaFilterSelect>
            <ArenaFilterSelect
              label="Select fair value drift market"
              value={selectedMarketId}
              onChange={(event) => onSelectMarketId(event.target.value)}
              className="arena-select"
              style={{ minWidth: 190 }}
            >
              {marketOptions.length === 0 ? (
                <option value="">No markets</option>
              ) : null}
              {marketOptions.map((option) => (
                <option key={option.marketId} value={option.marketId}>
                  {option.marketName}
                </option>
              ))}
            </ArenaFilterSelect>
          </div>
        }
      />
      <PanelBody>
        <ArenaPanelDataNotice
          state={state}
          retrying={retrying}
          onRetry={onRetry}
        />
        {state.showData ? (
          <>
            <div
              style={{
                display: "grid",
                gridTemplateColumns: "repeat(auto-fit, minmax(118px, 1fr))",
                gap: 10,
                marginBottom: 10,
              }}
            >
              <MiniStat
                label="Fair Value"
                value={latest ? pct(latest.fairValue) : "-"}
                tone="yes"
              />
              <MiniStat
                label="Market"
                value={latest ? pct(latest.marketPrice) : "-"}
                tone="accent"
              />
              <MiniStat
                label="Drift"
                value={latest ? pct(latest.edge) : "-"}
                tone={latest && latest.edge >= 0.1 ? "warn" : "accent"}
              />
            </div>
            <div style={{ ...muted, marginBottom: 8 }}>
              {selectedMarket
                ? selectedMarket.marketName
                : trader
                  ? "No market history for selected bot"
                  : "Select a bot"}
            </div>
            <DriftLineChart points={points} isLoading={false} />
          </>
        ) : null}
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
        actions={<span style={muted}>tap a bot to inspect history</span>}
      />
      <PanelBody>
        <div className="arena-bot-card-grid">
          {summaries.map((bot) => {
            const active = selectedTrader === bot.trader_name;
            const strategy = extractStrategy(bot.trader_name);
            const edge = edgeTone(bot.latest_edge);
            return (
              <button
                key={bot.trader_name}
                type="button"
                className="arena-bot-card"
                data-active={active}
                onClick={() => onSelectTrader(active ? "" : bot.trader_name)}
              >
                <div
                  style={{
                    display: "flex",
                    alignItems: "flex-start",
                    justifyContent: "space-between",
                    gap: 10,
                  }}
                >
                  <div style={{ minWidth: 0 }}>
                    <div
                      style={{
                        fontFamily: "var(--font-sans)",
                        fontSize: 14,
                        fontWeight: 650,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                      title={bot.trader_name}
                    >
                      {bot.trader_name}
                    </div>
                    <div style={{ ...muted, marginTop: 4 }}>
                      {bot.latest_market_name ?? "No recent market"}
                    </div>
                  </div>
                  <TonePill tone={strategyTone(strategy)}>{strategy}</TonePill>
                </div>
                <EquitySnapshot bot={bot} />
                <div
                  style={{
                    display: "grid",
                    gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
                    gap: 8,
                  }}
                >
                  <MiniStat
                    label="Portfolio"
                    value={money(bot.portfolio_value)}
                  />
                  <MiniStat
                    label="PnL"
                    value={money(bot.pnl, true)}
                    tone={(bot.pnl ?? 0) >= 0 ? "yes" : "no"}
                  />
                  <MiniStat
                    label="Decisions"
                    value={fmtInt(bot.decision_count)}
                  />
                  <MiniStat
                    label="Orders / Fills"
                    value={
                      fmtInt(bot.total_orders) + " / " + fmtInt(bot.total_fills)
                    }
                  />
                  <MiniStat
                    label="FV"
                    value={pct(bot.latest_fair_value)}
                    tone="yes"
                  />
                  <MiniStat
                    label="Edge"
                    value={pct(bot.latest_edge)}
                    {...(edge ? { tone: edge } : {})}
                  />
                </div>
              </button>
            );
          })}
        </div>
        {summaries.length === 0 ? (
          <div style={emptyMsg}>No bot roster rows yet.</div>
        ) : null}
      </PanelBody>
    </Panel>
  );
}

function EquitySnapshot({ bot }: { bot: ArenaBotSummary }) {
  if (bot.portfolio_value == null || bot.pnl == null) {
    return (
      <span style={{ color: "var(--fg-4)", fontSize: 12 }}>no snapshot</span>
    );
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
          <MiniStat
            label="All-time Welfare"
            value={allTimeWelfare}
            tone="yes"
          />
          <MiniStat label="24h Volume" value={last24hVolume} />
          <MiniStat label="24h Welfare" value={last24hWelfare} tone="yes" />
        </div>
        <div className="arena-panel-grid">
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
  tone?: Tone;
}) {
  const color =
    tone === "yes"
      ? "var(--yes)"
      : tone === "no"
        ? "var(--no)"
        : tone === "warn"
          ? "var(--warn)"
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

interface EquityChartPoint {
  t: number;
  value: number;
  timestamp: string | null;
}

function EquityLineChart({
  points,
  isLoading,
}: {
  points: EquityChartPoint[];
  isLoading: boolean;
}) {
  const W = 640;
  const H = 230;
  const pad = { l: 44, r: 18, t: 18, b: 30 };
  if (points.length < 2) {
    return (
      <ChartBox>
        <ChartEmpty>
          {isLoading ? "Loading equity..." : "No equity history yet."}
        </ChartEmpty>
      </ChartBox>
    );
  }

  const values = points.map((point) => point.value);
  let min = Math.min(...values);
  let max = Math.max(...values);
  if (!(max > min)) {
    const padValue = Math.max(1, Math.abs(max) * 0.02);
    min -= padValue;
    max += padValue;
  }
  const yPad = (max - min) * 0.08;
  min -= yPad;
  max += yPad;
  const tMin = points[0]!.t;
  const tMax = points[points.length - 1]!.t;
  const tSpan = Math.max(1, tMax - tMin);
  const ySpan = Math.max(1e-9, max - min);
  const innerW = W - pad.l - pad.r;
  const innerH = H - pad.t - pad.b;
  const xFor = (t: number) => pad.l + ((t - tMin) / tSpan) * innerW;
  const yFor = (value: number) => pad.t + (1 - (value - min) / ySpan) * innerH;
  const line = points
    .map(
      (point, index) =>
        `${index === 0 ? "M" : "L"} ${xFor(point.t).toFixed(2)} ${yFor(point.value).toFixed(2)}`,
    )
    .join(" ");
  const area = `${line} L ${xFor(tMax).toFixed(2)} ${H - pad.b} L ${xFor(tMin).toFixed(2)} ${H - pad.b} Z`;
  const end = points[points.length - 1]!;
  const start = points[0]!;
  const tone = end.value >= start.value ? "var(--yes)" : "var(--no)";
  const ticks = [0, 0.25, 0.5, 0.75, 1].map((n) => min + (max - min) * n);

  return (
    <ChartBox>
      <svg
        viewBox={`0 0 ${W} ${H}`}
        width="100%"
        height="100%"
        preserveAspectRatio="none"
      >
        {ticks.map((tick) => {
          const y = yFor(tick);
          return (
            <g key={tick}>
              <line
                x1={pad.l}
                x2={W - pad.r}
                y1={y}
                y2={y}
                stroke="var(--chart-grid)"
                vectorEffect="non-scaling-stroke"
              />
              <text
                x={pad.l - 8}
                y={y + 3}
                textAnchor="end"
                fill="var(--fg-4)"
                fontFamily="var(--font-mono)"
                fontSize={9}
              >
                {axisMoney(tick)}
              </text>
            </g>
          );
        })}
        <path d={area} fill={tone} opacity={0.09} />
        <path
          d={line}
          fill="none"
          stroke="var(--accent)"
          strokeWidth={2}
          strokeLinejoin="round"
          strokeLinecap="round"
          vectorEffect="non-scaling-stroke"
        />
        <circle
          cx={xFor(end.t)}
          cy={yFor(end.value)}
          r={3.5}
          fill="var(--accent)"
        />
        <AxisText
          x={pad.l}
          y={H - 9}
          text={shortDate(start.timestamp ?? start.t)}
        />
        <AxisText
          x={W - pad.r}
          y={H - 9}
          text={shortDate(end.timestamp ?? end.t)}
          anchor="end"
        />
      </svg>
    </ChartBox>
  );
}

function DriftLineChart({
  points,
  isLoading,
}: {
  points: ReturnType<typeof driftPointsFromDecisions>;
  isLoading: boolean;
}) {
  const W = 640;
  const H = 230;
  const pad = { l: 38, r: 18, t: 18, b: 30 };
  if (points.length < 2) {
    return (
      <ChartBox>
        <ChartEmpty>
          {isLoading ? "Loading drift..." : "No FV drift history yet."}
        </ChartEmpty>
      </ChartBox>
    );
  }

  const tMin = points[0]!.t;
  const tMax = points[points.length - 1]!.t;
  const tSpan = Math.max(1, tMax - tMin);
  const innerW = W - pad.l - pad.r;
  const innerH = H - pad.t - pad.b;
  const xFor = (t: number) => pad.l + ((t - tMin) / tSpan) * innerW;
  const yFor = (value: number) => pad.t + (1 - value) * innerH;
  const pathFor = (key: "fairValue" | "marketPrice") =>
    points
      .map(
        (point, index) =>
          `${index === 0 ? "M" : "L"} ${xFor(point.t).toFixed(2)} ${yFor(point[key]).toFixed(2)}`,
      )
      .join(" ");
  const start = points[0]!;
  const end = points[points.length - 1]!;

  return (
    <ChartBox>
      <svg
        viewBox={`0 0 ${W} ${H}`}
        width="100%"
        height="100%"
        preserveAspectRatio="none"
      >
        {[0, 0.25, 0.5, 0.75, 1].map((tick) => {
          const y = yFor(tick);
          return (
            <g key={tick}>
              <line
                x1={pad.l}
                x2={W - pad.r}
                y1={y}
                y2={y}
                stroke="var(--chart-grid)"
                vectorEffect="non-scaling-stroke"
              />
              <text
                x={pad.l - 8}
                y={y + 3}
                textAnchor="end"
                fill="var(--fg-4)"
                fontFamily="var(--font-mono)"
                fontSize={9}
              >
                {Math.round(tick * 100) + "%"}
              </text>
            </g>
          );
        })}
        <path
          d={pathFor("marketPrice")}
          fill="none"
          stroke="var(--accent)"
          strokeWidth={2}
          strokeLinejoin="round"
          strokeLinecap="round"
          vectorEffect="non-scaling-stroke"
        />
        <path
          d={pathFor("fairValue")}
          fill="none"
          stroke="var(--yes)"
          strokeWidth={2}
          strokeLinejoin="round"
          strokeLinecap="round"
          vectorEffect="non-scaling-stroke"
        />
        <circle
          cx={xFor(end.t)}
          cy={yFor(end.marketPrice)}
          r={3.2}
          fill="var(--accent)"
        />
        <circle
          cx={xFor(end.t)}
          cy={yFor(end.fairValue)}
          r={3.2}
          fill="var(--yes)"
        />
        <AxisText
          x={pad.l}
          y={H - 9}
          text={shortDate(start.timestamp ?? start.t)}
        />
        <AxisText
          x={W - pad.r}
          y={H - 9}
          text={shortDate(end.timestamp ?? end.t)}
          anchor="end"
        />
      </svg>
      <div
        style={{
          position: "absolute",
          top: 10,
          right: 12,
          display: "flex",
          gap: 8,
          fontSize: 11,
          color: "var(--fg-3)",
        }}
      >
        <LegendItem color="var(--yes)" label="FV" />
        <LegendItem color="var(--accent)" label="Market" />
      </div>
    </ChartBox>
  );
}

function ChartBox({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        position: "relative",
        height: 250,
        minHeight: 250,
        border: "1px solid var(--border-2)",
        borderRadius: 8,
        overflow: "hidden",
        background: "var(--surface-2)",
      }}
    >
      {children}
    </div>
  );
}

function ChartEmpty({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        position: "absolute",
        inset: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        color: "var(--fg-4)",
        fontSize: 12,
      }}
    >
      {children}
    </div>
  );
}

function AxisText({
  x,
  y,
  text,
  anchor = "start",
}: {
  x: number;
  y: number;
  text: string;
  anchor?: "start" | "end";
}) {
  return (
    <text
      x={x}
      y={y}
      fill="var(--fg-4)"
      fontFamily="var(--font-mono)"
      fontSize={9}
      textAnchor={anchor}
    >
      {text}
    </text>
  );
}

function LegendItem({ color, label }: { color: string; label: string }) {
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 5 }}>
      <span
        style={{
          width: 8,
          height: 8,
          borderRadius: 999,
          background: color,
          display: "inline-block",
        }}
      />
      {label}
    </span>
  );
}

function equityChartPoints(points: ArenaEquityPoint[]): EquityChartPoint[] {
  return points
    .map((point) => {
      const value = Number(point.portfolio_value);
      const parsedTime = point.timestamp
        ? new Date(point.timestamp).getTime()
        : NaN;
      const t = Number.isFinite(parsedTime) ? parsedTime : Number(point.id);
      if (!Number.isFinite(value) || !Number.isFinite(t)) return null;
      return { t, value, timestamp: point.timestamp ?? null };
    })
    .filter((point): point is EquityChartPoint => point != null)
    .sort((a, b) => a.t - b.t);
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
          <ArenaFilterSelect
            label="Filter recent decisions by bot"
            value={selectedTrader}
            onChange={(event) => onSelectTrader(event.target.value)}
            className="arena-select"
          >
            <option value="">All bots</option>
            {traderNames.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </ArenaFilterSelect>
        }
      />
      <PanelBody>
        <div className="arena-decision-grid" style={{}}>
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
          {fmtInt(decision.yes_pos) +
            " YES / " +
            fmtInt(decision.no_pos) +
            " NO"}
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
              className="mobile-action-link"
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

function axisMoney(value: number): string {
  const abs = Math.abs(value);
  if (abs >= 100_000) return "$" + Math.round(value / 1_000) + "K";
  if (abs >= 10_000) return "$" + (value / 1_000).toFixed(1) + "K";
  return "$" + Math.round(value).toLocaleString();
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

function shortDate(value: number | string | null | undefined): string {
  if (!value) return "-";
  const d = new Date(value);
  if (Number.isNaN(d.getTime())) return String(value);
  return d.toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
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

function strategyTone(
  strategy: ReturnType<typeof extractStrategy>,
): Tone | undefined {
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
