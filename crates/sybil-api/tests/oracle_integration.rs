//! End-to-end tests for the attestation-based resolution flow.

mod common;

use axum::http::StatusCode;
use matching_engine::{NANOS_PER_DOLLAR, Nanos};
use matching_sequencer::crypto::sign_attestation;
use p256::ecdsa::SigningKey;
use p256::elliptic_curve::rand_core::UnwrapErr;
use sybil_oracle::ResolutionAttestation;

use common::{get, post_json, test_app_with_bootstrap};

#[tokio::test]
async fn register_feed_happy_path() {
    let (app, _handle, _admin_key, _admin_id) = test_app_with_bootstrap(true).await;

    // Fresh P256 pubkey for a new feed.
    let key = <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(&mut UnwrapErr(
        getrandom::SysRng,
    ));
    let pubkey_hex =
        hex::encode(matching_sequencer::PublicKey(*key.verifying_key()).compressed_bytes());

    let (status, body) = post_json(
        app,
        "/v1/feeds",
        serde_json::json!({
            "pubkey_hex": pubkey_hex,
            "name": "polymarket_mirror",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "body: {:?}",
        String::from_utf8_lossy(&body)
    );
    let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["name"], "polymarket_mirror");
    assert_eq!(resp["pubkey_hex"], pubkey_hex);
}

#[tokio::test]
async fn signed_resolve_via_polymarket_template_succeeds() {
    let (app, handle, _admin_key, _admin_id) = test_app_with_bootstrap(true).await;

    // Register polymarket_mirror feed + template.
    let key = <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(&mut UnwrapErr(
        getrandom::SysRng,
    ));
    let pubkey = matching_sequencer::PublicKey(*key.verifying_key());
    let pubkey_hex = hex::encode(pubkey.compressed_bytes());

    let feed_id = handle
        .register_feed(
            sybil_oracle::FeedPubkey(pubkey.compressed_bytes()),
            "polymarket_mirror".into(),
        )
        .await
        .unwrap();
    handle
        .install_template(sybil_oracle::ResolutionTemplate {
            id: sybil_oracle::TemplateId("polymarket_mirror".into()),
            policy: sybil_oracle::ResolutionPolicy::Immediate { feed_id },
        })
        .await
        .unwrap();

    // Create a market with the polymarket_mirror template.
    let (status, body) = post_json(
        app.clone(),
        "/v1/markets",
        serde_json::json!({
            "name": "Will X happen?",
            "resolution_template": "polymarket_mirror",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "body: {:?}",
        String::from_utf8_lossy(&body)
    );
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let market_id = v["market_id"].as_u64().unwrap() as u32;

    // Sign an attestation and POST.
    let attestation = ResolutionAttestation {
        market_id: matching_engine::MarketId::new(market_id),
        payout_nanos: Nanos(NANOS_PER_DOLLAR),
        nonce: 1_700_000_000,
    };
    let signed = sign_attestation(attestation, &key);

    let (status, body) = post_json(
        app.clone(),
        &format!("/v1/markets/{}/resolve", market_id),
        serde_json::json!({
            "payout_nanos": NANOS_PER_DOLLAR,
            "attestation": {
                "pubkey_hex": pubkey_hex,
                "signature_hex": hex::encode(&signed.signature_der),
                "nonce": signed.attestation.nonce,
            },
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "expected 200 for signed resolve, body: {:?}",
        String::from_utf8_lossy(&body)
    );

    // GET /v1/markets/:id/resolution reflects the resolution.
    let (status, body) = get(app, &format!("/v1/markets/{}/resolution", market_id)).await;
    assert_eq!(status, StatusCode::OK);
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res["status"], "resolved");
    assert_eq!(res["payout_nanos"], NANOS_PER_DOLLAR);
    assert_eq!(res["resolved_by_feed_name"], "polymarket_mirror");
    assert_eq!(res["template"], "polymarket_mirror");
}

#[tokio::test]
async fn mis_signed_attestation_rejected() {
    let (app, handle, _admin_key, _admin_id) = test_app_with_bootstrap(true).await;

    // Register polymarket_mirror with one pubkey but sign with a different one.
    let registered_key = <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
        &mut UnwrapErr(getrandom::SysRng),
    );
    let attacker_key = <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
        &mut UnwrapErr(getrandom::SysRng),
    );
    let registered_pubkey = matching_sequencer::PublicKey(*registered_key.verifying_key());

    let feed_id = handle
        .register_feed(
            sybil_oracle::FeedPubkey(registered_pubkey.compressed_bytes()),
            "polymarket_mirror".into(),
        )
        .await
        .unwrap();
    handle
        .install_template(sybil_oracle::ResolutionTemplate {
            id: sybil_oracle::TemplateId("polymarket_mirror".into()),
            policy: sybil_oracle::ResolutionPolicy::Immediate { feed_id },
        })
        .await
        .unwrap();

    let (_s, body) = post_json(
        app.clone(),
        "/v1/markets",
        serde_json::json!({
            "name": "Rigged?",
            "resolution_template": "polymarket_mirror",
        }),
    )
    .await;
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let market_id = v["market_id"].as_u64().unwrap() as u32;

    // Sign the attestation with the attacker's key, but claim to be the
    // attacker's pubkey (so signature verifies but pubkey != registered feed).
    let attestation = ResolutionAttestation {
        market_id: matching_engine::MarketId::new(market_id),
        payout_nanos: Nanos(0),
        nonce: 42,
    };
    let signed = sign_attestation(attestation, &attacker_key);

    let (status, body) = post_json(
        app,
        &format!("/v1/markets/{}/resolve", market_id),
        serde_json::json!({
            "payout_nanos": 0,
            "attestation": {
                "pubkey_hex": hex::encode(&signed.signer.0),
                "signature_hex": hex::encode(&signed.signature_der),
                "nonce": signed.attestation.nonce,
            },
        }),
    )
    .await;
    assert_ne!(
        status,
        StatusCode::OK,
        "attacker-signed attestation must NOT succeed; got body: {:?}",
        String::from_utf8_lossy(&body)
    );
}

#[tokio::test]
async fn create_market_rejects_unknown_resolution_template() {
    let (app, _handle, _admin_key, _admin_id) = test_app_with_bootstrap(true).await;

    let (status, body) = post_json(
        app,
        "/v1/markets",
        serde_json::json!({
            "name": "Will anything happen?",
            "resolution_template": "does_not_exist",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "expected 400 for unknown template, body: {:?}",
        String::from_utf8_lossy(&body)
    );
}

#[tokio::test]
async fn register_feed_rejects_pubkey_conflict() {
    let (app, _handle, _admin_key, _admin_id) = test_app_with_bootstrap(true).await;

    let key = <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(&mut UnwrapErr(
        getrandom::SysRng,
    ));
    let pubkey_hex =
        hex::encode(matching_sequencer::PublicKey(*key.verifying_key()).compressed_bytes());

    // First registration succeeds.
    let (status, _) = post_json(
        app.clone(),
        "/v1/feeds",
        serde_json::json!({
            "pubkey_hex": pubkey_hex,
            "name": "polymarket_mirror",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Same pubkey, same name is idempotent.
    let (status, _) = post_json(
        app.clone(),
        "/v1/feeds",
        serde_json::json!({
            "pubkey_hex": pubkey_hex,
            "name": "polymarket_mirror",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Same pubkey, different name must 409.
    let (status, body) = post_json(
        app,
        "/v1/feeds",
        serde_json::json!({
            "pubkey_hex": pubkey_hex,
            "name": "kleros_bridge",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "expected 409 for pubkey reuse with different name, body: {:?}",
        String::from_utf8_lossy(&body)
    );
}
