use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "sybil-polymarket",
    about = "Mirror Polymarket markets onto Sybil"
)]
pub struct Config {
    /// Sybil API base URL.
    #[arg(long, default_value = "http://localhost:3000", env = "SYBIL_URL")]
    pub sybil_url: String,

    /// Polymarket Gamma API URL.
    #[arg(
        long,
        default_value = "https://gamma-api.polymarket.com",
        env = "POLYMARKET_GAMMA_URL"
    )]
    pub gamma_url: String,

    /// Polymarket CLOB WebSocket URL.
    #[arg(
        long,
        default_value = "wss://ws-subscriptions-clob.polymarket.com/ws/market",
        env = "POLYMARKET_WS_URL"
    )]
    pub ws_url: String,

    /// Polymarket CLOB REST URL (fallback for prices).
    #[arg(
        long,
        default_value = "https://clob.polymarket.com",
        env = "POLYMARKET_CLOB_URL"
    )]
    pub clob_url: String,

    /// Market sync interval in seconds.
    #[arg(long, default_value = "60", env = "SYNC_INTERVAL_SECS")]
    pub sync_interval_secs: u64,

    /// Categories to mirror (comma-separated, empty = all).
    #[arg(long, env = "MIRROR_CATEGORIES", value_delimiter = ',')]
    pub mirror_categories: Vec<String>,

    /// Categories to exclude from mirroring (comma-separated). Matched against
    /// Polymarket event tag labels and slugs.
    #[arg(long, env = "MIRROR_EXCLUDED_CATEGORIES", value_delimiter = ',')]
    pub mirror_excluded_categories: Vec<String>,

    /// Minimum Polymarket volume (USD) to mirror. 0 = all.
    #[arg(long, default_value = "0", env = "MIN_VOLUME_USD")]
    pub min_volume_usd: f64,

    /// Maximum number of events to mirror.
    #[arg(long, default_value = "50", env = "MAX_EVENTS")]
    pub max_events: usize,

    /// MM half-spread (e.g. 0.02 = 2 cents).
    #[arg(long, default_value = "0.02", env = "MM_HALF_SPREAD")]
    pub mm_half_spread: f64,

    /// MM budget in dollars (flash liquidity constraint).
    #[arg(long, default_value = "1000.0", env = "MM_BUDGET_DOLLARS")]
    pub mm_budget_dollars: f64,

    /// MM quote size per side in dollars.
    #[arg(long, default_value = "200.0", env = "MM_QUOTE_SIZE_DOLLARS")]
    pub mm_quote_size_dollars: f64,

    /// MM initial account balance in dollars.
    #[arg(long, default_value = "5000000.0", env = "MM_INITIAL_BALANCE_DOLLARS")]
    pub mm_initial_balance_dollars: f64,

    /// MM risk aversion (Avellaneda-Stoikov γ). Higher = more aggressive inventory skewing.
    #[arg(long, default_value = "0.05", env = "MM_GAMMA")]
    pub mm_gamma: f64,

    /// Max position per market (shares). At limit, only unwind side is quoted.
    #[arg(long, default_value = "20000", env = "MM_MAX_POSITION")]
    pub mm_max_position: u64,

    /// Maximum mirrored markets the live MM should actively quote. 0 = all.
    #[arg(long, default_value = "0", env = "MM_MAX_MARKETS")]
    pub mm_max_markets: usize,

    /// Legacy public-route rotation limit. Actor-authenticated MM epochs ignore
    /// this value and cover the entire committed universe atomically.
    #[arg(long, default_value = "0", env = "MM_MAX_ORDERS_PER_BLOCK")]
    pub mm_max_orders_per_block: usize,

    /// Dedicated role-bound token for `/v1/actor/*`; empty keeps legacy MM submission.
    #[arg(long, default_value = "", env = "SYBIL_MM_ACTOR_TOKEN")]
    pub mm_actor_token: String,

    /// Neutral YES+NO inventory floor per market, in whole shares.
    #[arg(long, default_value = "20000", env = "MM_COMPLETE_SET_TARGET_SHARES")]
    pub mm_complete_set_target_shares: u64,

    /// Max total dollar exposure across all markets. Budget → 0 as exposure approaches this.
    #[arg(long, default_value = "50000.0", env = "MM_MAX_EXPOSURE_DOLLARS")]
    pub mm_max_exposure_dollars: f64,

    /// Rolling window (blocks) for variance estimation.
    #[arg(long, default_value = "30", env = "MM_VOL_WINDOW")]
    pub mm_vol_window: usize,

    /// Minimum half-spread (floor). Below this, adverse selection eats all profit.
    #[arg(long, default_value = "0.005", env = "MM_MIN_SPREAD")]
    pub mm_min_spread: f64,

    /// Number of qualifying organic native-price observations retained.
    #[arg(long, default_value = "20", env = "MM_NATIVE_OBSERVATION_WINDOW")]
    pub mm_native_observation_window: usize,

    /// Minimum organic filled-order notional required to move a native mark.
    #[arg(
        long,
        default_value = "1.0",
        env = "MM_NATIVE_MIN_ORGANIC_NOTIONAL_DOLLARS"
    )]
    pub mm_native_min_organic_notional_dollars: f64,

    /// Per-observation weight cap for the native weighted median, in dollars.
    #[arg(
        long,
        default_value = "25.0",
        env = "MM_NATIVE_OBSERVATION_WEIGHT_CAP_DOLLARS"
    )]
    pub mm_native_observation_weight_cap_dollars: f64,

    /// Maximum native actor-mark candidate movement per block (probability).
    #[arg(long, default_value = "0.02", env = "MM_NATIVE_MAX_STEP")]
    pub mm_native_max_step: f64,

    /// EWMA weight applied to a capped qualifying native observation.
    #[arg(long, default_value = "0.15", env = "MM_NATIVE_EWMA_WEIGHT")]
    pub mm_native_ewma_weight: f64,

    /// Quiet-block fraction of remaining distance reverted toward the seed.
    #[arg(long, default_value = "0.002", env = "MM_NATIVE_SEED_REVERSION")]
    pub mm_native_seed_reversion: f64,

    /// Blocks between position syncs via GET /v1/accounts/{id}.
    ///
    /// Keep this at one block for live IOC quoting: inventory-aware sell
    /// quotes must observe fills from the immediately previous block before
    /// the next quote batch is generated.
    #[arg(long, default_value = "1", env = "MM_SYNC_INTERVAL_BLOCKS")]
    pub mm_sync_interval_blocks: u64,

    /// Per-token soft-staleness threshold. Quotes continue from the last sane
    /// reference with smaller size and wider spread until the hard threshold.
    #[arg(long, default_value = "30000", env = "MM_STALENESS_MS")]
    pub mm_staleness_ms: u64,

    /// Per-token hard-staleness threshold. Older references are evicted and
    /// carry a typed skip instead of being quoted indefinitely.
    #[arg(long, default_value = "300000", env = "MM_HARD_STALENESS_MS")]
    pub mm_hard_staleness_ms: u64,

    /// Spread multiplier while a mirror reference is soft-stale.
    #[arg(long, default_value = "2.0", env = "MM_SOFT_STALE_SPREAD_MULTIPLIER")]
    pub mm_soft_stale_spread_multiplier: f64,

    /// Quote-size multiplier while a mirror reference is soft-stale.
    #[arg(long, default_value = "0.5", env = "MM_SOFT_STALE_SIZE_MULTIPLIER")]
    pub mm_soft_stale_size_multiplier: f64,

    /// REST price poll interval in seconds (WebSocket fallback).
    #[arg(long, default_value = "5", env = "REST_POLL_INTERVAL_SECS")]
    pub rest_poll_interval_secs: u64,

    /// Path to persist mapping store (empty = in-memory only).
    #[arg(long, default_value = "", env = "MAPPING_STORE_PATH")]
    pub mapping_store_path: String,

    /// Path to a curated markets JSON file (SYB-150). When set, the mirror
    /// syncs ONLY the Polymarket events listed there, addressed by event id,
    /// instead of the volume-ranked category scan. Empty = legacy broad-mirror
    /// mode driven by `--mirror-categories` / `--max-events`.
    #[arg(long, default_value = "", env = "CURATED_MARKETS_PATH")]
    pub curated_markets_path: String,

    /// Path to a native Sybil market template catalog (SYB-151). When set, the
    /// sync actor ensures enabled native templates exist on Sybil before
    /// mirroring Polymarket events. Empty = no native catalog.
    #[arg(long, default_value = "", env = "NATIVE_MARKETS_PATH")]
    pub native_markets_path: String,

    /// Path to the P256 signing key used to attest to resolutions. Empty
    /// disables the ResolutionActor (mirrored markets won't auto-resolve).
    /// The key's compressed SEC1 pubkey must be registered on sybil-api as
    /// the `polymarket_mirror` feed (see `--polymarket-feed-pubkey-hex`).
    #[arg(long, default_value = "", env = "SIGNER_KEY_PATH")]
    pub signer_key_path: String,

    /// Resolution poll interval in seconds.
    #[arg(long, default_value = "120", env = "RESOLUTION_POLL_INTERVAL_SECS")]
    pub resolution_poll_interval_secs: u64,

    // --- SYB-48: LLM auto-resolution (native `api_poll` markets) ---
    /// Enable the LLM auto-resolution actor. DEFAULT OFF: must be explicitly
    /// turned on in deployment. Also requires `SIGNER_KEY_PATH` (for signing)
    /// and `OPENROUTER_API_KEY` (for the model); if either is missing the actor
    /// stays disabled.
    #[arg(long, default_value = "false", env = "AUTORESOLVE_ENABLED")]
    pub autoresolve_enabled: bool,

    /// Auto-resolution poll interval in seconds.
    #[arg(long, default_value = "300", env = "AUTORESOLVE_POLL_INTERVAL_SECS")]
    pub autoresolve_poll_interval_secs: u64,

    /// Confidence at/above which a signed proposal enters the challenge window.
    #[arg(long, default_value = "0.9", env = "AUTORESOLVE_CONFIDENCE_PROPOSE")]
    pub autoresolve_confidence_propose: f64,

    /// Confidence at/above which a market is queued for human review (but below
    /// the propose threshold).
    #[arg(long, default_value = "0.7", env = "AUTORESOLVE_CONFIDENCE_REVIEW")]
    pub autoresolve_confidence_review: f64,

    /// Challenge window (hours) a proposed resolution is held before it
    /// auto-finalizes, unless an operator rejects it first.
    #[arg(long, default_value = "24", env = "AUTORESOLVE_CHALLENGE_WINDOW_HOURS")]
    pub autoresolve_challenge_window_hours: u64,

    /// Minimum seconds between fetches of the same resolution endpoint.
    #[arg(
        long,
        default_value = "300",
        env = "AUTORESOLVE_SOURCE_MIN_INTERVAL_SECS"
    )]
    pub autoresolve_source_min_interval_secs: u64,

    /// OpenRouter model id used to evaluate resolutions.
    #[arg(
        long,
        default_value = "deepseek/deepseek-v4-flash",
        env = "AUTORESOLVE_MODEL"
    )]
    pub autoresolve_model: String,
}

impl Config {
    pub fn validate_liquidity_policy(&self) -> Result<(), String> {
        if self.mm_staleness_ms == 0 || self.mm_hard_staleness_ms <= self.mm_staleness_ms {
            return Err("MM_HARD_STALENESS_MS must exceed non-zero MM_STALENESS_MS".to_string());
        }
        if !self.mm_soft_stale_spread_multiplier.is_finite()
            || !(1.0..=10.0).contains(&self.mm_soft_stale_spread_multiplier)
        {
            return Err("MM_SOFT_STALE_SPREAD_MULTIPLIER must be in 1..=10".to_string());
        }
        if !self.mm_soft_stale_size_multiplier.is_finite()
            || !(0.01..=1.0).contains(&self.mm_soft_stale_size_multiplier)
        {
            return Err("MM_SOFT_STALE_SIZE_MULTIPLIER must be in 0.01..=1".to_string());
        }
        if !(1..=1_000).contains(&self.mm_native_observation_window) {
            return Err("MM_NATIVE_OBSERVATION_WINDOW must be in 1..=1000".to_string());
        }
        if !self.mm_native_min_organic_notional_dollars.is_finite()
            || !(0.01..=10_000.0).contains(&self.mm_native_min_organic_notional_dollars)
        {
            return Err(
                "MM_NATIVE_MIN_ORGANIC_NOTIONAL_DOLLARS must be in 0.01..=10000".to_string(),
            );
        }
        if !self.mm_native_observation_weight_cap_dollars.is_finite()
            || self.mm_native_observation_weight_cap_dollars
                < self.mm_native_min_organic_notional_dollars
            || self.mm_native_observation_weight_cap_dollars > 1_000_000.0
        {
            return Err(
                "MM_NATIVE_OBSERVATION_WEIGHT_CAP_DOLLARS must be at least the organic minimum and at most 1000000"
                    .to_string(),
            );
        }
        if !self.mm_native_max_step.is_finite()
            || !(0.0001..=0.25).contains(&self.mm_native_max_step)
        {
            return Err("MM_NATIVE_MAX_STEP must be in 0.0001..=0.25".to_string());
        }
        if !self.mm_native_ewma_weight.is_finite()
            || !(0.001..=1.0).contains(&self.mm_native_ewma_weight)
        {
            return Err("MM_NATIVE_EWMA_WEIGHT must be in 0.001..=1".to_string());
        }
        if !self.mm_native_seed_reversion.is_finite()
            || !(0.0..=0.1).contains(&self.mm_native_seed_reversion)
        {
            return Err("MM_NATIVE_SEED_REVERSION must be in 0..=0.1".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_native_mark_policy_is_valid() {
        let config = Config::parse_from(["sybil-polymarket"]);
        assert_eq!(config.mm_quote_size_dollars, 200.0);
        config.validate_liquidity_policy().expect("default policy");
    }

    #[test]
    fn native_mark_policy_rejects_dominating_weight_floor() {
        let mut config = Config::parse_from(["sybil-polymarket"]);
        config.mm_native_observation_weight_cap_dollars = 0.5;
        assert!(config.validate_liquidity_policy().is_err());
    }
}
