//! Integration coverage for the trader leaderboard (SYB-59).

mod common;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use common::{get, post_json, test_app, test_app_with_store};
use matching_sequencer::{
    AccountAuthScheme, AccountId, AuthenticatedProfileUpdate, PublicKey, RegisteredPubkey,
    SequencerHandle,
};
use p256::ecdsa::SigningKey;
use serde_json::Value;
use serde_json::json;
use sybil_api::app::create_router;
use sybil_api::config::ApiConfig;
use sybil_api::state::AppState;
use sybil_history_types::{EquityBaselines, EquityBaselinesQuery, ProjectionStatus};

#[derive(Clone, Copy)]
enum HistoryFrontier {
    BeforeBoundary,
    AtBoundary,
}

async fn projected_baselines(
    State(frontier): State<HistoryFrontier>,
    Json(query): Json<EquityBaselinesQuery>,
) -> Json<EquityBaselines> {
    let indexed_through_timestamp_ms = match frontier {
        HistoryFrontier::BeforeBoundary => query.at_or_before_ms.saturating_sub(1),
        HistoryFrontier::AtBoundary => query.at_or_before_ms,
    };
    Json(EquityBaselines {
        // These tests create their accounts after the seven-day cutoff, so a
        // caught-up projector legitimately has no opening anchors for them.
        baselines: vec![],
        status: ProjectionStatus {
            genesis_hash: Some([7; 32]),
            first_height: Some(1),
            first_timestamp_ms: Some(query.at_or_before_ms.saturating_sub(1)),
            indexed_through_height: Some(10),
            indexed_through_timestamp_ms: Some(indexed_through_timestamp_ms),
        },
    })
}

async fn app_with_history_frontier(handle: SequencerHandle, frontier: HistoryFrontier) -> Router {
    let history_app = Router::new()
        .route(
            "/internal/history/v1/query/equity-baselines",
            post(projected_baselines),
        )
        .with_state(frontier);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("history frontier listener");
    let addr = listener.local_addr().expect("history frontier address");
    tokio::spawn(async move {
        axum::serve(listener, history_app)
            .await
            .expect("history frontier server");
    });

    let config = ApiConfig {
        dev_mode: true,
        history_url: format!("http://{addr}"),
        ..ApiConfig::default()
    };
    let prometheus = metrics_exporter_prometheus::PrometheusBuilder::new()
        .build_recorder()
        .handle();
    let state = AppState::new(handle, &config, prometheus);
    state
        .initialize_read_models()
        .await
        .expect("leaderboard read model initializes");
    create_router(state)
}

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
    let yes_key = SigningKey::from_slice(&[1; 32]).unwrap();
    let no_key = SigningKey::from_slice(&[2; 32]).unwrap();
    let yes = handle
        .create_account_with_initial_key(
            1_000_000_000,
            PublicKey(*yes_key.verifying_key()),
            RegisteredPubkey::primary(AccountId(0), AccountAuthScheme::RawP256),
        )
        .await
        .unwrap();
    let no = handle
        .create_account_with_initial_key(
            1_000_000_000,
            PublicKey(*no_key.verifying_key()),
            RegisteredPubkey::primary(AccountId(0), AccountAuthScheme::RawP256),
        )
        .await
        .unwrap();
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
    for (account_id, signing_key) in [(yes.id, yes_key), (no.id, no_key)] {
        let signer = PublicKey(*signing_key.verifying_key());
        handle
            .set_profile_authenticated(AuthenticatedProfileUpdate {
                account_id,
                display_name: Some(format!("trader-{}", account_id.0)),
                avatar_seed: None,
                nonce: 1,
                signer,
            })
            .await
            .unwrap();
    }
    handle.produce_block().await.unwrap();
    (yes.id.0, no.id.0)
}

#[tokio::test]
async fn leaderboard_ranks_deterministically_and_excludes_system_accounts() {
    let (app, handle) = test_app_with_store(true).await;

    let (yes, no) = create_ranked_pair(&app, &handle).await;
    let funded_without_fill = handle.create_account(1_000_000_000).await.unwrap();
    let zero = handle.create_account(0).await.unwrap();

    let body = leaderboard(&app, "").await;
    assert_eq!(body["window"], "all");
    let entries = body["entries"].as_array().unwrap();

    // Funding is onboarding state, not trading activity. Only accounts with a
    // durable fill and signed public profile opt-in rank; never-traded,
    // never-funded, non-opted-in, and MINT stay out.
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
        assert_eq!(
            entry["display_name"],
            json!(format!("trader-{}", entry["account_id"].as_u64().unwrap()))
        );
    }

    // Determinism: identical requests return identical ordering.
    let again = leaderboard(&app, "").await;
    assert_eq!(again["entries"], body["entries"]);

    // Clearing the signed display name withdraws publication consent without
    // changing trading state.
    let yes_key = SigningKey::from_slice(&[1; 32]).unwrap();
    handle
        .set_profile_authenticated(AuthenticatedProfileUpdate {
            account_id: AccountId(yes),
            display_name: None,
            avatar_seed: None,
            nonce: 2,
            signer: PublicKey(*yes_key.verifying_key()),
        })
        .await
        .unwrap();
    handle.produce_block().await.unwrap();
    let after_clear = leaderboard(&app, "").await;
    assert_eq!(
        account_ids(after_clear["entries"].as_array().unwrap()),
        vec![no]
    );
}

#[tokio::test]
async fn leaderboard_honours_limit_cap_and_window_param() {
    let (app, handle) = test_app_with_store(true).await;
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

#[tokio::test]
async fn empty_windowed_leaderboard_does_not_require_history() {
    let (app, _handle) = test_app(true).await;

    let body = leaderboard(&app, "window=7d").await;
    assert_eq!(body["window"], "7d");
    assert_eq!(body["entries"], json!([]));
}

#[tokio::test]
async fn windowed_leaderboard_requires_history_to_reach_the_opening_boundary() {
    let (setup_app, handle) = test_app_with_store(true).await;
    let expected_ids = create_ranked_pair(&setup_app, &handle).await;

    let lagging_app =
        app_with_history_frontier(handle.clone(), HistoryFrontier::BeforeBoundary).await;
    let (status, body) = get(lagging_app, "/v1/leaderboard?window=7d").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let error: Value = serde_json::from_slice(&body).expect("structured history error");
    assert_eq!(error["code"], "HISTORY_INCOMPLETE");
    assert_eq!(
        error["error"],
        "Historical data has not caught up to the leaderboard window"
    );

    let exact_app = app_with_history_frontier(handle, HistoryFrontier::AtBoundary).await;
    let all_time = leaderboard(&exact_app, "window=all").await;
    let windowed = leaderboard(&exact_app, "window=7d").await;
    assert_eq!(
        account_ids(windowed["entries"].as_array().unwrap()),
        vec![expected_ids.0, expected_ids.1]
    );
    assert_eq!(
        windowed["entries"], all_time["entries"],
        "accounts created after the boundary legitimately use all of their lifetime PnL"
    );
}
