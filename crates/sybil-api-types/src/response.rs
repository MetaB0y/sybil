//! API response types (DTOs).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::request::BridgeWithdrawalL1Status;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AccountResponse {
    pub account_id: u64,
    /// Total (gross) account balance; see `available_balance_nanos` for spendable
    /// funds. Integer nanodollars; 1_000_000_000 = $1.
    pub balance_nanos: i64,
    /// Spendable account balance after live-order reservations. Integer
    /// nanodollars; 1_000_000_000 = $1.
    pub available_balance_nanos: i64,
    /// Balance reserved by live resting orders. Integer nanodollars;
    /// 1_000_000_000 = $1.
    pub reserved_balance_nanos: i64,
    /// Current validity key-set digest used to state-bind key operations.
    pub keys_digest_hex: String,
    /// Current event-chain digest used to make every key operation one-shot.
    pub events_digest_hex: String,
    #[serde(default)]
    pub positions: Vec<PositionResponse>,
    /// Optional public display name. A non-empty value opts this account into
    /// publication of its leaderboard financial row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Optional deterministic identicon seed (SYB-60).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_seed: Option<String>,
}

/// A registered signing key with SYB-60 management metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AccountKeyResponse {
    /// Hex-encoded compressed P256 public key (33 bytes).
    pub public_key_hex: String,
    /// Authentication scheme: `raw_p256` or `webauthn`.
    pub auth_scheme: String,
    /// Scope tag: `primary`, `agent`, or `custom`.
    pub scope: String,
    /// Optional human label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Registration time in Unix milliseconds (0 for keys predating SYB-60).
    pub created_at_ms: u64,
}

/// Public, non-secret state needed to construct a one-shot key operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct KeyOpStateResponse {
    pub account_id: u64,
    pub keys_digest_hex: String,
    pub events_digest_hex: String,
}

/// A read-scoped bearer API key's metadata (never the token or its hash).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ApiKeyResponse {
    /// Stable id used to reference this key for revocation.
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub created_at_ms: u64,
    /// Revocation time in Unix milliseconds, if revoked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at_ms: Option<u64>,
}

/// Response to creating a read API key (SYB-60). The plaintext `token` is shown
/// exactly once here and is not recoverable afterwards.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateApiKeyResponse {
    pub id: u64,
    /// The bearer token. Store it now — the server keeps only its blake3 hash.
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub created_at_ms: u64,
    /// The active signing key that authorized creation. This is especially
    /// useful during discoverable WebAuthn login, where the browser assertion
    /// does not itself expose the credential public key.
    pub signer_pubkey_hex: String,
}

/// Private account summary served behind owner-or-service read auth (SYB-60/237).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PrivateAccountSummaryResponse {
    pub account_id: u64,
    /// Total (gross) account balance; see `available_balance_nanos` for spendable
    /// funds. Integer nanodollars; 1_000_000_000 = $1.
    pub balance_nanos: i64,
    /// Spendable account balance after live-order reservations. Integer
    /// nanodollars; 1_000_000_000 = $1.
    pub available_balance_nanos: i64,
    /// Balance reserved by live resting orders. Integer nanodollars;
    /// 1_000_000_000 = $1.
    pub reserved_balance_nanos: i64,
    /// Total deposited to date. Integer nanodollars; 1_000_000_000 = $1.
    pub total_deposited_nanos: i64,
    /// Current mark-to-market portfolio value. Integer nanodollars; 1_000_000_000 = $1.
    pub portfolio_value_nanos: i64,
    /// Portfolio value minus deposits. Integer nanodollars; 1_000_000_000 = $1.
    pub pnl_nanos: i64,
    #[serde(default)]
    pub positions: Vec<PositionResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PositionResponse {
    pub market_id: u32,
    pub outcome: String,
    /// Signed position quantity. Integer share-units; 1000 units = 1 share.
    pub quantity: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MarketResponse {
    pub market_id: u32,
    pub name: String,
    /// Current YES clearing price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub yes_price_nanos: Option<u64>,
    /// Current NO clearing price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub no_price_nanos: Option<u64>,
    pub status: String,
    /// Resolution payout per YES share. Integer nanodollars; 1_000_000_000 = $1.
    /// Payouts are per-share probabilities in [0, 1e9].
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
    /// All-time traded notional. Integer nanodollars; 1_000_000_000 = $1.
    #[serde(default)]
    pub volume_nanos: u64,
    /// Reference price from external system (e.g., Polymarket), display only.
    /// Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
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
    /// Rolling 24h trading volume. Integer nanodollars; 1_000_000_000 = $1.
    /// Off-block;
    /// "since last restart" until prod persistence is enabled.
    #[serde(default)]
    pub volume_24h_nanos: u64,
    /// Clearing YES price ~24h ago, derived from the per-market
    /// hourly snapshot. `None` for markets younger than 24h or wiped on
    /// restart. FE computes the 24h delta as `current - snapshot`.
    /// Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yes_price_24h_ago_nanos: Option<u64>,
    /// Clearing NO price ~24h ago. See `yes_price_24h_ago_nanos`.
    /// Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_price_24h_ago_nanos: Option<u64>,
    /// Rolling last-10-batch band depth average. Integer nanodollars;
    /// 1_000_000_000 = $1. Zero for markets without a clearing price yet.
    /// Pair with `liquidity_band_nanos` for labelling.
    #[serde(default)]
    pub liquidity_avg10_nanos: u64,
    /// Width of the band the liquidity score uses (the ± in "$X ±$0.05").
    /// Integer nanodollars; 1_000_000_000 = $1.
    /// Always the live config value — `0` when no liquidity has been
    /// recorded yet.
    #[serde(default)]
    pub liquidity_band_nanos: u64,
    /// All-time non-MM admissions counted against this market. Multi-market
    /// orders credit every active market; sum-of-per-market over-counts vs.
    /// the platform total — that's the documented attribution rule.
    #[serde(default)]
    pub orders_placed_total: u64,
    /// All-time admissions that received at least one fill (B5's
    /// `has_been_matched` true at removal time). Cancels are NOT counted.
    #[serde(default)]
    pub orders_matched_total: u64,
    /// All-time admissions that exited the book without any fill. Cancels
    /// are tracked separately and do not count here.
    #[serde(default)]
    pub orders_unmatched_total: u64,
    /// Polymarket on-chain condition id — FE join key into
    /// `GET /v1/events/{event_id}/raw` `markets[].conditionId`. Off-block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polymarket_condition_id: Option<String>,
    /// Parent event start date (epoch ms) from Polymarket. Display/sort only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_start_date_ms: Option<u64>,
    /// Per-market start date (epoch ms) from Polymarket. Display/sort only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_start_date_ms: Option<u64>,
    /// Polymarket short outcome label (`groupItemTitle`, e.g. "May 15"). Off-
    /// block; the frontend uses it as the per-outcome name so it needn't fetch
    /// the raw event JSON just for labels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_item_title: Option<String>,
    /// Whether Polymarket has closed this market. Off-block; the frontend
    /// filters closed markets out of the listing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
}

/// Minimal market data for high-throughput dashboards (drops strings & metadata).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MarketSummaryResponse {
    pub market_id: u32,
    pub name: String,
    /// Current YES clearing price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub yes_price_nanos: Option<u64>,
    /// Current NO clearing price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub no_price_nanos: Option<u64>,
    /// Reference price from external system (e.g., Polymarket), display only.
    /// Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_price_nanos: Option<u64>,
    /// All-time traded notional. Integer nanodollars; 1_000_000_000 = $1.
    pub volume_nanos: u64,
    pub status: String,
    /// All-time unique trader count (mirrors `MarketResponse.trader_count`).
    #[serde(default)]
    pub trader_count: u32,
    /// Rolling 24h trading volume. Integer nanodollars; 1_000_000_000 = $1.
    /// Mirrors
    /// `MarketResponse.volume_24h_nanos`).
    #[serde(default)]
    pub volume_24h_nanos: u64,
    /// Clearing YES price ~24h ago. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yes_price_24h_ago_nanos: Option<u64>,
    /// Clearing NO price ~24h ago. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_price_24h_ago_nanos: Option<u64>,
    /// Liquidity depth score. Integer nanodollars; 1_000_000_000 = $1.
    /// Mirrors `MarketResponse`.
    #[serde(default)]
    pub liquidity_avg10_nanos: u64,
    /// Liquidity price-band width. Integer nanodollars; 1_000_000_000 = $1.
    /// Mirrors `MarketResponse`.
    #[serde(default)]
    pub liquidity_band_nanos: u64,
    /// All-time placed/matched/unmatched (mirrors `MarketResponse`).
    #[serde(default)]
    pub orders_placed_total: u64,
    #[serde(default)]
    pub orders_matched_total: u64,
    #[serde(default)]
    pub orders_unmatched_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MarketGroupResponse {
    pub group_id: u64,
    pub name: String,
    pub market_ids: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MarketPricesResponse {
    /// Market price map. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub prices: HashMap<String, MarketPriceResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MarketPriceResponse {
    /// YES clearing price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub yes_price_nanos: u64,
    /// NO clearing price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub no_price_nanos: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OrderAcceptedResponse {
    pub accepted: bool,
    /// Sequencer-assigned IDs for the admitted orders, in request order.
    pub order_ids: Vec<u64>,
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
    /// Fill quantity. Integer share-units; 1000 units = 1 share.
    pub fill_qty: u64,
    /// Fill price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
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
        /// Initial account balance. Integer nanodollars; 1_000_000_000 = $1.
        initial_balance_nanos: i64,
    },
    Deposit {
        account_id: u64,
        /// Account credit amount. Integer nanodollars; 1_000_000_000 = $1.
        amount_nanos: i64,
    },
    L1Deposit {
        account_id: u64,
        /// Account credit amount. Integer nanodollars; 1_000_000_000 = $1.
        amount_nanos: i64,
        deposit_id: u64,
        deposit_root_hex: String,
        sybil_account_key_hex: String,
    },
    WithdrawalCreated {
        account_id: u64,
        /// Account debit amount. Integer nanodollars; 1_000_000_000 = $1.
        amount_nanos: i64,
        withdrawal_id: u64,
        nullifier_hex: String,
    },
    WithdrawalRefunded {
        account_id: u64,
        /// Refunded account credit. Integer nanodollars; 1_000_000_000 = $1.
        amount_nanos: i64,
        withdrawal_id: u64,
        reason: String,
    },
    WithdrawalFinalized {
        account_id: u64,
        /// Finalized withdrawal amount. Integer nanodollars; 1_000_000_000 = $1.
        amount_nanos: i64,
        withdrawal_id: u64,
    },
    L1BlockObserved {
        height: u64,
    },
    MarketResolved {
        market_id: u32,
        /// Resolution payout per YES share. Integer nanodollars;
        /// 1_000_000_000 = $1. Payouts are per-share probabilities in [0, 1e9].
        payout_nanos: u64,
        affected_accounts: Vec<u64>,
    },
    /// On-chain cancellation event (D1). `side` is the categorical
    /// `OrderDirection` ("BuyYes"/"SellYes"/"BuyNo"/"SellNo") and
    /// `remaining_quantity` is the unfilled portion of `max_fill` at
    /// cancel time. Integer share-units; 1000 units = 1 share.
    /// Forward-additive: old clients ignore unknown
    /// variants via serde's `#[serde(tag = "type")]` shape.
    OrderCancelled {
        account_id: u64,
        order_id: u64,
        market_ids: Vec<u32>,
        side: String,
        /// Cancelled order's unfilled quantity. Integer share-units;
        /// 1000 units = 1 share.
        remaining_quantity: u64,
    },
    MarketGroupExtended {
        group_id: u64,
        market_id: u32,
    },
    KeyRegistered {
        account_id: u64,
        public_key_hex: String,
        auth_scheme: u8,
        capability_mask: u32,
    },
    KeyRevoked {
        account_id: u64,
        public_key_hex: String,
        auth_scheme: u8,
        capability_mask: u32,
    },
    DepositQuarantined {
        /// Amount parked in the system ledger. Integer nanodollars; 1_000_000_000 = $1.
        amount_nanos: i64,
        deposit_id: u64,
        deposit_root_hex: String,
        sybil_account_key_hex: String,
    },
    QuarantineClaimed {
        account_id: u64,
        /// Amount moved into the account. Integer nanodollars; 1_000_000_000 = $1.
        amount_nanos: i64,
        sybil_account_key_hex: String,
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
    /// Per-market volume contribution from this block's fills. Integer nanodollars;
    /// 1_000_000_000 = $1. Multi-market fills credit each active market with their
    /// full notional; the platform `total_volume_nanos` scalar counts each fill once.
    #[serde(default)]
    pub volume_nanos: u64,
    /// Non-MM admissions counted against this market in this block.
    /// Multi-market orders credit each active market.
    #[serde(default)]
    pub placed: u32,
    /// Resting orders touching this market that exited the book this
    /// block AFTER at least one fill (B5's `has_been_matched`).
    #[serde(default)]
    pub matched: u32,
    /// Resting orders touching this market that exited the book this
    /// block WITHOUT any fill. Cancels are excluded.
    #[serde(default)]
    pub unmatched: u32,
    /// Per-market welfare contribution from this block's fills (B7). Integer nanodollars;
    /// 1_000_000_000 = $1. Multi-market fills credit each active market with their
    /// full welfare; the platform `total_welfare_nanos` counts each fill once.
    /// Signed — solver rounding can yield small negatives.
    #[serde(default)]
    pub welfare_nanos: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DerivedViewSidecarResponse {
    /// Always `derived_unproven`: this sidecar is sequencer-derived read-model
    /// data and is not part of the witness, state root, events root, witness
    /// root, DA commitment, or ZK guest input.
    pub provenance: String,
    /// Resting orders removed during block production. Derived/unproven view
    /// rows used for analytics and lifecycle displays.
    #[serde(default)]
    pub removed_orders: Vec<RemovedOrderViewResponse>,
    /// Admission timing rows. `is_new=false` means the order was carried from
    /// a prior block's resting book; `is_new=true` means a distinct admission
    /// first became visible to this block's view.
    #[serde(default)]
    pub admits: Vec<AdmitTimingViewResponse>,
    /// Rejection rows that were intentionally mirrored into account history.
    /// Canonical rejections remain in `BlockResponse.rejections`.
    #[serde(default)]
    pub rejection_history: Vec<RejectedOrderViewResponse>,
}

impl Default for DerivedViewSidecarResponse {
    fn default() -> Self {
        Self {
            provenance: "derived_unproven".to_string(),
            removed_orders: Vec::new(),
            admits: Vec::new(),
            rejection_history: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RemovedOrderViewResponse {
    pub order_id: u64,
    pub account_id: u64,
    pub phase: String,
    pub exit_reason: String,
    pub has_been_matched: bool,
    /// Released reserved cash. Integer nanodollars; 1_000_000_000 = $1.
    pub reserved_balance_released: i64,
    #[serde(default)]
    pub reserved_positions_released: Vec<ReservedPositionReleaseResponse>,
    #[serde(default)]
    pub active_markets: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejection_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReservedPositionReleaseResponse {
    pub market_id: u32,
    pub outcome: u8,
    /// Released reserved position quantity. Integer share-units; 1000 units = 1 share.
    pub quantity: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AdmitTimingViewResponse {
    pub order_id: u64,
    pub account_id: u64,
    pub admit_height: u64,
    /// Admission timestamp in Unix epoch milliseconds.
    pub admit_timestamp_ms: u64,
    pub is_new: bool,
    pub is_mm: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RejectedOrderViewResponse {
    pub order_id: u64,
    pub account_id: u64,
    pub reason: String,
}

/// Privacy-preserving projection of a committed block for public REST and
/// streaming clients. Account-attributed fills, rejections, system events,
/// bridge leaves, and order-lifecycle rows deliberately do not exist on this
/// type; canonical full blocks remain available only to authenticated service
/// consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PublicBlockResponse {
    pub height: u64,
    pub parent_hash: String,
    /// Post-block state root. Hex-encoded 32-byte qMDB root.
    pub state_root: String,
    pub events_root: String,
    pub order_count: u32,
    pub fill_count: u32,
    /// Number of rejected orders without identities, order ids, or reasons.
    pub rejection_count: u32,
    pub timestamp_ms: u64,
    /// Clearing price vectors by market/group. Integer nanodollars;
    /// 1_000_000_000 = $1. Prices are per-share probabilities in [0, 1e9].
    #[serde(default)]
    pub clearing_prices_nanos: HashMap<String, Vec<u64>>,
    /// Public bridge commitment/count only. Individual deposits and
    /// withdrawals remain private.
    pub bridge: PublicBridgeBlockResponse,
    /// Market ids resolved in this block. The account-bearing affected-account
    /// list from the canonical event is intentionally omitted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resolved_market_ids: Vec<u32>,
    /// Total solver welfare in the block. Integer nanodollars;
    /// 1_000_000_000 = $1. Signed: solver rounding can yield small negatives.
    pub total_welfare_nanos: i64,
    /// Total traded notional in the block. Integer nanodollars;
    /// 1_000_000_000 = $1.
    pub total_volume_nanos: u64,
    pub orders_filled: usize,
    /// Unique non-MM accounts admitted into this block. This is an aggregate,
    /// never an account identifier list.
    #[serde(default)]
    pub unique_placers: u32,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub by_market: HashMap<String, BlockMarketStats>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PublicBridgeBlockResponse {
    pub deposit_count: u64,
    pub deposit_root_hex: String,
}

/// Authenticated service projection of a canonical block. This contains
/// account-attributed private data and must never be returned by a public
/// route. Public clients use [`PublicBlockResponse`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BlockResponse {
    pub height: u64,
    pub parent_hash: String,
    /// Post-block state root. Hex-encoded 32-byte qMDB root.
    pub state_root: String,
    pub events_root: String,
    pub order_count: u32,
    pub fill_count: u32,
    pub timestamp_ms: u64,
    #[serde(default)]
    pub system_events: Vec<SystemEventResponse>,
    #[serde(default)]
    pub fills: Vec<FillResponse>,
    /// Clearing price vectors by market/group. Integer nanodollars;
    /// 1_000_000_000 = $1. Prices are per-share probabilities in [0, 1e9].
    #[serde(default)]
    pub clearing_prices_nanos: HashMap<String, Vec<u64>>,
    #[serde(default)]
    pub rejections: Vec<RejectionResponse>,
    #[serde(default)]
    pub bridge: BridgeBlockResponse,
    /// Total solver welfare in the block. Integer nanodollars;
    /// 1_000_000_000 = $1. Signed: solver rounding can yield small negatives.
    pub total_welfare_nanos: i64,
    /// Total traded notional in the block. Integer nanodollars;
    /// 1_000_000_000 = $1.
    pub total_volume_nanos: u64,
    pub orders_filled: usize,
    /// Unique placers (non-MM accounts) admitted into this block. Platform
    /// scalar — `by_market[m].placers` is the per-market split.
    #[serde(default)]
    pub unique_placers: u32,
    /// Nested per-market block scalars. Each
    /// `BlockMarketStats` carries the per-market splits for this block. Old
    /// clients ignore it; new clients consume what they recognise.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub by_market: HashMap<String, BlockMarketStats>,
    /// Unproven derived-view lifecycle sidecar. The field is exposed on the
    /// same block surface as canonical data, but its `provenance` marks it as
    /// derived read-model data rather than consensus-proven data.
    #[serde(default)]
    pub derived_view_sidecar: DerivedViewSidecarResponse,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<u64>,
    /// Token base units accepted by the vault, e.g. USDC's 6-decimal units.
    pub amount_token_units: u64,
    pub deposit_root_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BridgeStatusResponse {
    pub deposit_cursor: u64,
    pub deposit_root_hex: String,
    pub observed_l1_height: u64,
    pub next_withdrawal_id: u64,
    pub withdrawal_count: usize,
    #[serde(default)]
    pub queued_withdrawal_count: usize,
    #[serde(default)]
    pub finalized_withdrawal_count: usize,
    #[serde(default)]
    pub cancelled_withdrawal_count: usize,
    #[serde(default)]
    pub refunded_withdrawal_count: usize,
    #[serde(default)]
    pub quarantine_ledger_size: usize,
    #[serde(default)]
    /// Sum of parked value. Integer nanodollars; 1_000_000_000 = $1.
    pub total_quarantined_nanos: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ObserveL1HeightResponse {
    pub observed_l1_height: u64,
    pub refunded_withdrawal_ids: Vec<u64>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<u64>,
    /// Account balance after the deposit. Integer nanodollars; 1_000_000_000 = $1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balance_nanos: Option<i64>,
    /// `credited` or `quarantined`.
    pub disposition: String,
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
    /// Token base units released by the vault.
    pub amount_token_units: u64,
    /// Off-chain balance amount burned for the withdrawal. Integer nanodollars;
    /// 1_000_000_000 = $1.
    pub amount_nanos: u64,
    pub expiry_height: u64,
    pub nullifier_hex: String,
    pub withdrawal_leaf_hex: String,
    pub withdrawal_leaf_digest_hex: String,
    pub created_at_height: u64,
    #[serde(default)]
    pub l1_status: BridgeWithdrawalL1Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l1_requested_at_unix: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l1_executable_at_unix: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l1_finalized_at_unix: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l1_cancelled_at_unix: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l1_tx_hash_hex: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BridgeWithdrawalL1EventResponse {
    /// False when the terminal withdrawal was already pruned; the observation
    /// is still accepted as an idempotent no-op.
    pub active_withdrawal_found: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub withdrawal: Option<BridgeWithdrawalResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct HealthResponse {
    pub status: String,
    #[serde(default)]
    pub height: Option<u64>,
    /// Hash of the height-1 block header. Hex-encoded 32-byte chain instance id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genesis_hash: Option<String>,
}

/// Development-only JSON projection of an enclave attestation.
///
/// These fields correspond to values carried by an AWS Nitro attestation
/// document, but this DTO is not itself the canonical CBOR/COSE document. A
/// response with `is_stub = true` has no cryptographic trust value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AttestationResponse {
    /// PCR index to lowercase hex-encoded measurement bytes. A real Nitro
    /// document uses SHA-384 PCR values; the development stub returns no PCRs.
    pub pcr_values: HashMap<u8, String>,
    /// Lowercase hex encoding of Nitro's optional DER-encoded `public_key`
    /// field. Empty in the development stub.
    pub enclave_pubkey: String,
    /// Lowercase hex encoding of protocol data carried in Nitro's optional
    /// `user_data` field. Empty in the development stub.
    pub report_data: String,
    /// Base64url encoding of the COSE_Sign1 signature bytes. Empty in the
    /// development stub; this field alone is insufficient to verify Nitro PKI.
    pub signature: String,
    /// Always true for the currently implemented development-only response.
    pub is_stub: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StateRootResponse {
    /// Current state root. Hex-encoded 32-byte qMDB root.
    pub state_root: String,
}

/// Typed DA manifest for retained witness payloads. SYB-120 will add encrypted
/// DA fields such as ciphertext hashes and key-custody metadata here, so this
/// must stay a structured DTO rather than ad-hoc JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DaManifestResponse {
    pub version: u8,
    pub payload_kind: String,
    pub payload_encoding: String,
    pub provider_refs_encoding: String,
    pub height: u64,
    /// Block hash bound into the state-transition public input. Hex-encoded 32-byte digest.
    pub block_hash: String,
    /// State root bound by the DA commitment. Hex-encoded 32-byte qMDB root.
    pub state_root: String,
    /// Witness root = BLAKE3("sybil/witness" || payload bytes). Hex-encoded 32-byte digest.
    pub witness_root: String,
    /// Payload root = BLAKE3("sybil/da/witness-payload/v1" || len || bytes).
    /// Hex-encoded 32-byte digest.
    pub payload_root: String,
    pub payload_len: u64,
    /// Hash of the canonical provider-reference byte list. Hex-encoded 32-byte digest.
    pub provider_refs_hash: String,
    #[serde(default)]
    pub provider_refs: Vec<DaProviderRefResponse>,
    /// DA commitment bound into the ZK public inputs and L1 RootRecord.
    /// Hex-encoded 32-byte digest.
    pub da_commitment: String,
    /// State-transition public input hash. Hex-encoded 32-byte digest.
    pub public_input_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DaProviderRefResponse {
    pub kind: String,
    pub encoding: String,
    /// Hex-encoded canonical provider-reference bytes, 0x-prefixed.
    pub bytes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// Payload root repeated when the provider ref is content-addressed.
    /// Hex-encoded 32-byte digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_len: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StateProofResponse {
    pub block_height: u64,
    /// State root this proof is anchored to. Hex-encoded 32-byte qMDB root.
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
    pub inactive_peaks: u64,
    pub digests_hex: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial_chunk_digest_hex: Option<String>,
    pub ops_root_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ResolveMarketResponse {
    pub market_id: u32,
    /// Resolution payout per YES share. Integer nanodollars;
    /// 1_000_000_000 = $1. Payouts are per-share probabilities in [0, 1e9].
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
    /// Resolution payout per YES share. Integer nanodollars;
    /// 1_000_000_000 = $1. Payouts are per-share probabilities in [0, 1e9].
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
    /// Total (gross) account balance; see `available_balance_nanos` for spendable
    /// funds. Integer nanodollars; 1_000_000_000 = $1.
    pub balance_nanos: i64,
    /// Spendable account balance after live-order reservations. Integer
    /// nanodollars; 1_000_000_000 = $1.
    pub available_balance_nanos: i64,
    /// Balance reserved by live resting orders. Integer nanodollars;
    /// 1_000_000_000 = $1.
    pub reserved_balance_nanos: i64,
    /// Total account deposits. Integer nanodollars; 1_000_000_000 = $1.
    pub total_deposited_nanos: i64,
    pub positions: Vec<PositionValueResponse>,
    /// Mark-to-market value of all positions. Integer nanodollars;
    /// 1_000_000_000 = $1.
    pub total_position_value_nanos: i64,
    /// Total portfolio value. Integer nanodollars; 1_000_000_000 = $1.
    pub portfolio_value_nanos: i64,
    /// Total profit and loss. Integer nanodollars; 1_000_000_000 = $1.
    pub pnl_nanos: i64,
    /// First-deposit timestamp in ms since epoch (B8). `0` for accounts
    /// with no recorded deposit history (FE renders as "—"). Same
    /// "since last restart" caveat as the other off-block aggregates
    /// until persistence runs in prod.
    #[serde(default)]
    pub first_deposit_ms: u64,
    /// All-time fill count (B8). The bounded fill window in
    /// `account_fills` may cap older trades; this counter never does,
    /// so FE shows the real number instead of "200+".
    #[serde(default)]
    pub total_fill_count: u64,
    /// Accumulated realized PnL across all closed positions (C1). Integer nanodollars;
    /// 1_000_000_000 = $1. Signed.
    /// `pnl_nanos = realized + unrealized` once both fields populate, but
    /// `pnl_nanos` is kept for backward compatibility with pre-C1 clients.
    #[serde(default)]
    pub realized_pnl_nanos: i64,
    /// Mark-to-market PnL on currently open positions (C1). Integer nanodollars;
    /// 1_000_000_000 = $1. Computed as
    /// `sum((current_price - avg_entry) * quantity / SHARE_SCALE)` across positions.
    #[serde(default)]
    pub unrealized_pnl_nanos: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PositionValueResponse {
    pub market_id: u32,
    pub outcome: String,
    /// Signed position quantity. Integer share-units; 1000 units = 1 share.
    pub quantity: i64,
    /// Current mark price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub current_price_nanos: u64,
    /// Mark-to-market position value. Integer nanodollars; 1_000_000_000 = $1.
    pub value_nanos: i64,
    /// Weighted-average entry price for this side of the market (C1). `0`
    /// for positions opened before C1 landed (`#[serde(default)]` forward
    /// compat). Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    #[serde(default)]
    pub avg_entry_price_nanos: u64,
}

// --- Price History ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PriceHistoryResponse {
    pub market_id: u32,
    pub points: Vec<PricePointResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_before_height: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_min_height: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PricePointResponse {
    pub height: u64,
    pub timestamp_ms: u64,
    /// YES clearing price at this point. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub yes_price_nanos: u64,
    /// NO clearing price at this point. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub no_price_nanos: u64,
    /// Traded notional at this point. Integer nanodollars; 1_000_000_000 = $1.
    pub volume_nanos: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PriceCandlesResponse {
    pub market_id: u32,
    pub resolution_secs: u32,
    pub candles: Vec<PriceCandleResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_before_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_min_bucket_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PriceCandleResponse {
    pub bucket_start_ms: u64,
    pub bucket_end_ms: u64,
    pub first_height: u64,
    pub last_height: u64,
    /// Bucket open YES price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub open_yes_price_nanos: u64,
    /// Bucket high YES price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub high_yes_price_nanos: u64,
    /// Bucket low YES price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub low_yes_price_nanos: u64,
    /// Bucket close YES price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub close_yes_price_nanos: u64,
    /// Bucket open NO price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub open_no_price_nanos: u64,
    /// Bucket high NO price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub high_no_price_nanos: u64,
    /// Bucket low NO price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub low_no_price_nanos: u64,
    /// Bucket close NO price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub close_no_price_nanos: u64,
    /// Bucket traded notional. Integer nanodollars; 1_000_000_000 = $1.
    pub volume_nanos: u64,
    pub point_count: u64,
}

// --- Equity Series ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EquityPointResponse {
    pub timestamp_ms: u64,
    pub height: u64,
    /// Portfolio value at this point. Integer nanodollars; 1_000_000_000 = $1.
    pub portfolio_value_nanos: i64,
    /// Deposited amount at this point. Integer nanodollars; 1_000_000_000 = $1.
    pub deposited_nanos: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EquitySeriesResponse {
    pub account_id: u64,
    pub points: Vec<EquityPointResponse>,
    /// Oldest timestamp for which durable history is guaranteed complete.
    /// `None` means retention is disabled.
    pub retention_min_timestamp_ms: Option<u64>,
    /// True when the requested range begins before the retained boundary.
    pub history_truncated: bool,
    /// `durable` for redb-backed history, `memory` for bounded dev fallback.
    pub history_scope: String,
    /// Number of retained source samples represented by `points`.
    pub source_points: usize,
    pub downsampled: bool,
}

// --- Leaderboard (SYB-59) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LeaderboardResponse {
    /// Window this leaderboard was ranked over: `7d`, `30d`, or `all`.
    pub window: String,
    /// Ranked entries, best PnL first. Ties break by ascending account id.
    pub entries: Vec<LeaderboardEntryResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LeaderboardEntryResponse {
    /// 1-based rank within the returned window.
    pub rank: u32,
    /// Account identifier. Clients render this anonymously as `Trader #<id>`;
    /// display-name opt-in awaits profiles (SYB-60).
    pub account_id: u64,
    /// Signed opt-in public profile name. Its presence is the publication
    /// consent boundary for this entire financial row.
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_seed: Option<String>,
    /// Net PnL over the window (realized + unrealized). Integer nanodollars; 1_000_000_000 = $1.
    pub pnl_nanos: i64,
    /// Return on invested capital over the window, in basis points (100 = 1%).
    pub roi_bps: i64,
    /// Distinct markets with a currently open position.
    pub markets_traded: u32,
    /// Current portfolio equity (balance + marked positions). Integer nanodollars; 1_000_000_000 = $1.
    pub equity_nanos: i64,
}

// --- Account Fills ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AccountFillResponse {
    /// Stable cursor for forward pagination (`GET .../fills?after=<cursor>`).
    /// Opaque to clients; current encoding is `<block_height>.<order_id>`.
    pub cursor: String,
    pub order_id: u64,
    /// Fill quantity. Integer share-units; 1000 units = 1 share.
    pub fill_qty: u64,
    /// Fill price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub fill_price_nanos: u64,
    pub block_height: u64,
    pub timestamp_ms: u64,
    pub position_deltas: Vec<PositionDeltaResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AccountFillPageResponse {
    pub fills: Vec<AccountFillResponse>,
    /// Cursor to continue forward pagination, when this was a forward page.
    pub next_after: Option<String>,
    pub retention_min_timestamp_ms: Option<u64>,
    /// Highest block from which this account had a fill row removed.
    pub pruned_through_height: Option<u64>,
    /// The supplied forward cursor may have skipped pruned fills.
    pub cursor_gap: bool,
    /// True means rows older than the retention boundary are unavailable.
    pub history_truncated: bool,
    pub history_scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PositionDeltaResponse {
    pub market_id: u32,
    pub outcome: String,
    /// Position quantity delta. Integer share-units; 1000 units = 1 share.
    pub delta: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AccountHistoryPageResponse {
    pub events: Vec<HistoryEventResponse>,
    pub next_before: Option<String>,
    pub retention_min_timestamp_ms: Option<u64>,
    pub history_truncated: bool,
    pub history_scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PendingOrderResponse {
    pub order_id: u64,
    pub account_id: u64,
    pub market_id: u32,
    pub side: String,
    /// Limit price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    pub limit_price_nanos: u64,
    /// Remaining fill quantity. Integer share-units; 1000 units = 1 share.
    pub remaining_quantity: u64,
    pub created_at_block: u64,
    pub expires_at_block: u64,
    /// Original `max_fill` at admit time. Integer share-units; 1000 units = 1 share.
    /// Lets the FE render a partial-fill progress bar as
    /// `(original - remaining) / original`.
    /// `0` for orders persisted before B5/B8 (#[serde(default)] forward
    /// compat).
    #[serde(default)]
    pub original_quantity: u64,
    /// Wall-clock admit time, ms since epoch. `0` for orders admitted before
    /// this field shipped (#[serde(default)] forward compat).
    #[serde(default)]
    pub created_at_ms: u64,
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
    /// Total traded notional for this bucket. Integer nanodollars;
    /// 1_000_000_000 = $1.
    #[serde(default)]
    pub total_volume_nanos: u64,
    /// Cumulative platform welfare for this bucket. Integer nanodollars;
    /// 1_000_000_000 = $1. Sum of per-block `total_welfare` (each fill counted
    /// once). Signed: solver rounding can yield small negatives.
    #[serde(default)]
    pub total_welfare_nanos: i64,
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
    /// Distinct orders admitted (counted once per order at intake), all-time
    /// or rolling 24h. `placed` above stays per-batch participation for
    /// back-compat: a resting order counts once here but once per batch there.
    #[serde(default)]
    pub placed_distinct: u64,
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
    /// Indicative YES price for the open batch. Integer nanodollars;
    /// 1_000_000_000 = $1. Prices are per-share probabilities in [0, 1e9].
    #[serde(default)]
    pub indicative_yes_price_nanos: Option<u64>,
    /// Indicative NO price for the open batch. Integer nanodollars;
    /// 1_000_000_000 = $1. Prices are per-share probabilities in [0, 1e9].
    #[serde(default)]
    pub indicative_no_price_nanos: Option<u64>,
    /// Indicative traded notional for the open batch. Integer nanodollars;
    /// 1_000_000_000 = $1.
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

/// One entry in the per-account history feed (`GET /v1/accounts/{id}/events`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct HistoryEventResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub category: String,
    pub timestamp_ms: u64,
    pub block_height: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    /// Event quantity. Integer share-units; 1000 units = 1 share.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qty: Option<u64>,
    /// Event price. Integer nanodollars; 1_000_000_000 = $1.
    /// Prices are per-share probabilities in [0, 1e9].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_nanos: Option<u64>,
    /// Event cash amount. Integer nanodollars; 1_000_000_000 = $1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount_nanos: Option<i64>,
    /// Event realized PnL. Integer nanodollars; 1_000_000_000 = $1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realized_pnl_nanos: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payout_outcome: Option<String>,
    /// Rejected only: reason code (`insufficient_balance` | `insufficient_position`
    /// | `complete_set` | …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Rejected-order required amount. Integer nanodollars; 1_000_000_000 = $1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_nanos: Option<i64>,
    /// Rejected-order available amount. Integer nanodollars; 1_000_000_000 = $1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_nanos: Option<i64>,
}

/// One entry on the automated-resolution review board (SYB-48).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AutoResolutionEntryResponse {
    pub market_id: u32,
    /// Display status derived at read time from the operator decision AND the
    /// market's live on-chain state: one of `pending`, `needs_review`,
    /// `escalated`, `approved`, `rejected`, `resolved`.
    pub status: String,
    /// Confidence tier the resolver assigned (`propose` | `review` |
    /// `escalate`).
    pub action: crate::request::AutoResolutionActionDto,
    /// Proposed YES payout per share. Integer nanodollars; 1_000_000_000 = $1.
    /// Payouts are per-share probabilities in [0, 1e9].
    pub payout_nanos: u64,
    /// Model confidence in [0, 1].
    pub confidence: f64,
    /// Model's free-text justification.
    pub reasoning: String,
    /// Short verbatim excerpts from the fetched source.
    #[serde(default)]
    pub evidence_excerpts: Vec<String>,
    /// When the proposal was first recorded. Unix milliseconds.
    pub proposed_at_ms: u64,
    /// Auto-finalize deadline for `propose` entries. Unix milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eta_ms: Option<u64>,
    /// When an operator approved/rejected, if they did. Unix milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at_ms: Option<u64>,
}

/// Response body of `GET /v1/admin/auto-resolutions` (SYB-48).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AutoResolutionListResponse {
    pub entries: Vec<AutoResolutionEntryResponse>,
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
            derived_view_sidecar: DerivedViewSidecarResponse::default(),
        }
    }

    /// `by_market` is omitted when empty, while `derived_view_sidecar` remains
    /// present so clients can see the derived/unproven provenance marker.
    #[test]
    fn block_response_serde_roundtrip() {
        let blk = empty_block_response();
        let json = serde_json::to_string(&blk).expect("serialize");
        assert!(
            !json.contains("by_market"),
            "empty by_market must not serialize: {json}"
        );
        assert!(
            json.contains("derived_unproven"),
            "derived sidecar provenance must serialize: {json}"
        );

        // Deserialize an old payload with no by_market or derived_view_sidecar keys.
        let old_shape = serde_json::json!({
            "height": 1,
            "parent_hash": "00",
            "state_root": "11",
            "events_root": "22",
            "order_count": 0,
            "fill_count": 0,
            "timestamp_ms": 0,
            "total_welfare_nanos": 0,
            "total_volume_nanos": 0,
            "orders_filled": 0
        });
        let parsed: BlockResponse =
            serde_json::from_value(old_shape).expect("deserialize old shape");
        assert!(parsed.by_market.is_empty());
        assert_eq!(parsed.derived_view_sidecar.provenance, "derived_unproven");

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
