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
  DevLiquidityHealth,
  DevBotsResponse,
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

export function priceState(m: DevMarket): "Sybil mark" | "ref only" | "unpriced" {
  if (present(m.yes_price_nanos)) return "Sybil mark";
  if (present(m.reference_price_nanos)) return "ref only";
  return "unpriced";
}

export function priceStateClass(m: DevMarket): "yes" | "accent" | "no" {
  if (present(m.yes_price_nanos)) return "yes";
  if (present(m.reference_price_nanos)) return "accent";
  return "no";
}

export function priceGap(m: DevMarket): number {
  if (!present(m.yes_price_nanos) || !present(m.reference_price_nanos)) return 0;
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
      map.set(id, { market_id: id, count: 0, BuyYes: 0, BuyNo: 0, SellYes: 0, SellNo: 0 });
    }
    const row = map.get(id)!;
    row.count++;
    if (o.side === "BuyYes" || o.side === "BuyNo" || o.side === "SellYes" || o.side === "SellNo") {
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
export function pendingByAccount(orders: DevPendingOrder[]): Map<number, number> {
  const map = new Map<number, number>();
  for (const o of orders) {
    const id = Number(o.account_id);
    if (!Number.isFinite(id)) continue;
    map.set(id, (map.get(id) || 0) + 1);
  }
  return map;
}

function accountPendingCount(byAccount: Map<number, number>, id: number): number {
  return byAccount.get(Number(id)) || 0;
}

// ── market filtering (index.html:1148-1169) ───────────────────────────

export interface FilterMarketsOpts {
  search: string;
  group: string;
  state: string; // "all" | "marked" | "ref" | "none" | "pending" | "mismatch"
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
      (m) => String(m.market_id).includes(q) || m.name.toLowerCase().includes(q),
    );
  }

  if (opts.state === "marked") {
    list = list.filter((m) => present(m.yes_price_nanos));
  } else if (opts.state === "ref") {
    list = list.filter((m) => !present(m.yes_price_nanos) && present(m.reference_price_nanos));
  } else if (opts.state === "none") {
    list = list.filter((m) => !present(m.yes_price_nanos) && !present(m.reference_price_nanos));
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
  if (!present(m.yes_price_24h_ago_nanos) || !present(m.yes_price_nanos)) return "—";
  const delta = (n(m.yes_price_nanos) - n(m.yes_price_24h_ago_nanos)) / 1e9;
  const sign = delta >= 0 ? "+" : "";
  return sign + (delta * 100).toFixed(1) + "¢";
}

export function yesDelta24hClass(m: DevMarket): "yes" | "no" | "dim" {
  if (!present(m.yes_price_24h_ago_nanos) || !present(m.yes_price_nanos)) return "dim";
  return n(m.yes_price_nanos) >= n(m.yes_price_24h_ago_nanos) ? "yes" : "no";
}

export function orderStatsSub(bucket: DevOverviewBucket | null | undefined): string {
  if (!bucket || !bucket.orders) return "placed 0 · unmatched 0";
  return (
    "placed " + fmtInt(bucket.orders.placed) + " · unmatched " + fmtInt(bucket.orders.unmatched)
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

export function articleLabel(a: string | ArticleLike | null | undefined): string {
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
      title: (latest.fill_count || 0) > 0 ? "Blocks are clearing trades" : "Blocks are alive but not clearing",
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
      " markets have committed Sybil marks. A mark uses the latest trade price, " +
      "or the current two-sided book midpoint when the market is quiet.",
  });

  items.push({
    title: "Reference price coverage",
    body:
      ref.length +
      " markets have external reference prices, and " +
      refOnly.length +
      " of those have no Sybil mark yet.",
  });

  if (pendingOrders.length) {
    const idx = pendingIndex(pendingOrders);
    const top = topPendingMarkets(idx)[0];
    const marketsWithPending = new Set(pendingOrders.map((o) => o.market_id)).size;
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
      title: uniqueStateRoots <= 1 ? "State root is not moving" : "State root is moving",
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
}

export function buildQuickAnswer(
  kind: "prices" | "chain" | "liquidity" | "mm",
  ctx: QuickAnswerContext,
): string {
  const { markets, blocks, pendingOrders, marketIndex: mIdx, aggregates } = ctx;

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
      " markets have committed Sybil marks.\n" +
      "A mark is the latest trade price, or the live two-sided book midpoint when quiet. " +
      noClear.length +
      " markets have no Sybil mark yet.\n" +
      refOnly.length +
      " markets have an external reference price but no Sybil mark."
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
    const idx = pendingIndex(pendingOrders);
    const marketsWithPending = new Set(pendingOrders.map((o) => o.market_id)).size;
    const rows = topPendingMarkets(idx)
      .slice(0, 8)
      .map((r) => "#" + r.market_id + " " + marketName(mIdx, r.market_id) + ": " + r.count + " pending")
      .join("\n");
    return (
      pendingOrders.length +
      " pending orders across " +
      marketsWithPending +
      " markets.\n" +
      (rows || "No pending orders are visible from /v1/orders/pending.")
    );
  }

  // kind === "mm"
  return (
    "Active trading accounts: " +
    aggregates.activeTradingAccounts.map((a) => "#" + a.account_id).join(", ") +
    "\n" +
    "Account #0 activity: " +
    (aggregates.accountZeroIsInactive ? "none visible" : "has activity") +
    "\n" +
    "Pending orders: " +
    aggregates.pendingOrders +
    "\n" +
    "Sybil-mark portfolio: $" +
    dollars(aggregates.portfolioValueNanos) +
    "\n" +
    "Sybil PnL: " +
    moneySigned(aggregates.pnlNanos / 1e9) +
    "\n" +
    "Positions: " +
    aggregates.positionCount
  );
}

// ── actor roles and canonical Sybil account aggregates ───────────────

export type ParticipantRole = "mm" | "noise" | "llm" | "other" | "system";

export function participantRoleIndex(
  health: DevLiquidityHealth | null | undefined,
  bots: DevBotsResponse | null | undefined,
): Map<number, ParticipantRole> {
  const roles = new Map<number, ParticipantRole>([[0, "system"]]);
  for (const actor of health?.actors ?? []) {
    const accountId = Number(actor.account_id);
    if (!Number.isFinite(accountId)) continue;
    roles.set(accountId, actor.role === "market_maker" ? "mm" : "noise");
  }
  for (const bot of bots?.summaries ?? []) {
    const accountId = Number(bot.account_id);
    if (!Number.isFinite(accountId) || roles.has(accountId)) continue;
    roles.set(accountId, bot.participant_kind === "llm" ? "llm" : "other");
  }
  return roles;
}

export function participantRoleLabel(
  accountId: number,
  roles: Map<number, ParticipantRole>,
): string {
  const role = roles.get(accountId) ?? "other";
  return role === "mm"
    ? "MM"
    : role === "noise"
      ? "Noise"
      : role === "llm"
        ? "LLM"
        : role === "system"
          ? "System"
          : "Other";
}

export interface PnlCohort {
  accountCount: number;
  portfolioValueNanos: number;
  pnlNanos: number;
}

export interface ActorPnlCohorts {
  mm: PnlCohort;
  noise: PnlCohort;
  llm: PnlCohort;
  all: PnlCohort;
  otherAccountCount: number;
}

function sumCohort(accounts: DevAccountPortfolio[]): PnlCohort {
  return accounts.reduce<PnlCohort>(
    (sum, account) => ({
      accountCount: sum.accountCount + 1,
      portfolioValueNanos:
        sum.portfolioValueNanos + (n(account.portfolio_value_nanos) || 0),
      pnlNanos: sum.pnlNanos + (n(account.pnl_nanos) || 0),
    }),
    { accountCount: 0, portfolioValueNanos: 0, pnlNanos: 0 },
  );
}

export function actorPnlCohorts(
  accounts: DevAccountPortfolio[],
  roles: Map<number, ParticipantRole>,
): ActorPnlCohorts {
  const byRole = (role: ParticipantRole) =>
    accounts.filter((account) => roles.get(account.account_id) === role);
  const mmAccounts = byRole("mm");
  const noiseAccounts = byRole("noise");
  const llmAccounts = byRole("llm");
  const actorIds = new Set(
    [...mmAccounts, ...noiseAccounts, ...llmAccounts].map((account) => account.account_id),
  );
  return {
    mm: sumCohort(mmAccounts),
    noise: sumCohort(noiseAccounts),
    llm: sumCohort(llmAccounts),
    all: sumCohort(accounts.filter((account) => actorIds.has(account.account_id))),
    otherAccountCount: accounts.filter(
      (account) => account.account_id !== 0 && !actorIds.has(account.account_id),
    ).length,
  };
}

export type PositionWithAccount = DevPosition & { account_id: number };

export interface OutcomePositionSummary {
  count: number;
  quantity: number;
  valueNanos: number;
}

export interface AccountAggregates {
  activeTradingAccounts: DevAccountPortfolio[];
  selectedTradingAccounts: DevAccountPortfolio[];
  cashNanos: number;
  portfolioValueNanos: number;
  pnlNanos: number;
  positionCount: number;
  pendingOrders: number;
  yes: OutcomePositionSummary;
  no: OutcomePositionSummary;
  positionsByValue: PositionWithAccount[];
  accountZeroIsInactive: boolean;
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
    .filter((a) => a.account_id !== 0)
    .filter(
      (a) =>
        (a.positions || []).length > 0 ||
        pendCount(a.account_id) > 0 ||
        Math.abs(n(a.pnl_nanos) || 0) > 0,
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

  const zero = accounts.find((row) => row.account_id === 0);
  const accountZeroIsInactive =
    !!zero && (zero.positions || []).length === 0 && pendCount(0) === 0;

  const cashNanos = selectedTradingAccounts.reduce(
    (sum, a) => sum + (n(a.balance_nanos) || 0),
    0,
  );
  const portfolioValueNanos = selectedTradingAccounts.reduce(
    (sum, a) => sum + (n(a.portfolio_value_nanos) || 0),
    0,
  );
  const pnlNanos = selectedTradingAccounts.reduce(
    (sum, a) => sum + (n(a.pnl_nanos) || 0),
    0,
  );
  const positionCount = selectedTradingAccounts.reduce(
    (sum, a) => sum + (a.positions || []).length,
    0,
  );
  const pendingOrders = selectedTradingAccounts.reduce(
    (sum, a) => sum + pendCount(a.account_id),
    0,
  );
  const positionsByValue = selectedTradingAccounts
    .flatMap((a) =>
      (a.positions || []).map((p) => ({ ...p, account_id: a.account_id })),
    )
    .sort((a, b) => Math.abs(n(b.value_nanos) || 0) - Math.abs(n(a.value_nanos) || 0));
  const outcomeSummary = (outcome: "YES" | "NO"): OutcomePositionSummary =>
    positionsByValue
      .filter((position) => position.outcome === outcome)
      .reduce<OutcomePositionSummary>(
        (sum, position) => ({
          count: sum.count + 1,
          quantity: sum.quantity + (n(position.quantity) || 0),
          valueNanos: sum.valueNanos + (n(position.value_nanos) || 0),
        }),
        { count: 0, quantity: 0, valueNanos: 0 },
      );

  return {
    activeTradingAccounts,
    selectedTradingAccounts,
    cashNanos,
    portfolioValueNanos,
    pnlNanos,
    positionCount,
    pendingOrders,
    yes: outcomeSummary("YES"),
    no: outcomeSummary("NO"),
    positionsByValue,
    accountZeroIsInactive,
  };
}
