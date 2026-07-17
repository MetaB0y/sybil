use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use sybil_market_maker::MmProgress;
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
    tracked_markets: usize,
    eligible_quote_markets: usize,
    quoted_markets: usize,
    quote_orders: usize,
    quote_capacity_limited: bool,
    last_observed_block: Option<u64>,
    last_completed_quote_block: Option<u64>,
    last_successful_submission_block: Option<u64>,
    last_progress_timestamp_ms: u64,
    progress_age_ms: Option<u64>,
    successful_submissions: u64,
    failed_submissions: u64,
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
        let status = if progress.last_completed_quote_block.is_none() {
            "starting"
        } else if progress.tracked_markets == 0 {
            "no_markets"
        } else if progress_age_ms.is_some_and(|age| age > self.stale_after_ms) {
            "stalled"
        } else {
            "ok"
        };
        ReadinessResponse {
            status,
            ready: status == "ok",
            tracked_markets: progress.tracked_markets,
            eligible_quote_markets: progress.last_eligible_quote_markets,
            quoted_markets: progress.last_quoted_markets,
            quote_orders: progress.last_quote_orders,
            quote_capacity_limited: progress.quote_capacity_limited,
            last_observed_block: progress.last_observed_block,
            last_completed_quote_block: progress.last_completed_quote_block,
            last_successful_submission_block: progress.last_successful_submission_block,
            last_progress_timestamp_ms: progress.last_progress_timestamp_ms,
            progress_age_ms,
            successful_submissions: progress.successful_submissions,
            failed_submissions: progress.failed_submissions,
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
            "sybil_native_mm_quote_orders",
            "Orders included in the latest native MM quote set.",
            progress.last_quote_orders.try_into().unwrap_or(u64::MAX),
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
            "sybil_native_mm_last_completed_quote_block",
            "Latest live block whose native quote cycle completed.",
            progress.last_completed_quote_block,
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
            "sybil_native_mm_submissions_failed_total",
            "Native MM IOC bundle submission failures in this process.",
            progress.failed_submissions,
        );
        out
    }
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
            last_observed_block: Some(7),
            last_completed_quote_block: Some(7),
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
        });
        let metrics = MonitoringState::new(rx, Duration::from_secs(60)).render();
        assert!(metrics.contains("sybil_native_mm_tracked_markets 4"));
        assert!(metrics.contains("sybil_native_mm_last_completed_quote_block 11"));
        assert!(metrics.contains("sybil_native_mm_eligible_quote_markets 4"));
        assert!(metrics.contains("sybil_native_mm_quoted_markets 3"));
        assert!(metrics.contains("sybil_native_mm_quote_orders 6"));
        assert!(metrics.contains("sybil_native_mm_quote_capacity_limited 1"));
        assert!(metrics.contains("sybil_native_mm_quote_capacity_limited_cycles_total 2"));
        assert!(metrics.contains("sybil_native_mm_submissions_success_total 9"));
        assert!(metrics.contains("sybil_native_mm_submissions_failed_total 2"));
    }
}
