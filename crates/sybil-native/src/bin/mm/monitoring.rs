use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use sybil_market_maker::{MmMode, MmProgress};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(super) struct MonitoringState {
    progress: watch::Receiver<MmProgress>,
    stale_after_ms: u64,
    started_timestamp_ms: u64,
}

#[derive(Debug, Serialize)]
struct ReadinessResponse {
    status: &'static str,
    ready: bool,
    mode: &'static str,
    tracked_markets: usize,
    eligible_quote_markets: usize,
    quoted_markets: usize,
    two_sided_quote_markets: usize,
    quote_orders: usize,
    compaction_markets: usize,
    compaction_quantity_units: u64,
    missing_price_markets: usize,
    stale_price_markets: usize,
    out_of_band_markets: usize,
    quote_capacity_limited: bool,
    last_observed_block: Option<u64>,
    last_completed_quote_block: Option<u64>,
    last_submission_attempt_block: Option<u64>,
    last_successful_submission_block: Option<u64>,
    last_compaction_attempt_block: Option<u64>,
    last_successful_compaction_block: Option<u64>,
    submission_lag_blocks: Option<u64>,
    last_progress_timestamp_ms: u64,
    progress_age_ms: Option<u64>,
    successful_submissions: u64,
    failed_submissions: u64,
    successful_compactions: u64,
    failed_compactions: u64,
}

impl MonitoringState {
    pub(super) fn new(progress: watch::Receiver<MmProgress>, stale_after: Duration) -> Self {
        Self {
            progress,
            stale_after_ms: stale_after.as_millis().try_into().unwrap_or(u64::MAX),
            started_timestamp_ms: now_ms(),
        }
    }

    fn readiness_at(&self, now_ms: u64) -> ReadinessResponse {
        let progress = self.progress.borrow().clone();
        let progress_age_ms = (progress.last_progress_timestamp_ms != 0)
            .then(|| now_ms.saturating_sub(progress.last_progress_timestamp_ms));
        let submission_lag_blocks = submission_lag_blocks(&progress);
        let status = if progress.last_completed_quote_block.is_none() {
            "starting"
        } else if progress.tracked_markets == 0 {
            "no_markets"
        } else if progress_age_ms.is_some_and(|age| age > self.stale_after_ms) {
            "stalled"
        } else if progress.last_eligible_quote_markets == 0 {
            "no_eligible_markets"
        } else if progress.last_quoted_markets == 0 {
            "no_quotes"
        } else if below_coverage_bar(
            progress.last_quoted_markets,
            progress.last_eligible_quote_markets,
        ) {
            "partial_coverage"
        } else if progress.mode == MmMode::Normal
            && below_coverage_bar(
                progress.last_two_sided_quote_markets,
                progress.last_eligible_quote_markets,
            )
        {
            "partial_two_sided_coverage"
        } else if submission_is_stalled(&progress) {
            "submission_stalled"
        } else if progress.mode == MmMode::ReduceOnly {
            "reduce_only"
        } else {
            "ok"
        };
        ReadinessResponse {
            status,
            ready: status == "ok",
            mode: mode_name(progress.mode),
            tracked_markets: progress.tracked_markets,
            eligible_quote_markets: progress.last_eligible_quote_markets,
            quoted_markets: progress.last_quoted_markets,
            two_sided_quote_markets: progress.last_two_sided_quote_markets,
            quote_orders: progress.last_quote_orders,
            compaction_markets: progress.last_compaction_markets,
            compaction_quantity_units: progress.last_compaction_quantity_units,
            missing_price_markets: progress.last_missing_price_markets,
            stale_price_markets: progress.last_stale_price_markets,
            out_of_band_markets: progress.last_out_of_band_markets,
            quote_capacity_limited: progress.quote_capacity_limited,
            last_observed_block: progress.last_observed_block,
            last_completed_quote_block: progress.last_completed_quote_block,
            last_submission_attempt_block: progress.last_submission_attempt_block,
            last_successful_submission_block: progress.last_successful_submission_block,
            last_compaction_attempt_block: progress.last_compaction_attempt_block,
            last_successful_compaction_block: progress.last_successful_compaction_block,
            submission_lag_blocks,
            last_progress_timestamp_ms: progress.last_progress_timestamp_ms,
            progress_age_ms,
            successful_submissions: progress.successful_submissions,
            failed_submissions: progress.failed_submissions,
            successful_compactions: progress.successful_compactions,
            failed_compactions: progress.failed_compactions,
        }
    }

    fn render(&self) -> String {
        let progress = self.progress.borrow().clone();
        let ready = self.readiness_at(now_ms()).ready;
        let mut out = String::with_capacity(2_000);
        gauge(
            &mut out,
            "sybil_native_mm_ready",
            "Whether the native MM has markets and recently completed a live quote cycle.",
            u64::from(ready),
        );
        mode_metric(&mut out, "sybil_native_mm_mode", progress.mode);
        gauge(
            &mut out,
            "sybil_native_mm_tracked_markets",
            "Number of native markets currently tracked by the MM actor.",
            progress.tracked_markets.try_into().unwrap_or(u64::MAX),
        );
        gauge(
            &mut out,
            "sybil_native_mm_eligible_quote_markets",
            "Markets eligible for quotes in the latest completed native MM cycle.",
            progress
                .last_eligible_quote_markets
                .try_into()
                .unwrap_or(u64::MAX),
        );
        gauge(
            &mut out,
            "sybil_native_mm_quoted_markets",
            "Distinct markets included in the latest native MM quote set.",
            progress.last_quoted_markets.try_into().unwrap_or(u64::MAX),
        );
        gauge(
            &mut out,
            "sybil_native_mm_two_sided_quote_markets",
            "Markets with both cash-backed YES and NO quotes in the latest native MM cycle.",
            progress
                .last_two_sided_quote_markets
                .try_into()
                .unwrap_or(u64::MAX),
        );
        gauge(
            &mut out,
            "sybil_native_mm_quote_orders",
            "Orders included in the latest native MM quote set.",
            progress.last_quote_orders.try_into().unwrap_or(u64::MAX),
        );
        gauge(
            &mut out,
            "sybil_native_mm_compaction_markets",
            "Markets with a paired complete-set redemption request in the latest native MM cycle.",
            progress
                .last_compaction_markets
                .try_into()
                .unwrap_or(u64::MAX),
        );
        gauge(
            &mut out,
            "sybil_native_mm_compaction_quantity_units",
            "Complete-set share-units submitted for redemption in the latest native MM cycle.",
            progress.last_compaction_quantity_units,
        );
        reason_metrics(
            &mut out,
            "sybil_native_mm_ineligible_markets",
            progress.last_missing_price_markets,
            progress.last_stale_price_markets,
            progress.last_out_of_band_markets,
        );
        gauge(
            &mut out,
            "sybil_native_mm_quote_capacity_limited",
            "Whether the latest native MM cycle omitted eligible markets after reaching its order cap.",
            u64::from(progress.quote_capacity_limited),
        );
        counter(
            &mut out,
            "sybil_native_mm_quote_capacity_limited_cycles_total",
            "Native MM quote cycles that rotated partial coverage after reaching the order cap.",
            progress.capacity_limited_quote_cycles,
        );
        gauge(
            &mut out,
            "sybil_native_mm_started_timestamp_seconds",
            "Unix timestamp when the native MM monitoring process started.",
            self.started_timestamp_ms / 1_000,
        );
        optional_gauge(
            &mut out,
            "sybil_native_mm_last_observed_block",
            "Latest live block observed by the MM actor.",
            progress.last_observed_block,
        );
        optional_gauge(
            &mut out,
            "sybil_native_mm_last_compaction_attempt_block",
            "Latest block on which the native MM attempted complete-set redemption.",
            progress.last_compaction_attempt_block,
        );
        optional_gauge(
            &mut out,
            "sybil_native_mm_last_successful_compaction_block",
            "Latest block on which the API accepted native MM complete-set redemption.",
            progress.last_successful_compaction_block,
        );
        optional_gauge(
            &mut out,
            "sybil_native_mm_last_submission_attempt_block",
            "Latest block on which the native MM attempted to submit a non-empty IOC bundle.",
            progress.last_submission_attempt_block,
        );
        optional_gauge(
            &mut out,
            "sybil_native_mm_last_completed_quote_block",
            "Latest live block whose native quote cycle completed.",
            progress.last_completed_quote_block,
        );
        optional_gauge(
            &mut out,
            "sybil_native_mm_submission_lag_blocks",
            "Observed block height minus the latest successful native MM submission block.",
            submission_lag_blocks(&progress),
        );
        optional_gauge(
            &mut out,
            "sybil_native_mm_last_successful_submission_block",
            "Latest live block on which the API accepted a native MM IOC bundle.",
            progress.last_successful_submission_block,
        );
        gauge(
            &mut out,
            "sybil_native_mm_last_progress_timestamp_seconds",
            "Unix timestamp when the latest native quote cycle completed.",
            progress.last_progress_timestamp_ms / 1_000,
        );
        counter(
            &mut out,
            "sybil_native_mm_submissions_success_total",
            "Native MM IOC bundles accepted by the API in this process.",
            progress.successful_submissions,
        );
        counter(
            &mut out,
            "sybil_native_mm_compactions_success_total",
            "Native MM complete-set redemption bundles accepted by the API.",
            progress.successful_compactions,
        );
        counter(
            &mut out,
            "sybil_native_mm_compactions_failed_total",
            "Native MM complete-set redemption bundle submission failures.",
            progress.failed_compactions,
        );
        counter(
            &mut out,
            "sybil_native_mm_submissions_failed_total",
            "Native MM IOC bundle submission failures in this process.",
            progress.failed_submissions,
        );
        gauge(
            &mut out,
            "sybil_native_mm_paired_position_units",
            "Redeemable YES+NO complete-set inventory in protocol share-units.",
            progress.paired_position_units,
        );
        gauge(
            &mut out,
            "sybil_native_mm_directional_position_units",
            "Absolute unpaired inventory across markets in protocol share-units.",
            progress.directional_position_units,
        );
        gauge(
            &mut out,
            "sybil_native_mm_directional_exposure_nanos",
            "Reference-marked value of directional inventory in nanodollars.",
            progress.directional_exposure_nanos,
        );
        signed_gauge(
            &mut out,
            "sybil_native_mm_balance_nanos",
            "Native MM cash balance in nanodollars.",
            progress.balance_nanos,
        );
        signed_gauge(
            &mut out,
            "sybil_native_mm_total_deposited_nanos",
            "Native MM cumulative deposits in nanodollars.",
            progress.total_deposited_nanos,
        );
        signed_gauge(
            &mut out,
            "sybil_native_mm_portfolio_value_nanos",
            "Native MM marked portfolio value in nanodollars.",
            progress.portfolio_value_nanos,
        );
        signed_gauge(
            &mut out,
            "sybil_native_mm_pnl_nanos",
            "Native MM marked PnL in nanodollars.",
            progress.pnl_nanos,
        );
        signed_gauge(
            &mut out,
            "sybil_native_mm_conservative_floor_nanos",
            "Native MM cash plus redeemable binary complete sets in nanodollars.",
            progress.conservative_floor_nanos,
        );
        signed_gauge(
            &mut out,
            "sybil_native_mm_worst_case_drawdown_nanos",
            "Deposits minus cash and redeemable binary complete sets, floored at zero.",
            progress.worst_case_drawdown_nanos,
        );
        out
    }
}

fn below_coverage_bar(covered: usize, eligible: usize) -> bool {
    covered.saturating_mul(100) < eligible.saturating_mul(95)
}

pub(super) async fn serve(
    listener: TcpListener,
    state: MonitoringState,
    cancel: CancellationToken,
) -> std::io::Result<()> {
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .with_state(state);
    axum::serve(listener, app)
        .with_graceful_shutdown(cancel.cancelled_owned())
        .await
}

async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn readyz(State(state): State<MonitoringState>) -> (StatusCode, Json<ReadinessResponse>) {
    let response = state.readiness_at(now_ms());
    let status = if response.ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(response))
}

async fn metrics(State(state): State<MonitoringState>) -> String {
    state.render()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn gauge(out: &mut String, name: &str, help: &str, value: u64) {
    out.push_str("# HELP ");
    out.push_str(name);
    out.push(' ');
    out.push_str(help);
    out.push_str("\n# TYPE ");
    out.push_str(name);
    out.push_str(" gauge\n");
    out.push_str(name);
    out.push(' ');
    out.push_str(&value.to_string());
    out.push('\n');
}

fn optional_gauge(out: &mut String, name: &str, help: &str, value: Option<u64>) {
    out.push_str("# HELP ");
    out.push_str(name);
    out.push(' ');
    out.push_str(help);
    out.push_str("\n# TYPE ");
    out.push_str(name);
    out.push_str(" gauge\n");
    if let Some(value) = value {
        out.push_str(name);
        out.push(' ');
        out.push_str(&value.to_string());
        out.push('\n');
    }
}

fn signed_gauge(out: &mut String, name: &str, help: &str, value: i64) {
    out.push_str("# HELP ");
    out.push_str(name);
    out.push(' ');
    out.push_str(help);
    out.push_str("\n# TYPE ");
    out.push_str(name);
    out.push_str(" gauge\n");
    out.push_str(name);
    out.push(' ');
    out.push_str(&value.to_string());
    out.push('\n');
}

fn mode_name(mode: MmMode) -> &'static str {
    match mode {
        MmMode::Normal => "normal",
        MmMode::ReduceOnly => "reduce_only",
    }
}

fn mode_metric(out: &mut String, name: &str, mode: MmMode) {
    out.push_str("# HELP ");
    out.push_str(name);
    out.push_str(" Active market-maker risk mode.\n# TYPE ");
    out.push_str(name);
    out.push_str(" gauge\n");
    out.push_str(name);
    out.push_str("{mode=\"");
    out.push_str(mode_name(mode));
    out.push_str("\"} 1\n");
}

fn reason_metrics(out: &mut String, name: &str, missing: usize, stale: usize, out_of_band: usize) {
    out.push_str("# HELP ");
    out.push_str(name);
    out.push_str(" Tracked markets excluded from quoting by reason.\n# TYPE ");
    out.push_str(name);
    out.push_str(" gauge\n");
    for (reason, value) in [
        ("missing_price", missing),
        ("stale_price", stale),
        ("out_of_band", out_of_band),
    ] {
        out.push_str(name);
        out.push_str("{reason=\"");
        out.push_str(reason);
        out.push_str("\"} ");
        out.push_str(&value.to_string());
        out.push('\n');
    }
}

fn submission_lag_blocks(progress: &MmProgress) -> Option<u64> {
    progress
        .last_observed_block
        .zip(progress.last_successful_submission_block)
        .map(|(observed, successful)| observed.saturating_sub(successful))
}

fn submission_is_stalled(progress: &MmProgress) -> bool {
    if progress.last_quote_orders == 0 || progress.last_submission_attempt_block.is_none() {
        return false;
    }
    match submission_lag_blocks(progress) {
        Some(lag) => lag > 2,
        None => true,
    }
}

fn counter(out: &mut String, name: &str, help: &str, value: u64) {
    out.push_str("# HELP ");
    out.push_str(name);
    out.push(' ');
    out.push_str(help);
    out.push_str("\n# TYPE ");
    out.push_str(name);
    out.push_str(" counter\n");
    out.push_str(name);
    out.push(' ');
    out.push_str(&value.to_string());
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_requires_fresh_completed_progress() {
        let (tx, rx) = watch::channel(MmProgress::default());
        let state = MonitoringState {
            progress: rx,
            stale_after_ms: 60_000,
            started_timestamp_ms: 1_000,
        };
        assert_eq!(state.readiness_at(1_000).status, "starting");

        tx.send(MmProgress {
            tracked_markets: 3,
            last_eligible_quote_markets: 3,
            last_quoted_markets: 3,
            last_two_sided_quote_markets: 3,
            last_quote_orders: 6,
            last_observed_block: Some(7),
            last_completed_quote_block: Some(7),
            last_submission_attempt_block: Some(7),
            last_successful_submission_block: Some(7),
            successful_submissions: 1,
            failed_submissions: 0,
            last_progress_timestamp_ms: 2_000,
            ..MmProgress::default()
        })
        .unwrap();
        assert_eq!(state.readiness_at(61_999).status, "ok");
        assert_eq!(state.readiness_at(62_001).status, "stalled");

        tx.send(MmProgress {
            tracked_markets: 0,
            last_completed_quote_block: Some(8),
            last_progress_timestamp_ms: 62_000,
            ..MmProgress::default()
        })
        .unwrap();
        assert_eq!(state.readiness_at(62_001).status, "no_markets");
    }

    #[test]
    fn prometheus_output_exposes_actor_progress_and_outcomes() {
        let (_tx, rx) = watch::channel(MmProgress {
            tracked_markets: 4,
            last_eligible_quote_markets: 4,
            last_quoted_markets: 3,
            last_quote_orders: 6,
            quote_capacity_limited: true,
            capacity_limited_quote_cycles: 2,
            last_observed_block: Some(12),
            last_completed_quote_block: Some(11),
            last_successful_submission_block: Some(11),
            successful_submissions: 9,
            failed_submissions: 2,
            last_progress_timestamp_ms: now_ms(),
            ..MmProgress::default()
        });
        let metrics = MonitoringState::new(rx, Duration::from_secs(60)).render();
        assert!(metrics.contains("sybil_native_mm_tracked_markets 4"));
        assert!(metrics.contains("sybil_native_mm_last_completed_quote_block 11"));
        assert!(metrics.contains("sybil_native_mm_eligible_quote_markets 4"));
        assert!(metrics.contains("sybil_native_mm_quoted_markets 3"));
        assert!(metrics.contains("sybil_native_mm_two_sided_quote_markets 0"));
        assert!(metrics.contains("sybil_native_mm_quote_orders 6"));
        assert!(metrics.contains("sybil_native_mm_quote_capacity_limited 1"));
        assert!(metrics.contains("sybil_native_mm_quote_capacity_limited_cycles_total 2"));
        assert!(metrics.contains("sybil_native_mm_submissions_success_total 9"));
        assert!(metrics.contains("sybil_native_mm_submissions_failed_total 2"));
        assert!(metrics.contains("sybil_native_mm_mode{mode=\"normal\"} 1"));
        assert!(metrics.contains("sybil_native_mm_paired_position_units 0"));
        assert!(metrics.contains("sybil_native_mm_compactions_failed_total 0"));
    }

    #[test]
    fn readiness_rejects_empty_partial_and_stalled_quote_work() {
        let now = 100_000;
        let healthy = MmProgress {
            tracked_markets: 4,
            last_eligible_quote_markets: 4,
            last_quoted_markets: 4,
            last_two_sided_quote_markets: 4,
            last_quote_orders: 8,
            last_observed_block: Some(20),
            last_completed_quote_block: Some(20),
            last_submission_attempt_block: Some(20),
            last_successful_submission_block: Some(20),
            last_progress_timestamp_ms: now,
            ..MmProgress::default()
        };
        let (_tx, rx) = watch::channel(healthy.clone());
        let mut state = MonitoringState {
            progress: rx,
            stale_after_ms: 60_000,
            started_timestamp_ms: 1,
        };
        assert_eq!(state.readiness_at(now).status, "ok");

        let mut progress = healthy.clone();
        progress.last_quoted_markets = 0;
        progress.last_two_sided_quote_markets = 0;
        progress.last_quote_orders = 0;
        state.progress = watch::channel(progress).1;
        assert_eq!(state.readiness_at(now).status, "no_quotes");

        let mut progress = healthy.clone();
        progress.last_quoted_markets = 3;
        progress.last_two_sided_quote_markets = 3;
        state.progress = watch::channel(progress).1;
        assert_eq!(state.readiness_at(now).status, "partial_coverage");

        let mut progress = healthy.clone();
        progress.tracked_markets = 20;
        progress.last_eligible_quote_markets = 20;
        progress.last_quoted_markets = 19;
        progress.last_two_sided_quote_markets = 19;
        state.progress = watch::channel(progress).1;
        assert_eq!(state.readiness_at(now).status, "ok");

        let mut progress = healthy;
        progress.last_observed_block = Some(24);
        state.progress = watch::channel(progress).1;
        assert_eq!(state.readiness_at(now).status, "submission_stalled");
    }
}
