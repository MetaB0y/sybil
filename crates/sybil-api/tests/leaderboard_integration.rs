//! Integration coverage for the trader leaderboard (SYB-59).

mod common;

use axum::http::StatusCode;
use common::{get, post_json, test_app};
use matching_sequencer::SequencerHandle;
use serde_json::Value;
use serde_json::json;

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

async fn create_ranked_pair(app: &axum::Router, handle: &SequencerHandle) -> (u64, u64) {
    let yes = handle.create_account(1_000_000_000).await.unwrap();
    let no = handle.create_account(1_000_000_000).await.unwrap();
    let market_id = handle
        .create_market("Leaderboard market".into())
        .await
        .unwrap();

    for (account_id, order_type, price) in [
        (yes.id.0, "BuyYes", 600_000_000u64),
        (no.id.0, "BuyNo", 500_000_000u64),
    ] {
        let (status, body) = post_json(
            app.clone(),
            "/v1/orders",
            json!({
                "account_id": account_id,
                "orders": [{
                    "type": order_type,
                    "market_id": market_id.0,
                    "limit_price_nanos": price,
                    "quantity": 10
                }]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    }
    let block = handle.produce_block().await.unwrap();
    assert_eq!(block.canonical.fills.len(), 2);
    (yes.id.0, no.id.0)
}

#[tokio::test]
async fn leaderboard_ranks_deterministically_and_excludes_system_accounts() {
    let (app, handle) = test_app(true).await;

    let (yes, no) = create_ranked_pair(&app, &handle).await;
    let funded_without_fill = handle.create_account(1_000_000_000).await.unwrap();
    let zero = handle.create_account(0).await.unwrap();

    let body = leaderboard(&app, "").await;
    assert_eq!(body["window"], "all");
    let entries = body["entries"].as_array().unwrap();

    // Funding is onboarding state, not trading activity. Only accounts with a
    // durable fill rank; never-traded, never-funded, and MINT stay out.
    let ids = account_ids(entries);
    assert_eq!(ids, vec![yes, no], "filled accounts only, id tie-break asc");
    assert!(
        !ids.contains(&funded_without_fill.id.0),
        "funded account without a fill excluded"
    );
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
    create_ranked_pair(&app, &handle).await;

    // Explicit limit truncates the result set.
    let body = leaderboard(&app, "limit=1").await;
    assert_eq!(body["entries"].as_array().unwrap().len(), 1);

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
