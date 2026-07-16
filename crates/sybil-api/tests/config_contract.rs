//! Config-contract tests (SYB-245).
//!
//! Table-driven guards for the class of regression that shipped an HTTP 401 on
//! browser passkey onboarding: `POST /v1/accounts` and the first-key bootstrap
//! `POST /v1/accounts/{id}/keys` were mounted on the service-token-gated tier,
//! bypassed only in `dev_mode`, so production onboarding returned 401.
//!
//! Where `route_policy.rs` pins the *exact membership* of each tier, this file
//! derives its assertions from the authoritative route tables and the app's own
//! CORS / WebAuthn config, then exercises them against the real router. The two
//! files are complementary: pinning membership catches a table edit; driving the
//! router catches a tier whose *runtime behavior* diverges from its table.
//!
//! Contracts locked here:
//!  1. Route tier matrix — every PUBLIC route is reachable without a service
//!     token (never returns the service-gate 401); every SERVICE route 401s
//!     without a token, 403s with a wrong token, and passes the gate with the
//!     right one. Onboarding is PUBLIC; `fund_account` stays SERVICE-gated.
//!  2. CORS — a configured origin gets a working preflight + allow-origin grant;
//!     an unconfigured origin gets neither.
//!  3. WebAuthn RP-ID / origin — `WebAuthnVerifierConfig` maps 1:1 from
//!     `ApiConfig`. Full browser rp.id-omission coverage is deferred to the
//!     Playwright e2e (see `// SYB-244:` notes).

mod common;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sybil_api::app::{
    DEV_ROUTE_TABLE, OWNER_ROUTE_TABLE, PUBLIC_ROUTE_TABLE, RouteMount, SERVICE_ROUTE_TABLE,
};
use sybil_api::config::ApiConfig;
use sybil_api::webauthn::WebAuthnVerifierConfig;
use tower::ServiceExt;

use common::test_app_with_config;

const TOKEN: &str = "config-contract-token";

#[test]
fn service_bulk_order_default_has_full_catalog_headroom() {
    assert_eq!(ApiConfig::default().max_orders_per_submission, 512);
}

fn method_of(method: &str) -> Method {
    match method {
        "GET" => Method::GET,
        "POST" => Method::POST,
        "PUT" => Method::PUT,
        "DELETE" => Method::DELETE,
        other => panic!("unhandled route method {other}"),
    }
}

/// Turn a route template into a concrete request path (`{id}` -> `1`). The exact
/// value is irrelevant: service-gate rejections happen before the handler, and
/// downstream domain errors (bad hex, missing account) are never 401/403.
fn concretize(template: &str) -> String {
    template
        .split('/')
        .map(|seg| if seg.starts_with('{') { "1" } else { seg })
        .collect::<Vec<_>>()
        .join("/")
}

fn temp_event_dir() -> String {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "sybil-config-contract-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("event snapshot test dir exists");
    dir.to_string_lossy().into_owned()
}

async fn prod_app() -> axum::Router {
    let (app, _) = test_app_with_config(ApiConfig {
        dev_mode: false,
        service_token: TOKEN.to_string(),
        event_snapshot_dir: temp_event_dir(),
        ..ApiConfig::default()
    })
    .await;
    app
}

async fn cors_app(origins: Vec<String>) -> axum::Router {
    let (app, _) = test_app_with_config(ApiConfig {
        dev_mode: false,
        service_token: TOKEN.to_string(),
        cors_origins: origins,
        ..ApiConfig::default()
    })
    .await;
    app
}

fn build_request(method: Method, uri: &str, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method(method.clone()).uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if matches!(method, Method::POST | Method::PUT) {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        builder.body(Body::from("{}")).unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    }
}

/// Read only the response *status*, never the body. Streaming public routes
/// (`/v2/blocks/ws`) never completes its body, so
/// collecting it would hang; the status is available as soon as the handler
/// returns the response head.
async fn status_only(
    app: axum::Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
) -> StatusCode {
    let resp = tokio::time::timeout(
        Duration::from_secs(10),
        app.oneshot(build_request(method, uri, token)),
    )
    .await
    .expect("request did not hang")
    .unwrap();
    resp.status()
}

async fn request_full(
    app: axum::Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
) -> (StatusCode, Vec<u8>) {
    let resp = app
        .oneshot(build_request(method, uri, token))
        .await
        .unwrap();
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

async fn request_with_headers(
    app: axum::Router,
    method: Method,
    uri: &str,
    headers: HeaderMap,
) -> (StatusCode, HeaderMap) {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        let Some(name) = name else { continue };
        builder = builder.header(name, value);
    }
    let resp = app
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    (status, resp.headers().clone())
}

fn allow_origin(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn table_contains(table: &[RouteMount], method: &str, path: &str) -> bool {
    table
        .iter()
        .any(|mount| mount.method == method && mount.path == path)
}

// ── 1. Route tier matrix ────────────────────────────────────────────────────

/// Public onboarding is a fixed-grant route; arbitrary account funding and
/// legacy account creation remain service-gated.
/// A future tier move breaks this before it can reach a deploy.
#[test]
fn onboarding_is_public_and_fund_is_service_gated() {
    assert!(table_contains(
        PUBLIC_ROUTE_TABLE,
        "POST",
        "/v1/onboarding/accounts"
    ));
    assert!(table_contains(PUBLIC_ROUTE_TABLE, "GET", "/v1/onboarding"));

    for (method, path) in [
        ("POST", "/v1/accounts"),
        ("POST", "/v1/accounts/{id}/keys"),
        ("POST", "/v1/accounts/{id}/fund"),
        ("GET", "/v1/da/{height}/payload"),
        ("GET", "/v1/proofs/state/{leaf_key_hex}"),
    ] {
        assert!(
            table_contains(SERVICE_ROUTE_TABLE, method, path),
            "{method} {path} must stay service-token-gated"
        );
        assert!(
            !table_contains(PUBLIC_ROUTE_TABLE, method, path),
            "{method} {path} must NOT be public"
        );
    }
}

/// A route may live in exactly one tier. Overlap makes the trust boundary
/// ambiguous and lets a "move" silently no-op.
#[test]
fn route_tiers_are_disjoint() {
    for mount in PUBLIC_ROUTE_TABLE {
        assert!(
            !SERVICE_ROUTE_TABLE.contains(mount),
            "{mount:?} is in both PUBLIC and SERVICE tiers"
        );
        assert!(
            !DEV_ROUTE_TABLE.contains(mount),
            "{mount:?} is in both PUBLIC and DEV tiers"
        );
    }
    for mount in OWNER_ROUTE_TABLE {
        assert!(!PUBLIC_ROUTE_TABLE.contains(mount));
        assert!(!SERVICE_ROUTE_TABLE.contains(mount));
        assert!(!DEV_ROUTE_TABLE.contains(mount));
    }
    for mount in SERVICE_ROUTE_TABLE {
        assert!(
            !DEV_ROUTE_TABLE.contains(mount),
            "{mount:?} is in both SERVICE and DEV tiers"
        );
    }
}

/// Every PUBLIC route must be reachable with NO service token — it must never
/// return the service-gate 401. This is the direct, table-driven guard for the
/// onboarding regression: had the public command been service-gated, this sweep
/// would flag it as 401-without-a-token. (Domain 400/404/422 are fine; only the
/// service-gate 401 is forbidden. Bearer-gated reads are excluded — see below.)
#[tokio::test]
async fn every_public_route_is_reachable_without_service_token() {
    let app = prod_app().await;
    for mount in PUBLIC_ROUTE_TABLE {
        let uri = concretize(mount.path);
        let status = status_only(app.clone(), method_of(mount.method), &uri, None).await;
        assert_ne!(
            status,
            StatusCode::UNAUTHORIZED,
            "PUBLIC {} {} returned 401 without a service token — it must not be service-gated",
            mount.method,
            mount.path
        );
    }
}

/// Owner-gated reads 401 without a read-scoped token.
#[tokio::test]
async fn bearer_gated_public_reads_are_401_without_a_token() {
    let app = prod_app().await;
    for mount in OWNER_ROUTE_TABLE {
        let uri = concretize(mount.path);
        let (status, body) = request_full(app.clone(), method_of(mount.method), &uri, None).await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "{} {} is a bearer-gated read and must 401 without a token",
            mount.method,
            mount.path
        );
        // It is the *read-scope* gate, not the service gate.
        let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        assert_ne!(
            json["error"],
            json!("Missing service bearer token"),
            "{} {} 401 must come from the read-scope gate, not the service gate",
            mount.method,
            mount.path
        );
    }
}

/// Every SERVICE route must fail closed without a token, reject a wrong token,
/// and pass the gate with the right one — driven off the authoritative table so
/// a route that drifts out of the service tier is caught.
#[tokio::test]
async fn every_service_route_is_gated_and_passes_with_token() {
    let app = prod_app().await;
    for mount in SERVICE_ROUTE_TABLE {
        let uri = concretize(mount.path);
        let method = method_of(mount.method);

        // No token: fail closed with the service gate's own message.
        let (status, body) = request_full(app.clone(), method.clone(), &uri, None).await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "SERVICE {} {} must 401 without a token",
            mount.method,
            mount.path
        );
        let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        assert_eq!(
            json["error"],
            json!("Missing service bearer token"),
            "SERVICE {} {} 401 must come from the service gate",
            mount.method,
            mount.path
        );

        // Wrong token: forbidden.
        let status = status_only(app.clone(), method.clone(), &uri, Some("wrong-token")).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "SERVICE {} {} must 403 with a wrong token",
            mount.method,
            mount.path
        );

        // Correct token: the gate lets the request through (any non-auth status).
        let status = status_only(app.clone(), method, &uri, Some(TOKEN)).await;
        assert_ne!(
            status,
            StatusCode::UNAUTHORIZED,
            "SERVICE {} {} 401 even WITH a valid service token",
            mount.method,
            mount.path
        );
        assert_ne!(
            status,
            StatusCode::FORBIDDEN,
            "SERVICE {} {} 403 even WITH a valid service token",
            mount.method,
            mount.path
        );
    }
}

// ── 2. CORS contract ────────────────────────────────────────────────────────

/// A configured origin gets a working preflight (allow-origin echoed + the POST
/// method allowed) and an allow-origin grant on the actual request.
#[tokio::test]
async fn cors_allows_configured_origin_preflight_and_request() {
    let app = cors_app(vec!["https://app.example".to_string()]).await;

    let mut headers = HeaderMap::new();
    headers.insert(header::ORIGIN, "https://app.example".parse().unwrap());
    headers.insert(
        header::ACCESS_CONTROL_REQUEST_METHOD,
        "POST".parse().unwrap(),
    );
    let (status, resp_headers) =
        request_with_headers(app.clone(), Method::OPTIONS, "/v1/accounts", headers).await;
    assert!(
        status.is_success(),
        "preflight from a configured origin should succeed, got {status}"
    );
    assert_eq!(
        allow_origin(&resp_headers),
        Some("https://app.example".to_string()),
        "preflight must echo the configured origin"
    );
    let methods = resp_headers
        .get(header::ACCESS_CONTROL_ALLOW_METHODS)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        methods.to_ascii_uppercase().contains("POST"),
        "preflight allow-methods must permit POST, got '{methods}'"
    );

    // Actual cross-origin request also carries the allow-origin grant.
    let mut headers = HeaderMap::new();
    headers.insert(header::ORIGIN, "https://app.example".parse().unwrap());
    let (status, resp_headers) =
        request_with_headers(app, Method::GET, "/v1/health", headers).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        allow_origin(&resp_headers),
        Some("https://app.example".to_string())
    );
}

/// An origin that is not on the allowlist receives no allow-origin grant on
/// either the preflight or the actual request — the browser blocks it.
#[tokio::test]
async fn cors_rejects_unconfigured_origin() {
    let app = cors_app(vec!["https://app.example".to_string()]).await;

    let mut headers = HeaderMap::new();
    headers.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
    headers.insert(
        header::ACCESS_CONTROL_REQUEST_METHOD,
        "POST".parse().unwrap(),
    );
    let (_status, resp_headers) =
        request_with_headers(app.clone(), Method::OPTIONS, "/v1/accounts", headers).await;
    assert!(
        allow_origin(&resp_headers).is_none(),
        "an unconfigured origin must not receive a preflight allow-origin grant"
    );

    let mut headers = HeaderMap::new();
    headers.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
    let (status, resp_headers) =
        request_with_headers(app, Method::GET, "/v1/health", headers).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        allow_origin(&resp_headers).is_none(),
        "an unconfigured origin must not receive an allow-origin grant"
    );
}

/// With no configured origins, production emits no cross-origin CORS headers at
/// all (same-origin browser traffic still works). Locks the "empty = closed"
/// posture of `cors_layer`.
#[tokio::test]
async fn cors_is_closed_when_no_origins_configured() {
    let app = cors_app(vec![]).await;
    let mut headers = HeaderMap::new();
    headers.insert(header::ORIGIN, "https://app.example".parse().unwrap());
    let (status, resp_headers) =
        request_with_headers(app, Method::GET, "/v1/health", headers).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        allow_origin(&resp_headers).is_none(),
        "no configured origins must mean no cross-origin allow-origin header"
    );
}

// ── 3. WebAuthn RP-ID / origin contract ─────────────────────────────────────

/// The server's WebAuthn verifier config maps 1:1 from `ApiConfig`: rp_id,
/// accepted browser origin, and the user-verification requirement. A passkey
/// assertion is only accepted when its rpIdHash matches sha256(rp_id) and its
/// clientDataJSON.origin equals this configured origin (enforced in
/// `webauthn::verify_assertion`), so these three fields *are* the accept
/// contract.
///
/// SYB-244: the browser-side half of this contract — the frontend deliberately
/// omits `rp.id` at registration so the authenticator derives the RP ID from
/// the page origin's registrable domain, which must hash-match
/// `SYBIL_WEBAUTHN_RP_ID` — cannot be exercised from a Rust unit test (no
/// browser / navigator.credentials). It is asserted end-to-end in the Playwright
/// e2e. Here we lock only the server-side config mapping.
#[test]
fn webauthn_config_maps_from_api_config() {
    let default = WebAuthnVerifierConfig::from_api_config(&ApiConfig::default());
    assert_eq!(
        default.rp_id, "localhost",
        "default RP ID is localhost for frontend dev"
    );
    assert_eq!(
        default.origin, "http://localhost:3000",
        "default accepted browser origin"
    );
    assert!(
        default.require_user_verification,
        "user verification is required by default"
    );

    let custom = WebAuthnVerifierConfig::from_api_config(&ApiConfig {
        webauthn_rp_id: "sybil.example".to_string(),
        webauthn_origin: "https://sybil.example".to_string(),
        webauthn_require_uv: false,
        ..ApiConfig::default()
    });
    assert_eq!(custom.rp_id, "sybil.example");
    assert_eq!(custom.origin, "https://sybil.example");
    assert!(!custom.require_user_verification);
}
