use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::Json;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::arena::{count_rows, table_exists};
use crate::state::AppState;
use crate::types::error::AppError;

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
        ("limit" = Option<usize>, Query, description = "Maximum returned decisions, clamped to 1..=200 (default 50)"),
        ("trader" = Option<String>, Query, description = "Filter decisions to a single trader name"),
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
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    let trader = params.trader.and_then(|t| {
        let trimmed = t.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });

    let response = tokio::task::spawn_blocking(move || load_bot_decisions(path, limit, trader))
        .await
        .map_err(|e| AppError::internal(format!("bot decision task failed: {e}")))?;

    Ok(Json(response))
}

#[derive(Debug, Deserialize)]
pub struct BotDecisionParams {
    pub limit: Option<usize>,
    pub trader: Option<String>,
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
    pub yes_pos: Option<i64>,
    pub no_pos: Option<i64>,
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

fn load_bot_decisions(
    db_path: String,
    limit: usize,
    trader: Option<String>,
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
    let decisions = match load_recent_decisions(&conn, limit, trader.as_deref()) {
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
) -> rusqlite::Result<Vec<BotDecisionResponse>> {
    let sql_all =
        "SELECT id, trader_name, market_id, market_name, timestamp, analysis, fair_value, market_price, \
                orders, motivation, llm_duration_s, balance, yes_pos, no_pos, article_urls \
         FROM decisions ORDER BY id DESC LIMIT ?1";
    let sql_trader =
        "SELECT id, trader_name, market_id, market_name, timestamp, analysis, fair_value, market_price, \
                orders, motivation, llm_duration_s, balance, yes_pos, no_pos, article_urls \
         FROM decisions WHERE trader_name = ?1 ORDER BY id DESC LIMIT ?2";

    let mut stmt = conn.prepare(if trader.is_some() {
        sql_trader
    } else {
        sql_all
    })?;
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

    let rows = if let Some(trader) = trader {
        stmt.query_map(rusqlite::params![trader, limit as i64], mapper)?
    } else {
        stmt.query_map([limit as i64], mapper)?
    };

    rows.collect()
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
