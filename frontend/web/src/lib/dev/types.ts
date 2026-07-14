/**
 * Minimal structural types for the Dev Zone. Deliberately loose: *_nanos
 * are `number | string` (the wire sends JSON numbers; the generated schema
 * types them as string) and the Dev Zone does float math after coercion
 * (see lib/dev/format.ts). Only fields actually consumed are listed. Field
 * names mirror the console's usage in index.html.
 */

export interface DevMarket {
  market_id: number;
  name: string;
  yes_price_nanos?: number | string | null;
  no_price_nanos?: number | string | null;
  reference_price_nanos?: number | string | null;
  volume_nanos?: number | string | null;
  volume_24h_nanos?: number | string | null;
  trader_count?: number | null;
  liquidity_avg10_nanos?: number | string | null;
  liquidity_band_nanos?: number | string | null;
  yes_price_24h_ago_nanos?: number | string | null;
  orders_placed_total?: number | null;
  orders_matched_total?: number | null;
  orders_unmatched_total?: number | null;
}

export interface DevMarketGroup {
  name: string;
  market_ids: number[];
}

export interface DevPendingOrder {
  market_id: number;
  account_id: number | string;
  side: "BuyYes" | "BuyNo" | "SellYes" | "SellNo" | string;
}

export interface DevPosition {
  market_id: number;
  outcome: "YES" | "NO" | string;
  quantity: number;
  value_nanos?: number | string | null;
  avg_entry_price_nanos?: number | string | null;
  current_price_nanos?: number | string | null;
}

export interface DevAccountPortfolio {
  account_id: number;
  balance_nanos?: number | string | null;
  portfolio_value_nanos?: number | string | null;
  pnl_nanos?: number | string | null;
  total_deposited_nanos?: number | string | null;
  total_fill_count?: number | null;
  first_deposit_ms?: number | null;
  realized_pnl_nanos?: number | string | null;
  unrealized_pnl_nanos?: number | string | null;
  positions?: DevPosition[];
}

export interface DevBlockMarketStats {
  placers?: number;
  volume_nanos?: number | string | null;
  placed?: number;
  matched?: number;
  unmatched?: number;
  welfare_nanos?: number | string | null;
  mm_matched_orders?: number;
  noise_matched_orders?: number;
  organic_matched_orders?: number;
  mm_fill_notional_nanos?: number | string | null;
  noise_fill_notional_nanos?: number | string | null;
  organic_fill_notional_nanos?: number | string | null;
}

export interface DevBlock {
  height: number;
  timestamp_ms?: number;
  state_root?: string | null;
  parent_hash?: string | null;
  order_count?: number;
  fill_count?: number;
  rejection_count?: number;
  total_volume_nanos?: number | string | null;
  total_welfare_nanos?: number | string | null;
  unique_placers?: number;
  clearing_prices_nanos?: Record<string, unknown>;
  fills?: unknown[];
  rejections?: unknown[];
  resolved_market_ids?: number[];
  by_market?: Record<string, DevBlockMarketStats>;
  bridge?: unknown;
}

export interface DevMarketLiquidityHealth {
  market_id: number;
  mm_orders: number;
  mm_skip_reason?: string | null;
  noise_actor_count: number;
  noise_orders: number;
  noise_crossing_orders: number;
  other_non_mm_orders: number;
  clearing_price_present: boolean;
  fill_volume_nanos: number | string;
}

export interface DevActorIdentity {
  account_id: number;
  principal_id: string;
  role: "market_maker" | "noise" | string;
  last_observed_height?: number | null;
  ready: boolean;
}

export interface DevLiquidityHealth {
  height: number;
  universe_generation: number;
  active_markets: number;
  mm_markets_quoted: number;
  mm_coverage_bps: number;
  mm_markets_two_sided: number;
  mm_two_sided_coverage_bps: number;
  expected_noise_actors: number;
  observed_noise_actors: number;
  markets_with_two_noise_actors: number;
  markets_with_three_noise_actors: number;
  noise_markets_selected: number;
  noise_coverage_bps: number;
  noise_markets_crossing_mm: number;
  noise_crossing_coverage_bps: number;
  markets_with_noise_fills: number;
  rolling_window_blocks: number;
  rolling_mm_coverage_bps: number;
  rolling_mm_two_sided_coverage_bps: number;
  rolling_noise_coverage_bps: number;
  rolling_noise_crossing_coverage_bps: number;
  rolling_noise_fill_coverage_bps: number;
  markets_with_clearing_prices: number;
  total_fills: number;
  total_rejections: number;
  total_volume_nanos: number | string;
  actors: DevActorIdentity[];
  markets: DevMarketLiquidityHealth[];
}

export interface DevOverviewBucket {
  unique_traders?: number;
  total_volume_nanos?: number | string | null;
  total_welfare_nanos?: number | string | null;
  orders?: { placed?: number; matched?: number; unmatched?: number };
}

export interface DevActivityOverview {
  all_time?: DevOverviewBucket;
  last_24h?: DevOverviewBucket;
}

export interface DevOpenBatch {
  unique_placers?: number;
  indicative_yes_price_nanos?: number | string | null;
  indicative_no_price_nanos?: number | string | null;
  indicative_volume_nanos?: number | string | null;
  indicative_computed_at_ms?: number | null;
}

export interface DevBotDecision {
  id: number | string;
  trader_name: string;
  market_id?: number;
  market_name?: string;
  timestamp?: number | string;
  edge?: number;
  fair_value?: number;
  market_price?: number;
  balance?: number;
  yes_pos?: number;
  no_pos?: number;
  llm_duration_s?: number;
  motivation?: string;
  analysis?: string;
  orders?: unknown[];
  article_urls?: unknown[];
}

export interface DevBotSummary {
  trader_name: string;
  account_id?: number | null;
  participant_kind?: "llm" | "noise" | "legacy" | string | null;
  decision_count?: number;
  avg_edge?: number;
  latest_market_name?: string;
  latest_fair_value?: number;
  latest_market_price?: number;
  pnl?: number;
  total_orders?: number;
  total_fills?: number;
}

export interface DevBotsResponse {
  db_available?: boolean;
  error?: string;
  stats?: Record<string, number | string>;
  summaries?: DevBotSummary[];
  decisions?: DevBotDecision[];
  token_usage?: unknown[];
}
