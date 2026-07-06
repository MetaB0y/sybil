//! Integration coverage for the trader leaderboard (SYB-59).

mod common;

use axum::http::StatusCode;
use common::{get, test_app};
use serde_json::Value;

async fn leaderboard(app: &axum::Router, query: &str) -> Value {
    let uri = if query.is_empty() {
        "/v1/leaderboard".to_string()
    } else {
        format!("/v1/leaderboard?{query}")
    };
    let (status, body) = get(app.clone(), &uri).await;
    assert_eq!(status, StatusCode::OK, "GET {uri}");
    serde_json::from_slice(&body).expect("leaderboard body is valid JSON")
}

fn account_ids(entries: &[Value]) -> Vec<u64> {
    entries
        .iter()
        .map(|e| e["account_id"].as_u64().unwrap())
        .collect()
}

#[tokio::test]
async fn leaderboard_ranks_deterministically_and_excludes_system_accounts() {
    let (app, handle) = test_app(true).await;

    // Funded accounts (ids 0,1,2) plus one never-funded account (id 3).
    for _ in 0..3 {
        handle.create_account(1_000_000_000).await.unwrap();
    }
    let zero = handle.create_account(0).await.unwrap();

    let body = leaderboard(&app, "").await;
    assert_eq!(body["window"], "all");
    let entries = body["entries"].as_array().unwrap();

    // Never-funded and the system MINT account are excluded; only the three
    // funded accounts rank.
    let ids = account_ids(entries);
    assert_eq!(ids, vec![0, 1, 2], "funded accounts only, id tie-break asc");
    assert!(!ids.contains(&zero.id.0), "zero-deposit account excluded");
    assert!(
        !ids.contains(&u64::MAX),
        "system MINT account must be excluded"
    );

    // Ranks are 1-based and sequential; equal PnL breaks by ascending id.
    for (index, entry) in entries.iter().enumerate() {
        assert_eq!(entry["rank"].as_u64().unwrap(), (index as u64) + 1);
    }

    // Determinism: identical requests return identical ordering.
    let again = leaderboard(&app, "").await;
    assert_eq!(again["entries"], body["entries"]);
}

#[tokio::test]
async fn leaderboard_honours_limit_cap_and_window_param() {
    let (app, handle) = test_app(true).await;
    for _ in 0..5 {
        handle.create_account(1_000_000_000).await.unwrap();
    }

    // Explicit limit truncates the result set.
    let body = leaderboard(&app, "limit=2").await;
    assert_eq!(body["entries"].as_array().unwrap().len(), 2);

    // Window tokens are echoed back canonically; unknown values fall back.
    for (query, expected) in [
        ("window=7d", "7d"),
        ("window=30d", "30d"),
        ("window=all", "all"),
        ("window=bogus", "all"),
    ] {
        let body = leaderboard(&app, query).await;
        assert_eq!(body["window"], expected, "query {query}");
    }
}
