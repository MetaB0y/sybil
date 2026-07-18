//! SYB-60 account profile + key-management integration tests.
//!
//! Exercises the signed-mutation surface end to end over the in-process router:
//! profile set (signed), signing-key revocation incl. last-key refusal, and the
//! read-only bearer API key lifecycle (create show-once, list, revoke, and the
//! bearer-gated private-summary extractor accept/reject).

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ciborium::value::Value as CborValue;
use common::{get, post_json, test_app_with_config};
use http_body_util::BodyExt;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use sybil_api::config::ApiConfig;
use tower::ServiceExt;

use matching_engine::{MarketId, Nanos, Order, Qty};
use matching_sequencer::crypto::{
    canonical_api_key_create_bytes, canonical_api_key_revoke_bytes,
    canonical_key_registration_bytes, canonical_key_revocation_bytes, canonical_order_bytes,
    canonical_profile_update_bytes,
};
use matching_sequencer::{
    AccountAuthScheme, AccountId, AuthenticatedKeyRegistration, KeyOpAuth, KeyScope, PublicKey,
    SequencerHandle,
};

const SERVICE_TOKEN: &str = "account-management-service";
const PASSKEY_ORIGIN: &str = "https://app.172-104-31-54.nip.io";

async fn test_app(_dev_mode: bool) -> (axum::Router, SequencerHandle) {
    test_app_with_config(ApiConfig {
        dev_mode: false,
        service_token: SERVICE_TOKEN.to_string(),
        ..ApiConfig::default()
    })
    .await
}

async fn get_as_service(app: axum::Router, uri: &str) -> (StatusCode, Vec<u8>) {
    get_with_bearer(app, uri, SERVICE_TOKEN).await
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn signing_key(seed: u8) -> SigningKey {
    SigningKey::from_bytes((&[seed; 32]).into()).expect("fixed signing key")
}

fn pubkey_hex(key: &SigningKey) -> String {
    to_hex(key.verifying_key().to_sec1_point(true).as_bytes())
}

fn webauthn_registration(key: &SigningKey) -> Value {
    let point = key.verifying_key().to_sec1_point(false);
    let bytes = point.as_bytes();
    let cose_key = CborValue::Map(vec![
        (CborValue::Integer(1.into()), CborValue::Integer(2.into())),
        (
            CborValue::Integer(3.into()),
            CborValue::Integer((-7).into()),
        ),
        (
            CborValue::Integer((-1).into()),
            CborValue::Integer(1.into()),
        ),
        (
            CborValue::Integer((-2).into()),
            CborValue::Bytes(bytes[1..33].to_vec()),
        ),
        (
            CborValue::Integer((-3).into()),
            CborValue::Bytes(bytes[33..65].to_vec()),
        ),
    ]);
    let mut cose_bytes = Vec::new();
    ciborium::ser::into_writer(&cose_key, &mut cose_bytes).unwrap();

    let mut auth_data = sybil_verifier::EXPECTED_RP_ID_HASH.to_vec();
    auth_data.push(0x01 | 0x04 | 0x40);
    auth_data.extend_from_slice(&1u32.to_be_bytes());
    auth_data.extend_from_slice(&[0x11; 16]);
    auth_data.extend_from_slice(&(12u16).to_be_bytes());
    auth_data.extend_from_slice(b"credential-1");
    auth_data.extend_from_slice(&cose_bytes);
    let attestation = CborValue::Map(vec![
        (
            CborValue::Text("fmt".into()),
            CborValue::Text("none".into()),
        ),
        (
            CborValue::Text("authData".into()),
            CborValue::Bytes(auth_data),
        ),
        (CborValue::Text("attStmt".into()), CborValue::Map(vec![])),
    ]);
    let mut attestation_bytes = Vec::new();
    ciborium::ser::into_writer(&attestation, &mut attestation_bytes).unwrap();
    let client_data = json!({
        "type": "webauthn.create",
        "challenge": "registration-fixture",
        "origin": PASSKEY_ORIGIN,
        "crossOrigin": false,
    })
    .to_string()
    .into_bytes();
    json!({
        "attestation_object_b64url": URL_SAFE_NO_PAD.encode(attestation_bytes),
        "client_data_json_b64url": URL_SAFE_NO_PAD.encode(client_data),
    })
}

fn webauthn_assertion(key: &SigningKey, canonical: &[u8], counter: u32) -> Value {
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(canonical));
    let client_data = json!({
        "type": "webauthn.get",
        "challenge": challenge,
        "origin": PASSKEY_ORIGIN,
        "crossOrigin": false,
    })
    .to_string()
    .into_bytes();
    let mut authenticator_data = sybil_verifier::EXPECTED_RP_ID_HASH.to_vec();
    authenticator_data.push(0x01 | 0x04);
    authenticator_data.extend_from_slice(&counter.to_be_bytes());
    let mut signed_message = authenticator_data.clone();
    signed_message.extend_from_slice(&Sha256::digest(&client_data));
    let signature: Signature = key.sign(&signed_message);
    json!({
        "credential_id_b64url": URL_SAFE_NO_PAD.encode(b"credential-1"),
        "authenticator_data_b64url": URL_SAFE_NO_PAD.encode(authenticator_data),
        "client_data_json_b64url": URL_SAFE_NO_PAD.encode(client_data),
        "signature_b64url": URL_SAFE_NO_PAD.encode(signature.to_der().as_bytes()),
    })
}

fn parse(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("valid JSON body")
}

async fn keyop_binding(app: &axum::Router, account_id: u64) -> ([u8; 32], [u8; 32]) {
    let (status, body) = get_as_service(app.clone(), &format!("/v1/accounts/{account_id}")).await;
    assert_eq!(status, StatusCode::OK);
    let account = parse(&body);
    let keys: [u8; 32] = hex::decode(account["keys_digest_hex"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let events: [u8; 32] = hex::decode(account["events_digest_hex"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    (keys, events)
}

/// Create a public fixed-grant account and register `key` as its primary key.
async fn account_with_key(app: &axum::Router, seed: u8) -> (u64, SigningKey) {
    let key = signing_key(seed);
    let (_, body) = post_json(
        app.clone(),
        "/v1/onboarding/accounts",
        json!({
            "initial_key": {"public_key_hex": pubkey_hex(&key)}
        }),
    )
    .await;
    let account_id = parse(&body)["account_id"].as_u64().unwrap();
    (account_id, key)
}

/// Establish the genesis hash (SYB-224) needed by the signed register path.
async fn ensure_genesis(handle: &SequencerHandle) -> [u8; 32] {
    if let Some(g) = handle.get_genesis_hash().await.unwrap() {
        return g;
    }
    handle.produce_block().await.unwrap();
    handle
        .get_genesis_hash()
        .await
        .unwrap()
        .expect("genesis hash after first committed block")
}

/// SYB-229: register an additional agent key via the SIGNED path, authorized by
/// `signer` (an existing account key).
#[allow(
    clippy::too_many_arguments,
    reason = "integration helper carries the complete signed key-registration context"
)]
async fn register_extra_key(
    app: &axum::Router,
    account_id: u64,
    signer: &SigningKey,
    new_key: &SigningKey,
    label: &str,
    _nonce: u64,
    genesis_hash: [u8; 32],
) -> StatusCode {
    let binding = keyop_binding(app, account_id).await;
    register_extra_key_at_binding(
        app,
        account_id,
        signer,
        new_key,
        label,
        genesis_hash,
        binding,
    )
    .await
}

async fn register_extra_key_at_binding(
    app: &axum::Router,
    account_id: u64,
    signer: &SigningKey,
    new_key: &SigningKey,
    label: &str,
    genesis_hash: [u8; 32],
    (bound_keys_digest, bound_events_digest): ([u8; 32], [u8; 32]),
) -> StatusCode {
    let new_hex = pubkey_hex(new_key);
    let signer_hex = pubkey_hex(signer);
    let key_record = sybil_verifier::KeyRecord {
        auth_scheme: AccountAuthScheme::RawP256.canonical_byte(),
        pubkey_sec1: hex::decode(&new_hex).unwrap().try_into().unwrap(),
        capability_mask: sybil_verifier::KeyRecord::FULL_CAPABILITY_MASK,
    };
    let sig: Signature = signer.sign(&canonical_key_registration_bytes(
        genesis_hash,
        AccountId(account_id),
        &key_record,
        bound_keys_digest,
        bound_events_digest,
    ));
    let (status, _) = post_json(
        app.clone(),
        &format!("/v1/accounts/{account_id}/keys/register"),
        json!({
            "public_key_hex": new_hex,
            "scope": "agent",
            "label": label,
            "signer_pubkey_hex": signer_hex,
            "signature_hex": to_hex(sig.to_bytes().as_slice()),
            "bound_keys_digest_hex": hex::encode(bound_keys_digest),
            "bound_events_digest_hex": hex::encode(bound_events_digest),
        }),
    )
    .await;
    status
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

async fn post_with_bearer(
    app: axum::Router,
    uri: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Vec<u8>) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
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

    let (status, body) = get_as_service(app, &format!("/v1/accounts/{account_id}")).await;
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
    let (app, handle) = test_app(true).await;
    let (account_id, primary) = account_with_key(&app, 13).await;
    let genesis = ensure_genesis(&handle).await;
    let agent = signing_key(23);
    let status = register_extra_key(
        &app,
        account_id,
        &primary,
        &agent,
        "agent:pricer",
        1,
        genesis,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = get_as_service(app, &format!("/v1/accounts/{account_id}/keys")).await;
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
    let (app, handle) = test_app(true).await;
    let (account_id, primary) = account_with_key(&app, 14).await;
    let genesis = ensure_genesis(&handle).await;

    // Revoking the sole key must be refused (lockout protection) → 409.
    let target = pubkey_hex(&primary);
    let (bound_keys_digest, bound_events_digest) = keyop_binding(&app, account_id).await;
    let target_record = sybil_verifier::KeyRecord {
        auth_scheme: AccountAuthScheme::RawP256.canonical_byte(),
        pubkey_sec1: hex::decode(&target).unwrap().try_into().unwrap(),
        capability_mask: sybil_verifier::KeyRecord::FULL_CAPABILITY_MASK,
    };
    let sig: Signature = primary.sign(&canonical_key_revocation_bytes(
        genesis,
        AccountId(account_id),
        &target_record,
        bound_keys_digest,
        bound_events_digest,
    ));
    let (status, body) = post_json(
        app.clone(),
        &format!("/v1/accounts/{account_id}/keys/revoke"),
        json!({
            "target_pubkey_hex": target,
            "signer_pubkey_hex": target,
            "signature_hex": to_hex(sig.to_bytes().as_slice()),
            "bound_keys_digest_hex": hex::encode(bound_keys_digest),
            "bound_events_digest_hex": hex::encode(bound_events_digest),
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
    let status = register_extra_key(&app, account_id, &primary, &agent, "agent", 1, genesis).await;
    assert_eq!(status, StatusCode::OK);
    let agent_hex = pubkey_hex(&agent);
    let (bound_keys_digest, bound_events_digest) = keyop_binding(&app, account_id).await;
    let agent_record = sybil_verifier::KeyRecord {
        auth_scheme: AccountAuthScheme::RawP256.canonical_byte(),
        pubkey_sec1: hex::decode(&agent_hex).unwrap().try_into().unwrap(),
        capability_mask: sybil_verifier::KeyRecord::FULL_CAPABILITY_MASK,
    };
    let sig: Signature = primary.sign(&canonical_key_revocation_bytes(
        genesis,
        AccountId(account_id),
        &agent_record,
        bound_keys_digest,
        bound_events_digest,
    ));
    let (status, _) = post_json(
        app.clone(),
        &format!("/v1/accounts/{account_id}/keys/revoke"),
        json!({
            "target_pubkey_hex": agent_hex,
            "signer_pubkey_hex": target,
            "signature_hex": to_hex(sig.to_bytes().as_slice()),
            "bound_keys_digest_hex": hex::encode(bound_keys_digest),
            "bound_events_digest_hex": hex::encode(bound_events_digest),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The revoked key is gone; only the primary remains.
    let (_, body) = get_as_service(app, &format!("/v1/accounts/{account_id}/keys")).await;
    let arr = parse(&body);
    let arr = arr.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["public_key_hex"], target);
}

#[tokio::test]
async fn webauthn_keyop_register_and_revoke_fixture_seals_valid_block() {
    let (app, handle) = test_app_with_config(ApiConfig {
        dev_mode: false,
        service_token: SERVICE_TOKEN.to_string(),
        webauthn_rp_id: sybil_verifier::EXPECTED_WEBAUTHN_RP_ID.to_string(),
        webauthn_origin: PASSKEY_ORIGIN.to_string(),
        webauthn_require_uv: true,
        ..ApiConfig::default()
    })
    .await;
    let primary = signing_key(71);
    let (_, body) = post_json(
        app.clone(),
        "/v1/onboarding/accounts",
        json!({
            "initial_key": {
                "public_key_hex": pubkey_hex(&primary),
                "auth_scheme": "webauthn",
                "credential_id_b64url": URL_SAFE_NO_PAD.encode(b"credential-1"),
                "webauthn_registration": webauthn_registration(&primary),
            }
        }),
    )
    .await;
    let account_id = parse(&body)["account_id"].as_u64().unwrap();
    let genesis = ensure_genesis(&handle).await;

    let agent = signing_key(72);
    let agent_record = sybil_verifier::KeyRecord {
        auth_scheme: AccountAuthScheme::RawP256.canonical_byte(),
        pubkey_sec1: agent
            .verifying_key()
            .to_sec1_point(true)
            .as_bytes()
            .try_into()
            .unwrap(),
        capability_mask: sybil_verifier::KeyRecord::FULL_CAPABILITY_MASK,
    };
    let (keys_digest, events_digest) = keyop_binding(&app, account_id).await;
    let canonical = canonical_key_registration_bytes(
        genesis,
        AccountId(account_id),
        &agent_record,
        keys_digest,
        events_digest,
    );
    let (status, body) = post_json(
        app.clone(),
        &format!("/v1/accounts/{account_id}/keys/register"),
        json!({
            "public_key_hex": pubkey_hex(&agent),
            "auth_scheme": "raw_p256",
            "scope": "agent",
            "signer_pubkey_hex": pubkey_hex(&primary),
            "signer_auth_scheme": "webauthn",
            "webauthn_assertion": webauthn_assertion(&primary, &canonical, 2),
            "bound_keys_digest_hex": hex::encode(keys_digest),
            "bound_events_digest_hex": hex::encode(events_digest),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (keys_digest, events_digest) = keyop_binding(&app, account_id).await;
    let canonical = canonical_key_revocation_bytes(
        genesis,
        AccountId(account_id),
        &agent_record,
        keys_digest,
        events_digest,
    );
    let (status, body) = post_json(
        app.clone(),
        &format!("/v1/accounts/{account_id}/keys/revoke"),
        json!({
            "target_pubkey_hex": pubkey_hex(&agent),
            "signer_pubkey_hex": pubkey_hex(&primary),
            "auth_scheme": "webauthn",
            "webauthn_assertion": webauthn_assertion(&primary, &canonical, 3),
            "bound_keys_digest_hex": hex::encode(keys_digest),
            "bound_events_digest_hex": hex::encode(events_digest),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let market_id = handle
        .create_market("WebAuthn fixture".to_string())
        .await
        .unwrap();
    let mut order = Order::new(0);
    order.markets[0] = MarketId::new(market_id.0);
    order.num_markets = 1;
    order.num_states = 2;
    order.payoffs[0] = 1;
    order.limit_price = Nanos(500_000_000);
    order.max_fill = Qty(3);
    let canonical = canonical_order_bytes(&order, 1, genesis);
    let (status, body) = post_json(
        app.clone(),
        "/v1/orders/signed",
        json!({
            "signer_pubkey_hex": pubkey_hex(&primary),
            "order": {
                "market_ids": [market_id.0],
                "payoffs": [1, 0],
                "limit_price_nanos": 500_000_000u64,
                "max_fill": 3u64,
            },
            "nonce": 1,
            "auth_scheme": "webauthn",
            "webauthn_assertion": webauthn_assertion(&primary, &canonical, 4),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    handle.produce_block().await.unwrap();
}

// --- SYB-229 signed key registration ---------------------------------------

#[tokio::test]
async fn atomic_onboarding_rejects_oversized_signing_key_labels_without_allocating_account() {
    let (app, handle) = test_app(true).await;
    let primary = signing_key(80);
    let cap = matching_sequencer::MAX_SIGNING_KEY_LABEL_BYTES;

    for label in ["x".repeat(cap + 1), "é".repeat(cap / 2 + 1)] {
        let (status, body) = post_json(
            app.clone(),
            "/v1/onboarding/accounts",
            json!({
                "initial_key": {
                    "public_key_hex": pubkey_hex(&primary),
                    "label": label,
                }
            }),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{}",
            String::from_utf8_lossy(&body)
        );
        assert!(
            handle.get_account(AccountId(0)).await.unwrap().is_none(),
            "invalid initial-key metadata must not allocate an account"
        );
    }

    let exact = "é".repeat(cap / 2);
    assert_eq!(exact.len(), cap);
    let (status, body) = post_json(
        app,
        "/v1/onboarding/accounts",
        json!({
            "initial_key": {
                "public_key_hex": pubkey_hex(&primary),
                "label": exact,
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(parse(&body)["account_id"], 0);
    let keys = handle.signing_keys_for_account(AccountId(0)).await.unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].1.label.as_deref(), Some(exact.as_str()));
}

#[tokio::test]
async fn first_key_bootstrap_label_limit_precedes_key_mutation() {
    let (app, handle) = test_app(true).await;
    let (status, body) = post_with_bearer(
        app.clone(),
        "/v1/accounts",
        SERVICE_TOKEN,
        json!({ "initial_balance_nanos": 0u64 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let account_id = parse(&body)["account_id"].as_u64().unwrap();
    let before = handle
        .get_account(AccountId(account_id))
        .await
        .unwrap()
        .unwrap();
    let primary = signing_key(81);
    let cap = matching_sequencer::MAX_SIGNING_KEY_LABEL_BYTES;

    let (status, body) = post_with_bearer(
        app.clone(),
        &format!("/v1/accounts/{account_id}/keys"),
        SERVICE_TOKEN,
        json!({
            "public_key_hex": pubkey_hex(&primary),
            "label": "x".repeat(cap + 1),
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert!(
        handle
            .signing_keys_for_account(AccountId(account_id))
            .await
            .unwrap()
            .is_empty()
    );
    let after = handle
        .get_account(AccountId(account_id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.last_nonce, before.last_nonce);
    assert_eq!(after.keys_digest, before.keys_digest);
    assert_eq!(after.events_digest, before.events_digest);

    let exact = "x".repeat(cap);
    let (status, body) = post_with_bearer(
        app,
        &format!("/v1/accounts/{account_id}/keys"),
        SERVICE_TOKEN,
        json!({
            "public_key_hex": pubkey_hex(&primary),
            "label": exact,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let keys = handle
        .signing_keys_for_account(AccountId(account_id))
        .await
        .unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].1.label.as_deref(), Some(exact.as_str()));
}

#[tokio::test]
async fn signed_additional_key_label_limit_preserves_key_state_and_nonce() {
    let (app, handle) = test_app(true).await;
    let (account_id, primary) = account_with_key(&app, 82).await;
    let genesis = ensure_genesis(&handle).await;
    let candidate = signing_key(83);
    let before = handle
        .get_account(AccountId(account_id))
        .await
        .unwrap()
        .unwrap();
    let cap = matching_sequencer::MAX_SIGNING_KEY_LABEL_BYTES;

    let status = register_extra_key(
        &app,
        account_id,
        &primary,
        &candidate,
        &"x".repeat(cap + 1),
        1,
        genesis,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let after = handle
        .get_account(AccountId(account_id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.last_nonce, before.last_nonce);
    assert_eq!(after.keys_digest, before.keys_digest);
    assert_eq!(after.events_digest, before.events_digest);
    assert_eq!(
        handle
            .signing_keys_for_account(AccountId(account_id))
            .await
            .unwrap()
            .len(),
        1
    );

    let exact = "é".repeat(cap / 2);
    let status =
        register_extra_key(&app, account_id, &primary, &candidate, &exact, 2, genesis).await;
    assert_eq!(status, StatusCode::OK);
    let keys = handle
        .signing_keys_for_account(AccountId(account_id))
        .await
        .unwrap();
    assert_eq!(keys.len(), 2);
    assert!(
        keys.iter()
            .any(|(_, meta)| meta.label.as_deref() == Some(exact.as_str()))
    );
}

/// A fresh account rejects a SECOND unsigned key over the (now service-tier,
/// dev-bypassed) first-key endpoint — the unsigned path is first-key only.
#[tokio::test]
async fn unsigned_register_second_key_conflicts() {
    let (app, _handle) = test_app(true).await;
    let (account_id, _primary) = account_with_key(&app, 40).await;

    let intruder = signing_key(41);
    let (status, body) = post_with_bearer(
        app.clone(),
        &format!("/v1/accounts/{account_id}/keys"),
        SERVICE_TOKEN,
        json!({ "public_key_hex": pubkey_hex(&intruder) }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "second unsigned key must be refused: {}",
        String::from_utf8_lossy(&body)
    );

    // The intruder key was never attached.
    let (_, body) = get_as_service(app, &format!("/v1/accounts/{account_id}/keys")).await;
    let arr = parse(&body);
    assert_eq!(arr.as_array().unwrap().len(), 1);
}

/// Signed registration by an existing key attaches the new key; replaying an
/// old state binding is refused.
#[tokio::test]
async fn signed_register_accepted_and_replay_rejected() {
    let (app, handle) = test_app(true).await;
    let (account_id, primary) = account_with_key(&app, 42).await;
    let genesis = ensure_genesis(&handle).await;
    let stale_binding = keyop_binding(&app, account_id).await;

    let agent = signing_key(43);
    let status = register_extra_key(
        &app,
        account_id,
        &primary,
        &agent,
        "agent:pricer",
        7,
        genesis,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, body) = get_as_service(app.clone(), &format!("/v1/accounts/{account_id}/keys")).await;
    let arr = parse(&body);
    assert_eq!(arr.as_array().unwrap().len(), 2);

    // A different operation signed against the already-consumed state binding
    // is rejected with 409 before it can enter the control-plane WAL.
    let other = signing_key(44);
    let status = register_extra_key_at_binding(
        &app,
        account_id,
        &primary,
        &other,
        "agent:dup",
        genesis,
        stale_binding,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "replayed state binding must be refused"
    );
}

/// A registration signed by a key that belongs to a DIFFERENT account is
/// refused (signer/account mismatch), and one signed by an unregistered key is
/// refused as an unknown signer.
#[tokio::test]
async fn signed_register_rejects_wrong_signer() {
    let (app, handle) = test_app(true).await;
    let (account_a, _key_a) = account_with_key(&app, 50).await;
    let (_account_b, key_b) = account_with_key(&app, 51).await;
    let genesis = ensure_genesis(&handle).await;

    // key_b is a valid key, but on account B → mismatch (403) for account A.
    let new_key = signing_key(52);
    let status = register_extra_key(&app, account_a, &key_b, &new_key, "agent", 1, genesis).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "cross-account signer must 403"
    );

    // A signer not registered anywhere → unknown signer (404).
    let stranger = signing_key(53);
    let status =
        register_extra_key(&app, account_a, &stranger, &new_key, "agent", 2, genesis).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown signer must 404");

    // Neither attempt attached the new key.
    let (_, body) = get_as_service(app, &format!("/v1/accounts/{account_a}/keys")).await;
    assert_eq!(parse(&body).as_array().unwrap().len(), 1);
}

/// A post-assertion authenticated intent is accepted only against its current
/// state-bound account digests.
#[tokio::test]
async fn authenticated_register_is_state_bound_and_one_shot() {
    let (app, handle) = test_app(true).await;
    let (account_id, primary) = account_with_key(&app, 60).await;
    let _ = ensure_genesis(&handle).await;

    let signer = PublicKey(*primary.verifying_key());
    let new_key = signing_key(61);
    let new_pubkey = PublicKey(*new_key.verifying_key());
    let account = handle
        .get_account(AccountId(account_id))
        .await
        .unwrap()
        .unwrap();
    let bound_keys_digest = account.keys_digest;
    let bound_events_digest = account.events_digest;

    let intent = || AuthenticatedKeyRegistration {
        account_id: AccountId(account_id),
        new_pubkey: new_pubkey.clone(),
        new_auth_scheme: AccountAuthScheme::WebAuthn,
        label: Some("passkey-agent".to_string()),
        scope: KeyScope::Agent,
        bound_keys_digest,
        bound_events_digest,
        signer: signer.clone(),
        authorization: KeyOpAuth::WebAuthn {
            signer_pubkey: signer.compressed_bytes().try_into().unwrap(),
            authenticator_data: vec![],
            client_data_json: vec![],
            signature: [0; 64],
        },
    };

    handle.register_key_authenticated(intent()).await.unwrap();

    let (_, body) = get_as_service(app, &format!("/v1/accounts/{account_id}/keys")).await;
    let arr = parse(&body);
    assert_eq!(arr.as_array().unwrap().len(), 2);
    let entry = arr
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["public_key_hex"] == pubkey_hex(&new_key))
        .expect("new key present");
    assert_eq!(entry["auth_scheme"], "webauthn");
    assert_eq!(entry["scope"], "agent");

    // Reusing the old binding (with a fresh key so duplicate-key validation is
    // not involved) is refused after the first op advances both digests.
    let third_key = signing_key(62);
    let replay = AuthenticatedKeyRegistration {
        account_id: AccountId(account_id),
        new_pubkey: PublicKey(*third_key.verifying_key()),
        new_auth_scheme: AccountAuthScheme::WebAuthn,
        label: None,
        scope: KeyScope::Agent,
        bound_keys_digest,
        bound_events_digest,
        signer: signer.clone(),
        authorization: KeyOpAuth::WebAuthn {
            signer_pubkey: signer.compressed_bytes().try_into().unwrap(),
            authenticator_data: vec![],
            client_data_json: vec![],
            signature: [0; 64],
        },
    };
    let err = handle.register_key_authenticated(replay).await;
    assert!(err.is_err(), "replayed state binding must be refused");
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
    let (status, body) =
        get_as_service(app.clone(), &format!("/v1/accounts/{account_id}/api-keys")).await;
    assert_eq!(status, StatusCode::OK);
    let list = parse(&body);
    let entry = &list.as_array().unwrap()[0];
    assert_eq!(entry["id"].as_u64().unwrap(), key_id);
    assert_eq!(entry["label"], label);
    assert!(entry.get("token").is_none());
    assert!(entry.get("hash").is_none());

    // Missing token → 401 in production mode.
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
    assert_eq!(
        common::nanos_i64(&summary["balance_nanos"]),
        ApiConfig::default().public_account_grant_nanos as i64
    );

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
    let (_, body) = get_as_service(app, &format!("/v1/accounts/{account_id}/api-keys")).await;
    let entry = parse(&body);
    let entry = &entry.as_array().unwrap()[0];
    assert!(entry["revoked_at_ms"].as_u64().is_some());
}
