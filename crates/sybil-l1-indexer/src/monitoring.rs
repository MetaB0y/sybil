use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use tokio::net::TcpListener;

#[derive(Clone, Default)]
pub(super) struct IndexerMetrics {
    inner: Arc<RwLock<MetricsSnapshot>>,
}

#[derive(Default)]
struct MetricsSnapshot {
    ready: bool,
    integrity_latched: bool,
    started_timestamp_seconds: u64,
    last_successful_poll_timestamp_seconds: u64,
    latest_block: Option<u64>,
    confirmed_tip_block: Option<u64>,
    checkpoint_block: Option<u64>,
    next_from_block: u64,
    confirmed_lag_blocks: u64,
    poll_failures: BTreeMap<&'static str, u64>,
    fatal_failures: BTreeMap<&'static str, u64>,
    rpc_failures_total: u64,
    consecutive_rpc_failures: u64,
    cursor_persistence_failures_total: u64,
    fatal_kind: Option<&'static str>,
    trust_mode: String,
    provider_count: u64,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    ready: bool,
    integrity_latched: bool,
    fatal_kind: Option<&'static str>,
    checkpoint_block: Option<u64>,
    next_from_block: u64,
    confirmed_lag_blocks: u64,
    last_successful_poll_timestamp_seconds: u64,
    trust_mode: String,
    provider_count: u64,
}

impl IndexerMetrics {
    pub(super) fn new() -> Self {
        let metrics = Self::default();
        metrics
            .inner
            .write()
            .expect("metrics lock poisoned")
            .started_timestamp_seconds = now_seconds();
        metrics
    }

    pub(super) fn configure_source(&self, trust_mode: &str, provider_count: usize) {
        let mut state = self.inner.write().expect("metrics lock poisoned");
        state.trust_mode = trust_mode.to_string();
        state.provider_count = u64::try_from(provider_count).unwrap_or(u64::MAX);
    }

    pub(super) fn mark_ready(&self, next_from: u64, checkpoint_block: Option<u64>) {
        let mut state = self.inner.write().expect("metrics lock poisoned");
        state.ready = true;
        state.next_from_block = next_from;
        state.checkpoint_block = checkpoint_block;
    }

    pub(super) fn record_successful_poll(
        &self,
        latest_block: u64,
        confirmed_tip_block: u64,
        next_from_block: u64,
        checkpoint_block: Option<u64>,
    ) {
        let mut state = self.inner.write().expect("metrics lock poisoned");
        state.ready = true;
        state.last_successful_poll_timestamp_seconds = now_seconds();
        state.latest_block = Some(latest_block);
        state.confirmed_tip_block = Some(confirmed_tip_block);
        state.next_from_block = next_from_block;
        state.checkpoint_block = checkpoint_block;
        let processed_through =
            checkpoint_block.unwrap_or_else(|| next_from_block.saturating_sub(1));
        state.confirmed_lag_blocks = confirmed_tip_block.saturating_sub(processed_through);
        state.consecutive_rpc_failures = 0;
    }

    pub(super) fn record_poll_failure(&self, kind: &'static str, is_rpc: bool) {
        let mut state = self.inner.write().expect("metrics lock poisoned");
        *state.poll_failures.entry(kind).or_default() += 1;
        if is_rpc {
            state.rpc_failures_total += 1;
            state.consecutive_rpc_failures += 1;
        } else {
            state.consecutive_rpc_failures = 0;
        }
    }

    pub(super) fn record_cursor_persistence_failure(&self) {
        self.inner
            .write()
            .expect("metrics lock poisoned")
            .cursor_persistence_failures_total += 1;
    }

    pub(super) fn mark_integrity_latched(&self) {
        self.inner
            .write()
            .expect("metrics lock poisoned")
            .integrity_latched = true;
    }

    pub(super) fn record_fatal(&self, kind: &'static str, integrity_latched: bool) {
        let mut state = self.inner.write().expect("metrics lock poisoned");
        state.ready = false;
        state.fatal_kind = Some(kind);
        state.integrity_latched |= integrity_latched;
        *state.fatal_failures.entry(kind).or_default() += 1;
    }

    fn health(&self) -> HealthResponse {
        let state = self.inner.read().expect("metrics lock poisoned");
        HealthResponse {
            status: if state.fatal_kind.is_some() {
                "fatal"
            } else if state.ready {
                "ok"
            } else {
                "starting"
            },
            ready: state.ready,
            integrity_latched: state.integrity_latched,
            fatal_kind: state.fatal_kind,
            checkpoint_block: state.checkpoint_block,
            next_from_block: state.next_from_block,
            confirmed_lag_blocks: state.confirmed_lag_blocks,
            last_successful_poll_timestamp_seconds: state.last_successful_poll_timestamp_seconds,
            trust_mode: state.trust_mode.clone(),
            provider_count: state.provider_count,
        }
    }

    pub(super) fn render(&self) -> String {
        let state = self.inner.read().expect("metrics lock poisoned");
        let mut out = String::with_capacity(3_000);
        gauge(
            &mut out,
            "sybil_l1_indexer_ready",
            "Whether the indexer is configured and not fail-stop halted.",
            u64::from(state.ready),
        );
        gauge(
            &mut out,
            "sybil_l1_indexer_integrity_latched",
            "Whether an L1 reorg, provider disagreement, or invalid authenticated view is durably latched.",
            u64::from(state.integrity_latched),
        );
        gauge(
            &mut out,
            "sybil_l1_indexer_provider_count",
            "Number of configured L1 JSON-RPC providers in the durable trust policy.",
            state.provider_count,
        );
        out.push_str(
            "# HELP sybil_l1_indexer_source_policy Active L1 source trust policy.\n\
             # TYPE sybil_l1_indexer_source_policy gauge\n",
        );
        if !state.trust_mode.is_empty() {
            out.push_str("sybil_l1_indexer_source_policy{mode=\"");
            out.push_str(&state.trust_mode);
            out.push_str("\"} 1\n");
        }
        gauge(
            &mut out,
            "sybil_l1_indexer_started_timestamp_seconds",
            "Unix timestamp when this indexer process started.",
            state.started_timestamp_seconds,
        );
        gauge(
            &mut out,
            "sybil_l1_indexer_last_successful_poll_timestamp_seconds",
            "Unix timestamp of the last fully successful poll.",
            state.last_successful_poll_timestamp_seconds,
        );
        optional_gauge(
            &mut out,
            "sybil_l1_indexer_latest_block",
            "Authenticated source tip observed during a successful poll (finalized in public mode).",
            state.latest_block,
        );
        optional_gauge(
            &mut out,
            "sybil_l1_indexer_confirmed_tip_block",
            "Highest authenticated L1 block eligible for ingestion.",
            state.confirmed_tip_block,
        );
        optional_gauge(
            &mut out,
            "sybil_l1_indexer_checkpoint_block",
            "Last fully processed L1 block with a persisted canonical hash.",
            state.checkpoint_block,
        );
        gauge(
            &mut out,
            "sybil_l1_indexer_checkpoint_present",
            "Whether the durable cursor currently contains a canonical checkpoint.",
            u64::from(state.checkpoint_block.is_some()),
        );
        gauge(
            &mut out,
            "sybil_l1_indexer_next_from_block",
            "First L1 block not yet fully processed.",
            state.next_from_block,
        );
        gauge(
            &mut out,
            "sybil_l1_indexer_confirmed_lag_blocks",
            "Authenticated source-prefix blocks not yet covered by the durable checkpoint.",
            state.confirmed_lag_blocks,
        );
        counter(
            &mut out,
            "sybil_l1_indexer_rpc_failures_total",
            "Total L1 JSON-RPC poll failures in this process.",
            state.rpc_failures_total,
        );
        gauge(
            &mut out,
            "sybil_l1_indexer_consecutive_rpc_failures",
            "Consecutive L1 JSON-RPC poll failures since the last successful poll.",
            state.consecutive_rpc_failures,
        );
        counter(
            &mut out,
            "sybil_l1_indexer_cursor_persistence_failures_total",
            "Total failures to durably persist a cursor or reorg latch.",
            state.cursor_persistence_failures_total,
        );
        labelled_counters(
            &mut out,
            "sybil_l1_indexer_poll_failures_total",
            "Total retryable indexer poll failures by stable kind.",
            &state.poll_failures,
        );
        labelled_counters(
            &mut out,
            "sybil_l1_indexer_fatal_failures_total",
            "Total fail-stop indexer failures by stable kind.",
            &state.fatal_failures,
        );
        out
    }
}

pub(super) async fn serve(listener: TcpListener, metrics: IndexerMetrics) -> std::io::Result<()> {
    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/healthz", get(health_handler))
        .with_state(metrics);
    axum::serve(listener, app).await
}

async fn metrics_handler(State(metrics): State<IndexerMetrics>) -> String {
    metrics.render()
}

async fn health_handler(
    State(metrics): State<IndexerMetrics>,
) -> (StatusCode, Json<HealthResponse>) {
    let response = metrics.health();
    let status = if response.ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(response))
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
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

fn labelled_counters(
    out: &mut String,
    name: &str,
    help: &str,
    values: &BTreeMap<&'static str, u64>,
) {
    out.push_str("# HELP ");
    out.push_str(name);
    out.push(' ');
    out.push_str(help);
    out.push_str("\n# TYPE ");
    out.push_str(name);
    out.push_str(" counter\n");
    for (kind, value) in values {
        out.push_str(name);
        out.push_str("{kind=\"");
        out.push_str(kind);
        out.push_str("\"} ");
        out.push_str(&value.to_string());
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fatal_state_remains_scrapeable_and_unhealthy() {
        let metrics = IndexerMetrics::new();
        metrics.configure_source("unanimous-finalized", 2);
        metrics.mark_ready(9, Some(8));
        metrics.record_successful_poll(10, 8, 9, Some(8));
        metrics.mark_integrity_latched();
        metrics.record_fatal("canonical_hash_mismatch", true);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(listener, metrics));

        let health = reqwest::get(format!("http://{address}/healthz"))
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::SERVICE_UNAVAILABLE);
        let health_body = health.text().await.unwrap();
        assert!(health_body.contains("\"status\":\"fatal\""));
        assert!(health_body.contains("\"integrity_latched\":true"));
        assert!(health_body.contains("\"fatal_kind\":\"canonical_hash_mismatch\""));
        assert!(health_body.contains("\"trust_mode\":\"unanimous-finalized\""));
        assert!(health_body.contains("\"provider_count\":2"));

        let body = reqwest::get(format!("http://{address}/metrics"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(body.contains("sybil_l1_indexer_ready 0"));
        assert!(body.contains("sybil_l1_indexer_integrity_latched 1"));
        assert!(body.contains("sybil_l1_indexer_provider_count 2"));
        assert!(body.contains("sybil_l1_indexer_source_policy{mode=\"unanimous-finalized\"} 1"));
        assert!(
            body.contains(
                "sybil_l1_indexer_fatal_failures_total{kind=\"canonical_hash_mismatch\"} 1"
            )
        );
        assert!(body.contains("sybil_l1_indexer_checkpoint_block 8"));

        server.abort();
    }

    #[test]
    fn successful_poll_resets_consecutive_rpc_failures_and_reports_lag() {
        let metrics = IndexerMetrics::new();
        metrics.mark_ready(5, Some(4));
        metrics.record_poll_failure("rpc", true);
        metrics.record_poll_failure("rpc", true);
        metrics.record_successful_poll(20, 18, 11, Some(10));

        let rendered = metrics.render();
        assert!(rendered.contains("sybil_l1_indexer_rpc_failures_total 2"));
        assert!(rendered.contains("sybil_l1_indexer_consecutive_rpc_failures 0"));
        assert!(rendered.contains("sybil_l1_indexer_confirmed_lag_blocks 8"));
        assert!(rendered.contains("sybil_l1_indexer_last_successful_poll_timestamp_seconds "));
    }
}
