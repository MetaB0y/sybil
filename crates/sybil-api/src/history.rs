use std::sync::Arc;
use std::time::Duration;

use matching_sequencer::store::{ProductHistoryOutboxAck, ProductHistoryOutboxStats, Store};
use reqwest::StatusCode;
use serde::Serialize;
use serde::de::DeserializeOwned;
use sybil_history_types::{
    AccountEquityFact, AccountEventFact, AccountEventQuery, AccountFillFact, ApplyBatchResponse,
    CommittedHistoryBatchV1, EquityBaselines, EquityBaselinesQuery, EquityQuery, FillQuery,
    HistoryPage, PriceCandlePage, PriceCandleQuery, PriceHistoryPage, PriceHistoryQuery,
    ProjectionStatus,
};
use tokio_util::sync::CancellationToken;

use crate::config::ApiConfig;

const OUTBOX_READ_BATCH_SIZE: usize = 16;

#[derive(Clone)]
pub struct HistoryClient {
    base_url: Arc<str>,
    token: Option<Arc<str>>,
    http: reqwest::Client,
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryClientError {
    #[error("history service request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("history batch encode failed: {0}")]
    Encode(#[from] rmp_serde::encode::Error),
    #[error("history service returned {status}: {body}")]
    Response { status: StatusCode, body: String },
    #[error("product-history outbox task failed: {0}")]
    Outbox(String),
}

impl HistoryClient {
    pub fn from_config(config: &ApiConfig) -> Result<Option<Self>, HistoryClientError> {
        let base_url = config.history_url.trim().trim_end_matches('/');
        if base_url.is_empty() {
            return Ok(None);
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.history_timeout_ms.max(1)))
            .build()?;
        Ok(Some(Self {
            base_url: Arc::from(base_url),
            token: (!config.history_token.trim().is_empty())
                .then(|| Arc::from(config.history_token.trim())),
            http,
        }))
    }

    pub async fn apply_batch(
        &self,
        batch: &CommittedHistoryBatchV1,
    ) -> Result<ApplyBatchResponse, HistoryClientError> {
        let url = format!(
            "{}/internal/history/v1/batches/{}",
            self.base_url, batch.height
        );
        let body = rmp_serde::to_vec(batch)?;
        let request = self
            .with_auth(self.http.put(url))
            .header(reqwest::header::CONTENT_TYPE, "application/msgpack")
            .body(body);
        decode_response(request.send().await?).await
    }

    pub async fn status(&self) -> Result<ProjectionStatus, HistoryClientError> {
        let url = format!("{}/internal/history/v1/status", self.base_url);
        decode_response(self.with_auth(self.http.get(url)).send().await?).await
    }

    pub async fn fills(
        &self,
        query: &FillQuery,
    ) -> Result<HistoryPage<AccountFillFact>, HistoryClientError> {
        self.query("fills", query).await
    }

    pub async fn events(
        &self,
        query: &AccountEventQuery,
    ) -> Result<HistoryPage<AccountEventFact>, HistoryClientError> {
        self.query("events", query).await
    }

    pub async fn equity(
        &self,
        query: &EquityQuery,
    ) -> Result<HistoryPage<AccountEquityFact>, HistoryClientError> {
        self.query("equity", query).await
    }

    pub async fn equity_baselines(
        &self,
        query: &EquityBaselinesQuery,
    ) -> Result<EquityBaselines, HistoryClientError> {
        self.query("equity-baselines", query).await
    }

    pub async fn prices(
        &self,
        query: &PriceHistoryQuery,
    ) -> Result<PriceHistoryPage, HistoryClientError> {
        self.query("prices", query).await
    }

    pub async fn candles(
        &self,
        query: &PriceCandleQuery,
    ) -> Result<PriceCandlePage, HistoryClientError> {
        self.query("candles", query).await
    }

    async fn query<Q: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        name: &str,
        query: &Q,
    ) -> Result<R, HistoryClientError> {
        let url = format!("{}/internal/history/v1/query/{name}", self.base_url);
        let request = self.with_auth(self.http.post(url)).json(query);
        decode_response(request.send().await?).await
    }

    fn with_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(token) => request.bearer_auth(token.as_ref()),
            None => request,
        }
    }
}

async fn decode_response<T: DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, HistoryClientError> {
    let status = response.status();
    if status.is_success() {
        return response.json().await.map_err(Into::into);
    }
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "unreadable response".to_string());
    Err(HistoryClientError::Response { status, body })
}

pub async fn run_outbox_publisher(
    store: Arc<Store>,
    client: HistoryClient,
    poll_interval: Duration,
    cancel: CancellationToken,
) {
    let poll_interval = poll_interval.max(Duration::from_millis(10));
    loop {
        let read_store = Arc::clone(&store);
        let read = tokio::task::spawn_blocking(move || {
            let stats = read_store.product_history_outbox_stats()?;
            let batches = read_store.product_history_outbox_batches(OUTBOX_READ_BATCH_SIZE)?;
            Ok::<_, matching_sequencer::store::StoreError>((stats, batches))
        });
        let read = tokio::select! {
            _ = cancel.cancelled() => return,
            result = read => result,
        };
        let (stats, batches) = match read {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => {
                metrics::counter!("sybil_product_history_outbox_read_failures_total").increment(1);
                tracing::warn!(%error, "failed to read product-history outbox");
                if wait_or_cancel(&cancel, poll_interval).await {
                    return;
                }
                continue;
            }
            Err(error) => {
                metrics::counter!("sybil_product_history_outbox_read_failures_total").increment(1);
                tracing::warn!(%error, "product-history outbox task failed");
                if wait_or_cancel(&cancel, poll_interval).await {
                    return;
                }
                continue;
            }
        };
        record_outbox_stats(stats);
        if batches.is_empty() {
            if wait_or_cancel(&cancel, poll_interval).await {
                return;
            }
            continue;
        }

        let mut delivery_failed = false;
        let mut delivered = Vec::with_capacity(batches.len());
        let mut delivery_durations = Vec::with_capacity(batches.len());
        for batch in batches {
            tracing::debug!(
                height = batch.height,
                fills = batch.fills.len(),
                events = batch.events.len(),
                equity = batch.equity.len(),
                prices = batch.prices.len(),
                "delivering committed history batch"
            );
            let started = std::time::Instant::now();
            let response = tokio::select! {
                _ = cancel.cancelled() => return,
                result = client.apply_batch(&batch) => result,
            };
            match response {
                Ok(response) if response.indexed_through_height >= batch.height => {
                    delivered.push(ProductHistoryOutboxAck {
                        height: batch.height,
                        payload_hash: batch.payload_hash,
                    });
                    delivery_durations.push(started.elapsed().as_secs_f64());
                }
                Ok(response) => {
                    tracing::warn!(
                        height = batch.height,
                        indexed_through_height = response.indexed_through_height,
                        "history service acknowledgement did not cover delivered batch"
                    );
                    delivery_failed = true;
                    break;
                }
                Err(error) => {
                    metrics::counter!("sybil_history_batch_delivery_failures_total").increment(1);
                    tracing::warn!(height = batch.height, %error, "history batch delivery failed; retaining outbox row");
                    delivery_failed = true;
                    break;
                }
            }
        }
        if !delivered.is_empty() {
            let ack_store = Arc::clone(&store);
            let ack_count = delivered.len() as u64;
            let ack_result = tokio::task::spawn_blocking(move || {
                ack_store.acknowledge_product_history_batches(&delivered)
            })
            .await;
            match ack_result {
                Ok(Ok(_)) => {
                    metrics::counter!("sybil_history_batches_delivered_total").increment(ack_count);
                    for duration in delivery_durations {
                        metrics::histogram!("sybil_history_batch_delivery_duration_seconds")
                            .record(duration);
                    }
                }
                Ok(Err(error)) => {
                    tracing::warn!(%error, "failed to acknowledge delivered product-history outbox prefix");
                    delivery_failed = true;
                }
                Err(error) => {
                    tracing::warn!(%error, "product-history outbox acknowledgement task failed");
                    delivery_failed = true;
                }
            }
        }
        if delivery_failed && wait_or_cancel(&cancel, poll_interval).await {
            return;
        }
    }
}

/// Keep the durable source stock visible even when no history endpoint is
/// configured. This is intentionally observation-only: overflow behavior is
/// an explicit architecture decision, never an implicit row drop.
pub async fn run_outbox_monitor(
    store: Arc<Store>,
    poll_interval: Duration,
    cancel: CancellationToken,
) {
    let poll_interval = poll_interval.max(Duration::from_millis(10));
    loop {
        let read_store = Arc::clone(&store);
        let read = tokio::task::spawn_blocking(move || read_store.product_history_outbox_stats());
        let read = tokio::select! {
            _ = cancel.cancelled() => return,
            result = read => result,
        };
        match read {
            Ok(Ok(stats)) => record_outbox_stats(stats),
            Ok(Err(error)) => {
                metrics::counter!("sybil_product_history_outbox_read_failures_total").increment(1);
                tracing::warn!(%error, "failed to inspect product-history outbox");
            }
            Err(error) => {
                metrics::counter!("sybil_product_history_outbox_read_failures_total").increment(1);
                tracing::warn!(%error, "product-history outbox monitor task failed");
            }
        }
        if wait_or_cancel(&cancel, poll_interval).await {
            return;
        }
    }
}

fn record_outbox_stats(stats: ProductHistoryOutboxStats) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let now_ms = u64::try_from(now_ms).unwrap_or(u64::MAX);
    let oldest_age_seconds = stats
        .oldest_committed_at_ms
        .map(|timestamp_ms| now_ms.saturating_sub(timestamp_ms) as f64 / 1_000.0)
        .unwrap_or(0.0);
    metrics::gauge!("sybil_product_history_outbox_backlog_rows").set(stats.rows as f64);
    metrics::gauge!("sybil_product_history_outbox_payload_bytes").set(stats.payload_bytes as f64);
    metrics::gauge!("sybil_product_history_outbox_oldest_height")
        .set(stats.oldest_height.unwrap_or(0) as f64);
    metrics::gauge!("sybil_product_history_outbox_newest_height")
        .set(stats.newest_height.unwrap_or(0) as f64);
    metrics::gauge!("sybil_product_history_outbox_oldest_age_seconds").set(oldest_age_seconds);
}

async fn wait_or_cancel(cancel: &CancellationToken, duration: Duration) -> bool {
    tokio::select! {
        () = cancel.cancelled() => true,
        () = tokio::time::sleep(duration) => false,
    }
}
