/**
 * Minimal structural types for the Dev Zone. Deliberately loose: *_nanos
 * are `number` because the wire sends JSON numbers and the Dev Zone does
 * float math (see lib/dev/format.ts). Only fields actually consumed are
 * listed. Field names mirror the console's usage in index.html.
 */

export interface DevMarket {
  market_id: number;
  name: string;
  yes_price_nanos?: number | null;
  no_price_nanos?: number | null;
  reference_price_nanos?: number | null;
  volume_nanos?: number | null;
  volume_24h_nanos?: number | null;
  trader_count?: number | null;
  liquidity_avg10_nanos?: number | null;
  liquidity_band_nanos?: number | null;
  yes_price_24h_ago_nanos?: number | null;
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
  value_nanos?: number | null;
  avg_entry_price_nanos?: number | null;
  current_price_nanos?: number | null;
}

export interface DevAccountPortfolio {
  account_id: number;
  balance_nanos?: number | null;
  portfolio_value_nanos?: number | null;
  pnl_nanos?: number | null;
  total_deposited_nanos?: number | null;
  total_fill_count?: number | null;
  first_deposit_ms?: number | null;
  realized_pnl_nanos?: number | null;
  unrealized_pnl_nanos?: number | null;
  positions?: DevPosition[];
}

export interface DevBlockMarketStats {
  placers?: number;
  volume_nanos?: number;
  placed?: number;
  matched?: number;
  unmatched?: number;
  welfare_nanos?: number;
}

export interface DevSystemEvent {
  type?: string;
  order_id?: number;
  account_id?: number;
  market_ids?: number[];
  side?: string;
  remaining_quantity?: number;
}

export interface DevBlock {
  height: number;
  timestamp_ms?: number;
  state_root?: string | null;
  parent_hash?: string | null;
  order_count?: number;
  fill_count?: number;
  total_volume_nanos?: number;
  total_welfare_nanos?: number;
  unique_placers?: number;
  clearing_prices_nanos?: Record<string, unknown>;
  fills?: unknown[];
  rejections?: unknown[];
  system_events?: DevSystemEvent[];
  by_market?: Record<string, DevBlockMarketStats>;
  bridge?: unknown;
}

export interface DevOverviewBucket {
  unique_traders?: number;
  total_volume_nanos?: number;
  orders?: { placed?: number; matched?: number; unmatched?: number };
}

export interface DevActivityOverview {
  all_time?: DevOverviewBucket;
  last_24h?: DevOverviewBucket;
}

export interface DevOpenBatch {
  unique_placers?: number;
  indicative_yes_price_nanos?: number | null;
  indicative_no_price_nanos?: number | null;
  indicative_volume_nanos?: number | null;
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
