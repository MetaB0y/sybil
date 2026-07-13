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
    pub total_fills: Option<i64>,
    pub total_orders: Option<i64>,
}

/// Snapshot of arena bot metrics for the Prometheus scrape.
pub fn load_bot_metrics_snapshot(path: &str) -> BotMetricsSnapshot {
    let Some(conn) = open_read_only(path) else {
        return BotMetricsSnapshot::unavailable();
    };
    if !table_exists(&conn, "portfolio_snapshots") {
        return BotMetricsSnapshot::unavailable();
    }

    BotMetricsSnapshot {
        db_available: true,
        traders: load_latest_trader_snapshots(&conn),
    }
}

fn load_latest_trader_snapshots(conn: &Connection) -> Vec<TraderMetricsSnapshot> {
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
            return Vec::new();
        }
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok(TraderMetricsSnapshot {
            name: row.get(0)?,
            total_fills: row.get(1)?,
            total_orders: row.get(2)?,
        })
    }) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}
