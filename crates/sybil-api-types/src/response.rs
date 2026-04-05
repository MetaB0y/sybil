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
pub struct FillResponse {
    pub order_id: u64,
    pub fill_qty: u64,
    pub fill_price_nanos: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RejectionResponse {
    pub order_id: u64,
    pub account_id: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BlockResponse {
    pub height: u64,
    pub parent_hash: String,
    pub state_root: String,
    pub order_count: u32,
    pub fill_count: u32,
    pub timestamp_ms: u64,
    #[serde(default)]
    pub fills: Vec<FillResponse>,
    #[serde(default)]
    pub clearing_prices_nanos: HashMap<String, Vec<u64>>,
    #[serde(default)]
    pub rejections: Vec<RejectionResponse>,
    pub total_welfare_nanos: i64,
    pub total_volume_nanos: u64,
    pub orders_filled: usize,
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
pub struct ResolveMarketResponse {
    pub market_id: u32,
    pub payout_nanos: u64,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub challenge_deadline_ms: Option<u64>,
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
