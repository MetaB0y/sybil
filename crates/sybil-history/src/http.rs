use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Serialize;
use sybil_history_types::{
    AccountEventQuery, CommittedHistoryBatchV1, EquityBaselinesQuery, EquityQuery, FillQuery,
    PriceCandleQuery, PriceHistoryQuery,
};

use crate::{HistoryError, HistoryHandle, HistoryStore};

#[derive(Clone)]
pub struct HistoryHttpConfig {
    pub dev_mode: bool,
    pub internal_token: Option<String>,
    pub max_query_concurrency: usize,
}

#[derive(Clone)]
struct HttpState {
    handle: HistoryHandle,
    store: HistoryStore,
    config: Arc<HistoryHttpConfig>,
    query_concurrency: Arc<tokio::sync::Semaphore>,
}

pub fn router(handle: HistoryHandle, store: HistoryStore, config: HistoryHttpConfig) -> Router {
    let state = HttpState {
        handle,
        store,
        query_concurrency: Arc::new(tokio::sync::Semaphore::new(
            config.max_query_concurrency.max(1),
        )),
        config: Arc::new(config),
    };
    let internal = Router::new()
        .route("/internal/history/v1/status", get(status))
        .route("/internal/history/v1/batches/{height}", put(apply_batch))
        .route("/internal/history/v1/query/fills", post(query_fills))
        .route("/internal/history/v1/query/events", post(query_events))
        .route("/internal/history/v1/query/equity", post(query_equity))
        .route(
            "/internal/history/v1/query/equity-baselines",
            post(query_equity_baselines),
        )
        .route("/internal/history/v1/query/prices", post(query_prices))
        .route("/internal/history/v1/query/candles", post(query_candles))
        // A committed block is the natural delivery unit. Rejecting one by a
        // transport-size constant would permanently head-of-line block every
        // later height. Authentication runs before extraction; MessagePack
        // keeps the trusted internal payload compact.
        .layer(DefaultBodyLimit::disable())
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            authorize_internal,
        ));
    Router::new()
        .route("/healthz", get(healthz))
        .merge(internal)
        .with_state(state)
}

async fn healthz() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn status(
    State(state): State<HttpState>,
) -> Result<Json<sybil_history_types::ProjectionStatus>, HttpError> {
    blocking_query(state.clone(), move || state.store.status())
        .await
        .map(Json)
}

async fn apply_batch(
    State(state): State<HttpState>,
    Path(height): Path<u64>,
    body: Bytes,
) -> Result<Json<sybil_history_types::ApplyBatchResponse>, HttpError> {
    let batch: CommittedHistoryBatchV1 = rmp_serde::from_slice(&body)
        .map_err(|error| HttpError::bad_request(format!("invalid batch encoding: {error}")))?;
    if batch.height != height {
        return Err(HttpError::bad_request("path and batch heights differ"));
    }
    state
        .handle
        .apply(batch)
        .await
        .map(Json)
        .map_err(Into::into)
}

async fn query_fills(
    State(state): State<HttpState>,
    Json(query): Json<FillQuery>,
) -> Result<Json<sybil_history_types::HistoryPage<sybil_history_types::AccountFillFact>>, HttpError>
{
    blocking_query(state.clone(), move || state.store.fills(query))
        .await
        .map(Json)
}

async fn query_events(
    State(state): State<HttpState>,
    Json(query): Json<AccountEventQuery>,
) -> Result<Json<sybil_history_types::HistoryPage<sybil_history_types::AccountEventFact>>, HttpError>
{
    blocking_query(state.clone(), move || state.store.events(query))
        .await
        .map(Json)
}

async fn query_equity(
    State(state): State<HttpState>,
    Json(query): Json<EquityQuery>,
) -> Result<Json<sybil_history_types::HistoryPage<sybil_history_types::AccountEquityFact>>, HttpError>
{
    blocking_query(state.clone(), move || state.store.equity(query))
        .await
        .map(Json)
}

async fn query_equity_baselines(
    State(state): State<HttpState>,
    Json(query): Json<EquityBaselinesQuery>,
) -> Result<Json<sybil_history_types::EquityBaselines>, HttpError> {
    blocking_query(state.clone(), move || state.store.equity_baselines(query))
        .await
        .map(Json)
}

async fn query_prices(
    State(state): State<HttpState>,
    Json(query): Json<PriceHistoryQuery>,
) -> Result<Json<sybil_history_types::PriceHistoryPage>, HttpError> {
    blocking_query(state.clone(), move || state.store.prices(query))
        .await
        .map(Json)
}

async fn query_candles(
    State(state): State<HttpState>,
    Json(query): Json<PriceCandleQuery>,
) -> Result<Json<sybil_history_types::PriceCandlePage>, HttpError> {
    blocking_query(state.clone(), move || state.store.candles(query))
        .await
        .map(Json)
}

async fn blocking_query<T: Send + 'static>(
    state: HttpState,
    work: impl FnOnce() -> Result<T, HistoryError> + Send + 'static,
) -> Result<T, HttpError> {
    let permit = Arc::clone(&state.query_concurrency)
        .acquire_owned()
        .await
        .map_err(|_| HttpError::internal("history query limiter closed"))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await
    .map_err(|error| HttpError::internal(format!("history task failed: {error}")))?
    .map_err(Into::into)
}

async fn authorize_internal(
    State(state): State<HttpState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, HttpError> {
    if state.config.dev_mode {
        return Ok(next.run(request).await);
    }
    let expected = state
        .config
        .internal_token
        .as_deref()
        .ok_or_else(|| HttpError::unauthorized("history token is not configured"))?;
    let actual = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if actual == Some(expected) {
        Ok(next.run(request).await)
    } else {
        Err(HttpError::unauthorized("invalid history token"))
    }
}

#[derive(Debug)]
struct HttpError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl HttpError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "INVALID_HISTORY_REQUEST",
            message: message.into(),
        }
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "HISTORY_UNAUTHORIZED",
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "HISTORY_INTERNAL",
            message: message.into(),
        }
    }
}

impl From<HistoryError> for HttpError {
    fn from(error: HistoryError) -> Self {
        let (status, code) = match error {
            HistoryError::InvalidBatch(_) => (StatusCode::BAD_REQUEST, "INVALID_HISTORY_BATCH"),
            HistoryError::InvalidQuery(_) => (StatusCode::BAD_REQUEST, "INVALID_HISTORY_QUERY"),
            HistoryError::GenesisMismatch
            | HistoryError::Gap { .. }
            | HistoryError::ParentHashMismatch { .. }
            | HistoryError::ConflictingBatch { .. }
            | HistoryError::ConflictingProjection { .. } => {
                (StatusCode::CONFLICT, "HISTORY_CONFLICT")
            }
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "HISTORY_INTERNAL"),
        };
        Self {
            status,
            code,
            message: error.to_string(),
        }
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: &'a str,
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                code: self.code,
                message: &self.message,
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_router(dev_mode: bool) -> (tempfile::TempDir, Router) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store =
            HistoryStore::open(dir.path().join("history.redb"), vec![60]).expect("history store");
        let handle = HistoryHandle::spawn(store.clone());
        let app = router(
            handle,
            store,
            HistoryHttpConfig {
                dev_mode,
                internal_token: Some("history-secret".into()),
                max_query_concurrency: 4,
            },
        );
        (dir, app)
    }

    #[tokio::test]
    async fn health_is_public_but_internal_queries_require_history_token() {
        let (_dir, app) = test_router(false);
        let health = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("health response");
        assert_eq!(health.status(), StatusCode::NO_CONTENT);

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/internal/history/v1/status")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("unauthorized response");
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let authorized = app
            .oneshot(
                Request::builder()
                    .uri("/internal/history/v1/status")
                    .header(axum::http::header::AUTHORIZATION, "Bearer history-secret")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("authorized response");
        assert_eq!(authorized.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn dev_mode_allows_loopback_queries_without_token() {
        let (_dir, app) = test_router(true);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/internal/history/v1/status")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn authentication_precedes_batch_decode_and_msgpack_batches_apply() {
        let (_dir, app) = test_router(false);
        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/internal/history/v1/batches/1")
                    .body(Body::from(vec![0xff; 1024]))
                    .expect("request"),
            )
            .await
            .expect("unauthorized response");
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let batch = CommittedHistoryBatchV1::new(
            [9; 32],
            1,
            [0; 32],
            [1; 32],
            [2; 32],
            1_000,
            vec![],
            vec![],
            vec![],
            vec![],
        )
        .expect("valid batch");
        let response = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/internal/history/v1/batches/1")
                    .header(axum::http::header::AUTHORIZATION, "Bearer history-secret")
                    .header(axum::http::header::CONTENT_TYPE, "application/msgpack")
                    .body(Body::from(rmp_serde::to_vec(&batch).expect("msgpack")))
                    .expect("request"),
            )
            .await
            .expect("apply response");
        assert_eq!(response.status(), StatusCode::OK);
    }
}
