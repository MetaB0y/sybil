use std::collections::HashMap;

use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct AccountResponse {
    pub account_id: u64,
    pub balance_dollars: f64,
    pub positions: Vec<PositionResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PositionResponse {
    pub market_id: u32,
    pub outcome: String,
    pub quantity: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MarketResponse {
    pub market_id: u32,
    pub name: String,
    pub yes_price: Option<f64>,
    pub no_price: Option<f64>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub winning_outcome: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub challenge_deadline_ms: Option<u64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MarketGroupResponse {
    pub name: String,
    pub market_ids: Vec<u32>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MarketPricesResponse {
    pub prices: HashMap<String, MarketPriceResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MarketPriceResponse {
    pub yes_price: f64,
    pub no_price: f64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OrderAcceptedResponse {
    pub accepted: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FillResponse {
    pub order_id: u64,
    pub fill_qty: u64,
    pub fill_price: f64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RejectionResponse {
    pub order_id: u64,
    pub account_id: u64,
    pub reason: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BlockResponse {
    pub height: u64,
    pub parent_hash: String,
    pub state_root: String,
    pub order_count: u32,
    pub fill_count: u32,
    pub timestamp_ms: u64,
    pub fills: Vec<FillResponse>,
    pub clearing_prices: HashMap<String, Vec<f64>>,
    pub rejections: Vec<RejectionResponse>,
    pub total_welfare: f64,
    pub total_volume: f64,
    pub orders_filled: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub height: Option<u64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StateRootResponse {
    pub state_root: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ResolveMarketResponse {
    pub market_id: u32,
    pub winning_outcome: u8,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub challenge_deadline_ms: Option<u64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreateMarketResponse {
    pub market_id: u32,
    pub name: String,
}
