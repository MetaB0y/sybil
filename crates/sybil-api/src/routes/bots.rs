use axum::extract::State;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::extract::{Json, Query};
use crate::state::AppState;
use crate::types::error::AppError;

const DEFAULT_BOT_DECISION_LIMIT: usize = 50;
const MAX_BOT_DECISION_LIMIT: usize = 500;
const DEFAULT_BOT_EQUITY_LIMIT: usize = 200;
const MAX_BOT_EQUITY_LIMIT: usize = 1_000;

/// GET /v1/bots/decisions
///
/// Public bot analytics backed by Arena's private typed read service. The Rust
/// API owns the public route and contract, while Python owns its storage and
/// query semantics.
#[utoipa::path(
    tag = "routesbots",
    get,
    path = "/v1/bots/decisions",
    params(
        ("limit" = Option<usize>, Query, maximum = 500, description = "Maximum returned decisions (default 50); zero is normalized to one and values above 500 are rejected"),
        ("trader" = Option<String>, Query, description = "Filter decisions to a single trader name"),
        ("market_id" = Option<u32>, Query, description = "Filter decisions to one market ID. Combine with `trader` for FV-drift history."),
        ("since" = Option<String>, Query, description = "ISO-8601 lower-bound timestamp filter (`decisions.timestamp >= since`) for historical reads."),
    ),
    responses(
        (status = 200, description = "Bot decision feed", body = BotDecisionFeedResponse)
    )
)]
pub async fn get_bot_decisions(
    State(state): State<AppState>,
    Query(params): Query<BotDecisionParams>,
) -> Result<Json<BotDecisionFeedResponse>, AppError> {
    let query = ArenaDecisionQuery {
        limit: bot_decision_query_limit(params.limit)?,
        trader: clean_query_text(params.trader),
        market_id: params.market_id,
        since: clean_query_text(params.since),
    };
    let Some(client) = &state.arena else {
        return Ok(Json(unavailable("Arena read service is not configured")));
    };
    Ok(match client.decisions(&query).await {
        Ok(response) => Json(response),
        Err(error) => {
            tracing::warn!(%error, "Arena decision read failed");
            Json(unavailable("Arena read service is unavailable"))
        }
    })
}

/// GET /v1/bots/equity-series
///
/// Public per-bot portfolio-value series proxied from Arena's private typed
/// read service. Dense results are bounded and downsampled by Arena.
#[utoipa::path(
    tag = "routesbots",
    get,
    path = "/v1/bots/equity-series",
    params(
        ("trader" = Option<String>, Query, description = "Filter portfolio snapshots to a single trader name"),
        ("since" = Option<String>, Query, description = "ISO-8601 lower-bound timestamp filter (`portfolio_snapshots.timestamp >= since`)"),
        ("limit" = Option<usize>, Query, maximum = 1000, description = "Maximum returned sampled points (default 200); zero is normalized to one and values above 1000 are rejected. Dense rows are downsampled by a naive stride."),
    ),
    responses(
        (status = 200, description = "Bot portfolio-value time series", body = BotEquitySeriesResponse)
    )
)]
pub async fn get_bot_equity_series(
    State(state): State<AppState>,
    Query(params): Query<BotEquitySeriesParams>,
) -> Result<Json<BotEquitySeriesResponse>, AppError> {
    let query = ArenaEquityQuery {
        trader: clean_query_text(params.trader),
        since: clean_query_text(params.since),
        limit: bot_equity_query_limit(params.limit)?,
    };
    let Some(client) = &state.arena else {
        return Ok(Json(unavailable_equity(
            &query,
            "Arena read service is not configured",
        )));
    };
    Ok(match client.equity_series(&query).await {
        Ok(response) => Json(response),
        Err(error) => {
            tracing::warn!(%error, "Arena equity read failed");
            Json(unavailable_equity(
                &query,
                "Arena read service is unavailable",
            ))
        }
    })
}

#[derive(Debug, Deserialize)]
pub struct BotDecisionParams {
    pub limit: Option<usize>,
    pub trader: Option<String>,
    pub market_id: Option<u32>,
    pub since: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BotEquitySeriesParams {
    pub trader: Option<String>,
    pub since: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct ArenaDecisionQuery {
    limit: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    trader: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    market_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    since: Option<String>,
}

#[derive(Debug, Serialize)]
struct ArenaEquityQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    trader: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    since: Option<String>,
    limit: usize,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct BotDecisionFeedResponse {
    pub db_available: bool,
    pub db_path: Option<String>,
    pub error: Option<String>,
    pub stats: BotStatsResponse,
    pub summaries: Vec<BotSummaryResponse>,
    pub decisions: Vec<BotDecisionResponse>,
    pub token_usage: Vec<TokenUsageResponse>,
}

#[derive(Debug, Default, Deserialize, Serialize, utoipa::ToSchema)]
pub struct BotStatsResponse {
    pub decisions: i64,
    pub articles: i64,
    pub snapshots: i64,
    pub token_usage: i64,
    pub traders: usize,
    pub latest_decision_timestamp: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, utoipa::ToSchema)]
pub struct BotSummaryResponse {
    pub trader_name: String,
    /// Durable sequencer account recorded with the latest Arena snapshot.
    pub account_id: Option<i64>,
    /// Member of the most recent non-stale Arena runtime cohort.
    pub active: bool,
    /// Runtime role such as competitor, load, or noise.
    pub role: Option<String>,
    /// Eligible for public competition totals within the active runtime.
    pub scored: bool,
    pub decision_count: i64,
    pub avg_edge: Option<f64>,
    pub latest_timestamp: Option<String>,
    pub latest_market_id: Option<i64>,
    pub latest_market_name: Option<String>,
    pub latest_fair_value: Option<f64>,
    pub latest_market_price: Option<f64>,
    pub latest_edge: Option<f64>,
    pub latest_balance: Option<f64>,
    pub portfolio_value: Option<f64>,
    pub pnl: Option<f64>,
    pub total_fills: Option<i64>,
    pub total_orders: Option<i64>,
    pub snapshot_timestamp: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct BotDecisionResponse {
    pub id: i64,
    pub trader_name: String,
    pub market_id: Option<i64>,
    pub market_name: Option<String>,
    pub timestamp: Option<String>,
    pub analysis: Option<String>,
    pub motivation: Option<String>,
    pub fair_value: Option<f64>,
    pub market_price: Option<f64>,
    pub edge: Option<f64>,
    pub orders: Value,
    pub article_urls: Value,
    pub llm_duration_s: Option<f64>,
    pub balance: Option<f64>,
    pub yes_pos: Option<f64>,
    pub no_pos: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct TokenUsageResponse {
    pub trader_name: String,
    pub calls: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub avg_latency_s: Option<f64>,
    pub latest_model: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct BotEquitySeriesResponse {
    pub db_available: bool,
    pub db_path: Option<String>,
    pub error: Option<String>,
    pub trader: Option<String>,
    pub since: Option<String>,
    pub limit: usize,
    pub server_cap: usize,
    pub source_rows: usize,
    pub returned_rows: usize,
    pub downsampled: bool,
    pub stride: usize,
    pub points: Vec<BotEquityPointResponse>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct BotEquityPointResponse {
    pub id: i64,
    pub trader_name: String,
    pub timestamp: Option<String>,
    pub balance: Option<f64>,
    pub portfolio_value: Option<f64>,
    pub pnl: Option<f64>,
    pub total_fills: Option<i64>,
    pub total_orders: Option<i64>,
}

fn unavailable(error: impl Into<String>) -> BotDecisionFeedResponse {
    BotDecisionFeedResponse {
        db_available: false,
        db_path: None,
        error: Some(error.into()),
        stats: BotStatsResponse::default(),
        summaries: Vec::new(),
        decisions: Vec::new(),
        token_usage: Vec::new(),
    }
}

fn unavailable_equity(
    query: &ArenaEquityQuery,
    error: impl Into<String>,
) -> BotEquitySeriesResponse {
    BotEquitySeriesResponse {
        db_available: false,
        db_path: None,
        error: Some(error.into()),
        trader: query.trader.clone(),
        since: query.since.clone(),
        limit: query.limit,
        server_cap: MAX_BOT_EQUITY_LIMIT,
        source_rows: 0,
        returned_rows: 0,
        downsampled: false,
        stride: 1,
        points: Vec::new(),
    }
}

fn clean_query_text(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

fn bot_decision_query_limit(limit: Option<usize>) -> Result<usize, AppError> {
    if limit.is_some_and(|limit| limit > MAX_BOT_DECISION_LIMIT) {
        return Err(AppError::bad_request(format!(
            "limit must be at most {MAX_BOT_DECISION_LIMIT}"
        )));
    }
    Ok(limit.unwrap_or(DEFAULT_BOT_DECISION_LIMIT).max(1))
}

fn bot_equity_query_limit(limit: Option<usize>) -> Result<usize, AppError> {
    if limit.is_some_and(|limit| limit > MAX_BOT_EQUITY_LIMIT) {
        return Err(AppError::bad_request(format!(
            "limit must be at most {MAX_BOT_EQUITY_LIMIT}"
        )));
    }
    Ok(limit.unwrap_or(DEFAULT_BOT_EQUITY_LIMIT).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_default_and_clamp_at_public_boundary() {
        assert_eq!(bot_decision_query_limit(None).unwrap(), 50);
        assert_eq!(bot_decision_query_limit(Some(0)).unwrap(), 1);
        assert!(bot_decision_query_limit(Some(501)).is_err());
        assert_eq!(bot_equity_query_limit(None).unwrap(), 200);
        assert_eq!(bot_equity_query_limit(Some(0)).unwrap(), 1);
        assert!(bot_equity_query_limit(Some(1_001)).is_err());
    }

    #[test]
    fn empty_filters_are_not_forwarded() {
        assert_eq!(clean_query_text(None), None);
        assert_eq!(clean_query_text(Some("  ".to_string())), None);
        assert_eq!(
            clean_query_text(Some(" alice ".to_string())).as_deref(),
            Some("alice")
        );
    }

    #[test]
    fn unavailable_equity_preserves_effective_query() {
        let query = ArenaEquityQuery {
            trader: Some("alice".to_string()),
            since: Some("2026-07-01".to_string()),
            limit: 42,
        };
        let response = unavailable_equity(&query, "offline");
        assert!(!response.db_available);
        assert_eq!(response.trader.as_deref(), Some("alice"));
        assert_eq!(response.limit, 42);
        assert_eq!(response.stride, 1);
    }
}
