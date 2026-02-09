use serde::Deserialize;
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateAccountRequest {
    /// Initial balance in nanos (1 dollar = 1_000_000_000 nanos).
    #[schema(example = 100_000_000_000u64)]
    pub initial_balance_nanos: u64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct FundAccountRequest {
    /// Amount to add in nanos (1 dollar = 1_000_000_000 nanos).
    #[schema(example = 50_000_000_000u64)]
    pub amount_nanos: u64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RegisterKeyRequest {
    /// Hex-encoded compressed P256 public key (33 bytes).
    #[schema(example = "02a1b2c3...")]
    pub public_key_hex: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateMarketRequest {
    /// Name of the binary market.
    #[schema(example = "Will it rain tomorrow?")]
    pub name: String,
    /// Optional description of the market.
    #[serde(default)]
    pub description: Option<String>,
    /// Optional category (e.g., "sports", "politics", "crypto").
    #[serde(default)]
    pub category: Option<String>,
    /// Optional tags for discovery.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Optional resolution criteria.
    #[serde(default)]
    pub resolution_criteria: Option<String>,
    /// Optional expiry timestamp in ms (0 = no expiry).
    #[serde(default)]
    pub expiry_timestamp_ms: Option<u64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateMarketGroupRequest {
    /// Name for the group of mutually exclusive markets.
    #[schema(example = "2024 Election")]
    pub name: String,
    /// Market IDs in the group.
    pub market_ids: Vec<u32>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ResolveMarketRequest {
    /// Payout per YES share in nanos (0 to 1_000_000_000).
    /// 1_000_000_000 = YES wins ($1), 0 = NO wins, 700_000_000 = $0.70 fractional.
    #[schema(example = 1_000_000_000u64)]
    pub payout_nanos: u64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SubmitOrderRequest {
    /// Account ID submitting the orders.
    pub account_id: u64,
    /// Orders to submit.
    pub orders: Vec<OrderSpec>,
    /// If set, treat these orders as market maker orders with flash liquidity.
    /// The value is the MM's total capital budget in nanos.
    /// MM orders skip per-order balance validation; instead the solver enforces
    /// the portfolio-level budget constraint at clearing time.
    #[serde(default)]
    pub mm_budget_nanos: Option<u64>,
}

/// Tagged enum representing different order types.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(tag = "type")]
pub enum OrderSpec {
    /// Buy YES shares on a single market.
    BuyYes {
        market_id: u32,
        /// Limit price in nanos (0 to 1_000_000_000).
        limit_price_nanos: u64,
        quantity: u64,
    },
    /// Buy NO shares on a single market.
    BuyNo {
        market_id: u32,
        limit_price_nanos: u64,
        quantity: u64,
    },
    /// Sell YES shares on a single market.
    SellYes {
        market_id: u32,
        limit_price_nanos: u64,
        quantity: u64,
    },
    /// Sell NO shares on a single market.
    SellNo {
        market_id: u32,
        limit_price_nanos: u64,
        quantity: u64,
    },
    /// Spread: buy A YES, sell B YES.
    Spread {
        market_a: u32,
        market_b: u32,
        limit_price_nanos: u64,
        quantity: u64,
    },
    /// Bundle YES: all markets must be YES to win.
    BundleYes {
        market_ids: Vec<u32>,
        limit_price_nanos: u64,
        quantity: u64,
    },
    /// Bundle Sell: sell the all-YES bundle.
    BundleSell {
        market_ids: Vec<u32>,
        limit_price_nanos: u64,
        quantity: u64,
    },
    /// Custom payoff vector.
    Custom {
        market_ids: Vec<u32>,
        payoffs: Vec<i8>,
        limit_price_nanos: u64,
        min_fill: u64,
        max_fill: u64,
    },
}

/// Query parameters for market search.
#[derive(Debug, Deserialize, ToSchema)]
pub struct MarketSearchParams {
    /// Text search (searches name + description).
    #[serde(default)]
    pub q: Option<String>,
    /// Comma-separated tags to filter by.
    #[serde(default)]
    pub tags: Option<String>,
    /// Exact category match.
    #[serde(default)]
    pub category: Option<String>,
    /// Status filter ("active" or "resolved").
    #[serde(default)]
    pub status: Option<String>,
    /// Minimum YES price in nanos.
    #[serde(default)]
    pub min_yes_price: Option<u64>,
    /// Maximum YES price in nanos.
    #[serde(default)]
    pub max_yes_price: Option<u64>,
    /// Minimum cumulative volume in nanos.
    #[serde(default)]
    pub min_volume: Option<u64>,
    /// Sort field: "volume", "created_at", "name", "price".
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SubmitSignedOrderRequest {
    /// Hex-encoded compressed P256 public key of the signer.
    pub signer_pubkey_hex: String,
    /// The order to submit.
    pub order: SignedOrderData,
    /// Hex-encoded P256 ECDSA signature.
    pub signature_hex: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SignedOrderData {
    /// Market IDs this order spans.
    pub market_ids: Vec<u32>,
    /// Payoff vector.
    pub payoffs: Vec<i8>,
    /// Limit price in nanos (0 to 1_000_000_000).
    pub limit_price_nanos: u64,
    /// Minimum fill quantity.
    pub min_fill: u64,
    /// Maximum fill quantity.
    pub max_fill: u64,
}
