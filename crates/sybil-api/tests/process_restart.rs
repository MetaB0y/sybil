//! Process-level restart tests for the sybil-api binary.
//!
//! These intentionally spawn the real binary instead of the in-process Axum
//! router, so they exercise CLI config, persistent-store hydration, actor
//! startup, and HTTP request/response boundaries together.

use std::time::Duration;

use matching_engine::{MarketId, Nanos, Order, Qty, shares_to_qty};
use matching_sequencer::AccountId;
use matching_sequencer::crypto::{canonical_cancel_bytes, canonical_order_bytes};
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use reqwest::StatusCode;
use serde_json::{Value, json};

mod common;

use common::process::{
    ProcessTestRoot, get_json, get_status_and_body, pause_blocks, post_json, restart_api,
    restart_api_with_env, resume_blocks, spawn_api, spawn_api_with_env, wait_for_block,
    wait_for_health, wait_for_height_at_least,
};

fn to_hex(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

fn new_signing_key() -> SigningKey {
    SigningKey::from_bytes((&[9u8; 32]).into()).expect("fixed signing key")
}

fn genesis_hash_from_health(health: &Value) -> [u8; 32] {
    let hash = health["genesis_hash"]
        .as_str()
        .expect("health exposes committed genesis_hash");
    let bytes = hex::decode(hash.strip_prefix("0x").unwrap_or(hash)).expect("genesis_hash hex");
    bytes.try_into().expect("genesis_hash is 32 bytes")
}

fn signed_buy_yes_payload(
    market_id: u32,
    limit_price_nanos: u64,
    quantity: u64,
    nonce: u64,
    genesis_hash: [u8; 32],
    key: &SigningKey,
) -> Value {
    let mut order = Order::new(0);
    order.markets[0] = MarketId::new(market_id);
    order.num_markets = 1;
    order.num_states = 2;
    order.limit_price = Nanos(limit_price_nanos);
    order.max_fill = Qty(quantity);
    order.payoffs[0] = 1;
    order.payoffs[1] = 0;

    let signature: Signature = key.sign(&canonical_order_bytes(&order, nonce, genesis_hash));
    json!({
        "signer_pubkey_hex": to_hex(key.verifying_key().to_sec1_point(true).as_bytes()),
        "order": {
            "market_ids": [market_id],
            "payoffs": [1, 0],
            "limit_price_nanos": limit_price_nanos,
            "max_fill": quantity
        },
        "nonce": nonce,
        "signature_hex": to_hex(signature.to_bytes().as_slice())
    })
}

fn signed_cancel_payload(
    account_id: u64,
    order_id: u64,
    nonce: u64,
    genesis_hash: [u8; 32],
    key: &SigningKey,
) -> Value {
    let signature: Signature = key.sign(&canonical_cancel_bytes(
        AccountId(account_id),
        order_id,
        nonce,
        genesis_hash,
    ));
    json!({
        "account_id": account_id,
        "order_id": order_id,
        "signer_pubkey_hex": to_hex(key.verifying_key().to_sec1_point(true).as_bytes()),
        "nonce": nonce,
        "signature_hex": to_hex(signature.to_bytes().as_slice())
    })
}

struct CrossingTrade {
    buyer: u64,
    seller: u64,
    market_id: u64,
    yes_price_nanos: u64,
    no_price_nanos: u64,
    quantity: u64,
}

async fn submit_crossing_trade(client: &reqwest::Client, base_url: &str, trade: CrossingTrade) {
    let CrossingTrade {
        buyer,
        seller,
        market_id,
        yes_price_nanos,
        no_price_nanos,
        quantity,
    } = trade;

    post_json(
        client,
        base_url,
        "/v1/orders",
        json!({
            "account_id": buyer,
            "orders": [{
                "type": "BuyYes",
                "market_id": market_id,
                "limit_price_nanos": yes_price_nanos,
                "quantity": quantity
            }]
        }),
    )
    .await;
    post_json(
        client,
        base_url,
        "/v1/orders",
        json!({
            "account_id": seller,
            "orders": [{
                "type": "BuyNo",
                "market_id": market_id,
                "limit_price_nanos": no_price_nanos,
                "quantity": quantity
            }]
        }),
    )
    .await;
}

fn assert_funding_history_once(history: &Value) {
    let events = history
        .as_array()
        .expect("funding history response is an array");
    let created: Vec<_> = events
        .iter()
        .filter(|event| event["type"].as_str() == Some("created"))
        .collect();
    assert_eq!(
        created.len(),
        1,
        "account creation history must appear once; history={history}"
    );
    assert_eq!(created[0]["amount_nanos"].as_i64(), Some(1_000));

    let deposits: Vec<_> = events
        .iter()
        .filter(|event| event["type"].as_str() == Some("deposit"))
        .collect();
    assert_eq!(
        deposits.len(),
        1,
        "deposit history must appear once; history={history}"
    );
    assert_eq!(deposits[0]["amount_nanos"].as_i64(), Some(250));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acknowledged_dev_api_writes_survive_kill_and_process_restart_before_next_block() {
    let root = ProcessTestRoot::new("process-restart");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();

    let writer = spawn_api(root.data_dir(), root.admin_key_path(), 50).await;
    wait_for_height_at_least(&client, &writer.base_url, 1).await;
    pause_blocks(&client, &writer.base_url).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let pre_write_health = wait_for_health(&client, &writer.base_url).await;
    let pre_write_height = pre_write_health["height"]
        .as_u64()
        .expect("baseline height exists before WAL writes");
    let genesis_hash = genesis_hash_from_health(&pre_write_health);

    let created = post_json(
        &client,
        &writer.base_url,
        "/v1/accounts",
        json!({ "initial_balance_nanos": 1_000u64 }),
    )
    .await;
    let account_id = created["account_id"].as_u64().unwrap();

    let funded = post_json(
        &client,
        &writer.base_url,
        &format!("/v1/accounts/{account_id}/fund"),
        json!({ "amount_nanos": 250u64 }),
    )
    .await;
    assert_eq!(funded["balance_nanos"].as_i64(), Some(1_250));

    let market = post_json(
        &client,
        &writer.base_url,
        "/v1/markets",
        json!({ "name": "process restart WAL market" }),
    )
    .await;
    let market_id = market["market_id"].as_u64().unwrap();

    let metadata_market = post_json(
        &client,
        &writer.base_url,
        "/v1/markets",
        json!({
            "name": "process restart metadata market",
            "description": "metadata should replay after process restart",
            "category": "restart-tests",
            "tags": ["persistence", "process"],
            "resolution_criteria": "resolved by the restart test",
            "expiry_timestamp_ms": 1_800_000_000_000u64,
            "resolution_template": "admin_immediate"
        }),
    )
    .await;
    let metadata_market_id = metadata_market["market_id"].as_u64().unwrap();

    let resolved_market = post_json(
        &client,
        &writer.base_url,
        "/v1/markets",
        json!({ "name": "process restart resolved market" }),
    )
    .await;
    let resolved_market_id = resolved_market["market_id"].as_u64().unwrap();
    post_json(
        &client,
        &writer.base_url,
        &format!("/v1/markets/{resolved_market_id}/resolve"),
        json!({ "payout_nanos": 1_000_000_000u64 }),
    )
    .await;

    let signing_key = new_signing_key();
    let public_key_hex = to_hex(signing_key.verifying_key().to_sec1_point(true).as_bytes());
    post_json(
        &client,
        &writer.base_url,
        &format!("/v1/accounts/{account_id}/keys"),
        json!({ "public_key_hex": public_key_hex }),
    )
    .await;

    let signed_order_nonce_1 =
        signed_buy_yes_payload(market_id as u32, 400, 3, 1, genesis_hash, &signing_key);
    post_json(
        &client,
        &writer.base_url,
        "/v1/orders/signed",
        signed_order_nonce_1.clone(),
    )
    .await;
    let pending = get_json(
        &client,
        &writer.base_url,
        &format!("/v1/accounts/{account_id}/orders"),
    )
    .await;
    let pending = pending
        .as_array()
        .expect("pending orders response is an array");
    assert_eq!(pending.len(), 1);
    let order_id = pending[0]["order_id"].as_u64().unwrap();

    let signed_cancel_nonce_2 =
        signed_cancel_payload(account_id, order_id, 2, genesis_hash, &signing_key);
    post_json(
        &client,
        &writer.base_url,
        "/v1/orders/cancel/signed",
        signed_cancel_nonce_2.clone(),
    )
    .await;
    let pending_after_cancel = get_json(
        &client,
        &writer.base_url,
        &format!("/v1/accounts/{account_id}/orders"),
    )
    .await;
    assert!(pending_after_cancel.as_array().unwrap().is_empty());
    let reader = restart_api(writer, &root, 60_000).await;
    let post_restart_health = wait_for_health(&client, &reader.base_url).await;
    assert_eq!(
        post_restart_health["height"].as_u64(),
        Some(pre_write_height),
        "health should report restored committed height before a new block is produced"
    );
    assert_eq!(genesis_hash_from_health(&post_restart_health), genesis_hash);
    pause_blocks(&client, &reader.base_url).await;

    let restored_account = get_json(
        &client,
        &reader.base_url,
        &format!("/v1/accounts/{account_id}"),
    )
    .await;
    assert_eq!(restored_account["balance_nanos"].as_i64(), Some(1_250));
    let restored_funding_history = get_json(
        &client,
        &reader.base_url,
        &format!("/v1/accounts/{account_id}/events?category=funding&limit=10"),
    )
    .await;
    assert_funding_history_once(&restored_funding_history);

    let restored_market = get_json(
        &client,
        &reader.base_url,
        &format!("/v1/markets/{market_id}"),
    )
    .await;
    assert_eq!(
        restored_market["name"].as_str(),
        Some("process restart WAL market")
    );
    let restored_metadata_market = get_json(
        &client,
        &reader.base_url,
        &format!("/v1/markets/{metadata_market_id}"),
    )
    .await;
    assert_eq!(
        restored_metadata_market["description"].as_str(),
        Some("metadata should replay after process restart")
    );
    assert_eq!(
        restored_metadata_market["category"].as_str(),
        Some("restart-tests")
    );
    assert_eq!(
        restored_metadata_market["tags"].as_array().unwrap()[0].as_str(),
        Some("persistence")
    );
    assert_eq!(
        restored_metadata_market["resolution_criteria"].as_str(),
        Some("resolved by the restart test")
    );
    assert_eq!(
        restored_metadata_market["expiry_timestamp_ms"].as_u64(),
        Some(1_800_000_000_000)
    );

    let restored_resolved_market = get_json(
        &client,
        &reader.base_url,
        &format!("/v1/markets/{resolved_market_id}"),
    )
    .await;
    assert_eq!(
        restored_resolved_market["status"].as_str(),
        Some("resolved")
    );
    assert_eq!(
        restored_resolved_market["payout_nanos"].as_u64(),
        Some(1_000_000_000)
    );

    let restored_pending = get_json(
        &client,
        &reader.base_url,
        &format!("/v1/accounts/{account_id}/orders"),
    )
    .await;
    assert!(
        restored_pending.as_array().unwrap().is_empty(),
        "signed cancel should survive restart without resurrecting the resting order"
    );

    let replay_order_resp = client
        .post(format!("{}/v1/orders/signed", reader.base_url))
        .json(&signed_order_nonce_1)
        .send()
        .await
        .expect("replay signed order request succeeds");
    let replay_order_status = replay_order_resp.status();
    let replay_order_body: Value =
        serde_json::from_str(&replay_order_resp.text().await.unwrap_or_default())
            .expect("replay signed order error body is JSON");
    assert_eq!(replay_order_status, StatusCode::CONFLICT);
    assert_eq!(
        replay_order_body["code"].as_str(),
        Some("REPLAY_NONCE_STALE")
    );

    let replay_cancel_resp = client
        .post(format!("{}/v1/orders/cancel/signed", reader.base_url))
        .json(&signed_cancel_nonce_2)
        .send()
        .await
        .expect("replay signed cancel request succeeds");
    let replay_cancel_status = replay_cancel_resp.status();
    let replay_cancel_body: Value =
        serde_json::from_str(&replay_cancel_resp.text().await.unwrap_or_default())
            .expect("replay signed cancel error body is JSON");
    // Cancel validation runs before replay-nonce validation, so after the
    // pre-restart cancel removed the order, the replay is rejected as not
    // found without consuming or otherwise consulting the stale nonce.
    assert_eq!(replay_cancel_status, StatusCode::NOT_FOUND);
    assert_eq!(replay_cancel_body["code"].as_str(), Some("NOT_FOUND"));

    post_json(
        &client,
        &reader.base_url,
        "/v1/orders/signed",
        signed_buy_yes_payload(market_id as u32, 450, 2, 3, genesis_hash, &signing_key),
    )
    .await;
    let post_restart_pending = get_json(
        &client,
        &reader.base_url,
        &format!("/v1/accounts/{account_id}/orders"),
    )
    .await;
    assert_eq!(post_restart_pending.as_array().unwrap().len(), 1);

    let committer = restart_api(reader, &root, 50).await;
    wait_for_height_at_least(&client, &committer.base_url, pre_write_height + 1).await;
    pause_blocks(&client, &committer.base_url).await;
    let committed_funding_history = get_json(
        &client,
        &committer.base_url,
        &format!("/v1/accounts/{account_id}/events?category=funding&limit=10"),
    )
    .await;
    assert_funding_history_once(&committed_funding_history);

    committer.kill().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn history_retention_and_candles_survive_process_restart() {
    let root = ProcessTestRoot::new("process-restart-history");
    let retention_env = [
        ("SYBIL_BLOCK_HISTORY_RETENTION_BLOCKS", "2"),
        ("SYBIL_RAW_PRICE_RETENTION_BLOCKS", "100"),
        ("SYBIL_HISTORY_PRUNE_INTERVAL_BLOCKS", "1"),
        ("SYBIL_HISTORY_PRUNE_MAX_ROWS", "100"),
        ("SYBIL_PRICE_CANDLE_RESOLUTIONS_SECS", "60"),
        ("SYBIL_PRICE_CANDLE_RETENTION_SECS", "2592000"),
    ];
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();

    let writer =
        spawn_api_with_env(root.data_dir(), root.admin_key_path(), 50, &retention_env).await;
    wait_for_height_at_least(&client, &writer.base_url, 1).await;
    pause_blocks(&client, &writer.base_url).await;

    let buyer = post_json(
        &client,
        &writer.base_url,
        "/v1/accounts",
        json!({ "initial_balance_nanos": 100_000_000_000u64 }),
    )
    .await["account_id"]
        .as_u64()
        .unwrap();
    let seller = post_json(
        &client,
        &writer.base_url,
        "/v1/accounts",
        json!({ "initial_balance_nanos": 100_000_000_000u64 }),
    )
    .await["account_id"]
        .as_u64()
        .unwrap();
    let market_id = post_json(
        &client,
        &writer.base_url,
        "/v1/markets",
        json!({ "name": "process restart history market" }),
    )
    .await["market_id"]
        .as_u64()
        .unwrap();

    let mut first_trade_height = None;
    let mut last_trade_height = 0;
    for (yes, no) in [
        (600_000_000u64, 500_000_000u64),
        (650_000_000u64, 450_000_000u64),
        (700_000_000u64, 400_000_000u64),
    ] {
        let before = wait_for_health(&client, &writer.base_url).await["height"]
            .as_u64()
            .expect("height while paused");
        submit_crossing_trade(
            &client,
            &writer.base_url,
            CrossingTrade {
                buyer,
                seller,
                market_id,
                yes_price_nanos: yes,
                no_price_nanos: no,
                quantity: 5,
            },
        )
        .await;
        resume_blocks(&client, &writer.base_url).await;
        let committed = wait_for_height_at_least(&client, &writer.base_url, before + 1).await;
        pause_blocks(&client, &writer.base_url).await;
        first_trade_height.get_or_insert(before + 1);
        last_trade_height = committed;
    }
    let first_trade_height = first_trade_height.expect("at least one trade committed");

    let reader = restart_api_with_env(writer, &root, 60_000, &retention_env).await;
    let restored_height = wait_for_health(&client, &reader.base_url).await["height"]
        .as_u64()
        .expect("restored height");
    assert!(
        restored_height >= last_trade_height,
        "restart should restore at least the last committed trade block"
    );
    pause_blocks(&client, &reader.base_url).await;

    let history = get_json(
        &client,
        &reader.base_url,
        &format!("/v1/markets/{market_id}/prices/history?limit=10"),
    )
    .await;
    let points = history["points"].as_array().unwrap();
    assert_eq!(
        points.len(),
        3,
        "raw price history after restart: {history}"
    );
    assert!(
        points
            .iter()
            .all(|point| point["volume_nanos"].as_u64().unwrap() > 0)
    );

    let candles = get_json(
        &client,
        &reader.base_url,
        &format!("/v1/markets/{market_id}/prices/candles?resolution=1m&limit=10"),
    )
    .await;
    assert!(
        candles["retention_min_bucket_ms"].as_u64().is_some(),
        "candle retention floor after restart: {candles}"
    );
    let point_count: u64 = candles["candles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|candle| candle["point_count"].as_u64().unwrap())
        .sum();
    assert_eq!(point_count, 3, "candle history after restart: {candles}");

    let (status, body) = get_status_and_body(
        &client,
        &reader.base_url,
        &format!("/v1/blocks/{first_trade_height}"),
    )
    .await;
    assert_eq!(status, StatusCode::GONE, "body={body}");
    assert_eq!(body["code"].as_str(), Some("RETENTION_GONE"));

    reader.kill().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deferred_bundle_revalidates_against_replayed_admit_after_process_restart() {
    let root = ProcessTestRoot::new("process-restart-deferred");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();

    let writer = spawn_api(root.data_dir(), root.admin_key_path(), 50).await;
    wait_for_height_at_least(&client, &writer.base_url, 1).await;
    pause_blocks(&client, &writer.base_url).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let pre_write_health = wait_for_health(&client, &writer.base_url).await;
    let pre_write_height = pre_write_health["height"]
        .as_u64()
        .expect("baseline height exists before deferred writes");

    let created = post_json(
        &client,
        &writer.base_url,
        "/v1/accounts",
        json!({ "initial_balance_nanos": 1_000_000_000u64 }),
    )
    .await;
    let account_id = created["account_id"].as_u64().unwrap();

    let market = post_json(
        &client,
        &writer.base_url,
        "/v1/markets",
        json!({ "name": "process restart deferred market" }),
    )
    .await;
    let market_id = market["market_id"].as_u64().unwrap();
    let one_share = shares_to_qty(1);

    post_json(
        &client,
        &writer.base_url,
        "/v1/orders",
        json!({
            "account_id": account_id,
            "orders": [{
                "type": "BuyYes",
                "market_id": market_id,
                "limit_price_nanos": 800_000_000u64,
                "quantity": one_share
            }]
        }),
    )
    .await;
    let pending_after_direct = get_json(
        &client,
        &writer.base_url,
        &format!("/v1/accounts/{account_id}/orders"),
    )
    .await;
    let direct_orders = pending_after_direct
        .as_array()
        .expect("pending orders response is an array");
    assert_eq!(direct_orders.len(), 1);
    let direct_order_id = direct_orders[0]["order_id"].as_u64().unwrap();

    post_json(
        &client,
        &writer.base_url,
        "/v1/orders",
        json!({
            "account_id": account_id,
            "orders": [
                {
                    "type": "BuyYes",
                    "market_id": market_id,
                    "limit_price_nanos": 600_000_000u64,
                    "quantity": one_share
                },
                {
                    "type": "BuyYes",
                    "market_id": market_id,
                    "limit_price_nanos": 600_000_000u64,
                    "quantity": one_share
                }
            ]
        }),
    )
    .await;
    let pending_after_deferred = get_json(
        &client,
        &writer.base_url,
        &format!("/v1/accounts/{account_id}/orders"),
    )
    .await;
    assert_eq!(
        pending_after_deferred.as_array().unwrap().len(),
        1,
        "deferred bundle must not appear as directly admitted resting orders"
    );

    let committer = restart_api(writer, &root, 50).await;
    let target_height = pre_write_height + 1;
    wait_for_height_at_least(&client, &committer.base_url, target_height).await;
    pause_blocks(&client, &committer.base_url).await;
    let restored_block = wait_for_block(&client, &committer.base_url, target_height).await;

    assert_eq!(
        restored_block["order_count"].as_u64(),
        Some(3),
        "restarted block should contain the replayed direct order plus two rejected deferred orders"
    );
    let rejections = restored_block["rejections"]
        .as_array()
        .expect("block rejections response is an array");
    assert_eq!(rejections.len(), 2, "restored block={restored_block}");
    let mut rejection_ids: Vec<u64> = rejections
        .iter()
        .map(|rejection| rejection["order_id"].as_u64().unwrap())
        .collect();
    rejection_ids.sort_unstable();
    assert_eq!(
        rejection_ids,
        vec![direct_order_id + 1, direct_order_id + 2],
        "restored deferred bundle must allocate fresh IDs after the replayed direct admit"
    );
    assert!(rejections.iter().all(|rejection| {
        rejection["account_id"].as_u64() == Some(account_id)
            && rejection["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("InsufficientBalance"))
    }));

    let post_restart_pending = get_json(
        &client,
        &committer.base_url,
        &format!("/v1/accounts/{account_id}/orders"),
    )
    .await;
    let pending = post_restart_pending.as_array().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0]["order_id"].as_u64(), Some(direct_order_id));

    committer.kill().await;
}
