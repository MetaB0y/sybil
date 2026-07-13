use axum::Json;
use axum::extract::{Query, State};
use std::collections::HashMap;

use matching_sequencer::LeaderboardRow;
use sybil_history_types::EquityBaselinesQuery;

use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::response::{LeaderboardEntryResponse, LeaderboardResponse};
use crate::util::now_ms;

const DEFAULT_LEADERBOARD_LIMIT: usize = 50;
const MAX_LEADERBOARD_LIMIT: usize = 100;

const DAY_MS: u64 = 24 * 3_600_000;

fn leaderboard_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_LEADERBOARD_LIMIT)
        .min(MAX_LEADERBOARD_LIMIT)
}

/// Canonical window token. Unknown/absent values fall back to all-time.
fn normalize_window(window: Option<&str>) -> &'static str {
    match window {
        Some("7d") => "7d",
        Some("30d") => "30d",
        _ => "all",
    }
}

/// Window start in ms-since-epoch; `0` means all-time (no lower bound).
fn window_since_ms(window: &str, now_ms: u64) -> u64 {
    match window {
        "7d" => now_ms.saturating_sub(7 * DAY_MS),
        "30d" => now_ms.saturating_sub(30 * DAY_MS),
        _ => 0,
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct LeaderboardParams {
    pub window: Option<String>,
    pub limit: Option<usize>,
}

/// GET /v1/leaderboard?window&limit
#[utoipa::path(
    get,
    path = "/v1/leaderboard",
    params(
        ("window" = Option<String>, Query, description = "Ranking window: 7d | 30d | all (default all)"),
        ("limit" = Option<usize>, Query, description = "Result limit (default 50, cap 100)"),
    ),
    responses(
        (status = 200, description = "Ranked trader leaderboard, best PnL first", body = LeaderboardResponse),
        (status = 503, description = "History service unavailable for windowed ranking")
    )
)]
pub async fn get_leaderboard(
    State(state): State<AppState>,
    Query(params): Query<LeaderboardParams>,
) -> Result<Json<LeaderboardResponse>, AppError> {
    let limit = leaderboard_limit(params.limit);
    let window = normalize_window(params.window.as_deref());
    let since_ms = window_since_ms(window, now_ms());

    let bases = state.cached_leaderboard_bases().await?;
    let baselines = if since_ms == 0 {
        HashMap::new()
    } else {
        let history = state.history.as_ref().ok_or_else(|| {
            AppError::history_unavailable("Historical data service is not configured")
        })?;
        let response = history
            .equity_baselines(&EquityBaselinesQuery {
                account_ids: bases.iter().map(|base| base.account_id.0).collect(),
                at_or_before_ms: since_ms,
            })
            .await?;
        if response
            .status
            .first_height
            .is_some_and(|height| height > 1)
            && response
                .status
                .first_timestamp_ms
                .is_some_and(|first| first > since_ms)
        {
            return Err(AppError::history_unavailable(
                "Leaderboard window predates available historical data",
            ));
        }
        response
            .baselines
            .into_iter()
            .map(|point| (point.account_id, point))
            .collect()
    };
    let mut rows: Vec<LeaderboardRow> = bases
        .into_iter()
        .map(|base| {
            let (pnl_nanos, basis_nanos) = baselines.get(&base.account_id.0).map_or(
                (base.pnl_nanos, base.deposited_nanos),
                |point| {
                    (
                        base.pnl_nanos - (point.portfolio_value_nanos - point.deposited_nanos),
                        point.portfolio_value_nanos,
                    )
                },
            );
            let roi_bps = if basis_nanos > 0 {
                ((pnl_nanos as i128 * 10_000) / basis_nanos as i128) as i64
            } else {
                0
            };
            LeaderboardRow {
                account_id: base.account_id,
                display_name: base.display_name,
                avatar_seed: base.avatar_seed,
                pnl_nanos,
                roi_bps,
                markets_traded: base.markets_traded,
                equity_nanos: base.equity_nanos,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        b.pnl_nanos
            .cmp(&a.pnl_nanos)
            .then(a.account_id.0.cmp(&b.account_id.0))
    });
    rows.truncate(limit);
    let entries: Vec<LeaderboardEntryResponse> = rows
        .into_iter()
        .enumerate()
        .map(|(index, row)| LeaderboardEntryResponse {
            rank: (index as u32) + 1,
            account_id: row.account_id.0,
            display_name: row.display_name,
            avatar_seed: row.avatar_seed,
            pnl_nanos: row.pnl_nanos,
            roi_bps: row.roi_bps,
            markets_traded: row.markets_traded,
            equity_nanos: row.equity_nanos,
        })
        .collect();

    Ok(Json(LeaderboardResponse {
        window: window.to_string(),
        entries,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaderboard_limit_defaults_and_clamps() {
        assert_eq!(leaderboard_limit(None), DEFAULT_LEADERBOARD_LIMIT);
        assert_eq!(leaderboard_limit(Some(0)), 0);
        assert_eq!(leaderboard_limit(Some(25)), 25);
        assert_eq!(
            leaderboard_limit(Some(MAX_LEADERBOARD_LIMIT + 1)),
            MAX_LEADERBOARD_LIMIT
        );
    }

    #[test]
    fn normalize_window_falls_back_to_all() {
        assert_eq!(normalize_window(Some("7d")), "7d");
        assert_eq!(normalize_window(Some("30d")), "30d");
        assert_eq!(normalize_window(Some("all")), "all");
        assert_eq!(normalize_window(Some("bogus")), "all");
        assert_eq!(normalize_window(None), "all");
    }

    #[test]
    fn window_since_ms_bounds() {
        let now = 100 * DAY_MS;
        assert_eq!(window_since_ms("all", now), 0);
        assert_eq!(window_since_ms("7d", now), now - 7 * DAY_MS);
        assert_eq!(window_since_ms("30d", now), now - 30 * DAY_MS);
        // Saturating: window longer than elapsed time clamps to 0.
        assert_eq!(window_since_ms("30d", 5 * DAY_MS), 0);
    }
}
