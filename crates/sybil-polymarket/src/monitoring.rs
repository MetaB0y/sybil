use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use sybil_market_maker::{MmProgress, PriceSnapshot, PriceUpdateSource};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Default)]
struct ActorProgress {
    last_success_timestamp_ms: u64,
    successes: u64,
    failures: u64,
}

impl ActorProgress {
    fn record(&mut self, succeeded: bool, now_ms: u64) {
        if succeeded {
            self.last_success_timestamp_ms = now_ms;
            self.successes = self.successes.saturating_add(1);
        } else {
            self.failures = self.failures.saturating_add(1);
        }
    }
}

#[derive(Debug, Clone, Default)]
struct IntegrationSnapshot {
    sync: ActorProgress,
    feed_refresh: ActorProgress,
    resolution: ActorProgress,
}

/// Write-only progress sink shared by the integration's owning actors.
/// Monitoring reads snapshots; no actor reads this state or coordinates work
/// through it.
#[derive(Debug, Clone, Default)]
pub struct IntegrationProgress {
    inner: Arc<RwLock<IntegrationSnapshot>>,
}

impl IntegrationProgress {
    pub fn record_sync_cycle(&self, succeeded: bool) {
        self.inner
            .write()
            .expect("integration progress lock poisoned")
            .sync
            .record(succeeded, now_ms());
    }

    pub fn record_feed_refresh(&self, succeeded: bool) {
        self.inner
            .write()
            .expect("integration progress lock poisoned")
            .feed_refresh
            .record(succeeded, now_ms());
    }

    pub fn record_resolution_tick(&self, succeeded: bool) {
        self.inner
            .write()
            .expect("integration progress lock poisoned")
            .resolution
            .record(succeeded, now_ms());
    }

    fn snapshot(&self) -> IntegrationSnapshot {
        self.inner
            .read()
            .expect("integration progress lock poisoned")
            .clone()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MonitoringWindows {
    sync_ms: u64,
    feed_ms: u64,
    mm_ms: u64,
    resolution_ms: u64,
}

impl MonitoringWindows {
    pub fn for_cadences(
        sync_interval_secs: u64,
        rest_poll_interval_secs: u64,
        mm_staleness_ms: u64,
        resolution_interval_secs: u64,
    ) -> Self {
        Self {
            sync_ms: cadence_window_ms(sync_interval_secs, 60_000),
            feed_ms: cadence_window_ms(rest_poll_interval_secs, 30_000)
                .max(mm_staleness_ms.saturating_mul(2)),
            mm_ms: 60_000,
            resolution_ms: cadence_window_ms(resolution_interval_secs, 60_000),
        }
    }
}

fn cadence_window_ms(interval_secs: u64, floor_ms: u64) -> u64 {
    interval_secs
        .saturating_mul(3)
        .saturating_mul(1_000)
        .max(floor_ms)
}

#[derive(Clone)]
pub struct MonitoringState {
    integration: IntegrationProgress,
    prices: watch::Receiver<PriceSnapshot>,
    mm: watch::Receiver<MmProgress>,
    windows: MonitoringWindows,
    resolution_enabled: bool,
}

#[derive(Debug, Serialize)]
struct ReadinessResponse {
    status: &'static str,
    ready: bool,
    problems: Vec<&'static str>,
    tracked_tokens: usize,
    tracked_markets: usize,
    last_sync_success_timestamp_ms: u64,
    last_feed_update_timestamp_ms: u64,
    last_mm_progress_timestamp_ms: u64,
    last_resolution_success_timestamp_ms: Option<u64>,
    sync_age_ms: Option<u64>,
    feed_age_ms: Option<u64>,
    mm_age_ms: Option<u64>,
    resolution_age_ms: Option<u64>,
}

impl MonitoringState {
    pub fn new(
        integration: IntegrationProgress,
        prices: watch::Receiver<PriceSnapshot>,
        mm: watch::Receiver<MmProgress>,
        windows: MonitoringWindows,
        resolution_enabled: bool,
    ) -> Self {
        Self {
            integration,
            prices,
            mm,
            windows,
            resolution_enabled,
        }
    }

    fn readiness_at(&self, now_ms: u64) -> ReadinessResponse {
        let integration = self.integration.snapshot();
        let prices = self.prices.borrow().clone();
        let mm = self.mm.borrow().clone();
        let last_feed_update_ms = latest_token_update_ms(&prices);
        let sync_age_ms = age(now_ms, integration.sync.last_success_timestamp_ms);
        let feed_age_ms = age(now_ms, last_feed_update_ms);
        let mm_age_ms = age(now_ms, mm.last_progress_timestamp_ms);
        let resolution_age_ms = self
            .resolution_enabled
            .then(|| age(now_ms, integration.resolution.last_success_timestamp_ms))
            .flatten();

        let mut problems = Vec::new();
        if sync_age_ms.is_some_and(|value| value > self.windows.sync_ms) {
            problems.push("sync_stalled");
        }
        if prices.midpoints.is_empty() {
            problems.push("feed_has_no_prices");
        } else if feed_age_ms.is_some_and(|value| value > self.windows.feed_ms) {
            problems.push("feed_stalled");
        }
        if mm.tracked_markets == 0 {
            problems.push("mm_has_no_markets");
        } else if mm_age_ms.is_some_and(|value| value > self.windows.mm_ms) {
            problems.push("mm_stalled");
        }
        if self.resolution_enabled
            && resolution_age_ms.is_some_and(|value| value > self.windows.resolution_ms)
        {
            problems.push("resolution_stalled");
        }

        let starting = sync_age_ms.is_none()
            || feed_age_ms.is_none()
            || mm_age_ms.is_none()
            || (self.resolution_enabled && resolution_age_ms.is_none());
        let status = if starting {
            "starting"
        } else if problems.is_empty() {
            "ok"
        } else {
            "stalled"
        };
        ReadinessResponse {
            status,
            ready: status == "ok",
            problems,
            tracked_tokens: prices.midpoints.len(),
            tracked_markets: mm.tracked_markets,
            last_sync_success_timestamp_ms: integration.sync.last_success_timestamp_ms,
            last_feed_update_timestamp_ms: last_feed_update_ms,
            last_mm_progress_timestamp_ms: mm.last_progress_timestamp_ms,
            last_resolution_success_timestamp_ms: self
                .resolution_enabled
                .then_some(integration.resolution.last_success_timestamp_ms),
            sync_age_ms,
            feed_age_ms,
            mm_age_ms,
            resolution_age_ms,
        }
    }

    fn render(&self) -> String {
        let integration = self.integration.snapshot();
        let prices = self.prices.borrow().clone();
        let mm = self.mm.borrow().clone();
        let last_feed_update_ms = latest_token_update_ms(&prices);
        let ready = self.readiness_at(now_ms()).ready;
        let mut out = String::with_capacity(4_000);

        gauge(
            &mut out,
            "sybil_polymarket_ready",
            "Whether required Polymarket actors have recent successful progress.",
            u64::from(ready),
        );
        actor_metrics(
            &mut out,
            "sync_cycles",
            "Polymarket catalog sync cycles",
            &integration.sync,
        );
        actor_metrics(
            &mut out,
            "feed_rest_refreshes",
            "Polymarket REST price refreshes",
            &integration.feed_refresh,
        );
        if self.resolution_enabled {
            actor_metrics(
                &mut out,
                "resolution_ticks",
                "Polymarket resolution reconciliation ticks",
                &integration.resolution,
            );
        }
        gauge(
            &mut out,
            "sybil_polymarket_resolution_enabled",
            "Whether the resolution actor is configured in this process.",
            u64::from(self.resolution_enabled),
        );
        gauge(
            &mut out,
            "sybil_polymarket_feed_tracked_tokens",
            "Token midpoint count in the latest provider-owned price snapshot.",
            prices.midpoints.len().try_into().unwrap_or(u64::MAX),
        );
        gauge(
            &mut out,
            "sybil_polymarket_feed_last_update_timestamp_seconds",
            "Unix timestamp of the latest actual token midpoint update.",
            last_feed_update_ms / 1_000,
        );
        out.push_str(
            "# HELP sybil_polymarket_feed_source Active source of the latest price snapshot.\n\
             # TYPE sybil_polymarket_feed_source gauge\n",
        );
        out.push_str("sybil_polymarket_feed_source{source=\"");
        out.push_str(match prices.source {
            PriceUpdateSource::None => "none",
            PriceUpdateSource::WebSocket => "websocket",
            PriceUpdateSource::RestFallback => "rest",
        });
        out.push_str("\"} 1\n");

        gauge(
            &mut out,
            "sybil_polymarket_mm_tracked_markets",
            "Markets currently tracked by the shared MM actor.",
            mm.tracked_markets.try_into().unwrap_or(u64::MAX),
        );
        optional_gauge(
            &mut out,
            "sybil_polymarket_mm_last_observed_block",
            "Latest live Sybil block observed by the MM actor.",
            mm.last_observed_block,
        );
        optional_gauge(
            &mut out,
            "sybil_polymarket_mm_last_completed_quote_block",
            "Latest live Sybil block whose MM quote cycle completed.",
            mm.last_completed_quote_block,
        );
        optional_gauge(
            &mut out,
            "sybil_polymarket_mm_last_successful_submission_block",
            "Latest block on which the API accepted a Polymarket MM IOC bundle.",
            mm.last_successful_submission_block,
        );
        gauge(
            &mut out,
            "sybil_polymarket_mm_last_progress_timestamp_seconds",
            "Unix timestamp when the latest Polymarket MM quote cycle completed.",
            mm.last_progress_timestamp_ms / 1_000,
        );
        counter(
            &mut out,
            "sybil_polymarket_mm_submissions_success_total",
            "Polymarket MM IOC bundles accepted by the API in this process.",
            mm.successful_submissions,
        );
        counter(
            &mut out,
            "sybil_polymarket_mm_submissions_failed_total",
            "Polymarket MM IOC bundle submission failures in this process.",
            mm.failed_submissions,
        );
        out.push_str(
            "# HELP sybil_polymarket_health_stale_window_seconds Maximum actor progress age accepted by readiness.\n\
             # TYPE sybil_polymarket_health_stale_window_seconds gauge\n",
        );
        window_metric(&mut out, "sync", self.windows.sync_ms);
        window_metric(&mut out, "feed", self.windows.feed_ms);
        window_metric(&mut out, "mm", self.windows.mm_ms);
        window_metric(&mut out, "resolution", self.windows.resolution_ms);
        out
    }
}

pub async fn serve(
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

fn age(now_ms: u64, timestamp_ms: u64) -> Option<u64> {
    (timestamp_ms != 0).then(|| now_ms.saturating_sub(timestamp_ms))
}

fn latest_token_update_ms(prices: &PriceSnapshot) -> u64 {
    prices
        .token_updated_ms
        .values()
        .copied()
        .max()
        .unwrap_or_default()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn actor_metrics(out: &mut String, suffix: &str, help: &str, progress: &ActorProgress) {
    counter(
        out,
        &format!("sybil_polymarket_{suffix}_success_total"),
        &format!("Successful {help} in this process."),
        progress.successes,
    );
    counter(
        out,
        &format!("sybil_polymarket_{suffix}_failed_total"),
        &format!("Failed {help} in this process."),
        progress.failures,
    );
    gauge(
        out,
        &format!("sybil_polymarket_{suffix}_last_success_timestamp_seconds"),
        &format!("Unix timestamp of the latest successful {help}."),
        progress.last_success_timestamp_ms / 1_000,
    );
}

fn window_metric(out: &mut String, actor: &str, value_ms: u64) {
    out.push_str("sybil_polymarket_health_stale_window_seconds{actor=\"");
    out.push_str(actor);
    out.push_str("\"} ");
    out.push_str(&(value_ms / 1_000).to_string());
    out.push('\n');
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

    fn ready_state(now: u64) -> MonitoringState {
        let integration = IntegrationProgress {
            inner: Arc::new(RwLock::new(IntegrationSnapshot {
                sync: ActorProgress {
                    last_success_timestamp_ms: now,
                    successes: 2,
                    failures: 1,
                },
                feed_refresh: ActorProgress {
                    last_success_timestamp_ms: now,
                    successes: 3,
                    failures: 1,
                },
                resolution: ActorProgress {
                    last_success_timestamp_ms: now,
                    successes: 4,
                    failures: 0,
                },
            })),
        };
        let (_price_tx, price_rx) = watch::channel(PriceSnapshot {
            midpoints: [("token".to_string(), 0.5)].into_iter().collect(),
            token_updated_ms: [("token".to_string(), now)].into_iter().collect(),
            last_updated_ms: now,
            source: PriceUpdateSource::WebSocket,
        });
        let (_mm_tx, mm_rx) = watch::channel(MmProgress {
            tracked_markets: 1,
            last_observed_block: Some(10),
            last_completed_quote_block: Some(10),
            last_successful_submission_block: Some(10),
            successful_submissions: 8,
            failed_submissions: 1,
            last_progress_timestamp_ms: now,
        });
        MonitoringState::new(
            integration,
            price_rx,
            mm_rx,
            MonitoringWindows {
                sync_ms: 10_000,
                feed_ms: 10_000,
                mm_ms: 10_000,
                resolution_ms: 10_000,
            },
            true,
        )
    }

    #[test]
    fn readiness_reports_actor_that_stopped_progressing() {
        let now = 100_000;
        let state = ready_state(now);
        assert_eq!(state.readiness_at(now + 9_000).status, "ok");
        let response = state.readiness_at(now + 11_000);
        assert_eq!(response.status, "stalled");
        assert!(response.problems.contains(&"sync_stalled"));
        assert!(response.problems.contains(&"feed_stalled"));
        assert!(response.problems.contains(&"mm_stalled"));
        assert!(response.problems.contains(&"resolution_stalled"));
    }

    #[test]
    fn empty_refresh_heartbeat_cannot_hide_stale_token_prices() {
        let now = 100_000;
        let mut state = ready_state(now);
        let (_price_tx, price_rx) = watch::channel(PriceSnapshot {
            midpoints: [("token".to_string(), 0.5)].into_iter().collect(),
            token_updated_ms: [("token".to_string(), now - 20_000)].into_iter().collect(),
            last_updated_ms: now,
            source: PriceUpdateSource::RestFallback,
        });
        state.prices = price_rx;

        let response = state.readiness_at(now);
        assert_eq!(response.status, "stalled");
        assert!(response.problems.contains(&"feed_stalled"));
        assert_eq!(response.last_feed_update_timestamp_ms, now - 20_000);
    }

    #[test]
    fn prometheus_output_covers_every_owned_actor() {
        let metrics = ready_state(now_ms()).render();
        assert!(metrics.contains("sybil_polymarket_ready 1"));
        assert!(metrics.contains("sybil_polymarket_sync_cycles_success_total 2"));
        assert!(metrics.contains("sybil_polymarket_feed_tracked_tokens 1"));
        assert!(metrics.contains("sybil_polymarket_mm_submissions_success_total 8"));
        assert!(metrics.contains("sybil_polymarket_resolution_ticks_success_total 4"));
    }

    #[test]
    fn disabled_resolution_is_not_rendered_as_epoch_zero_progress() {
        let mut state = ready_state(now_ms());
        state.resolution_enabled = false;
        let metrics = state.render();
        assert!(metrics.contains("sybil_polymarket_resolution_enabled 0"));
        assert!(!metrics.contains("sybil_polymarket_resolution_ticks_last_success"));
    }

    #[test]
    fn health_windows_follow_actor_cadences() {
        let windows = MonitoringWindows::for_cadences(120, 5, 30_000, 120);
        assert_eq!(windows.sync_ms, 360_000);
        assert_eq!(windows.feed_ms, 60_000);
        assert_eq!(windows.mm_ms, 60_000);
        assert_eq!(windows.resolution_ms, 360_000);
    }
}
