use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub const NANOS_PER_DOLLAR: u64 = 1_000_000_000;

// --- Requests ---

#[derive(Debug, Serialize)]
pub struct CreateAccountRequest {
    pub initial_balance_nanos: u64,
}

#[derive(Debug, Serialize)]
pub struct FundAccountRequest {
    pub amount_nanos: u64,
}

#[derive(Debug, Serialize)]
pub struct CreateMarketRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution_criteria: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiry_timestamp_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct CreateMarketGroupRequest {
    pub name: String,
    pub market_ids: Vec<u32>,
}

#[derive(Debug, Serialize)]
pub struct ResolveMarketRequest {
    pub payout_nanos: u64,
}

#[derive(Debug, Serialize)]
pub struct SubmitOrderRequest {
    pub account_id: u64,
    pub orders: Vec<OrderSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mm_budget_nanos: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum OrderSpec {
    BuyYes {
        market_id: u32,
        limit_price_nanos: u64,
        quantity: u64,
    },
    BuyNo {
        market_id: u32,
        limit_price_nanos: u64,
        quantity: u64,
    },
}

// --- Responses ---

#[derive(Debug, Deserialize)]
pub struct AccountResponse {
    pub account_id: u64,
    pub balance_nanos: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateMarketResponse {
    pub market_id: u32,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct MarketGroupResponse {
    pub name: String,
    pub market_ids: Vec<u32>,
}

#[derive(Debug, Deserialize)]
pub struct OrderAcceptedResponse {
    pub accepted: bool,
}

#[derive(Debug, Deserialize)]
pub struct BlockResponse {
    pub height: u64,
    #[serde(default)]
    pub clearing_prices_nanos: HashMap<String, Vec<u64>>,
    #[serde(default)]
    pub timestamp_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct HealthResponse {
    pub status: String,
}
