use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::Json;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params_from_iter, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::arena::{count_rows, table_exists};
use crate::state::AppState;
use crate::types::error::AppError;

const DEFAULT_BOT_DECISION_LIMIT: usize = 50;
const MAX_BOT_DECISION_LIMIT: usize = 500;
const DEFAULT_BOT_EQUITY_LIMIT: usize = 200;
const MAX_BOT_EQUITY_LIMIT: usize = 1_000;

type SnapshotRow = (
    String,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<i64>,
    Option<i64>,
    Option<String>,
);

/// GET /v1/bots/decisions
///
/// Native arena / bot analytics feed. Public (unauthenticated) read route.
#[utoipa::path(
    get,
    path = "/v1/bots/decisions",
    params(
        ("limit" = Option<usize>, Query, description = "Maximum returned decisions, clamped to 1..=500 (default 50)"),
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
    let path = state.arena_db_path.clone();
    let limit = bot_decision_query_limit(params.limit);
    let trader = clean_query_text(params.trader);
    let market_id = params.market_id;
    let since = clean_query_text(params.since);

    let response = tokio::task::spawn_blocking(move || {
        load_bot_decisions(path, limit, trader, market_id, since)
    })
    .await
    .map_err(|e| AppError::internal(format!("bot decision task failed: {e}")))?;

    Ok(Json(response))
}

/// GET /v1/bots/equity-series
///
/// Native arena per-bot portfolio-value time series from `portfolio_snapshots`.
/// Public (unauthenticated) read route. Dense result sets are downsampled with a
/// naive stride after a bounded count query; the latest point is retained.
#[utoipa::path(
    get,
    path = "/v1/bots/equity-series",
    params(
        ("trader" = Option<String>, Query, description = "Filter portfolio snapshots to a single trader name"),
        ("since" = Option<String>, Query, description = "ISO-8601 lower-bound timestamp filter (`portfolio_snapshots.timestamp >= since`)"),
        ("limit" = Option<usize>, Query, description = "Maximum returned sampled points, clamped to 1..=1000 (default 200). Dense rows are downsampled by a naive stride."),
    ),
    responses(
        (status = 200, description = "Bot portfolio-value time series", body = BotEquitySeriesResponse)
    )
)]
pub async fn get_bot_equity_series(
    State(state): State<AppState>,
    Query(params): Query<BotEquitySeriesParams>,
) -> Result<Json<BotEquitySeriesResponse>, AppError> {
    let path = state.arena_db_path.clone();
    let limit = bot_equity_query_limit(params.limit);
    let trader = clean_query_text(params.trader);
    let since = clean_query_text(params.since);

    let response =
        tokio::task::spawn_blocking(move || load_bot_equity_series(path, limit, trader, since))
            .await
            .map_err(|e| AppError::internal(format!("bot equity task failed: {e}")))?;

    Ok(Json(response))
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

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct BotDecisionFeedResponse {
    pub db_available: bool,
    pub db_path: Option<String>,
    pub error: Option<String>,
    pub stats: BotStatsResponse,
    pub summaries: Vec<BotSummaryResponse>,
    pub decisions: Vec<BotDecisionResponse>,
    pub token_usage: Vec<TokenUsageResponse>,
}

#[derive(Debug, Default, Serialize, utoipa::ToSchema)]
pub struct BotStatsResponse {
    pub decisions: i64,
    pub articles: i64,
    pub snapshots: i64,
    pub token_usage: i64,
    pub traders: usize,
    pub latest_decision_timestamp: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, utoipa::ToSchema)]
pub struct BotSummaryResponse {
    pub trader_name: String,
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

#[derive(Debug, Serialize, utoipa::ToSchema)]
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

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TokenUsageResponse {
    pub trader_name: String,
    pub calls: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub avg_latency_s: Option<f64>,
    pub latest_model: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
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

#[derive(Debug, Serialize, utoipa::ToSchema)]
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

fn load_bot_decisions(
    db_path: String,
    limit: usize,
    trader: Option<String>,
    market_id: Option<u32>,
    since: Option<String>,
) -> BotDecisionFeedResponse {
    let path = db_path.trim();
    if path.is_empty() {
        return unavailable(None, "SYBIL_ARENA_DB_PATH is not configured");
    }

    if !Path::new(path).exists() {
        return unavailable(Some(path.to_string()), "arena decision database not found");
    }

    let conn = match Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(conn) => conn,
        Err(e) => {
            return unavailable(
                Some(path.to_string()),
                format!("failed to open arena decision database: {e}"),
            );
        }
    };
    let _ = conn.busy_timeout(Duration::from_millis(750));

    if !table_exists(&conn, "decisions") {
        return unavailable(Some(path.to_string()), "decisions table is missing");
    }

    let summaries = match load_summaries(&conn) {
        Ok(rows) => rows,
        Err(e) => {
            return unavailable(
                Some(path.to_string()),
                format!("failed to load bot summaries: {e}"),
            );
        }
    };
    let decisions =
        match load_recent_decisions(&conn, limit, trader.as_deref(), market_id, since.as_deref()) {
            Ok(rows) => rows,
            Err(e) => {
                return unavailable(
                    Some(path.to_string()),
                    format!("failed to load bot decisions: {e}"),
                );
            }
        };
    let token_usage = load_token_usage(&conn).unwrap_or_default();

    let stats = BotStatsResponse {
        decisions: count_rows(&conn, "decisions"),
        articles: count_rows(&conn, "articles"),
        snapshots: count_rows(&conn, "portfolio_snapshots"),
        token_usage: count_rows(&conn, "token_usage"),
        traders: summaries.len(),
        latest_decision_timestamp: latest_decision_timestamp(&conn),
    };

    BotDecisionFeedResponse {
        db_available: true,
        db_path: Some(path.to_string()),
        error: None,
        stats,
        summaries,
        decisions,
        token_usage,
    }
}

fn load_bot_equity_series(
    db_path: String,
    limit: usize,
    trader: Option<String>,
    since: Option<String>,
) -> BotEquitySeriesResponse {
    let path = db_path.trim();
    if path.is_empty() {
        return unavailable_equity(
            None,
            trader,
            since,
            limit,
            "SYBIL_ARENA_DB_PATH is not configured",
        );
    }

    if !Path::new(path).exists() {
        return unavailable_equity(
            Some(path.to_string()),
            trader,
            since,
            limit,
            "arena decision database not found",
        );
    }

    let conn = match Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(conn) => conn,
        Err(e) => {
            return unavailable_equity(
                Some(path.to_string()),
                trader,
                since,
                limit,
                format!("failed to open arena decision database: {e}"),
            );
        }
    };
    let _ = conn.busy_timeout(Duration::from_millis(750));

    if !table_exists(&conn, "portfolio_snapshots") {
        return unavailable_equity(
            Some(path.to_string()),
            trader,
            since,
            limit,
            "portfolio_snapshots table is missing",
        );
    }

    let (points, source_rows, stride) =
        match load_equity_points(&conn, limit, trader.as_deref(), since.as_deref()) {
            Ok(rows) => rows,
            Err(e) => {
                return unavailable_equity(
                    Some(path.to_string()),
                    trader,
                    since,
                    limit,
                    format!("failed to load bot equity series: {e}"),
                );
            }
        };
    let returned_rows = points.len();

    BotEquitySeriesResponse {
        db_available: true,
        db_path: Some(path.to_string()),
        error: None,
        trader,
        since,
        limit,
        server_cap: MAX_BOT_EQUITY_LIMIT,
        source_rows,
        returned_rows,
        downsampled: source_rows > returned_rows,
        stride,
        points,
    }
}

fn unavailable(path: Option<String>, error: impl Into<String>) -> BotDecisionFeedResponse {
    BotDecisionFeedResponse {
        db_available: false,
        db_path: path,
        error: Some(error.into()),
        stats: BotStatsResponse::default(),
        summaries: Vec::new(),
        decisions: Vec::new(),
        token_usage: Vec::new(),
    }
}

fn unavailable_equity(
    path: Option<String>,
    trader: Option<String>,
    since: Option<String>,
    limit: usize,
    error: impl Into<String>,
) -> BotEquitySeriesResponse {
    BotEquitySeriesResponse {
        db_available: false,
        db_path: path,
        error: Some(error.into()),
        trader,
        since,
        limit,
        server_cap: MAX_BOT_EQUITY_LIMIT,
        source_rows: 0,
        returned_rows: 0,
        downsampled: false,
        stride: 1,
        points: Vec::new(),
    }
}

fn latest_decision_timestamp(conn: &Connection) -> Option<String> {
    conn.query_row(
        "SELECT timestamp FROM decisions ORDER BY id DESC LIMIT 1",
        [],
        |row| row.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
}

fn load_summaries(conn: &Connection) -> rusqlite::Result<Vec<BotSummaryResponse>> {
    let mut summaries = HashMap::<String, BotSummaryResponse>::new();

    let mut stmt = conn.prepare(
        "SELECT trader_name, COUNT(*) AS decision_count, AVG(ABS(fair_value - market_price)) AS avg_edge \
         FROM decisions GROUP BY trader_name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(BotSummaryResponse {
            trader_name: row.get(0)?,
            decision_count: row.get(1)?,
            avg_edge: row.get(2)?,
            ..BotSummaryResponse::default()
        })
    })?;
    for row in rows {
        let summary = row?;
        summaries.insert(summary.trader_name.clone(), summary);
    }

    let mut stmt = conn.prepare(
        "SELECT d.trader_name, d.market_id, d.market_name, d.timestamp, d.fair_value, d.market_price, d.balance \
         FROM decisions d \
         JOIN (SELECT trader_name, MAX(id) AS id FROM decisions GROUP BY trader_name) latest \
           ON d.trader_name = latest.trader_name AND d.id = latest.id",
    )?;
    let rows = stmt.query_map([], |row| {
        let fair_value: Option<f64> = row.get(4)?;
        let market_price: Option<f64> = row.get(5)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            fair_value,
            market_price,
            row.get::<_, Option<f64>>(6)?,
            edge(fair_value, market_price),
        ))
    })?;
    for row in rows {
        let (
            trader_name,
            market_id,
            market_name,
            timestamp,
            fair_value,
            market_price,
            balance,
            edge,
        ) = row?;
        let summary = summaries
            .entry(trader_name.clone())
            .or_insert_with(|| BotSummaryResponse {
                trader_name,
                ..BotSummaryResponse::default()
            });
        summary.latest_market_id = market_id;
        summary.latest_market_name = market_name;
        summary.latest_timestamp = timestamp;
        summary.latest_fair_value = fair_value;
        summary.latest_market_price = market_price;
        summary.latest_balance = balance;
        summary.latest_edge = edge;
    }

    load_latest_snapshots(conn, &mut summaries);

    let mut rows: Vec<_> = summaries.into_values().collect();
    rows.sort_by(|a, b| {
        b.latest_timestamp
            .cmp(&a.latest_timestamp)
            .then_with(|| b.decision_count.cmp(&a.decision_count))
            .then_with(|| a.trader_name.cmp(&b.trader_name))
    });
    Ok(rows)
}

fn load_latest_snapshots(conn: &Connection, summaries: &mut HashMap<String, BotSummaryResponse>) {
    if !table_exists(conn, "portfolio_snapshots") {
        return;
    }

    let with_totals = conn.prepare(
        "SELECT p.trader_name, p.balance, p.portfolio_value, p.pnl, p.total_fills, p.total_orders, p.timestamp \
         FROM portfolio_snapshots p \
         JOIN (SELECT trader_name, MAX(id) AS id FROM portfolio_snapshots GROUP BY trader_name) latest \
           ON p.trader_name = latest.trader_name AND p.id = latest.id",
    );

    if let Ok(mut stmt) = with_totals {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<f64>>(1)?,
                row.get::<_, Option<f64>>(2)?,
                row.get::<_, Option<f64>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        }) {
            for row in rows.flatten() {
                apply_snapshot(summaries, row);
            }
            return;
        }
    }

    let Ok(mut stmt) = conn.prepare(
        "SELECT p.trader_name, p.balance, p.portfolio_value, p.pnl, p.timestamp \
         FROM portfolio_snapshots p \
         JOIN (SELECT trader_name, MAX(id) AS id FROM portfolio_snapshots GROUP BY trader_name) latest \
           ON p.trader_name = latest.trader_name AND p.id = latest.id",
    ) else {
        return;
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<f64>>(1)?,
            row.get::<_, Option<f64>>(2)?,
            row.get::<_, Option<f64>>(3)?,
            None,
            None,
            row.get::<_, Option<String>>(4)?,
        ))
    }) else {
        return;
    };
    for row in rows.flatten() {
        apply_snapshot(summaries, row);
    }
}

fn apply_snapshot(summaries: &mut HashMap<String, BotSummaryResponse>, row: SnapshotRow) {
    let (trader_name, balance, portfolio_value, pnl, total_fills, total_orders, timestamp) = row;
    let summary = summaries
        .entry(trader_name.clone())
        .or_insert_with(|| BotSummaryResponse {
            trader_name,
            ..BotSummaryResponse::default()
        });
    if summary.latest_balance.is_none() {
        summary.latest_balance = balance;
    }
    summary.portfolio_value = portfolio_value;
    summary.pnl = pnl;
    summary.total_fills = total_fills;
    summary.total_orders = total_orders;
    summary.snapshot_timestamp = timestamp;
}

fn load_recent_decisions(
    conn: &Connection,
    limit: usize,
    trader: Option<&str>,
    market_id: Option<u32>,
    since: Option<&str>,
) -> rusqlite::Result<Vec<BotDecisionResponse>> {
    let (where_clause, mut params) = decision_filters(trader, market_id, since);
    let sql = format!(
        "SELECT id, trader_name, market_id, market_name, timestamp, analysis, fair_value, market_price, \
                orders, motivation, llm_duration_s, balance, yes_pos, no_pos, article_urls \
         FROM decisions {where_clause} ORDER BY id DESC LIMIT ?"
    );
    params.push(SqlValue::Integer(limit as i64));

    let mut stmt = conn.prepare(&sql)?;
    let mapper = |row: &rusqlite::Row<'_>| {
        let fair_value: Option<f64> = row.get(6)?;
        let market_price: Option<f64> = row.get(7)?;
        Ok(BotDecisionResponse {
            id: row.get(0)?,
            trader_name: row.get(1)?,
            market_id: row.get(2)?,
            market_name: row.get(3)?,
            timestamp: row.get(4)?,
            analysis: row.get(5)?,
            motivation: row.get(9)?,
            fair_value,
            market_price,
            edge: edge(fair_value, market_price),
            orders: json_column(row.get(8)?),
            article_urls: json_column(row.get(14)?),
            llm_duration_s: row.get(10)?,
            balance: row.get(11)?,
            yes_pos: row.get(12)?,
            no_pos: row.get(13)?,
        })
    };

    let rows = stmt.query_map(params_from_iter(params), mapper)?;
    rows.collect()
}

fn load_equity_points(
    conn: &Connection,
    limit: usize,
    trader: Option<&str>,
    since: Option<&str>,
) -> rusqlite::Result<(Vec<BotEquityPointResponse>, usize, usize)> {
    let (where_clause, params) = snapshot_filters(trader, since);
    let count_sql = format!("SELECT COUNT(*) FROM portfolio_snapshots {where_clause}");
    let source_rows: usize = conn
        .query_row(&count_sql, params_from_iter(params.clone()), |row| {
            row.get::<_, i64>(0)
        })?
        .max(0) as usize;
    if source_rows == 0 {
        return Ok((Vec::new(), 0, 1));
    }

    let stride = if source_rows > limit {
        source_rows.div_ceil(limit)
    } else {
        1
    };
    let mut query_params = params;
    query_params.push(SqlValue::Integer(stride as i64));
    query_params.push(SqlValue::Integer(stride as i64));
    query_params.push(SqlValue::Integer((limit + 1) as i64));

    let sql = equity_sample_sql(&where_clause, true);
    let rows = match query_equity_points(conn, &sql, query_params.clone()) {
        Ok(rows) => rows,
        Err(_) => {
            let fallback_sql = equity_sample_sql(&where_clause, false);
            query_equity_points(conn, &fallback_sql, query_params)?
        }
    };

    Ok((trim_sampled_rows(rows, limit), source_rows, stride))
}

fn query_equity_points(
    conn: &Connection,
    sql: &str,
    params: Vec<SqlValue>,
) -> rusqlite::Result<Vec<BotEquityPointResponse>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params_from_iter(params), |row| {
        Ok(BotEquityPointResponse {
            id: row.get(0)?,
            trader_name: row.get(1)?,
            timestamp: row.get(2)?,
            balance: row.get(3)?,
            portfolio_value: row.get(4)?,
            pnl: row.get(5)?,
            total_fills: row.get(6)?,
            total_orders: row.get(7)?,
        })
    })?;
    rows.collect()
}

fn equity_sample_sql(where_clause: &str, include_totals: bool) -> String {
    let totals = if include_totals {
        "total_fills, total_orders"
    } else {
        "NULL AS total_fills, NULL AS total_orders"
    };
    format!(
        "SELECT id, trader_name, timestamp, balance, portfolio_value, pnl, total_fills, total_orders \
         FROM ( \
           SELECT id, trader_name, timestamp, balance, portfolio_value, pnl, {totals}, \
                  ROW_NUMBER() OVER (ORDER BY id ASC) AS rn, \
                  COUNT(*) OVER () AS total_rows \
           FROM portfolio_snapshots {where_clause} \
         ) sampled \
         WHERE ? <= 1 OR ((rn - 1) % ?) = 0 OR rn = total_rows \
         ORDER BY id ASC \
         LIMIT ?"
    )
}

fn trim_sampled_rows(
    mut rows: Vec<BotEquityPointResponse>,
    limit: usize,
) -> Vec<BotEquityPointResponse> {
    if rows.len() <= limit {
        return rows;
    }
    let latest = rows.pop().expect("rows is non-empty");
    rows.truncate(limit.saturating_sub(1));
    rows.push(latest);
    rows
}

fn decision_filters(
    trader: Option<&str>,
    market_id: Option<u32>,
    since: Option<&str>,
) -> (String, Vec<SqlValue>) {
    let mut clauses = Vec::new();
    let mut params = Vec::new();
    if let Some(trader) = trader {
        clauses.push("trader_name = ?");
        params.push(SqlValue::Text(trader.to_string()));
    }
    if let Some(market_id) = market_id {
        clauses.push("market_id = ?");
        params.push(SqlValue::Integer(i64::from(market_id)));
    }
    if let Some(since) = since {
        clauses.push("timestamp >= ?");
        params.push(SqlValue::Text(since.to_string()));
    }
    (where_clause(&clauses), params)
}

fn snapshot_filters(trader: Option<&str>, since: Option<&str>) -> (String, Vec<SqlValue>) {
    let mut clauses = Vec::new();
    let mut params = Vec::new();
    if let Some(trader) = trader {
        clauses.push("trader_name = ?");
        params.push(SqlValue::Text(trader.to_string()));
    }
    if let Some(since) = since {
        clauses.push("timestamp >= ?");
        params.push(SqlValue::Text(since.to_string()));
    }
    (where_clause(&clauses), params)
}

fn where_clause(clauses: &[&str]) -> String {
    if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    }
}

fn load_token_usage(conn: &Connection) -> rusqlite::Result<Vec<TokenUsageResponse>> {
    if !table_exists(conn, "token_usage") {
        return Ok(Vec::new());
    }

    let mut stmt = conn.prepare(
        "SELECT trader_name, COUNT(*) AS calls, \
                COALESCE(SUM(prompt_tokens), 0), COALESCE(SUM(completion_tokens), 0), \
                AVG(duration_s), MAX(model) \
         FROM token_usage GROUP BY trader_name ORDER BY calls DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(TokenUsageResponse {
            trader_name: row.get(0)?,
            calls: row.get(1)?,
            prompt_tokens: row.get(2)?,
            completion_tokens: row.get(3)?,
            avg_latency_s: row.get(4)?,
            latest_model: row.get(5)?,
        })
    })?;
    rows.collect()
}

fn edge(fair_value: Option<f64>, market_price: Option<f64>) -> Option<f64> {
    Some((fair_value? - market_price?).abs())
}

fn json_column(text: Option<String>) -> Value {
    text.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| Value::Array(Vec::new()))
}

fn clean_query_text(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

fn bot_decision_query_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_BOT_DECISION_LIMIT)
        .clamp(1, MAX_BOT_DECISION_LIMIT)
}

fn bot_equity_query_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_BOT_EQUITY_LIMIT)
        .clamp(1, MAX_BOT_EQUITY_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decisions_table(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE decisions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                trader_name TEXT,
                market_id INTEGER,
                market_name TEXT,
                timestamp TEXT,
                article_urls TEXT,
                analysis TEXT,
                fair_value REAL,
                market_price REAL,
                orders TEXT,
                motivation TEXT,
                raw_llm_response TEXT,
                llm_duration_s REAL,
                balance REAL,
                yes_pos REAL,
                no_pos REAL
            );",
        )
        .expect("create decisions table");
    }

    fn snapshots_table(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE portfolio_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                trader_name TEXT,
                timestamp TEXT,
                balance REAL,
                portfolio_value REAL,
                pnl REAL,
                positions TEXT,
                total_fills INTEGER DEFAULT 0,
                total_orders INTEGER DEFAULT 0
            );",
        )
        .expect("create snapshots table");
    }

    #[test]
    fn bot_decision_query_limit_defaults_and_clamps() {
        assert_eq!(bot_decision_query_limit(None), DEFAULT_BOT_DECISION_LIMIT);
        assert_eq!(bot_decision_query_limit(Some(0)), 1);
        assert_eq!(bot_decision_query_limit(Some(42)), 42);
        assert_eq!(
            bot_decision_query_limit(Some(MAX_BOT_DECISION_LIMIT + 1)),
            MAX_BOT_DECISION_LIMIT
        );
    }

    #[test]
    fn load_recent_decisions_filters_trader_market_and_since() {
        let conn = Connection::open_in_memory().expect("sqlite");
        decisions_table(&conn);
        for (trader, market_id, timestamp, fair_value) in [
            ("alice", 7, "2026-07-01T00:00:00+00:00", 0.41),
            ("alice", 7, "2026-07-02T00:00:00+00:00", 0.42),
            ("alice", 8, "2026-07-03T00:00:00+00:00", 0.81),
            ("bob", 7, "2026-07-04T00:00:00+00:00", 0.50),
        ] {
            conn.execute(
                "INSERT INTO decisions (
                    trader_name, market_id, market_name, timestamp, article_urls,
                    analysis, fair_value, market_price, orders, motivation,
                    raw_llm_response, llm_duration_s, balance, yes_pos, no_pos
                ) VALUES (?1, ?2, 'Market', ?3, '[]', 'analysis', ?4, 0.40, '[]', 'why', '{}', 1.0, 99.0, 5.0, 3.0)",
                rusqlite::params![trader, market_id, timestamp, fair_value],
            )
            .expect("insert decision");
        }

        let rows = load_recent_decisions(
            &conn,
            10,
            Some("alice"),
            Some(7),
            Some("2026-07-02T00:00:00+00:00"),
        )
        .expect("load decisions");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].trader_name, "alice");
        assert_eq!(rows[0].market_id, Some(7));
        assert_eq!(rows[0].fair_value, Some(0.42));
        assert_eq!(rows[0].yes_pos, Some(5.0));
        assert_eq!(rows[0].no_pos, Some(3.0));
    }

    #[test]
    fn load_equity_points_downsamples_and_keeps_latest() {
        let conn = Connection::open_in_memory().expect("sqlite");
        snapshots_table(&conn);
        for idx in 0..5 {
            conn.execute(
                "INSERT INTO portfolio_snapshots (
                    trader_name, timestamp, balance, portfolio_value, pnl,
                    positions, total_fills, total_orders
                ) VALUES ('alice', ?1, 100.0, ?2, ?3, '{}', ?4, ?5)",
                rusqlite::params![
                    format!("2026-07-0{}T00:00:00+00:00", idx + 1),
                    100.0 + f64::from(idx),
                    f64::from(idx),
                    idx,
                    idx + 10,
                ],
            )
            .expect("insert snapshot");
        }

        let (rows, source_rows, stride) =
            load_equity_points(&conn, 2, Some("alice"), None).expect("load equity");

        assert_eq!(source_rows, 5);
        assert_eq!(stride, 3);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].portfolio_value, Some(100.0));
        assert_eq!(rows[1].portfolio_value, Some(104.0));
        assert_eq!(rows[1].total_orders, Some(14));
    }
}
