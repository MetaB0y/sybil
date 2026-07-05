//! API request types (DTOs).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateAccountRequest {
    /// Initial balance in nanos (1 dollar = 1_000_000_000 nanos).
    #[cfg_attr(feature = "openapi", schema(example = 100_000_000_000u64))]
    pub initial_balance_nanos: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FundAccountRequest {
    /// Amount to add in nanos (1 dollar = 1_000_000_000 nanos).
    #[cfg_attr(feature = "openapi", schema(example = 50_000_000_000u64))]
    pub amount_nanos: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SubmitL1DepositRequest {
    /// Sequential L1 vault deposit id.
    pub deposit_id: u64,
    /// Sybil account receiving the credit.
    pub account_id: u64,
    /// Source chain id.
    pub chain_id: u64,
    /// Hex-encoded vault contract address (20 bytes).
    pub vault_address_hex: String,
    /// Hex-encoded token contract address (20 bytes).
    pub token_address_hex: String,
    /// Hex-encoded L1 sender address (20 bytes).
    pub sender_hex: String,
    /// Optional Sybil bridge account key. If omitted, the API derives it for the account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sybil_account_key_hex: Option<String>,
    /// Token base units accepted by the vault, e.g. USDC's 6-decimal units.
    pub amount_token_units: u64,
    /// Hex-encoded post-deposit L1 deposit tree root (32 bytes).
    pub deposit_root_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateBridgeWithdrawalRequest {
    /// Sybil account whose available balance is burned.
    pub account_id: u64,
    /// Destination chain id.
    pub chain_id: u64,
    /// Hex-encoded vault contract address (20 bytes).
    pub vault_address_hex: String,
    /// Hex-encoded L1 recipient address (20 bytes).
    pub recipient_hex: String,
    /// Hex-encoded token contract address (20 bytes).
    pub token_address_hex: String,
    /// Token base units released by the vault.
    pub amount_token_units: u64,
    /// Last Sybil block height at which this withdrawal leaf is valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiry_height: Option<u64>,
    /// Per-account replay nonce. Required for signed bridge withdrawals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateSignedBridgeWithdrawalRequest {
    /// Withdrawal payload covered by the P256 signature.
    pub withdrawal: CreateBridgeWithdrawalRequest,
    /// Hex-encoded compressed P256 public key of the signer.
    pub signer_pubkey_hex: String,
    /// Hex-encoded P256 ECDSA signature over the canonical withdrawal payload.
    pub signature_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RegisterKeyRequest {
    /// Hex-encoded compressed P256 public key (33 bytes).
    #[cfg_attr(feature = "openapi", schema(example = "02a1b2c3..."))]
    pub public_key_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateMarketRequest {
    /// Name of the binary market.
    #[cfg_attr(feature = "openapi", schema(example = "Will it rain tomorrow?"))]
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
    /// Resolution template id to use for this market (e.g. "admin_immediate",
    /// "polymarket_mirror"). `None` -> `admin_immediate`.
    #[serde(default)]
    pub resolution_template: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateMarketGroupRequest {
    /// Name for the group of mutually exclusive markets.
    #[cfg_attr(feature = "openapi", schema(example = "2024 Election"))]
    pub name: String,
    /// Market IDs in the group.
    pub market_ids: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ExtendMarketGroupRequest {
    /// Market ID to add to the existing group.
    pub market_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ResolveMarketRequest {
    /// Payout per YES share in nanos (0 to 1_000_000_000).
    /// 1_000_000_000 = YES wins ($1), 0 = NO wins, 700_000_000 = $0.70 fractional.
    #[cfg_attr(feature = "openapi", schema(example = 1_000_000_000u64))]
    pub payout_nanos: u64,
    /// Optional signed attestation. When provided, the market's resolution
    /// template drives verification; dev_mode is not required. When omitted,
    /// the server falls back to the legacy unsigned admin path, which
    /// requires dev_mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attestation: Option<SignedAttestationDto>,
}

/// Wire form of a signed resolution attestation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SignedAttestationDto {
    /// Hex-encoded compressed SEC1 P256 public key (33 bytes).
    pub pubkey_hex: String,
    /// Hex-encoded P256 ECDSA signature over the canonical attestation bytes.
    pub signature_hex: String,
    /// Nonce the signer chose (typically `timestamp_ms`).
    pub nonce: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RegisterFeedRequest {
    /// Hex-encoded compressed P256 public key (33 bytes).
    pub pubkey_hex: String,
    /// Human-readable name (e.g. "admin", "polymarket_mirror").
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SubmitOrderRequest {
    /// Account ID submitting the orders.
    pub account_id: u64,
    /// Orders to submit.
    pub orders: Vec<OrderSpec>,
    /// Time-in-force policy applied to all orders in this submission.
    #[serde(default)]
    pub time_in_force: TimeInForce,
    /// Last eligible block height for explicit-expiry orders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_block: Option<u64>,
    /// If set, treat these orders as market maker orders with flash liquidity.
    /// The value is the MM's total capital budget in nanos.
    /// MM orders skip per-order balance validation; instead the solver enforces
    /// the portfolio-level budget constraint at clearing time.
    #[serde(default)]
    pub mm_budget_nanos: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "UPPERCASE")]
pub enum TimeInForce {
    #[default]
    Gtc,
    Ioc,
    Gtd,
}

/// Tagged enum representing public order types.
///
/// Public submission is intentionally limited to single-market binary orders.
/// Compound payoff-vector orders remain available inside `matching-engine` for
/// research and tests, but are not accepted at the HTTP API edge.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type")]
pub enum OrderSpec {
    /// Buy YES share-units on a single market (`1000` units = 1 share).
    BuyYes {
        market_id: u32,
        /// Limit price in nanos (0 to 1_000_000_000).
        limit_price_nanos: u64,
        /// Quantity in fixed-point share-units.
        quantity: u64,
    },
    /// Buy NO share-units on a single market (`1000` units = 1 share).
    BuyNo {
        market_id: u32,
        limit_price_nanos: u64,
        /// Quantity in fixed-point share-units.
        quantity: u64,
    },
    /// Sell YES share-units on a single market (`1000` units = 1 share).
    SellYes {
        market_id: u32,
        limit_price_nanos: u64,
        /// Quantity in fixed-point share-units.
        quantity: u64,
    },
    /// Sell NO share-units on a single market (`1000` units = 1 share).
    SellNo {
        market_id: u32,
        limit_price_nanos: u64,
        /// Quantity in fixed-point share-units.
        quantity: u64,
    },
}

/// Query parameters for market search.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SubmitSignedOrderRequest {
    /// Hex-encoded compressed P256 public key of the signer.
    pub signer_pubkey_hex: String,
    /// The order to submit.
    pub order: SignedOrderData,
    /// API time-in-force policy. Signed IOC/GTD orders commit to `expires_at_block`.
    #[serde(default)]
    pub time_in_force: TimeInForce,
    /// Last eligible block height, covered by the P256 signature. Required for signed IOC/GTD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_block: Option<u64>,
    /// Per-account replay nonce covered by the P256 signature.
    pub nonce: u64,
    /// Hex-encoded P256 ECDSA signature.
    pub signature_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CancelSignedOrderRequest {
    /// Account ID claiming ownership of the order being cancelled.
    pub account_id: u64,
    /// The pending order to cancel.
    pub order_id: u64,
    /// Hex-encoded compressed P256 public key of the signer.
    pub signer_pubkey_hex: String,
    /// Per-account replay nonce covered by the P256 signature.
    pub nonce: u64,
    /// Hex-encoded P256 ECDSA signature over the canonical cancel payload.
    pub signature_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SignedOrderData {
    /// Market IDs this order spans.
    pub market_ids: Vec<u32>,
    /// Payoff vector.
    pub payoffs: Vec<i8>,
    /// Limit price in nanos (0 to 1_000_000_000).
    pub limit_price_nanos: u64,
    /// Maximum fill quantity in fixed-point share-units.
    pub max_fill: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SetReferencePricesRequest {
    /// Map of market_id -> reference price in nanos.
    pub prices: std::collections::HashMap<u32, u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SetMarketMetadataRequest {
    /// External URL (e.g., Polymarket link).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_url: Option<String>,
    /// Polymarket parent event id — used by the frontend to group sibling
    /// markets (e.g., "Fed Decision in June" sub-questions). Distinct from the
    /// matching engine's NegRisk `MarketGroup`, which it does not affect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    /// Polymarket parent event title — rendered as the MultiCard header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_title: Option<String>,
    /// Event-level image URL (primary).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_image_url: Option<String>,
    /// Event-level icon URL (secondary; frontend uses as `onError` fallback).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_icon_url: Option<String>,
    /// Event-level expected end date (epoch ms). Display only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_end_date_ms: Option<u64>,
    /// Per-market image URL (primary).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_image_url: Option<String>,
    /// Per-market icon URL (secondary; frontend uses as `onError` fallback).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_icon_url: Option<String>,
    /// Per-market expected end date (epoch ms). Display only; matching engine
    /// does not enforce trading cutoffs at this time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_end_date_ms: Option<u64>,
    /// Single display category. **Legacy** — populated only for sybil-native
    /// markets at create time. Mirrored markets now use `categories` (plural)
    /// and let the frontend pick one for display via its own priority order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// All category buckets the parent event matched in the mirror's tag-to-
    /// bucket lookup (e.g. `["Sports", "Politics"]` for an NBA + Trump
    /// event). One per matched row; the frontend picks which to render
    /// using its own priority list, so reordering display priority is
    /// frontend-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<String>>,
    /// Polymarket on-chain condition id — the FE join key into the event JSON
    /// snapshot (`/v1/events/{id}/raw` `markets[].conditionId`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polymarket_condition_id: Option<String>,
    /// Parent event start date (epoch ms). Display/sort only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_start_date_ms: Option<u64>,
    /// Per-market start date (epoch ms). Display/sort only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_start_date_ms: Option<u64>,
    /// Polymarket short outcome label (`groupItemTitle`, e.g. "May 15"). The
    /// frontend renders this as the per-outcome name on multi-cards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_item_title: Option<String>,
    /// Whether Polymarket has closed this market. The frontend hides closed
    /// markets from the listing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn custom_order_spec_is_not_part_of_public_submit_api() {
        let payload = json!({
            "account_id": 1,
            "orders": [{
                "type": "Custom",
                "market_ids": [0],
                "payoffs": [2, 0],
                "limit_price_nanos": 500_000_000u64,
                "max_fill": 1000u64
            }]
        });

        assert!(serde_json::from_value::<SubmitOrderRequest>(payload).is_err());
    }

    #[test]
    fn bundle_order_specs_are_not_part_of_public_submit_api() {
        for order_type in ["Spread", "BundleYes", "BundleSell"] {
            let order = match order_type {
                "Spread" => json!({
                    "type": order_type,
                    "market_a": 0,
                    "market_b": 1,
                    "limit_price_nanos": 500_000_000u64,
                    "quantity": 1000u64
                }),
                _ => json!({
                    "type": order_type,
                    "market_ids": [0, 1],
                    "limit_price_nanos": 500_000_000u64,
                    "quantity": 1000u64
                }),
            };
            let payload = json!({
                "account_id": 1,
                "orders": [order]
            });

            assert!(
                serde_json::from_value::<SubmitOrderRequest>(payload).is_err(),
                "{order_type} must not deserialize as a public OrderSpec"
            );
        }
    }
}
