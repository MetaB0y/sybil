//! Deployment-profile guardrails (SYB-133).
//!
//! The public devnet is tuned with dev-only tradeoffs (in-memory store,
//! permissive dev mode, reduced caches). Nothing used to stop those
//! tradeoffs from silently leaking into a `prod` / devnet-v2 deployment. This
//! module:
//!
//! 1. names the active [`DeploymentProfile`] (`SYBIL_DEPLOYMENT_PROFILE`);
//! 2. surfaces every durability/cache/prover knob whose value differs from the
//!    prod-intended baseline in one structured startup log block; and
//! 3. fail-closes locked profiles when a forbidden dev-only value is wired in,
//!    mirroring the fail-closed service-token posture in [`crate::app`]. The
//!    `SYBIL_ALLOW_DEV_KNOBS=1` escape hatch downgrades the refusal to a loud
//!    warning for deliberate one-off operations.
//!
//! Scope: config surface + logging + validation only. It does not change the
//! matching-sequencer store or settlement logic, and it cannot see
//! compose-level choices such as which prover container is wired in (the mock
//! prover is a separate service, not a `sybil-api` env knob).

use crate::config::ApiConfig;

/// Which deployment this process believes it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeploymentProfile {
    /// Developer laptop / CI. Dev conveniences expected, no durability.
    Local,
    /// Public shared devnet. Dev-tuned but multi-user; no prod guarantees.
    Devnet,
    /// Operator-only prelaunch sandbox. Prod guardrails with a fixed public
    /// grant as the sole permitted dev-only deviation.
    Prelaunch,
    /// Real-value production. Durable, locked-down, fail-closed.
    Prod,
}

impl DeploymentProfile {
    /// Parse the `SYBIL_DEPLOYMENT_PROFILE` value. Case-insensitive; accepts
    /// `production` as an alias.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "devnet" => Ok(Self::Devnet),
            "prelaunch" => Ok(Self::Prelaunch),
            "prod" | "production" => Ok(Self::Prod),
            other => Err(format!(
                "unknown SYBIL_DEPLOYMENT_PROFILE '{other}' \
                 (expected local|devnet|prelaunch|prod)"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Devnet => "devnet",
            Self::Prelaunch => "prelaunch",
            Self::Prod => "prod",
        }
    }

    fn fail_closed(self) -> bool {
        matches!(self, Self::Prelaunch | Self::Prod)
    }

    fn permits_deviation(self, deviation: &Deviation) -> bool {
        self == Self::Prelaunch && deviation.knob == "SYBIL_PUBLIC_ACCOUNT_GRANT_NANOS"
    }
}

/// A single knob whose current value diverges from the prod-intended baseline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deviation {
    /// Env var name, e.g. `SYBIL_DEV_MODE`.
    pub knob: &'static str,
    /// The currently-configured value, rendered for logs.
    pub value: String,
    /// The prod-intended value, rendered for logs.
    pub prod_intended: &'static str,
    /// `true` when this value is unsafe in a real-value production posture
    /// (loses durability / opens the trust boundary / mints play money).
    /// Locked profiles block it unless explicitly permitted or overridden.
    /// `false` is an informational deviation (logged, never blocks).
    pub dev_only: bool,
}

/// The startup preflight snapshot: the active profile plus every knob that
/// diverges from the prod-intended baseline.
#[derive(Clone, Debug)]
pub struct PreflightReport {
    pub profile: DeploymentProfile,
    pub deviations: Vec<Deviation>,
}

impl PreflightReport {
    /// Every deviation that is unsafe in a real-value production posture.
    pub fn violations(&self) -> Vec<&Deviation> {
        self.deviations.iter().filter(|d| d.dev_only).collect()
    }

    /// Deviations forbidden by the selected fail-closed posture. Prelaunch
    /// permits only its explicit fixed play-money grant.
    pub fn blocking_violations(&self) -> Vec<&Deviation> {
        self.violations()
            .into_iter()
            .filter(|deviation| !self.profile.permits_deviation(deviation))
            .collect()
    }

    pub fn blocks_start(&self, allow_dev_knobs: bool) -> bool {
        self.profile.fail_closed() && !allow_dev_knobs && !self.blocking_violations().is_empty()
    }
}

fn is_set(value: &str) -> bool {
    !value.trim().is_empty()
}

/// Compare the config against the prod-intended baseline and collect every
/// divergence. The baseline mirrors the effective `docker-compose.prod.yml`
/// posture (see `docs/architecture/Deployment Profiles.md`).
///
/// `dev_only` classification (blocks prod):
/// - `SYBIL_DEV_MODE=true` — mounts dev routes, permissive CORS, skips the
///   service bearer check. Trust-boundary breach.
/// - `SYBIL_SERVICE_TOKEN` unset — service/operator writes cannot be
///   authenticated; fail closed (mirrors [`crate::app`] request-time posture,
///   promoted to startup).
/// - `SYBIL_DATA_DIR` unset — in-memory only; the whole store (state, equity,
///   canonical state and the product-history outbox are lost on restart.
/// - `SYBIL_HISTORY_URL` unset — product-history reads and outbox delivery are
///   disabled.
/// - `SYBIL_HISTORY_TOKEN` unset — the private history boundary is unauthenticated.
/// - `SYBIL_ADMIN_FEED_KEY_PATH` unset — the admin resolution feed identity is
///   regenerated on every restart, so the configured signer is not durable.
/// - `SYBIL_PUBLIC_ACCOUNT_GRANT_NANOS` nonzero — anonymous play-money minting
///   must not enter a real-value production balance domain.
///
/// Informational-only deviations (logged, never block):
/// - `SYBIL_RECENT_BLOCK_CACHE_CAPACITY` — recent canonical-block cache size.
/// - `SYBIL_MARKET_REF_DATA_PATH` — unset means volatile mirror metadata.
///   Degraded but not data loss for trading; flagged for operator attention.
pub fn collect_deviations(config: &ApiConfig) -> Vec<Deviation> {
    let mut out = Vec::new();

    if config.dev_mode {
        out.push(Deviation {
            knob: "SYBIL_DEV_MODE",
            value: "true".to_string(),
            prod_intended: "false",
            dev_only: true,
        });
    }
    if config.public_account_grant_nanos != 0 {
        out.push(Deviation {
            knob: "SYBIL_PUBLIC_ACCOUNT_GRANT_NANOS",
            value: config.public_account_grant_nanos.to_string(),
            prod_intended: "0",
            dev_only: true,
        });
    }
    if config.webauthn_rp_id != sybil_verifier::key_op_auth::EXPECTED_WEBAUTHN_RP_ID {
        out.push(Deviation {
            knob: "SYBIL_WEBAUTHN_RP_ID",
            value: config.webauthn_rp_id.clone(),
            prod_intended: sybil_verifier::key_op_auth::EXPECTED_WEBAUTHN_RP_ID,
            dev_only: true,
        });
    }
    if config.webauthn_origin != sybil_verifier::key_op_auth::EXPECTED_WEBAUTHN_ORIGIN {
        out.push(Deviation {
            knob: "SYBIL_WEBAUTHN_ORIGIN",
            value: config.webauthn_origin.clone(),
            prod_intended: sybil_verifier::key_op_auth::EXPECTED_WEBAUTHN_ORIGIN,
            dev_only: true,
        });
    }
    if !config.webauthn_require_uv {
        out.push(Deviation {
            knob: "SYBIL_WEBAUTHN_REQUIRE_UV",
            value: "false".to_string(),
            prod_intended: "true",
            dev_only: true,
        });
    }
    if !is_set(&config.service_token) {
        out.push(Deviation {
            knob: "SYBIL_SERVICE_TOKEN",
            value: "<unset>".to_string(),
            prod_intended: "<set>",
            dev_only: true,
        });
    }
    if !is_set(&config.data_dir) {
        out.push(Deviation {
            knob: "SYBIL_DATA_DIR",
            value: "<unset> (in-memory, no persistence)".to_string(),
            prod_intended: "<set>",
            dev_only: true,
        });
    }
    if !is_set(&config.history_url) {
        out.push(Deviation {
            knob: "SYBIL_HISTORY_URL",
            value: "<unset> (history delivery and reads disabled)".to_string(),
            prod_intended: "<set>",
            dev_only: true,
        });
    }
    if !is_set(&config.history_token) {
        out.push(Deviation {
            knob: "SYBIL_HISTORY_TOKEN",
            value: "<unset> (private history boundary unauthenticated)".to_string(),
            prod_intended: "<set>",
            dev_only: true,
        });
    }
    if !is_set(&config.arena_read_url) {
        out.push(Deviation {
            knob: "SYBIL_ARENA_READ_URL",
            value: "<unset> (bot analytics disabled)".to_string(),
            prod_intended: "<set>",
            dev_only: true,
        });
    }
    if !is_set(&config.arena_read_token) {
        out.push(Deviation {
            knob: "SYBIL_ARENA_READ_TOKEN",
            value: "<unset> (private Arena boundary unauthenticated)".to_string(),
            prod_intended: "<set>",
            dev_only: true,
        });
    }
    if config.recent_block_cache_capacity != 100 {
        out.push(Deviation {
            knob: "SYBIL_RECENT_BLOCK_CACHE_CAPACITY",
            value: config.recent_block_cache_capacity.to_string(),
            prod_intended: "100",
            dev_only: false,
        });
    }
    if config.ws_client_idle_timeout_ms != 90_000 {
        out.push(Deviation {
            knob: "SYBIL_WS_CLIENT_IDLE_TIMEOUT_MS",
            value: config.ws_client_idle_timeout_ms.to_string(),
            prod_intended: "90000",
            dev_only: false,
        });
    }
    if !is_set(&config.market_ref_data_path) {
        out.push(Deviation {
            knob: "SYBIL_MARKET_REF_DATA_PATH",
            value: "<unset> (volatile mirror metadata)".to_string(),
            prod_intended: "<set>",
            dev_only: false,
        });
    }
    if !is_set(&config.admin_feed_key_path) {
        out.push(Deviation {
            knob: "SYBIL_ADMIN_FEED_KEY_PATH",
            value: "<unset> (ephemeral admin key)".to_string(),
            prod_intended: "<set>",
            dev_only: true,
        });
    }

    out
}

/// Build the preflight report. Errors only if the profile string is invalid.
pub fn build_report(config: &ApiConfig) -> Result<PreflightReport, String> {
    let profile = DeploymentProfile::parse(&config.deployment_profile)?;
    Ok(PreflightReport {
        profile,
        deviations: collect_deviations(config),
    })
}

/// Emit the one structured startup log block (criterion 2): active profile plus
/// every knob diverging from the prod-intended baseline.
pub fn log_report(report: &PreflightReport) {
    let summary = if report.deviations.is_empty() {
        "none".to_string()
    } else {
        report
            .deviations
            .iter()
            .map(|d| {
                format!(
                    "{}={} (prod={}{})",
                    d.knob,
                    d.value,
                    d.prod_intended,
                    if d.dev_only { ", DEV-ONLY" } else { "" }
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    };
    tracing::info!(
        deployment_profile = report.profile.as_str(),
        deviation_count = report.deviations.len(),
        dev_only_count = report.violations().len(),
        deviations = %summary,
        "deployment profile preflight"
    );
}

/// Run the full preflight: build the report, log it, and enforce the
/// locked-profile fail-closed guardrail (criterion 3).
///
/// Returns `Err` with a human-readable message when a locked start must be
/// refused. On the `SYBIL_ALLOW_DEV_KNOBS=1` override, logs a loud error and
/// returns `Ok`.
pub fn run_preflight(config: &ApiConfig) -> Result<(), String> {
    if config.public_account_grant_nanos > i64::MAX as u64 {
        return Err(format!(
            "SYBIL_PUBLIC_ACCOUNT_GRANT_NANOS={} exceeds the signed account-balance range",
            config.public_account_grant_nanos
        ));
    }
    config.bridge_domain()?;
    let report = build_report(config)?;
    log_report(&report);

    if report.profile == DeploymentProfile::Prelaunch {
        if config.public_account_capacity == 0 {
            return Err("prelaunch requires SYBIL_PUBLIC_ACCOUNT_CAPACITY > 0".to_string());
        }
        if config.public_account_grant_nanos == 0 {
            return Err("prelaunch requires SYBIL_PUBLIC_ACCOUNT_GRANT_NANOS > 0".to_string());
        }
    }

    if !report.profile.fail_closed() {
        return Ok(());
    }

    let violations = report.blocking_violations();
    if violations.is_empty() {
        return Ok(());
    }

    let listed = violations
        .iter()
        .map(|d| format!("{}={}", d.knob, d.value))
        .collect::<Vec<_>>()
        .join(", ");

    if config.allow_dev_knobs {
        tracing::error!(
            dev_only_knobs = %listed,
            deployment_profile = report.profile.as_str(),
            "SYBIL_ALLOW_DEV_KNOBS override active: starting a locked profile with forbidden dev-only knobs set — NOT a safe steady state"
        );
        return Ok(());
    }

    Err(format!(
        "refusing to start with SYBIL_DEPLOYMENT_PROFILE={}: forbidden dev-only knobs are set [{listed}]. \
         Fix the configuration, or set SYBIL_ALLOW_DEV_KNOBS=1 to override (loudly, at your own risk).",
        report.profile.as_str()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An `ApiConfig` that passes a prod preflight cleanly: durable, locked
    /// down, with the canonical block cache at its prod-intended value.
    fn prod_ready_config() -> ApiConfig {
        ApiConfig {
            deployment_profile: "prod".to_string(),
            dev_mode: false,
            service_token: "tok".to_string(),
            history_url: "http://sybil-history:3003".to_string(),
            history_token: "history-tok".to_string(),
            arena_read_url: "http://sybil-arena:9103".to_string(),
            arena_read_token: "arena-tok".to_string(),
            data_dir: "/data".to_string(),
            market_ref_data_path: "/data/ref.json".to_string(),
            admin_feed_key_path: "/data/admin.key".to_string(),
            webauthn_rp_id: sybil_verifier::key_op_auth::EXPECTED_WEBAUTHN_RP_ID.to_string(),
            webauthn_origin: sybil_verifier::key_op_auth::EXPECTED_WEBAUTHN_ORIGIN.to_string(),
            webauthn_require_uv: true,
            public_account_grant_nanos: 0,
            recent_block_cache_capacity: 100,
            ..ApiConfig::default()
        }
    }

    #[test]
    fn parse_accepts_known_profiles_case_insensitively() {
        assert_eq!(
            DeploymentProfile::parse("local"),
            Ok(DeploymentProfile::Local)
        );
        assert_eq!(
            DeploymentProfile::parse("  DEVNET "),
            Ok(DeploymentProfile::Devnet)
        );
        assert_eq!(
            DeploymentProfile::parse("prelaunch"),
            Ok(DeploymentProfile::Prelaunch)
        );
        assert_eq!(
            DeploymentProfile::parse("Prod"),
            Ok(DeploymentProfile::Prod)
        );
        assert_eq!(
            DeploymentProfile::parse("production"),
            Ok(DeploymentProfile::Prod)
        );
        assert!(DeploymentProfile::parse("private-devnet").is_err());
        assert!(DeploymentProfile::parse("staging").is_err());
    }

    #[test]
    fn prod_ready_config_has_no_dev_only_violations() {
        let report = build_report(&prod_ready_config()).unwrap();
        assert!(
            report.violations().is_empty(),
            "unexpected violations: {:?}",
            report.violations()
        );
        assert!(!report.blocks_start(false));
        assert!(run_preflight(&prod_ready_config()).is_ok());
    }

    #[test]
    fn dev_mode_blocks_prod_start() {
        let config = ApiConfig {
            dev_mode: true,
            ..prod_ready_config()
        };
        let report = build_report(&config).unwrap();
        assert!(
            report
                .violations()
                .iter()
                .any(|d| d.knob == "SYBIL_DEV_MODE")
        );
        assert!(report.blocks_start(false));
        assert!(run_preflight(&config).is_err());
    }

    #[test]
    fn webauthn_guest_pin_mismatch_blocks_prod_start() {
        for config in [
            ApiConfig {
                webauthn_rp_id: "example.com".to_string(),
                ..prod_ready_config()
            },
            ApiConfig {
                webauthn_origin: "https://example.com".to_string(),
                ..prod_ready_config()
            },
            ApiConfig {
                webauthn_require_uv: false,
                ..prod_ready_config()
            },
        ] {
            assert!(run_preflight(&config).is_err());
        }
    }

    #[test]
    fn missing_service_token_blocks_prod_start() {
        let config = ApiConfig {
            service_token: String::new(),
            ..prod_ready_config()
        };
        assert!(build_report(&config).unwrap().blocks_start(false));
        assert!(run_preflight(&config).is_err());
    }

    #[test]
    fn in_memory_store_blocks_prod_start() {
        let config = ApiConfig {
            data_dir: String::new(),
            ..prod_ready_config()
        };
        let report = build_report(&config).unwrap();
        assert!(
            report
                .violations()
                .iter()
                .any(|d| d.knob == "SYBIL_DATA_DIR")
        );
        assert!(run_preflight(&config).is_err());
    }

    #[test]
    fn missing_admin_feed_key_path_blocks_prod_start() {
        let config = ApiConfig {
            admin_feed_key_path: String::new(),
            ..prod_ready_config()
        };
        let report = build_report(&config).unwrap();
        assert!(
            report
                .violations()
                .iter()
                .any(|d| d.knob == "SYBIL_ADMIN_FEED_KEY_PATH")
        );
        assert!(report.blocks_start(false));
        assert!(run_preflight(&config).is_err());
    }

    #[test]
    fn allow_dev_knobs_override_lets_prod_start_with_violations() {
        let config = ApiConfig {
            dev_mode: true,
            allow_dev_knobs: true,
            ..prod_ready_config()
        };
        let report = build_report(&config).unwrap();
        assert!(!report.violations().is_empty());
        // The report still records the violation; only the override gates start.
        assert!(!report.blocks_start(true));
        assert!(run_preflight(&config).is_ok());
    }

    #[test]
    fn devnet_profile_never_blocks_even_with_dev_knobs() {
        let config = ApiConfig {
            deployment_profile: "devnet".to_string(),
            dev_mode: true,
            service_token: String::new(),
            data_dir: String::new(),
            ..ApiConfig::default()
        };
        let report = build_report(&config).unwrap();
        assert_eq!(report.profile, DeploymentProfile::Devnet);
        // Deviations are still surfaced for the log block…
        assert!(!report.violations().is_empty());
        // …but only locked profiles fail-close.
        assert!(!report.blocks_start(false));
        assert!(run_preflight(&config).is_ok());
    }

    #[test]
    fn prelaunch_requires_and_permits_only_a_fixed_public_grant() {
        let config = ApiConfig {
            deployment_profile: "prelaunch".to_string(),
            public_account_capacity: 1000,
            public_account_grant_nanos: 1_000_000_000_000,
            ..prod_ready_config()
        };
        let report = build_report(&config).unwrap();
        assert_eq!(report.profile, DeploymentProfile::Prelaunch);
        assert_eq!(report.blocking_violations(), Vec::<&Deviation>::new());
        assert!(!report.blocks_start(false));
        assert!(run_preflight(&config).is_ok());
    }

    #[test]
    fn prelaunch_rejects_empty_onboarding_and_other_dev_knobs() {
        for config in [
            ApiConfig {
                deployment_profile: "prelaunch".to_string(),
                public_account_capacity: 0,
                public_account_grant_nanos: 1,
                ..prod_ready_config()
            },
            ApiConfig {
                deployment_profile: "prelaunch".to_string(),
                public_account_capacity: 1000,
                public_account_grant_nanos: 0,
                ..prod_ready_config()
            },
            ApiConfig {
                deployment_profile: "prelaunch".to_string(),
                public_account_capacity: 1000,
                public_account_grant_nanos: 1,
                dev_mode: true,
                ..prod_ready_config()
            },
        ] {
            assert!(run_preflight(&config).is_err());
        }
    }

    #[test]
    fn invalid_profile_is_rejected() {
        let config = ApiConfig {
            deployment_profile: "staging".to_string(),
            ..ApiConfig::default()
        };
        assert!(build_report(&config).is_err());
        assert!(run_preflight(&config).is_err());
    }

    #[test]
    fn local_default_config_starts_clean() {
        // The zero-config developer path must never be blocked.
        assert!(run_preflight(&ApiConfig::default()).is_ok());
    }

    #[test]
    fn public_grant_must_fit_the_signed_account_balance() {
        let config = ApiConfig {
            public_account_grant_nanos: i64::MAX as u64 + 1,
            ..ApiConfig::default()
        };
        let error = run_preflight(&config).unwrap_err();
        assert!(error.contains("SYBIL_PUBLIC_ACCOUNT_GRANT_NANOS"));
        assert!(error.contains("signed account-balance range"));
    }

    #[test]
    fn public_play_money_grant_blocks_prod_start() {
        let config = ApiConfig {
            public_account_grant_nanos: 1,
            ..prod_ready_config()
        };
        let report = build_report(&config).unwrap();
        assert!(
            report
                .violations()
                .iter()
                .any(|deviation| deviation.knob == "SYBIL_PUBLIC_ACCOUNT_GRANT_NANOS")
        );
        assert!(run_preflight(&config).is_err());
    }
}
