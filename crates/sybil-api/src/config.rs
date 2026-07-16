use std::path::PathBuf;

use clap::Parser;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BridgeDomain {
    pub chain_id: u64,
    pub vault_address: [u8; 20],
    pub token_address: [u8; 20],
}

fn parse_bridge_address(value: &str, name: &str) -> Result<[u8; 20], String> {
    let value = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    let bytes = hex::decode(value).map_err(|_| format!("{name} must be hex encoded"))?;
    let len = bytes.len();
    let address = bytes
        .try_into()
        .map_err(|_| format!("{name} must decode to 20 bytes, got {len}"))?;
    if address == [0; 20] {
        return Err(format!("{name} must be nonzero"));
    }
    Ok(address)
}

#[derive(Parser, Debug, Clone)]
#[command(name = "sybil-api", about = "HTTP API for Sybil prediction markets")]
pub struct ApiConfig {
    /// Port to listen on.
    #[arg(long, default_value = "3000", env = "SYBIL_PORT")]
    pub port: u16,

    /// Enable local dev conveniences (free account funding, simulation
    /// controls, diagnostic orderbook listings, permissive CORS).
    #[arg(long, default_value = "false", env = "SYBIL_DEV_MODE")]
    pub dev_mode: bool,

    /// Deployment profile: `local`, `devnet`, or `prod`. Selects the intended
    /// durability/cache posture and drives the startup preflight guardrail. On
    /// `prod`, the server refuses to start when dev-only knobs are wired in
    /// (see [`crate::preflight`]) unless `SYBIL_ALLOW_DEV_KNOBS=1` is set.
    #[arg(long, default_value = "local", env = "SYBIL_DEPLOYMENT_PROFILE")]
    pub deployment_profile: String,

    /// Escape hatch: allow a `prod` profile to start despite dev-only knobs.
    /// Fail-open override, mirroring the fail-closed service-token posture.
    /// Intended for deliberate one-off operations, never steady state.
    #[arg(long, default_value = "false", env = "SYBIL_ALLOW_DEV_KNOBS")]
    pub allow_dev_knobs: bool,

    /// Bearer token required for service/operator routes when dev mode is off.
    /// Empty/unset means service routes fail closed in production.
    #[arg(long, default_value = "", env = "SYBIL_SERVICE_TOKEN")]
    pub service_token: String,

    /// L1 chain accepted by bridge deposit and withdrawal routes. This and
    /// both bridge addresses are all-or-none; an unset domain keeps monetary
    /// bridge admission unavailable while leaving status reads operational.
    #[arg(long, default_value = "", env = "SYBIL_BRIDGE_CHAIN_ID")]
    pub bridge_chain_id: String,

    /// Hex-encoded 20-byte vault accepted by bridge routes.
    #[arg(long, default_value = "", env = "SYBIL_BRIDGE_VAULT_ADDRESS")]
    pub bridge_vault_address: String,

    /// Hex-encoded 20-byte collateral token accepted by bridge routes.
    #[arg(long, default_value = "", env = "SYBIL_BRIDGE_TOKEN_ADDRESS")]
    pub bridge_token_address: String,

    /// Private history projector base URL. Empty keeps ingestion disabled and
    /// makes historical product endpoints explicitly unavailable; trading and
    /// the durable outbox continue normally.
    #[arg(long, default_value = "", env = "SYBIL_HISTORY_URL")]
    pub history_url: String,

    /// Dedicated bearer shared only with `sybil-history`. Do not reuse the
    /// operator service token, which grants mutation capabilities.
    #[arg(long, default_value = "", env = "SYBIL_HISTORY_TOKEN")]
    pub history_token: String,

    /// Product-history outbox delivery polling interval.
    #[arg(long, default_value = "250", env = "SYBIL_HISTORY_POLL_MS")]
    pub history_poll_ms: u64,

    /// Timeout for one internal history request.
    #[arg(long, default_value = "10000", env = "SYBIL_HISTORY_TIMEOUT_MS")]
    pub history_timeout_ms: u64,

    /// Private Arena analytics service base URL. Empty keeps bot analytics
    /// unavailable without coupling the API to Arena's storage.
    #[arg(long, default_value = "", env = "SYBIL_ARENA_READ_URL")]
    pub arena_read_url: String,

    /// Dedicated read-only bearer shared with the Arena analytics service.
    #[arg(long, default_value = "", env = "SYBIL_ARENA_READ_TOKEN")]
    pub arena_read_token: String,

    /// Timeout for one internal Arena analytics request.
    #[arg(long, default_value = "3000", env = "SYBIL_ARENA_READ_TIMEOUT_MS")]
    pub arena_read_timeout_ms: u64,

    /// Comma-separated browser origins allowed by CORS in production. Empty =
    /// no cross-origin CORS headers; same-origin browser requests still work.
    #[arg(long, env = "SYBIL_CORS_ORIGINS", value_delimiter = ',')]
    pub cors_origins: Vec<String>,

    /// Immediate reverse-proxy networks allowed to supply client IP headers.
    /// Empty means `X-Forwarded-For` and `X-Real-IP` are ignored. Each trusted
    /// proxy must sanitize or append to X-Forwarded-For before forwarding.
    #[arg(long, env = "SYBIL_HTTP_TRUSTED_PROXY_CIDRS", value_delimiter = ',')]
    pub http_trusted_proxy_cidrs: Vec<ipnet::IpNet>,

    /// Block production interval in milliseconds.
    #[arg(long, default_value = "500", env = "SYBIL_BLOCK_INTERVAL_MS")]
    pub block_interval_ms: u64,

    /// Maximum age of one externally published reference price before public
    /// market reads omit it. Freshness is tracked per market, so a partial
    /// publisher update cannot keep untouched tokens alive.
    #[arg(
        long,
        default_value = "60000",
        env = "SYBIL_REFERENCE_PRICE_TTL_MS",
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub reference_price_ttl_ms: u64,

    /// Seed markets to create on startup (comma-separated names).
    #[arg(long, env = "SYBIL_SEED_MARKETS", value_delimiter = ',')]
    pub seed_markets: Vec<String>,

    /// Order time-to-live in blocks. Default is ~1 year at 500ms blocks (GTC).
    #[arg(long, default_value = "63072000", env = "SYBIL_ORDER_TTL_BLOCKS")]
    pub order_ttl_blocks: u64,

    /// Cap on buffered MM / multi-market submissions waiting for the next block.
    #[arg(long, default_value = "10000", env = "SYBIL_MAX_PENDING_BUNDLES")]
    pub max_pending_bundles: usize,

    /// Maximum number of orders accepted through the service-only bulk route.
    #[arg(long, default_value = "512", env = "SYBIL_MAX_ORDERS_PER_SUBMISSION")]
    pub max_orders_per_submission: usize,

    /// Per-account sustained order/cancel submission rate.
    #[arg(
        long,
        default_value = "50",
        env = "SYBIL_MAX_SUBMISSIONS_PER_ACCOUNT_PER_SECOND"
    )]
    pub max_submissions_per_account_per_second: u32,

    /// Per-account submission burst allowance.
    #[arg(
        long,
        default_value = "200",
        env = "SYBIL_SUBMISSION_BURST_PER_ACCOUNT"
    )]
    pub submission_burst_per_account: u32,

    /// Global sustained order/cancel submission rate.
    #[arg(
        long,
        default_value = "1000",
        env = "SYBIL_MAX_GLOBAL_SUBMISSIONS_PER_SECOND"
    )]
    pub max_global_submissions_per_second: u32,

    /// Global submission burst allowance.
    #[arg(long, default_value = "3000", env = "SYBIL_GLOBAL_SUBMISSION_BURST")]
    pub global_submission_burst: u32,

    /// Maximum resting non-MM orders per account.
    #[arg(
        long,
        default_value = "1000",
        env = "SYBIL_MAX_OPEN_ORDERS_PER_ACCOUNT"
    )]
    pub max_open_orders_per_account: usize,

    /// Minimum cash/position notional backing each non-MM resting order.
    /// Default is one tenth of a cent (1,000,000 nanodollars), matching one
    /// minimum quantity unit at a $1 limit.
    #[arg(
        long,
        default_value = "1000000",
        env = "SYBIL_MIN_RESTING_ORDER_NOTIONAL_NANOS"
    )]
    pub min_resting_order_notional_nanos: u64,

    /// Maximum deferred MM / multi-market submissions per account.
    #[arg(
        long,
        default_value = "100",
        env = "SYBIL_MAX_PENDING_BUNDLES_PER_ACCOUNT"
    )]
    pub max_pending_bundles_per_account: usize,

    /// Pre-handler global HTTP rate for order endpoints. This protects CPU
    /// before JSON parsing and P256 verification.
    #[arg(long, default_value = "500", env = "SYBIL_HTTP_ORDER_GLOBAL_RPS")]
    pub http_order_global_rps: u32,

    /// Pre-handler global HTTP burst for order endpoints.
    #[arg(long, default_value = "2000", env = "SYBIL_HTTP_ORDER_GLOBAL_BURST")]
    pub http_order_global_burst: u32,

    /// Pre-handler per-client HTTP rate for order endpoints.
    #[arg(long, default_value = "250", env = "SYBIL_HTTP_ORDER_CLIENT_RPS")]
    pub http_order_client_rps: u32,

    /// Pre-handler per-client HTTP burst for order endpoints.
    #[arg(long, default_value = "1000", env = "SYBIL_HTTP_ORDER_CLIENT_BURST")]
    pub http_order_client_burst: u32,

    /// Lifetime ceiling for anonymous self-service account allocations. This
    /// is compared with the durable next account id; zero disables public
    /// onboarding. Service/dev account creation remains an operator action.
    #[arg(long, default_value = "1000", env = "SYBIL_PUBLIC_ACCOUNT_CAPACITY")]
    pub public_account_capacity: u64,

    /// Fixed play-money grant assigned by the server to a public account.
    /// Anonymous callers cannot choose this value. Production should use zero
    /// so capital arrives only through authenticated bridge deposits.
    #[arg(
        long,
        default_value = "1000000000000",
        env = "SYBIL_PUBLIC_ACCOUNT_GRANT_NANOS"
    )]
    pub public_account_grant_nanos: u64,

    /// Pre-handler global request rate for public account onboarding.
    #[arg(
        long,
        default_value = "5",
        env = "SYBIL_HTTP_ONBOARDING_GLOBAL_RPS",
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub http_onboarding_global_rps: u32,

    /// Global public-onboarding burst allowance.
    #[arg(
        long,
        default_value = "20",
        env = "SYBIL_HTTP_ONBOARDING_GLOBAL_BURST",
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub http_onboarding_global_burst: u32,

    /// Per-client public-onboarding request rate.
    #[arg(
        long,
        default_value = "1",
        env = "SYBIL_HTTP_ONBOARDING_CLIENT_RPS",
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub http_onboarding_client_rps: u32,

    /// Per-client public-onboarding burst allowance.
    #[arg(
        long,
        default_value = "3",
        env = "SYBIL_HTTP_ONBOARDING_CLIENT_BURST",
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub http_onboarding_client_burst: u32,

    /// Pre-handler global request rate for retained DA manifest/payload reads.
    #[arg(long, default_value = "20", env = "SYBIL_HTTP_DA_GLOBAL_RPS")]
    pub http_da_global_rps: u32,

    /// Global DA read burst allowance.
    #[arg(long, default_value = "40", env = "SYBIL_HTTP_DA_GLOBAL_BURST")]
    pub http_da_global_burst: u32,

    /// Per-client retained DA read rate.
    #[arg(long, default_value = "10", env = "SYBIL_HTTP_DA_CLIENT_RPS")]
    pub http_da_client_rps: u32,

    /// Per-client retained DA read burst allowance.
    #[arg(long, default_value = "20", env = "SYBIL_HTTP_DA_CLIENT_BURST")]
    pub http_da_client_burst: u32,

    /// Maximum retained DA reads executing concurrently.
    #[arg(long, default_value = "4", env = "SYBIL_HTTP_DA_MAX_CONCURRENCY")]
    pub http_da_max_concurrency: usize,

    /// Maximum anonymous public WebSocket block streams held concurrently.
    /// The authenticated service stream has a separate trust boundary and
    /// does not consume this budget.
    #[arg(
        long,
        default_value = "256",
        env = "SYBIL_HTTP_PUBLIC_STREAM_MAX_CONNECTIONS"
    )]
    pub http_public_stream_max_connections: usize,

    /// Close a WebSocket block stream when the server has received no frame
    /// from its client for this many milliseconds.
    #[arg(
        long,
        default_value = "90000",
        env = "SYBIL_WS_CLIENT_IDLE_TIMEOUT_MS",
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub ws_client_idle_timeout_ms: u64,

    /// WebAuthn relying-party id. For local frontend dev this is `localhost`.
    #[arg(long, default_value = "localhost", env = "SYBIL_WEBAUTHN_RP_ID")]
    pub webauthn_rp_id: String,

    /// Browser origin allowed in WebAuthn clientDataJSON.
    #[arg(
        long,
        default_value = "http://localhost:3000",
        env = "SYBIL_WEBAUTHN_ORIGIN"
    )]
    pub webauthn_origin: String,

    /// Require the authenticator's user-verification flag on WebAuthn assertions.
    #[arg(long, default_value = "true", env = "SYBIL_WEBAUTHN_REQUIRE_UV")]
    pub webauthn_require_uv: bool,

    /// In-memory cache size for recent canonical blocks.
    #[arg(long, default_value = "100", env = "SYBIL_RECENT_BLOCK_CACHE_CAPACITY")]
    pub recent_block_cache_capacity: usize,

    /// Canonical replay heights and paired DA artifacts retained locally.
    #[arg(
        long,
        default_value = "0",
        env = "SYBIL_CANONICAL_ARCHIVE_RETENTION_BLOCKS"
    )]
    pub canonical_archive_retention_blocks: u64,

    /// Block cadence for canonical archive maintenance.
    #[arg(
        long,
        default_value = "1000",
        env = "SYBIL_CANONICAL_ARCHIVE_MAINTENANCE_INTERVAL_BLOCKS"
    )]
    pub canonical_archive_maintenance_interval_blocks: u64,

    /// Maximum replay-block or DA-artifact rows deleted in one pass.
    #[arg(
        long,
        default_value = "10000",
        env = "SYBIL_CANONICAL_ARCHIVE_MAX_ROWS_PER_PASS"
    )]
    pub canonical_archive_max_rows_per_pass: usize,

    /// Acknowledged proof-job heights retained by the sequencer after durable
    /// prover ingest. Uses the canonical maintenance cadence and row budget.
    #[arg(
        long,
        default_value = "0",
        env = "SYBIL_ACKNOWLEDGED_PROOF_JOB_RETENTION_BLOCKS"
    )]
    pub acknowledged_proof_job_retention_blocks: u64,

    /// Block cadence for acknowledged proof-job maintenance.
    #[arg(
        long,
        default_value = "1000",
        env = "SYBIL_ACKNOWLEDGED_PROOF_JOB_MAINTENANCE_INTERVAL_BLOCKS"
    )]
    pub acknowledged_proof_job_maintenance_interval_blocks: u64,

    /// Maximum old proof-job rows examined in one maintenance pass.
    #[arg(
        long,
        default_value = "10000",
        env = "SYBIL_ACKNOWLEDGED_PROOF_JOB_MAX_ROWS_PER_PASS"
    )]
    pub acknowledged_proof_job_max_rows_per_pass: usize,

    /// Retain portable proof jobs and DA artifacts for an attached validity
    /// stack. Product-only deployments disable this explicitly; changing it
    /// for a persistent store requires a fresh genesis.
    #[arg(long, default_value = "true", env = "SYBIL_RETAIN_VALIDITY_ARTIFACTS")]
    pub retain_validity_artifacts: bool,

    /// Sequencer actor queue depth that logs a warning.
    #[arg(long, default_value = "1000", env = "SYBIL_ACTOR_QUEUE_WARN_DEPTH")]
    pub actor_queue_warn_depth: usize,

    /// Sequencer actor queue depth that logs an error and should page.
    #[arg(long, default_value = "5000", env = "SYBIL_ACTOR_QUEUE_ERROR_DEPTH")]
    pub actor_queue_error_depth: usize,

    /// Width of the ±band (in nanos) the off-block LiquidityTracker uses to
    /// score "near-the-money" depth. Default 50_000_000 nanos = $0.05.
    #[arg(long, default_value = "50000000", env = "SYBIL_LIQUIDITY_BAND_NANOS")]
    pub liquidity_band_nanos: u64,

    /// Data directory for persistent storage. Empty = in-memory only (no persistence).
    #[arg(long, default_value = "", env = "SYBIL_DATA_DIR")]
    pub data_dir: String,

    /// Import a canonical block witness payload into an empty persistent store,
    /// then exit. The imported store continues the witness chain instance; use
    /// `--genesis-hash` when the payload cannot derive the height-1 header.
    #[arg(long, default_value = "false", env = "SYBIL_IMPORT_WITNESS")]
    pub import_witness: bool,

    /// Canonical witness payload file to import when `--import-witness` is set.
    #[arg(long, env = "SYBIL_IMPORT_WITNESS_PAYLOAD", value_name = "PATH")]
    pub payload: Option<PathBuf>,

    /// Expected post-state root for the imported witness, as 32-byte hex.
    #[arg(long, env = "SYBIL_IMPORT_WITNESS_EXPECT_STATE_ROOT")]
    pub expect_state_root: Option<String>,

    /// Original chain genesis hash for witness imports when the payload does
    /// not include block 1. Required for importing heads beyond height 2.
    #[arg(long, env = "SYBIL_IMPORT_WITNESS_GENESIS_HASH")]
    pub genesis_hash: Option<String>,

    /// Path to the JSON file that persists off-block `MarketRefData`
    /// (Polymarket mirror metadata: event id/title, images, end dates,
    /// category). Empty = volatile in-memory only (state lost on restart;
    /// mirror re-fills on the next sync cycle).
    #[arg(long, default_value = "", env = "SYBIL_MARKET_REF_DATA_PATH")]
    pub market_ref_data_path: String,

    /// Directory holding full Polymarket event JSON snapshots, served at
    /// `GET /v1/events/{id}/raw`. Persisted across restarts (created if missing,
    /// never wiped on boot — SYB-153) so mirror raw JSON survives a restart
    /// without waiting for the next sync. Empty = disabled (raw endpoints 404).
    #[arg(long, default_value = "", env = "SYBIL_EVENT_SNAPSHOT_DIR")]
    pub event_snapshot_dir: String,

    /// Path to the P256 signing key used by the admin feed. The file stores
    /// the raw 32-byte SEC1 scalar, hex-encoded. Empty = generate a fresh
    /// ephemeral key at startup (dev-mode convenience; will NOT persist
    /// across restarts).
    #[arg(long, default_value = "", env = "SYBIL_ADMIN_FEED_KEY_PATH")]
    pub admin_feed_key_path: String,

    /// Hex-encoded compressed SEC1 P256 pubkey (33 bytes) for the
    /// Polymarket-mirror resolution feed. When set, the server registers a
    /// `polymarket_mirror` feed and installs a matching `polymarket_mirror`
    /// resolution template. When unset, Polymarket-mirrored markets still
    /// work for trading but can only be resolved via the admin path.
    #[arg(long, default_value = "", env = "SYBIL_POLYMARKET_FEED_PUBKEY_HEX")]
    pub polymarket_feed_pubkey_hex: String,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            port: 3000,
            dev_mode: false,
            deployment_profile: "local".to_string(),
            allow_dev_knobs: false,
            service_token: String::new(),
            bridge_chain_id: String::new(),
            bridge_vault_address: String::new(),
            bridge_token_address: String::new(),
            history_url: String::new(),
            history_token: String::new(),
            history_poll_ms: 250,
            history_timeout_ms: 10_000,
            arena_read_url: String::new(),
            arena_read_token: String::new(),
            arena_read_timeout_ms: 3_000,
            cors_origins: Vec::new(),
            http_trusted_proxy_cidrs: Vec::new(),
            block_interval_ms: 500,
            reference_price_ttl_ms: 60_000,
            seed_markets: Vec::new(),
            order_ttl_blocks: 63_072_000,
            max_pending_bundles: 10_000,
            max_orders_per_submission: 512,
            max_submissions_per_account_per_second: 50,
            submission_burst_per_account: 200,
            max_global_submissions_per_second: 1_000,
            global_submission_burst: 3_000,
            max_open_orders_per_account: 1_000,
            min_resting_order_notional_nanos: 1_000_000,
            max_pending_bundles_per_account: 100,
            http_order_global_rps: 500,
            http_order_global_burst: 2_000,
            http_order_client_rps: 250,
            http_order_client_burst: 1_000,
            public_account_capacity: 1_000,
            public_account_grant_nanos: 1_000_000_000_000,
            http_onboarding_global_rps: 5,
            http_onboarding_global_burst: 20,
            http_onboarding_client_rps: 1,
            http_onboarding_client_burst: 3,
            http_da_global_rps: 20,
            http_da_global_burst: 40,
            http_da_client_rps: 10,
            http_da_client_burst: 20,
            http_da_max_concurrency: 4,
            http_public_stream_max_connections: 256,
            ws_client_idle_timeout_ms: 90_000,
            webauthn_rp_id: "localhost".to_string(),
            webauthn_origin: "http://localhost:3000".to_string(),
            webauthn_require_uv: true,
            recent_block_cache_capacity: 100,
            canonical_archive_retention_blocks: 0,
            canonical_archive_maintenance_interval_blocks: 1_000,
            canonical_archive_max_rows_per_pass: 10_000,
            acknowledged_proof_job_retention_blocks: 0,
            acknowledged_proof_job_maintenance_interval_blocks: 1_000,
            acknowledged_proof_job_max_rows_per_pass: 10_000,
            retain_validity_artifacts: true,
            actor_queue_warn_depth: 1_000,
            actor_queue_error_depth: 5_000,
            liquidity_band_nanos: 50_000_000,
            data_dir: String::new(),
            import_witness: false,
            payload: None,
            expect_state_root: None,
            genesis_hash: None,
            market_ref_data_path: String::new(),
            event_snapshot_dir: String::new(),
            admin_feed_key_path: String::new(),
            polymarket_feed_pubkey_hex: String::new(),
        }
    }
}

impl ApiConfig {
    pub fn bridge_domain(&self) -> Result<Option<BridgeDomain>, String> {
        let chain = self.bridge_chain_id.trim();
        let vault = self.bridge_vault_address.trim();
        let token = self.bridge_token_address.trim();
        let configured = [!chain.is_empty(), !vault.is_empty(), !token.is_empty()];

        if configured == [false, false, false] {
            return Ok(None);
        }
        if configured != [true, true, true] {
            return Err("SYBIL_BRIDGE_CHAIN_ID, SYBIL_BRIDGE_VAULT_ADDRESS, and \
                 SYBIL_BRIDGE_TOKEN_ADDRESS must be configured together"
                .to_string());
        }

        let chain_id = chain
            .parse::<u64>()
            .map_err(|_| "SYBIL_BRIDGE_CHAIN_ID must be an unsigned integer".to_string())?;
        if chain_id == 0 {
            return Err("SYBIL_BRIDGE_CHAIN_ID must be nonzero".to_string());
        }

        Ok(Some(BridgeDomain {
            chain_id,
            vault_address: parse_bridge_address(vault, "SYBIL_BRIDGE_VAULT_ADDRESS")?,
            token_address: parse_bridge_address(token, "SYBIL_BRIDGE_TOKEN_ADDRESS")?,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_domain_is_all_or_none_and_strictly_typed() {
        assert_eq!(ApiConfig::default().bridge_domain().unwrap(), None);

        let valid = ApiConfig {
            bridge_chain_id: "11155111".to_string(),
            bridge_vault_address: format!("0x{}", "11".repeat(20)),
            bridge_token_address: "22".repeat(20),
            ..ApiConfig::default()
        };
        assert_eq!(
            valid.bridge_domain().unwrap(),
            Some(BridgeDomain {
                chain_id: 11_155_111,
                vault_address: [0x11; 20],
                token_address: [0x22; 20],
            })
        );

        for invalid in [
            ApiConfig {
                bridge_chain_id: "11155111".to_string(),
                ..ApiConfig::default()
            },
            ApiConfig {
                bridge_chain_id: "0".to_string(),
                bridge_vault_address: "11".repeat(20),
                bridge_token_address: "22".repeat(20),
                ..ApiConfig::default()
            },
            ApiConfig {
                bridge_chain_id: "11155111".to_string(),
                bridge_vault_address: "11".repeat(19),
                bridge_token_address: "22".repeat(20),
                ..ApiConfig::default()
            },
            ApiConfig {
                bridge_chain_id: "11155111".to_string(),
                bridge_vault_address: "00".repeat(20),
                bridge_token_address: "22".repeat(20),
                ..ApiConfig::default()
            },
        ] {
            assert!(invalid.bridge_domain().is_err());
        }
    }
}
