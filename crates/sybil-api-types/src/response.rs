//! API response types (DTOs).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AccountResponse {
    pub account_id: u64,
    pub balance_nanos: i64,
    #[serde(default)]
    pub positions: Vec<PositionResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PositionResponse {
    pub market_id: u32,
    pub outcome: String,
    pub quantity: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MarketResponse {
    pub market_id: u32,
    pub name: String,
    pub yes_price_nanos: Option<u64>,
    pub no_price_nanos: Option<u64>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payout_nanos: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub challenge_deadline_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_criteria: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiry_timestamp_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at_ms: Option<u64>,
    #[serde(default)]
    pub volume_nanos: u64,
    /// Reference price from external system (e.g., Polymarket), display only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_price_nanos: Option<u64>,
    /// External URL (e.g., Polymarket link).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_url: Option<String>,
    /// Polymarket parent event id — frontend grouping key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    /// Polymarket parent event title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_title: Option<String>,
    /// Event-level image URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_image_url: Option<String>,
    /// Event-level icon URL (secondary image fallback).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_icon_url: Option<String>,
    /// Event-level expected end date (epoch ms). Display only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_end_date_ms: Option<u64>,
    /// Per-market image URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_image_url: Option<String>,
    /// Per-market icon URL (secondary image fallback).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_icon_url: Option<String>,
    /// Per-market expected end date (epoch ms). Display only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_end_date_ms: Option<u64>,
    /// All category buckets the parent event matched on the mirror's
    /// tag-to-bucket lookup (e.g. `["Sports", "Politics"]`). Frontend picks
    /// one for display via its own priority list. None for sybil-native
    /// markets (use the singular `category` field instead).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<String>>,
    /// All-time unique trader count for this market (decision Q-table:
    /// MM, MINT, multi-market split, etc.). Off-block — "since last
    /// restart" until prod persistence is enabled.
    #[serde(default)]
    pub trader_count: u32,
    /// Rolling 24h trading volume in nanos (±1h bucket resolution). Off-block;
    /// "since last restart" until prod persistence is enabled.
    #[serde(default)]
    pub volume_24h_nanos: u64,
    /// Clearing YES price ~24h ago in nanos, derived from the per-market
    /// hourly snapshot. `None` for markets younger than 24h or wiped on
    /// restart. FE computes the 24h delta as `current − snapshot`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yes_price_24h_ago_nanos: Option<u64>,
    /// Clearing NO price ~24h ago in nanos. See `yes_price_24h_ago_nanos`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_price_24h_ago_nanos: Option<u64>,
    /// Rolling last-10-batch ±band depth average in nanos. Zero for markets
    /// without a clearing price yet. Pair with `liquidity_band_nanos` for
    /// labelling.
    #[serde(default)]
    pub liquidity_avg10_nanos: u64,
    /// Width of the band the liquidity score uses (the ± in "$X ±$0.05").
    /// Always the live config value — `0` when no liquidity has been
    /// recorded yet.
    #[serde(default)]
    pub liquidity_band_nanos: u64,
}

/// Minimal market data for high-throughput dashboards (drops strings & metadata).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MarketSummaryResponse {
    pub market_id: u32,
    pub name: String,
    pub yes_price_nanos: Option<u64>,
    pub no_price_nanos: Option<u64>,
    /// Reference price from external system (e.g., Polymarket), display only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_price_nanos: Option<u64>,
    pub volume_nanos: u64,
    pub status: String,
    /// All-time unique trader count (mirrors `MarketResponse.trader_count`).
    #[serde(default)]
    pub trader_count: u32,
    /// Rolling 24h trading volume in nanos (mirrors
    /// `MarketResponse.volume_24h_nanos`).
    #[serde(default)]
    pub volume_24h_nanos: u64,
    /// Clearing YES / NO prices ~24h ago (mirror of `MarketResponse`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yes_price_24h_ago_nanos: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_price_24h_ago_nanos: Option<u64>,
    /// Liquidity score + band (mirrors `MarketResponse`).
    #[serde(default)]
    pub liquidity_avg10_nanos: u64,
    #[serde(default)]
    pub liquidity_band_nanos: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MarketGroupResponse {
    pub name: String,
    pub market_ids: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MarketPricesResponse {
    pub prices: HashMap<String, MarketPriceResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MarketPriceResponse {
    pub yes_price_nanos: u64,
    pub no_price_nanos: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OrderAcceptedResponse {
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CancelOrderResponse {
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FillResponse {
    pub order_id: u64,
    pub fill_qty: u64,
    pub fill_price_nanos: u64,
    #[serde(default)]
    pub account_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SystemEventResponse {
    CreateAccount {
        account_id: u64,
        initial_balance_nanos: i64,
    },
    Deposit {
        account_id: u64,
        amount_nanos: i64,
    },
    L1Deposit {
        account_id: u64,
        amount_nanos: i64,
        deposit_id: u64,
        deposit_root_hex: String,
        sybil_account_key_hex: String,
    },
    WithdrawalCreated {
        account_id: u64,
        amount_nanos: i64,
        withdrawal_id: u64,
        nullifier_hex: String,
    },
    MarketResolved {
        market_id: u32,
        payout_nanos: u64,
        affected_accounts: Vec<u64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RejectionResponse {
    pub order_id: u64,
    pub account_id: u64,
    pub reason: String,
}

/// Nested per-market sidecar on `BlockResponse.by_market`. Grows append-only
/// across steps (each new field carries `#[serde(default)]` so partial
/// reverts stay clean). Volume/orders/welfare join in B2 / B6 / B7.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BlockMarketStats {
    /// Unique placers (non-MM accounts) admitted touching this market in
    /// the block. Multi-market orders credit each active market; the
    /// platform `unique_placers` scalar counts the account once.
    #[serde(default)]
    pub placers: u32,
    /// Per-market volume contribution from this block's fills, in nanos.
    /// Multi-market fills credit each active market with their full
    /// notional; the platform `total_volume_nanos` scalar counts each fill
    /// once.
    #[serde(default)]
    pub volume_nanos: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BlockResponse {
    pub height: u64,
    pub parent_hash: String,
    pub state_root: String,
    pub events_root: String,
    pub order_count: u32,
    pub fill_count: u32,
    pub timestamp_ms: u64,
    #[serde(default)]
    pub system_events: Vec<SystemEventResponse>,
    #[serde(default)]
    pub fills: Vec<FillResponse>,
    #[serde(default)]
    pub clearing_prices_nanos: HashMap<String, Vec<u64>>,
    #[serde(default)]
    pub rejections: Vec<RejectionResponse>,
    #[serde(default)]
    pub bridge: BridgeBlockResponse,
    pub total_welfare_nanos: i64,
    pub total_volume_nanos: u64,
    pub orders_filled: usize,
    /// Unique placers (non-MM accounts) admitted into this block. Platform
    /// scalar — `by_market[m].placers` is the per-market split.
    #[serde(default)]
    pub unique_placers: u32,
    /// Nested per-market scalars (decision Q1 in BACKEND_DATA_PLAN.md). Each
    /// `BlockMarketStats` carries the per-market splits for this block. Old
    /// clients ignore it; new clients consume what they recognise.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub by_market: HashMap<String, BlockMarketStats>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BridgeBlockResponse {
    pub deposit_count: u64,
    pub deposit_root_hex: String,
    #[serde(default)]
    pub consumed_deposits: Vec<BridgeDepositEventResponse>,
    #[serde(default)]
    pub withdrawal_leaves: Vec<BridgeWithdrawalResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BridgeDepositEventResponse {
    pub deposit_id: u64,
    pub account_id: u64,
    pub amount_token_units: u64,
    pub deposit_root_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BridgeStatusResponse {
    pub deposit_cursor: u64,
    pub deposit_root_hex: String,
    pub next_withdrawal_id: u64,
    pub withdrawal_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BridgeAccountKeyResponse {
    pub account_id: u64,
    pub sybil_account_key_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BridgeDepositResponse {
    pub account_id: u64,
    pub balance_nanos: i64,
    pub deposit_id: u64,
    pub deposit_root_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BridgeWithdrawalResponse {
    pub withdrawal_id: u64,
    pub account_id: u64,
    pub recipient_hex: String,
    pub token_hex: String,
    pub amount_token_units: u64,
    pub amount_nanos: u64,
    pub expiry_height: u64,
    pub nullifier_hex: String,
    pub withdrawal_leaf_hex: String,
    pub withdrawal_leaf_digest_hex: String,
    pub created_at_height: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct HealthResponse {
    pub status: String,
    #[serde(default)]
    pub height: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StateRootResponse {
    pub state_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StateProofResponse {
    pub block_height: u64,
    pub state_root: String,
    pub state_slot: String,
    pub leaf_key_hex: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leaf_key_ascii: Option<String>,
    pub proof_kind: String,
    pub proof_format: String,
    pub verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leaf_value_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inclusion_proof: Option<QmdbStateInclusionProofResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclusion_proof: Option<QmdbStateExclusionProofResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct QmdbStateInclusionProofResponse {
    pub operation: QmdbStateOperationProofResponse,
    pub next_key_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct QmdbStateExclusionProofResponse {
    pub variant: String,
    pub operation: QmdbStateOperationProofResponse,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_key_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_value_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_next_key_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata_hex: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct QmdbStateOperationProofResponse {
    pub location: u64,
    pub activity_chunk_hex: String,
    pub range: QmdbStateRangeProofResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct QmdbStateRangeProofResponse {
    pub leaves: u64,
    pub digests_hex: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_prefix_acc_hex: Option<String>,
    pub unfolded_prefix_peaks_hex: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial_chunk_digest_hex: Option<String>,
    pub ops_root_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ResolveMarketResponse {
    pub market_id: u32,
    pub payout_nanos: u64,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub challenge_deadline_ms: Option<u64>,
}

/// Detailed view of a market's resolution state. Unresolved markets return
/// `status = "active"` (or `proposed`/`challenged` for future policies) with
/// `payout_nanos = None`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ResolutionResponse {
    pub market_id: u32,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payout_nanos: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_by_feed_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_by_feed_name: Option<String>,
    pub template: String,
}

/// Registered data feed view, returned by GET/POST /v1/feeds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RegisteredFeedResponse {
    pub feed_id: u64,
    pub pubkey_hex: String,
    pub name: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateMarketResponse {
    pub market_id: u32,
    pub name: String,
}

// --- Portfolio ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PortfolioResponse {
    pub account_id: u64,
    pub balance_nanos: i64,
    pub total_deposited_nanos: i64,
    pub positions: Vec<PositionValueResponse>,
    pub total_position_value_nanos: i64,
    pub portfolio_value_nanos: i64,
    pub pnl_nanos: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PositionValueResponse {
    pub market_id: u32,
    pub outcome: String,
    pub quantity: i64,
    pub current_price_nanos: u64,
    pub value_nanos: i64,
}

// --- Price History ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PriceHistoryResponse {
    pub market_id: u32,
    pub points: Vec<PricePointResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PricePointResponse {
    pub height: u64,
    pub timestamp_ms: u64,
    pub yes_price_nanos: u64,
    pub no_price_nanos: u64,
    pub volume_nanos: u64,
}

// --- Account Fills ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AccountFillResponse {
    pub order_id: u64,
    pub fill_qty: u64,
    pub fill_price_nanos: u64,
    pub block_height: u64,
    pub timestamp_ms: u64,
    pub position_deltas: Vec<PositionDeltaResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PositionDeltaResponse {
    pub market_id: u32,
    pub outcome: String,
    pub delta: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PendingOrderResponse {
    pub order_id: u64,
    pub account_id: u64,
    pub market_id: u32,
    pub side: String,
    pub limit_price_nanos: u64,
    pub remaining_quantity: u64,
    pub created_at_block: u64,
    pub expires_at_block: u64,
}

// --- Aggregates (B1 onward) ------------------------------------------------

/// Per-bucket platform totals returned by `/v1/activity/overview`. B1
/// populates `unique_traders` only; volume + orders join in B2 / B6 and
/// remain zero until then.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OverviewBucketResponse {
    #[serde(default)]
    pub unique_traders: u64,
    #[serde(default)]
    pub total_volume_nanos: u64,
    #[serde(default)]
    pub orders: OverviewOrderStatsResponse,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OverviewOrderStatsResponse {
    #[serde(default)]
    pub placed: u64,
    #[serde(default)]
    pub matched: u64,
    #[serde(default)]
    pub unmatched: u64,
}

/// Response shape for `GET /v1/activity/overview`. All-time + 24h slices.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ActivityOverviewResponse {
    pub all_time: OverviewBucketResponse,
    pub last_24h: OverviewBucketResponse,
}

/// Response shape for `GET /v1/markets/{id}/open-batch`. B1 populates
/// `unique_placers`; indicative fields stub `None`/`0` until C2.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OpenBatchResponse {
    pub unique_placers: u32,
    #[serde(default)]
    pub indicative_yes_price_nanos: Option<u64>,
    #[serde(default)]
    pub indicative_no_price_nanos: Option<u64>,
    #[serde(default)]
    pub indicative_volume_nanos: u64,
    #[serde(default)]
    pub indicative_computed_at_ms: u64,
}

/// Response shape for `GET /v1/events/{event_id}/traders`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EventTradersResponse {
    pub trader_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_block_response() -> BlockResponse {
        BlockResponse {
            height: 1,
            parent_hash: "00".into(),
            state_root: "11".into(),
            events_root: "22".into(),
            order_count: 0,
            fill_count: 0,
            timestamp_ms: 0,
            system_events: vec![],
            fills: vec![],
            clearing_prices_nanos: HashMap::new(),
            rejections: vec![],
            bridge: BridgeBlockResponse::default(),
            total_welfare_nanos: 0,
            total_volume_nanos: 0,
            orders_filled: 0,
            unique_placers: 0,
            by_market: HashMap::new(),
        }
    }

    /// `by_market` is `skip_serializing_if = HashMap::is_empty` so an empty
    /// map produces JSON byte-identical to pre-A1 BlockResponse. Old clients
    /// that don't know the field see no change.
    #[test]
    fn block_response_serde_roundtrip() {
        let blk = empty_block_response();
        let json = serde_json::to_string(&blk).expect("serialize");
        assert!(
            !json.contains("by_market"),
            "empty by_market must not serialize: {json}"
        );

        // Deserialize an "old shape" payload that has no by_market key at all.
        let old_shape = serde_json::to_string(&blk).expect("serialize");
        let parsed: BlockResponse = serde_json::from_str(&old_shape).expect("deserialize");
        assert!(parsed.by_market.is_empty());

        // Round-trip with a populated map.
        let mut blk2 = empty_block_response();
        blk2.by_market
            .insert("7".into(), BlockMarketStats::default());
        let json2 = serde_json::to_string(&blk2).expect("serialize with map");
        assert!(json2.contains("by_market"));
        let parsed2: BlockResponse = serde_json::from_str(&json2).expect("deserialize with map");
        assert_eq!(parsed2.by_market.len(), 1);
        assert!(parsed2.by_market.contains_key("7"));
    }
}
