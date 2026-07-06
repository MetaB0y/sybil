//! SYB-60 account profile + key-management integration tests.
//!
//! Exercises the signed-mutation surface end to end over the in-process router:
//! profile set (signed), signing-key revocation incl. last-key refusal, and the
//! read-only bearer API key lifecycle (create show-once, list, revoke, and the
//! bearer-gated private-summary extractor accept/reject).

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use common::{get, post_json, test_app};
use http_body_util::BodyExt;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use serde_json::{json, Value};
use tower::ServiceExt;

use matching_sequencer::crypto::{
    canonical_api_key_create_bytes, canonical_api_key_revoke_bytes, canonical_key_revocation_bytes,
    canonical_profile_update_bytes,
};
use matching_sequencer::AccountId;

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn signing_key(seed: u8) -> SigningKey {
    SigningKey::from_bytes((&[seed; 32]).into()).expect("fixed signing key")
}

fn pubkey_hex(key: &SigningKey) -> String {
    to_hex(key.verifying_key().to_sec1_point(true).as_bytes())
}

fn parse(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("valid JSON body")
}

/// Create a dev-mode account and register `key` as its primary signing key.
async fn account_with_key(app: &axum::Router, seed: u8) -> (u64, SigningKey) {
    let (_, body) = post_json(
        app.clone(),
        "/v1/accounts",
        json!({ "initial_balance_nanos": 100_000_000_000u64 }),
    )
    .await;
    let account_id = parse(&body)["account_id"].as_u64().unwrap();
    let key = signing_key(seed);
    let (status, _) = post_json(
        app.clone(),
        &format!("/v1/accounts/{account_id}/keys"),
        json!({ "public_key_hex": pubkey_hex(&key) }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    (account_id, key)
}

async fn register_extra_key(app: &axum::Router, account_id: u64, key: &SigningKey, label: &str) {
    let (status, _) = post_json(
        app.clone(),
        &format!("/v1/accounts/{account_id}/keys"),
        json!({ "public_key_hex": pubkey_hex(key), "scope": "agent", "label": label }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

async fn get_with_bearer(app: axum::Router, uri: &str, token: &str) -> (StatusCode, Vec<u8>) {
    let req = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let body = resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, body)
}

#[tokio::test]
async fn signed_profile_set_updates_account_response() {
    let (app, _handle) = test_app(true).await;
    let (account_id, key) = account_with_key(&app, 11).await;

    let nonce = 1_700_000_000_000u64;
    let display_name = "Alice";
    let avatar_seed = "seed-abc";
    let sig: Signature = key.sign(&canonical_profile_update_bytes(
        AccountId(account_id),
        Some(display_name),
        Some(avatar_seed),
        nonce,
    ));
    let (status, body) = post_json(
        app.clone(),
        &format!("/v1/accounts/{account_id}/profile"),
        json!({
            "display_name": display_name,
            "avatar_seed": avatar_seed,
            "signer_pubkey_hex": pubkey_hex(&key),
            "signature_hex": to_hex(sig.to_bytes().as_slice()),
            "nonce": nonce,
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "body: {}",
        String::from_utf8_lossy(&body)
    );

    let (status, body) = get(app, &format!("/v1/accounts/{account_id}")).await;
    assert_eq!(status, StatusCode::OK);
    let account = parse(&body);
    assert_eq!(account["display_name"], display_name);
    assert_eq!(account["avatar_seed"], avatar_seed);
}

#[tokio::test]
async fn profile_set_rejects_bad_signature_and_replay() {
    let (app, _handle) = test_app(true).await;
    let (account_id, key) = account_with_key(&app, 12).await;
    let other = signing_key(99);

    // Signature by a key that isn't the claimed signer → 400 invalid signature.
    let nonce = 5u64;
    let sig: Signature = other.sign(&canonical_profile_update_bytes(
        AccountId(account_id),
        Some("X"),
        None,
        nonce,
    ));
    let (status, _) = post_json(
        app.clone(),
        &format!("/v1/accounts/{account_id}/profile"),
        json!({
            "display_name": "X",
            "signer_pubkey_hex": pubkey_hex(&key),
            "signature_hex": to_hex(sig.to_bytes().as_slice()),
            "nonce": nonce,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A valid mutation then a replay at the same nonce → 409.
    let sig: Signature = key.sign(&canonical_profile_update_bytes(
        AccountId(account_id),
        Some("Ok"),
        None,
        nonce,
    ));
    let payload = json!({
        "display_name": "Ok",
        "signer_pubkey_hex": pubkey_hex(&key),
        "signature_hex": to_hex(sig.to_bytes().as_slice()),
        "nonce": nonce,
    });
    let (status, _) = post_json(
        app.clone(),
        &format!("/v1/accounts/{account_id}/profile"),
        payload.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = post_json(app, &format!("/v1/accounts/{account_id}/profile"), payload).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn list_keys_reflects_registered_metadata() {
    let (app, _handle) = test_app(true).await;
    let (account_id, _primary) = account_with_key(&app, 13).await;
    let agent = signing_key(23);
    register_extra_key(&app, account_id, &agent, "agent:pricer").await;

    let (status, body) = get(app, &format!("/v1/accounts/{account_id}/keys")).await;
    assert_eq!(status, StatusCode::OK);
    let keys = parse(&body);
    let arr = keys.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    let scopes: Vec<&str> = arr.iter().map(|k| k["scope"].as_str().unwrap()).collect();
    assert!(scopes.contains(&"primary"));
    assert!(scopes.contains(&"agent"));
    let agent_entry = arr.iter().find(|k| k["scope"] == "agent").unwrap();
    assert_eq!(agent_entry["label"], "agent:pricer");
    assert_eq!(agent_entry["public_key_hex"], pubkey_hex(&agent));
}

#[tokio::test]
async fn revoke_last_key_is_refused_but_second_key_can_be_revoked() {
    let (app, _handle) = test_app(true).await;
    let (account_id, primary) = account_with_key(&app, 14).await;

    // Revoking the sole key must be refused (lockout protection) → 409.
    let nonce = 1u64;
    let target = pubkey_hex(&primary);
    let sig: Signature = primary.sign(&canonical_key_revocation_bytes(
        AccountId(account_id),
        &hex::decode(&target).unwrap(),
        nonce,
    ));
    let (status, body) = post_json(
        app.clone(),
        &format!("/v1/accounts/{account_id}/keys/revoke"),
        json!({
            "target_pubkey_hex": target,
            "signer_pubkey_hex": target,
            "signature_hex": to_hex(sig.to_bytes().as_slice()),
            "nonce": nonce,
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "body: {}",
        String::from_utf8_lossy(&body)
    );

    // Add a second key, then revoking the agent key succeeds (one remains).
    let agent = signing_key(24);
    register_extra_key(&app, account_id, &agent, "agent").await;
    let agent_hex = pubkey_hex(&agent);
    let nonce = 2u64;
    let sig: Signature = primary.sign(&canonical_key_revocation_bytes(
        AccountId(account_id),
        &hex::decode(&agent_hex).unwrap(),
        nonce,
    ));
    let (status, _) = post_json(
        app.clone(),
        &format!("/v1/accounts/{account_id}/keys/revoke"),
        json!({
            "target_pubkey_hex": agent_hex,
            "signer_pubkey_hex": target,
            "signature_hex": to_hex(sig.to_bytes().as_slice()),
            "nonce": nonce,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The revoked key is gone; only the primary remains.
    let (_, body) = get(app, &format!("/v1/accounts/{account_id}/keys")).await;
    let arr = parse(&body);
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["public_key_hex"], target);
}

#[tokio::test]
async fn api_key_create_show_once_then_gate_private_summary() {
    let (app, _handle) = test_app(true).await;
    let (account_id, key) = account_with_key(&app, 15).await;

    // Create a read API key (signed). The token is shown exactly once.
    let nonce = 1u64;
    let label = "grafana";
    let sig: Signature = key.sign(&canonical_api_key_create_bytes(
        AccountId(account_id),
        Some(label),
        nonce,
    ));
    let (status, body) = post_json(
        app.clone(),
        &format!("/v1/accounts/{account_id}/api-keys"),
        json!({
            "label": label,
            "signer_pubkey_hex": pubkey_hex(&key),
            "signature_hex": to_hex(sig.to_bytes().as_slice()),
            "nonce": nonce,
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "body: {}",
        String::from_utf8_lossy(&body)
    );
    let created = parse(&body);
    let token = created["token"].as_str().unwrap().to_string();
    let key_id = created["id"].as_u64().unwrap();
    assert!(token.starts_with("sybk_"));

    // The listing never exposes the token or its hash.
    let (status, body) = get(app.clone(), &format!("/v1/accounts/{account_id}/api-keys")).await;
    assert_eq!(status, StatusCode::OK);
    let list = parse(&body);
    let entry = &list.as_array().unwrap()[0];
    assert_eq!(entry["id"].as_u64().unwrap(), key_id);
    assert_eq!(entry["label"], label);
    assert!(entry.get("token").is_none());
    assert!(entry.get("hash").is_none());

    // Bearer extractor: missing token → 401.
    let (status, _) = get(
        app.clone(),
        &format!("/v1/accounts/{account_id}/private-summary"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Wrong token → 401.
    let (status, _) = get_with_bearer(
        app.clone(),
        &format!("/v1/accounts/{account_id}/private-summary"),
        "sybk_deadbeef",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Valid token → 200 with private fields.
    let (status, body) = get_with_bearer(
        app.clone(),
        &format!("/v1/accounts/{account_id}/private-summary"),
        &token,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "body: {}",
        String::from_utf8_lossy(&body)
    );
    let summary = parse(&body);
    assert_eq!(summary["account_id"].as_u64().unwrap(), account_id);
    assert_eq!(summary["balance_nanos"].as_i64().unwrap(), 100_000_000_000);

    // A token scoped to this account cannot read another account.
    let (other_id, _other_key) = account_with_key(&app, 16).await;
    let (status, _) = get_with_bearer(
        app.clone(),
        &format!("/v1/accounts/{other_id}/private-summary"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Revoke the key (signed) → the bearer token stops working.
    let nonce = 2u64;
    let sig: Signature = key.sign(&canonical_api_key_revoke_bytes(
        AccountId(account_id),
        key_id,
        nonce,
    ));
    let (status, _) = post_json(
        app.clone(),
        &format!("/v1/accounts/{account_id}/api-keys/revoke"),
        json!({
            "api_key_id": key_id,
            "signer_pubkey_hex": pubkey_hex(&key),
            "signature_hex": to_hex(sig.to_bytes().as_slice()),
            "nonce": nonce,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = get_with_bearer(
        app.clone(),
        &format!("/v1/accounts/{account_id}/private-summary"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // The revoked key remains listed with a revocation timestamp for audit.
    let (_, body) = get(app, &format!("/v1/accounts/{account_id}/api-keys")).await;
    let entry = parse(&body);
    let entry = &entry.as_array().unwrap()[0];
    assert!(entry["revoked_at_ms"].as_u64().is_some());
}
