//! Shared read-only access to the arena bot SQLite database.
//!
//! Two call sites read this DB: the Prometheus scrape ([`load_bot_metrics_snapshot`],
//! driven from `app.rs`) and the `/v1/bots/decisions` feed (`routes::bots`). The
//! connection opener and the small `sqlite_master`/`COUNT(*)` helpers live here so
//! both readers share one implementation rather than keeping private copies.
//!
//! This is intentionally the read side only — pushing metrics from the arena itself
//! is a separate, larger redesign and out of scope here.

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::util::now_secs;

/// Open the arena DB read-only, or `None` when the path is unset, missing, or
/// cannot be opened. A short busy timeout lets a scrape ride out a concurrent
/// arena write instead of erroring immediately.
pub fn open_read_only(path: &str) -> Option<Connection> {
    let path = path.trim();
    if path.is_empty() || !Path::new(path).exists() {
        return None;
    }
    match Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(conn) => {
            let _ = conn.busy_timeout(Duration::from_millis(750));
            Some(conn)
        }
        Err(error) => {
            tracing::warn!(path, error = %error, "failed to open arena bot db");
            None
        }
    }
}

/// Whether `table` exists in the arena DB.
pub fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get::<_, i64>(0),
    )
    .map(|v| v == 1)
    .unwrap_or(false)
}

/// Row count for `table`, or 0 when the table is missing or the query fails.
pub fn count_rows(conn: &Connection, table: &str) -> i64 {
    if !table_exists(conn, table) {
        return 0;
    }
    let sql = format!("SELECT COUNT(*) FROM {table}");
    conn.query_row(&sql, [], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
}

#[derive(Debug, Default)]
pub struct BotMetricsSnapshot {
    pub db_available: bool,
    pub decisions: i64,
    pub latest_decision_age_seconds: Option<u64>,
    pub traders: Vec<TraderMetricsSnapshot>,
}

impl BotMetricsSnapshot {
    pub fn unavailable() -> Self {
        Self::default()
    }
}

#[derive(Debug, Default)]
pub struct TraderMetricsSnapshot {
    pub name: String,
    pub decisions: i64,
    pub latest_decision_age_seconds: Option<u64>,
    pub total_fills: Option<i64>,
    pub total_orders: Option<i64>,
}

/// Snapshot of arena bot metrics for the Prometheus scrape.
pub fn load_bot_metrics_snapshot(path: &str) -> BotMetricsSnapshot {
    let Some(conn) = open_read_only(path) else {
        return BotMetricsSnapshot::unavailable();
    };
    if !table_exists(&conn, "decisions") {
        return BotMetricsSnapshot::unavailable();
    }

    let now = now_secs();
    let decisions = count_rows(&conn, "decisions");
    let latest_decision_age_seconds = latest_timestamp_seconds(
        &conn,
        "SELECT MAX(strftime('%s', timestamp)) FROM decisions",
    )
    .map(|ts| now.saturating_sub(ts));
    let mut traders = load_trader_decision_metrics(&conn, now);
    load_trader_snapshot_metrics(&conn, &mut traders);

    BotMetricsSnapshot {
        db_available: true,
        decisions,
        latest_decision_age_seconds,
        traders,
    }
}

fn latest_timestamp_seconds(conn: &Connection, sql: &str) -> Option<u64> {
    conn.query_row(sql, [], |row| row.get::<_, Option<String>>(0))
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u64>().ok())
}

fn load_trader_decision_metrics(conn: &Connection, now: u64) -> Vec<TraderMetricsSnapshot> {
    let mut stmt = match conn.prepare(
        "SELECT trader_name, COUNT(*), MAX(strftime('%s', timestamp))
         FROM decisions GROUP BY trader_name",
    ) {
        Ok(stmt) => stmt,
        Err(error) => {
            tracing::warn!(error = %error, "failed to prepare trader decision metrics query");
            return Vec::new();
        }
    };
    let Ok(rows) = stmt.query_map([], |row| {
        let latest: Option<String> = row.get(2)?;
        Ok(TraderMetricsSnapshot {
            name: row.get(0)?,
            decisions: row.get(1)?,
            latest_decision_age_seconds: latest
                .and_then(|value| value.parse::<u64>().ok())
                .map(|ts| now.saturating_sub(ts)),
            total_fills: None,
            total_orders: None,
        })
    }) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

fn load_trader_snapshot_metrics(conn: &Connection, traders: &mut [TraderMetricsSnapshot]) {
    if !table_exists(conn, "portfolio_snapshots") {
        return;
    }
    let mut stmt = match conn.prepare(
        "SELECT p.trader_name, p.total_fills, p.total_orders
         FROM portfolio_snapshots p
         JOIN (
           SELECT trader_name, MAX(id) AS id FROM portfolio_snapshots GROUP BY trader_name
         ) latest ON p.trader_name = latest.trader_name AND p.id = latest.id",
    ) {
        Ok(stmt) => stmt,
        Err(error) => {
            tracing::warn!(error = %error, "failed to prepare trader snapshot metrics query");
            return;
        }
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, Option<i64>>(2)?,
        ))
    }) else {
        return;
    };
    let snapshots: std::collections::HashMap<_, _> = rows
        .filter_map(Result::ok)
        .map(|(name, fills, orders)| (name, (fills, orders)))
        .collect();
    for trader in traders {
        if let Some((fills, orders)) = snapshots.get(&trader.name) {
            trader.total_fills = *fills;
            trader.total_orders = *orders;
        }
    }
}
