import type {
  ArenaBotSummary,
  ArenaDecision,
  ArenaTokenUsage,
} from "./use-arena-feed";

export type StrategyName = "Kelly" | "Flat" | "Legacy" | "Noise";

export interface StrategyRow {
  strategy: StrategyName;
  traders: number;
  totalPnl: number;
  avgPnl: number;
  avgEdge: number | null;
  totalOrders: number;
  totalFills: number;
}

export interface ArenaTotals {
  portfolioValue: number;
  pnl: number;
  orders: number;
  fills: number;
}

export interface ArenaMarketOption {
  marketId: number;
  marketName: string;
  decisions: number;
  latestId: number;
  latestTimestamp: string | null;
  latestEdge: number | null;
}

export interface ArenaDriftPoint {
  id: number;
  t: number;
  timestamp: string | null;
  fairValue: number;
  marketPrice: number;
  edge: number;
}

export function extractStrategy(name: string): StrategyName {
  if (name.includes("(Kelly)")) return "Kelly";
  if (name.includes("(Flat)")) return "Flat";
  if (name.startsWith("Noise")) return "Noise";
  return "Legacy";
}

export function summarizeBots(summaries: ArenaBotSummary[]): ArenaTotals {
  return scoredBots(summaries).reduce<ArenaTotals>(
    (acc, row) => {
      acc.portfolioValue += num(row.portfolio_value);
      acc.pnl += num(row.pnl);
      acc.orders += row.total_orders ?? 0;
      acc.fills += row.total_fills ?? 0;
      return acc;
    },
    { portfolioValue: 0, pnl: 0, orders: 0, fills: 0 },
  );
}

export function strategyRows(summaries: ArenaBotSummary[]): StrategyRow[] {
  const rows = new Map<StrategyName, ArenaBotSummary[]>();
  for (const summary of scoredBots(summaries)) {
    const strategy = extractStrategy(summary.trader_name);
    rows.set(strategy, [...(rows.get(strategy) ?? []), summary]);
  }

  return [...rows.entries()]
    .map(([strategy, bots]) => {
      const totalPnl = bots.reduce((sum, bot) => sum + num(bot.pnl), 0);
      const edges = bots
        .map((bot) => bot.avg_edge)
        .filter(
          (edge): edge is number => edge != null && Number.isFinite(edge),
        );
      return {
        strategy,
        traders: bots.length,
        totalPnl,
        avgPnl: bots.length ? totalPnl / bots.length : 0,
        avgEdge: edges.length
          ? edges.reduce((sum, edge) => sum + edge, 0) / edges.length
          : null,
        totalOrders: bots.reduce(
          (sum, bot) => sum + (bot.total_orders ?? 0),
          0,
        ),
        totalFills: bots.reduce((sum, bot) => sum + (bot.total_fills ?? 0), 0),
      };
    })
    .sort((a, b) => {
      const order: Record<StrategyName, number> = {
        Kelly: 0,
        Flat: 1,
        Legacy: 2,
        Noise: 3,
      };
      return order[a.strategy] - order[b.strategy];
    });
}

function scoredBots(summaries: ArenaBotSummary[]): ArenaBotSummary[] {
  return summaries.filter((summary) => summary.active && summary.scored);
}

export function estimateTokenCost(
  rows: ArenaTokenUsage[],
  dollarsPerMillionTokens = 0.7,
): number {
  const tokens = rows.reduce(
    (sum, row) => sum + row.prompt_tokens + row.completion_tokens,
    0,
  );
  return (tokens * dollarsPerMillionTokens) / 1_000_000;
}

export function totalTokenCalls(rows: ArenaTokenUsage[]): number {
  return rows.reduce((sum, row) => sum + row.calls, 0);
}

export function totalTokens(rows: ArenaTokenUsage[]): number {
  return rows.reduce(
    (sum, row) => sum + row.prompt_tokens + row.completion_tokens,
    0,
  );
}

export function marketOptionsFromDecisions(
  decisions: ArenaDecision[],
): ArenaMarketOption[] {
  const byId = new Map<number, ArenaMarketOption>();
  for (const decision of decisions) {
    if (decision.market_id == null) continue;
    const marketId = Number(decision.market_id);
    if (!Number.isFinite(marketId)) continue;
    const current = byId.get(marketId);
    const latestId = Number(decision.id);
    const latestEdge =
      decision.edge != null && Number.isFinite(decision.edge)
        ? Number(decision.edge)
        : null;
    if (!current) {
      byId.set(marketId, {
        marketId,
        marketName: decision.market_name || "Market #" + marketId,
        decisions: 1,
        latestId,
        latestTimestamp: decision.timestamp ?? null,
        latestEdge,
      });
      continue;
    }
    current.decisions += 1;
    if (latestId > current.latestId) {
      current.latestId = latestId;
      current.marketName = decision.market_name || current.marketName;
      current.latestTimestamp = decision.timestamp ?? null;
      current.latestEdge = latestEdge;
    }
  }
  return [...byId.values()].sort((a, b) => {
    return (
      b.latestId - a.latestId ||
      b.decisions - a.decisions ||
      a.marketName.localeCompare(b.marketName)
    );
  });
}

export function driftPointsFromDecisions(
  decisions: ArenaDecision[],
): ArenaDriftPoint[] {
  return decisions
    .map((decision) => {
      const fairValue = finiteOrNull(decision.fair_value);
      const marketPrice = finiteOrNull(decision.market_price);
      if (fairValue == null || marketPrice == null) return null;
      const parsedTime = decision.timestamp
        ? new Date(decision.timestamp).getTime()
        : NaN;
      const t = Number.isFinite(parsedTime) ? parsedTime : Number(decision.id);
      return {
        id: Number(decision.id),
        t,
        timestamp: decision.timestamp ?? null,
        fairValue,
        marketPrice,
        edge: Math.abs(fairValue - marketPrice),
      };
    })
    .filter((point): point is ArenaDriftPoint => point != null)
    .sort((a, b) => a.t - b.t || a.id - b.id);
}

export function orderList(decision: Pick<ArenaDecision, "orders">): unknown[] {
  return Array.isArray(decision.orders) ? decision.orders : [];
}

export function articleList(
  decision: Pick<ArenaDecision, "article_urls">,
): unknown[] {
  return Array.isArray(decision.article_urls) ? decision.article_urls : [];
}

export function articleUrl(article: unknown): string {
  if (typeof article === "string") return article;
  if (isRecord(article) && typeof article.url === "string") return article.url;
  return "#";
}

export function articleLabel(article: unknown): string {
  if (typeof article === "string") return article;
  if (!isRecord(article)) return "article";
  const source = typeof article.source === "string" ? article.source : "";
  const title =
    typeof article.title === "string"
      ? article.title
      : typeof article.url === "string"
        ? article.url
        : "";
  return [source, title].filter(Boolean).join(": ") || "article";
}

export function formatDecisionOrder(order: unknown): string {
  if (!isRecord(order)) return "order";
  const side = text(order.side ?? order.action ?? order.type, "order");
  const qty = order.quantity ?? order.qty ?? order.size ?? order.shares;
  const price =
    order.price ?? order.limit_price ?? order.limit ?? order.limit_price_nanos;
  const qtyPart = qty == null || qty === "" ? "" : " " + String(qty);
  const pricePart =
    price == null || price === ""
      ? ""
      : " @ " + formatOrderPrice(Number(price));
  return side + qtyPart + pricePart;
}

export function orderSideTone(order: unknown): "yes" | "no" | "accent" {
  if (!isRecord(order)) return "accent";
  const side = text(order.side ?? order.action ?? order.type, "");
  if (side.includes("Yes")) return "yes";
  if (side.includes("No")) return "no";
  return "accent";
}

function formatOrderPrice(value: number): string {
  if (!Number.isFinite(value)) return "-";
  if (value > 1) return "$" + (value / 1e9).toFixed(3);
  return (value * 100).toFixed(1) + "%";
}

function num(value: number | null | undefined): number {
  return Number.isFinite(value) ? Number(value) : 0;
}

function finiteOrNull(value: number | null | undefined): number | null {
  const n = Number(value);
  return Number.isFinite(n) ? n : null;
}

function text(value: unknown, fallback: string): string {
  return typeof value === "string" && value ? value : fallback;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
