/**
 * Dev Zone derived selectors — pure, side-effect-free ports of the Sybil
 * console's Alpine getters/methods (formerly crates/sybil-api/static/index.html,
 * deleted in SYB-174; this Dev Zone is now the sole dev console).
 * The React views call these so they stay thin and the logic is testable.
 *
 * `*_nanos` arithmetic mirrors the old console: raw `Number()` coercion, float
 * math. Fields that may arrive as strings on the wire are coerced via `n()`.
 */

import type {
  DevMarket,
  DevMarketGroup,
  DevPendingOrder,
  DevPosition,
  DevAccountPortfolio,
  DevBlock,
  DevBlockMarketStats,
  DevOverviewBucket,
} from "./types";
import { fmtInt, dollars, moneySigned, fmtProb } from "./format";

// ── coercion helpers ──────────────────────────────────────────────────

/** Raw numeric coercion. `null`/`undefined` → NaN, mirroring the console. */
function n(v: unknown): number {
  if (v == null) return NaN;
  return Number(v);
}

/** True when a nanos field is present (not null/undefined). */
function present(v: unknown): boolean {
  return v != null;
}

/** Build a market_id → DevMarket lookup. */
export function marketIndex(markets: DevMarket[]): Map<number, DevMarket> {
  const map = new Map<number, DevMarket>();
  for (const m of markets) map.set(Number(m.market_id), m);
  return map;
}

function marketName(idx: Map<number, DevMarket>, id: number): string {
  const m = idx.get(Number(id));
  return m ? m.name : "#" + id;
}

// ── price state (index.html:1506-1519) ────────────────────────────────

export function priceState(m: DevMarket): "cleared" | "ref only" | "no clears" {
  if (present(m.yes_price_nanos)) return "cleared";
  if (present(m.reference_price_nanos)) return "ref only";
  return "no clears";
}

export function priceStateClass(m: DevMarket): "yes" | "accent" | "no" {
  if (present(m.yes_price_nanos)) return "yes";
  if (present(m.reference_price_nanos)) return "accent";
  return "no";
}

export function priceGap(m: DevMarket): number {
  if (!present(m.yes_price_nanos) || !present(m.reference_price_nanos))
    return 0;
  return Math.abs(n(m.yes_price_nanos) - n(m.reference_price_nanos)) / 1e9;
}

// ── pending orders (index.html:1135-1147, 1170-1178, 1496-1502) ────────

export interface PendingRow {
  market_id: number;
  count: number;
  BuyYes: number;
  BuyNo: number;
  SellYes: number;
  SellNo: number;
}

export type PendingIndex = Map<number, PendingRow>;

export function pendingIndex(orders: DevPendingOrder[]): PendingIndex {
  const map: PendingIndex = new Map();
  for (const o of orders) {
    const id = Number(o.market_id);
    if (!map.has(id)) {
      map.set(id, {
        market_id: id,
        count: 0,
        BuyYes: 0,
        BuyNo: 0,
        SellYes: 0,
        SellNo: 0,
      });
    }
    const row = map.get(id)!;
    row.count++;
    if (
      o.side === "BuyYes" ||
      o.side === "BuyNo" ||
      o.side === "SellYes" ||
      o.side === "SellNo"
    ) {
      row[o.side] += 1;
    }
  }
  return map;
}

export function pendingCount(idx: PendingIndex, id: number): number {
  const row = idx.get(Number(id));
  return row ? row.count : 0;
}

export function topPendingMarkets(idx: PendingIndex): PendingRow[] {
  return Array.from(idx.values())
    .sort((a, b) => b.count - a.count)
    .slice(0, 30);
}

/** Pending-order count keyed by account id (index.html:1170-1178, 1500-1502). */
export function pendingByAccount(
  orders: DevPendingOrder[],
): Map<number, number> {
  const map = new Map<number, number>();
  for (const o of orders) {
    const id = Number(o.account_id);
    if (!Number.isFinite(id)) continue;
    map.set(id, (map.get(id) || 0) + 1);
  }
  return map;
}

function accountPendingCount(
  byAccount: Map<number, number>,
  id: number,
): number {
  return byAccount.get(Number(id)) || 0;
}

// ── market filtering (index.html:1148-1169) ───────────────────────────

export interface FilterMarketsOpts {
  search: string;
  group: string;
  state: string; // "all" | "cleared" | "ref" | "none" | "pending" | "mismatch"
}

export function filterMarkets(
  markets: DevMarket[],
  opts: FilterMarketsOpts,
  pendingIdx: PendingIndex,
  groups?: DevMarketGroup[],
): DevMarket[] {
  const q = (opts.search || "").trim().toLowerCase();
  let list = markets;

  if (opts.group) {
    const group = (groups || []).find((g) => g.name === opts.group);
    const ids = new Set(group ? group.market_ids : []);
    list = list.filter((m) => ids.has(m.market_id));
  }

  if (q) {
    list = list.filter(
      (m) =>
        String(m.market_id).includes(q) || m.name.toLowerCase().includes(q),
    );
  }

  if (opts.state === "cleared") {
    list = list.filter((m) => present(m.yes_price_nanos));
  } else if (opts.state === "ref") {
    list = list.filter(
      (m) => !present(m.yes_price_nanos) && present(m.reference_price_nanos),
    );
  } else if (opts.state === "none") {
    list = list.filter(
      (m) => !present(m.yes_price_nanos) && !present(m.reference_price_nanos),
    );
  } else if (opts.state === "pending") {
    list = list.filter((m) => pendingCount(pendingIdx, m.market_id) > 0);
  } else if (opts.state === "mismatch") {
    list = list.filter((m) => priceGap(m) >= 0.1);
  }

  return list.slice().sort((a, b) => {
    const pa = pendingCount(pendingIdx, a.market_id);
    const pb = pendingCount(pendingIdx, b.market_id);
    return (
      (n(b.volume_nanos) || 0) - (n(a.volume_nanos) || 0) ||
      pb - pa ||
      a.market_id - b.market_id
    );
  });
}

// ── aggregates tab (index.html:1608-1673) ─────────────────────────────

export function topMarketsByVolume24h(markets: DevMarket[]): DevMarket[] {
  return markets
    .slice()
    .sort(
      (a, b) =>
        (n(b.volume_24h_nanos) || 0) - (n(a.volume_24h_nanos) || 0) ||
        (n(b.volume_nanos) || 0) - (n(a.volume_nanos) || 0) ||
        a.market_id - b.market_id,
    )
    .slice(0, 80);
}

export interface BlockMarketRow {
  market_id: number;
  name: string;
  label: string;
  placers: number;
  volume_nanos: number;
  placed: number;
  matched: number;
  unmatched: number;
  welfare_nanos: number;
}

export function latestBlockByMarketRows(
  block: DevBlock | null,
  marketIdx: Map<number, DevMarket>,
): BlockMarketRow[] {
  if (!block || !block.by_market) return [];
  return Object.entries(block.by_market)
    .map(([mid, stats]: [string, DevBlockMarketStats]) => {
      const id = Number(mid);
      const m = marketIdx.get(id);
      return {
        market_id: id,
        name: m ? m.name : "#" + id,
        label: m ? "#" + id + " · " + m.name : "#" + id,
        placers: stats.placers || 0,
        volume_nanos: n(stats.volume_nanos) || 0,
        placed: stats.placed || 0,
        matched: stats.matched || 0,
        unmatched: stats.unmatched || 0,
        welfare_nanos: n(stats.welfare_nanos) || 0,
      };
    })
    .sort(
      (a, b) =>
        b.volume_nanos - a.volume_nanos ||
        b.placers - a.placers ||
        a.market_id - b.market_id,
    );
}

export function fmtLiquidity(m: DevMarket): string {
  if (!m.liquidity_avg10_nanos) return "—";
  const avg = "$" + (n(m.liquidity_avg10_nanos) / 1e9).toFixed(2);
  if (!m.liquidity_band_nanos) return avg;
  const band = "±$" + (n(m.liquidity_band_nanos) / 1e9).toFixed(2);
  return avg + " " + band;
}

export function fmtYesDelta24h(m: DevMarket): string {
  if (!present(m.yes_price_24h_ago_nanos) || !present(m.yes_price_nanos))
    return "—";
  const delta = (n(m.yes_price_nanos) - n(m.yes_price_24h_ago_nanos)) / 1e9;
  const sign = delta >= 0 ? "+" : "";
  return sign + (delta * 100).toFixed(1) + "¢";
}

export function yesDelta24hClass(m: DevMarket): "yes" | "no" | "dim" {
  if (!present(m.yes_price_24h_ago_nanos) || !present(m.yes_price_nanos))
    return "dim";
  return n(m.yes_price_nanos) >= n(m.yes_price_24h_ago_nanos) ? "yes" : "no";
}

export function orderStatsSub(
  bucket: DevOverviewBucket | null | undefined,
): string {
  if (!bucket || !bucket.orders) return "placed 0 · unmatched 0";
  return (
    "placed " +
    fmtInt(bucket.orders.placed) +
    " · unmatched " +
    fmtInt(bucket.orders.unmatched)
  );
}

// ── positions / orders / articles (index.html:1520-1548) ──────────────

interface OrderLike {
  side?: unknown;
  action?: unknown;
  type?: unknown;
  quantity?: unknown;
  qty?: unknown;
  size?: unknown;
  shares?: unknown;
  price?: unknown;
  limit_price?: unknown;
  limit?: unknown;
  limit_price_nanos?: unknown;
}

export function formatOrder(o: OrderLike): string {
  const side = o.side || o.action || o.type || "order";
  const qty = o.quantity ?? o.qty ?? o.size ?? o.shares ?? "";
  const price = o.price ?? o.limit_price ?? o.limit ?? o.limit_price_nanos;
  // Heuristic: a value > 1 is treated as nanos, <= 1 as a probability —
  // ambiguous for a literal price of exactly 1.
  const px =
    price == null
      ? ""
      : " @ " +
        (Number(price) > 1
          ? "$" + (Number(price) / 1e9).toFixed(3)
          : fmtProb(price as number));
  return String(side) + (qty !== "" ? " " + qty : "") + px;
}

interface DecisionLike {
  orders?: unknown;
  article_urls?: unknown;
}

export function orderList(d: DecisionLike): unknown[] {
  return Array.isArray(d.orders) ? d.orders : [];
}

export function articleList(d: DecisionLike): unknown[] {
  return Array.isArray(d.article_urls) ? d.article_urls : [];
}

interface ArticleLike {
  url?: string;
  source?: string;
  title?: string;
}

export function articleUrl(a: string | ArticleLike | null | undefined): string {
  return typeof a === "string" ? a : (a && a.url) || "#";
}

export function articleLabel(
  a: string | ArticleLike | null | undefined,
): string {
  if (typeof a === "string") return a;
  if (!a) return "article";
  return [a.source, a.title || a.url].filter(Boolean).join(": ");
}

// ── insights (index.html:1234-1265) ───────────────────────────────────

export interface InsightsContext {
  markets: DevMarket[];
  blocks: DevBlock[];
  pendingOrders: DevPendingOrder[];
}

interface Insight {
  title: string;
  body: string;
}

export function buildInsights(ctx: InsightsContext): Insight[] {
  const { markets, blocks, pendingOrders } = ctx;
  const items: Insight[] = [];

  const latest = blocks.length ? blocks[blocks.length - 1] : null;
  if (latest) {
    items.push({
      title:
        (latest.fill_count || 0) > 0
          ? "Blocks are clearing trades"
          : "Blocks are alive but not clearing",
      body:
        "Latest block #" +
        latest.height +
        " has " +
        (latest.order_count || 0) +
        " orders, " +
        (latest.fill_count || 0) +
        " fills, and $" +
        dollars(latest.total_volume_nanos) +
        " volume.",
    });
  }

  const priced = markets.filter((m) => present(m.yes_price_nanos));
  const ref = markets.filter((m) => present(m.reference_price_nanos));
  const refOnly = markets.filter(
    (m) => !present(m.yes_price_nanos) && present(m.reference_price_nanos),
  );

  items.push({
    title: "Price coverage",
    body:
      priced.length +
      " of " +
      markets.length +
      " markets have Sybil clearing prices. A market gets a clearing price only after a fill.",
  });

  items.push({
    title: "Reference price coverage",
    body:
      ref.length +
      " markets have external reference prices, and " +
      refOnly.length +
      " of those have no Sybil clears yet.",
  });

  if (pendingOrders.length) {
    const idx = pendingIndex(pendingOrders);
    const top = topPendingMarkets(idx)[0];
    const marketsWithPending = new Set(pendingOrders.map((o) => o.market_id))
      .size;
    items.push({
      title: "Pending liquidity is concentrated",
      body: top
        ? "Top pending market is #" +
          top.market_id +
          " with " +
          top.count +
          " resting orders. Only " +
          marketsWithPending +
          " markets have any pending orders."
        : "No pending-order detail available.",
    });
  }

  if (blocks.length) {
    const uniqueStateRoots = new Set(
      blocks.map((b) => b.state_root).filter(Boolean),
    ).size;
    items.push({
      title:
        uniqueStateRoots <= 1
          ? "State root is not moving"
          : "State root is moving",
      body:
        uniqueStateRoots +
        " unique state roots across " +
        blocks.length +
        " recent blocks. No fills usually means the root stays constant.",
    });
  }

  return items;
}

// ── quick answers (index.html:1461-1490) ──────────────────────────────

export interface QuickAnswerContext {
  markets: DevMarket[];
  blocks: DevBlock[];
  pendingOrders: DevPendingOrder[];
  accounts: DevAccountPortfolio[];
  marketIndex: Map<number, DevMarket>;
  aggregates: AccountAggregates;
  /** False when `/v1/orders/pending` could not be read (dev-only route). */
  pendingAvailable?: boolean;
  /** False when the per-account portfolio reads were rejected (bearer-gated). */
  accountsAvailable?: boolean;
}

export function buildQuickAnswer(
  kind: "prices" | "chain" | "liquidity" | "mm",
  ctx: QuickAnswerContext,
): string {
  const {
    markets,
    blocks,
    pendingOrders,
    marketIndex: mIdx,
    aggregates,
    pendingAvailable = true,
    accountsAvailable = true,
  } = ctx;

  if (kind === "prices") {
    const priced = markets.filter((m) => present(m.yes_price_nanos));
    const noClear = markets.filter((m) => !present(m.yes_price_nanos));
    const refOnly = markets.filter(
      (m) => !present(m.yes_price_nanos) && present(m.reference_price_nanos),
    );
    return (
      priced.length +
      " / " +
      markets.length +
      " markets have Sybil clearing prices.\n" +
      "A Sybil price appears only after a market has fills. " +
      noClear.length +
      " markets have no clears, so they show no clearing price.\n" +
      refOnly.length +
      " markets have an external reference price but still no Sybil fill."
    );
  }

  if (kind === "chain") {
    const latest = blocks.length ? blocks[blocks.length - 1] : null;
    const recentFills = blocks.reduce((s, b) => s + (b.fill_count || 0), 0);
    const recentVolumeNanos = blocks.reduce(
      (s, b) => s + (n(b.total_volume_nanos) || 0),
      0,
    );
    const uniqueStateRoots = new Set(
      blocks.map((b) => b.state_root).filter(Boolean),
    ).size;
    return (
      "Latest block: #" +
      (latest?.height ?? "unknown") +
      "\n" +
      "Recent window: " +
      blocks.length +
      " blocks, " +
      recentFills +
      " fills, $" +
      dollars(recentVolumeNanos) +
      " volume.\n" +
      "State roots in window: " +
      uniqueStateRoots +
      ". If this is 1, blocks are being produced but state is not changing."
    );
  }

  if (kind === "liquidity") {
    // Resting depth comes from /v1/orders/pending, which lives in the dev-only
    // route table and 404s against a deployment running SYBIL_DEV_MODE=false.
    // Answer from the public market summary instead of reporting a zero that
    // only means "never received".
    const quoted = markets.filter((m) => present(m.liquidity_avg10_nanos));
    const byDepth = [...quoted].sort(
      (a, b) => n(b.liquidity_avg10_nanos) - n(a.liquidity_avg10_nanos),
    );
    const totalDepth = quoted.reduce(
      (sum, m) => sum + n(m.liquidity_avg10_nanos),
      0,
    );
    const unmatched = markets.reduce(
      (sum, m) => sum + (m.orders_unmatched_total || 0),
      0,
    );
    const top = byDepth
      .slice(0, 8)
      .map(
        (m) =>
          "#" +
          m.market_id +
          " " +
          marketName(mIdx, m.market_id) +
          ": $" +
          dollars(n(m.liquidity_avg10_nanos)) +
          " avg depth",
      )
      .join("\n");

    const restingLine = pendingAvailable
      ? pendingOrders.length +
        " resting orders across " +
        new Set(pendingOrders.map((o) => o.market_id)).size +
        " markets."
      : "Resting-order detail unavailable: /v1/orders/pending is a dev-only " +
        "route and is not mounted on this deployment.";

    return (
      quoted.length +
      " / " +
      markets.length +
      " markets carry quoted depth, $" +
      dollars(totalDepth) +
      " total (10-block average).\n" +
      fmtInt(unmatched) +
      " orders placed all-time went unmatched.\n" +
      restingLine +
      (top ? "\n" + top : "")
    );
  }

  // kind === "mm"
  if (!accountsAvailable) {
    return (
      "Account figures unavailable.\n" +
      "/v1/accounts/{id}/portfolio is bearer-gated and the dev zone holds no " +
      "token, so MM cash, positions and PnL cannot be read here.\n" +
      "This is expected against a deployment running SYBIL_DEV_MODE=false; " +
      "it is not a sign that the market maker is flat."
    );
  }
  return (
    "Active trading accounts: " +
    (aggregates.activeTradingAccounts.map((a) => "#" + a.account_id).join(", ") ||
      "none") +
    "\n" +
    "Pending orders: " +
    (pendingAvailable ? aggregates.mmPendingOrders : "unavailable") +
    "\n" +
    "Canonical portfolio: $" +
    dollars(aggregates.mmPortfolioValueNanos) +
    "\n" +
    "Canonical PnL: " +
    moneySigned(aggregates.mmPnlNanos / 1e9) +
    "\n" +
    "Positions: " +
    aggregates.mmPositionCount
  );
}

// ── MM-tab account aggregates (index.html:1179-1226) ──────────────────

export type PositionWithAccount = DevPosition & { account_id: number };

export interface AccountAggregates {
  activeTradingAccounts: DevAccountPortfolio[];
  selectedTradingAccounts: DevAccountPortfolio[];
  mmCashNanos: number;
  mmPortfolioValueNanos: number;
  mmPnlNanos: number;
  mmPositionCount: number;
  mmPendingOrders: number;
  topMmPositions: PositionWithAccount[];
}

export function accountAggregates(
  accounts: DevAccountPortfolio[],
  selectedAccountId: number | null,
  pendingByAccountIdx?: Map<number, number>,
): AccountAggregates {
  // The console keys pending counts by account separately (pendingByAccount).
  // Callers may pass that map; if absent it degrades to zero counts.
  const byAccount = pendingByAccountIdx || new Map<number, number>();
  const pendCount = (id: number): number => accountPendingCount(byAccount, id);

  const activeTradingAccounts = accounts
    .filter(
      (a) =>
        (a.positions || []).length > 0 ||
        pendCount(a.account_id) > 0 ||
        Math.abs(n(a.pnl_nanos) || 0) > 0 ||
        (n(a.total_deposited_nanos) || 0) > 0,
    )
    .sort(
      (a, b) =>
        pendCount(b.account_id) - pendCount(a.account_id) ||
        (b.positions || []).length - (a.positions || []).length ||
        b.account_id - a.account_id,
    );

  const selectedTradingAccounts =
    selectedAccountId === null
      ? activeTradingAccounts
      : accounts.filter((a) => a.account_id === selectedAccountId);

  const mmCashNanos = selectedTradingAccounts.reduce(
    (sum, a) => sum + (n(a.balance_nanos) || 0),
    0,
  );
  const mmPortfolioValueNanos = selectedTradingAccounts.reduce(
    (sum, a) => sum + (n(a.portfolio_value_nanos) || 0),
    0,
  );
  const mmPnlNanos = selectedTradingAccounts.reduce(
    (sum, a) => sum + (n(a.pnl_nanos) || 0),
    0,
  );
  const mmPositionCount = selectedTradingAccounts.reduce(
    (sum, a) => sum + (a.positions || []).length,
    0,
  );
  const mmPendingOrders = selectedTradingAccounts.reduce(
    (sum, a) => sum + pendCount(a.account_id),
    0,
  );
  const topMmPositions = selectedTradingAccounts
    .flatMap((a) =>
      (a.positions || []).map((p) => ({ ...p, account_id: a.account_id })),
    )
    .sort(
      (a, b) =>
        Math.abs(n(b.value_nanos) || 0) - Math.abs(n(a.value_nanos) || 0),
    )
    .slice(0, 25);

  return {
    activeTradingAccounts,
    selectedTradingAccounts,
    mmCashNanos,
    mmPortfolioValueNanos,
    mmPnlNanos,
    mmPositionCount,
    mmPendingOrders,
    topMmPositions,
  };
}
