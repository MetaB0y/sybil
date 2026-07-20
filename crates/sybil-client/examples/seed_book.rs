//! Deterministic crossing-order fixture for deployment and Compose smoke tests.
//!
//! `seed_book` atomically creates exactly two zero-balance accounts with fixed
//! P256 identities, funds each account with $10, creates one named binary
//! market, and submits two deterministic signed GTC orders:
//!
//! - BuyYes: 0.60, quantity 1000
//! - BuyNo:  0.50, quantity 2000
//!
//! The first 1000 units cross via minting. The remaining 1000-unit BuyNo pins
//! the dual, so the exact YES/NO clearing vector is [0.50, 0.50].
//!
//! The v1 fixture is deliberately single-use per chain state, not idempotent:
//! fixed signing keys may only belong to one account and fixed replay nonces
//! may only be accepted once. Re-running against the same state fails closed;
//! use a fresh throwaway state/volume. A future fixture change must use a new
//! `FIXTURE_VERSION` and new keys/nonces rather than silently changing v1.
//!
//! Safety: a positive dev/demo marker in `/v1/health` permits seeding. Current
//! servers do not expose one, so callers must pass `--i-know-this-is-dev`.
//! Explicit mainnet/production markers are never overridden by that flag.

use std::process::ExitCode;
use std::time::Duration;

use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use reqwest::{Client, Method};
use serde::Serialize;
use serde_json::{Value, json};
use sybil_signing::{MAX_MARKETS_PER_ORDER, MAX_STATES, MarketId, Order, canonical_order_bytes};

const FIXTURE_VERSION: &str = "SYB-247-v1";
const MARKET_NAME: &str = "SYB-247 deterministic crossing v1";
const ACCOUNT_FUNDING_NANOS: u64 = 10_000_000_000;
const YES_LIMIT_NANOS: u64 = 600_000_000;
const NO_LIMIT_NANOS: u64 = 500_000_000;
const YES_QUANTITY: u64 = 1_000;
const NO_QUANTITY: u64 = 2_000;
const MATCHED_VOLUME: u64 = 1_000;
const CLEARING_PRICE_NANOS: u64 = 500_000_000;
const NONCE_BASE: u64 = 247_000_001;

#[derive(Debug)]
struct Args {
    base_url: String,
    service_token: Option<String>,
    explicit_dev_ack: bool,
    run_id: u64,
}

#[derive(Debug, Serialize)]
struct HttpStep {
    name: &'static str,
    method: String,
    path: String,
    status: u16,
}

#[derive(Debug, Serialize)]
struct GuardSummary {
    health_status: String,
    positive_dev_marker: bool,
    explicit_dev_ack: bool,
}

#[derive(Debug, Serialize)]
struct MarketSummary {
    market_id: u32,
    name: String,
}

#[derive(Debug, Serialize)]
struct AccountSummary {
    role: &'static str,
    account_id: u64,
    public_key_hex: String,
    funded_balance_nanos: u64,
    order_nonce: u64,
}

#[derive(Debug, Serialize)]
struct OrderSummary {
    side: &'static str,
    order_id: u64,
    account_id: u64,
    limit_price_nanos: u64,
    quantity: u64,
    expected_fill_quantity: u64,
    expected_fill_price_nanos: u64,
}

#[derive(Debug, Serialize)]
struct ExpectedSummary {
    matched_volume: u64,
    total_fill_quantity: u64,
    fill_count: u64,
    yes_price_nanos: u64,
    no_price_nanos: u64,
    total_volume_nanos: u64,
    total_welfare_nanos: u64,
    funded_balance_total_nanos: u64,
    marked_position_value_nanos: u64,
    post_trade_balance_total_nanos: u64,
}

#[derive(Debug, Serialize)]
struct SeedSummary {
    schema: &'static str,
    fixture_version: String,
    semantics: &'static str,
    account_count: usize,
    guard: GuardSummary,
    market: MarketSummary,
    accounts: Vec<AccountSummary>,
    orders: Vec<OrderSummary>,
    expected: ExpectedSummary,
    http_steps: Vec<HttpStep>,
}

fn usage() -> &'static str {
    "Usage: seed_book --base-url URL [--service-token TOKEN] [--run-id N] [--i-know-this-is-dev]\n\
     \n\
     Seeds the single-use deterministic SYB-247-v1 fixture and prints one JSON\n\
     summary to stdout. Current Sybil health responses have no dev/demo marker,\n\
     so --i-know-this-is-dev is required. Re-run only against fresh state. Use a\n\
     new numeric --run-id for another deterministic seed on persistent devnets."
}

fn parse_args() -> Result<Args, String> {
    let mut base_url = None;
    let mut service_token = None;
    let mut explicit_dev_ack = false;
    let mut run_id = 0;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("{}", usage());
                std::process::exit(0);
            }
            "--base-url" => {
                i += 1;
                base_url = Some(
                    args.get(i)
                        .ok_or("--base-url requires a value")?
                        .trim_end_matches('/')
                        .to_string(),
                );
            }
            "--service-token" => {
                i += 1;
                service_token = Some(
                    args.get(i)
                        .ok_or("--service-token requires a value")?
                        .to_string(),
                );
            }
            "--run-id" => {
                i += 1;
                run_id = args
                    .get(i)
                    .ok_or("--run-id requires a value")?
                    .parse()
                    .map_err(|_| "--run-id must be a u64")?;
            }
            "--i-know-this-is-dev" => explicit_dev_ack = true,
            unknown => return Err(format!("unknown argument {unknown:?}\n{}", usage())),
        }
        i += 1;
    }
    let base_url = base_url.ok_or_else(|| format!("--base-url is required\n{}", usage()))?;
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
        return Err("--base-url must start with http:// or https://".to_string());
    }
    if service_token.as_deref() == Some("") {
        service_token = None;
    }
    order_nonce(run_id, 2)?;
    key_seed(run_id, 2)?;
    Ok(Args {
        base_url,
        service_token,
        explicit_dev_ack,
        run_id,
    })
}

fn string_field<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("response is missing string field {field:?}: {value}"))
}

fn u64_field(value: &Value, field: &str) -> Result<u64, String> {
    let raw = value
        .get(field)
        .ok_or_else(|| format!("response is missing u64 field {field:?}: {value}"))?;
    raw.as_u64()
        .or_else(|| raw.as_str()?.parse().ok())
        .ok_or_else(|| format!("response has invalid u64 field {field:?}: {value}"))
}

fn safe_environment_marker(health: &Value) -> bool {
    health.get("dev_mode").and_then(Value::as_bool) == Some(true)
        || health.get("demo").and_then(Value::as_bool) == Some(true)
        || ["environment", "mode", "network"]
            .iter()
            .filter_map(|field| health.get(field).and_then(Value::as_str))
            .any(|value| {
                matches!(
                    value.to_ascii_lowercase().as_str(),
                    "dev" | "development" | "devnet" | "demo" | "local" | "test"
                )
            })
}

fn explicit_production_marker(health: &Value) -> Option<String> {
    ["environment", "mode", "network"].iter().find_map(|field| {
        let value = health.get(field)?.as_str()?;
        matches!(
            value.to_ascii_lowercase().as_str(),
            "prod" | "production" | "mainnet"
        )
        .then(|| format!("{field}={value}"))
    })
}

async fn request_json(
    client: &Client,
    args: &Args,
    steps: &mut Vec<HttpStep>,
    name: &'static str,
    method: Method,
    path: &str,
    body: Option<&Value>,
) -> Result<Value, String> {
    let mut request = client.request(method.clone(), format!("{}{}", args.base_url, path));
    if let Some(token) = args.service_token.as_deref() {
        request = request.bearer_auth(token);
    }
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("{name}: {method} {path}: {error}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| format!("{name}: read response: {error}"))?;
    if !status.is_success() {
        return Err(format!(
            "{name}: {method} {path} returned HTTP {}: {text}",
            status.as_u16()
        ));
    }
    let value: Value = serde_json::from_str(&text).map_err(|error| {
        format!(
            "{name}: HTTP {} was not JSON: {error}: {text}",
            status.as_u16()
        )
    })?;
    steps.push(HttpStep {
        name,
        method: method.as_str().to_string(),
        path: path.to_string(),
        status: status.as_u16(),
    });
    Ok(value)
}

fn signing_key(seed: &[u8; 32]) -> Result<SigningKey, String> {
    SigningKey::from_slice(seed).map_err(|error| format!("invalid fixture key seed: {error}"))
}

/// Deterministic fixed-width seed derivation. Run 0 yields scalars 1 and 2;
/// every other run gets a disjoint adjacent pair without entropy or clocks.
fn key_seed(run_id: u64, role_offset: u64) -> Result<[u8; 32], String> {
    let scalar = run_id
        .checked_mul(2)
        .and_then(|value| value.checked_add(role_offset))
        .ok_or_else(|| "--run-id is too large for deterministic key derivation".to_string())?;
    let mut seed = [0u8; 32];
    seed[24..].copy_from_slice(&scalar.to_be_bytes());
    Ok(seed)
}

fn order_nonce(run_id: u64, role_offset: u64) -> Result<u64, String> {
    NONCE_BASE
        .checked_add(
            run_id
                .checked_mul(2)
                .ok_or_else(|| "--run-id is too large for nonce derivation".to_string())?,
        )
        .and_then(|value| value.checked_add(role_offset - 1))
        .ok_or_else(|| "--run-id is too large for nonce derivation".to_string())
}

fn public_key_hex(key: &SigningKey) -> String {
    hex::encode(key.verifying_key().to_sec1_point(true).as_bytes())
}

fn signed_order_body(
    key: &SigningKey,
    genesis_hash: [u8; 32],
    market_id: u32,
    payoffs_pair: [i8; 2],
    limit_price: u64,
    quantity: u64,
    nonce: u64,
) -> Value {
    let mut markets = [MarketId::NONE; MAX_MARKETS_PER_ORDER];
    markets[0] = MarketId(market_id);
    let mut payoffs = [0i8; MAX_STATES];
    payoffs[..2].copy_from_slice(&payoffs_pair);
    let order = Order {
        markets,
        num_markets: 1,
        payoffs,
        num_states: 2,
        limit_price,
        max_fill: quantity,
        condition: None,
        expires_at_block: None,
        nonce,
    };
    let signature: Signature = key.sign(&canonical_order_bytes(&order, genesis_hash));
    json!({
        "signer_pubkey_hex": public_key_hex(key),
        "order": {
            "market_ids": [market_id],
            "payoffs": payoffs_pair,
            "limit_price_nanos": limit_price,
            "max_fill": quantity
        },
        "time_in_force": "GTC",
        "nonce": nonce,
        "signature_hex": hex::encode(signature.to_bytes())
    })
}

async fn run(args: Args) -> Result<SeedSummary, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("build HTTP client: {error}"))?;
    let mut steps = Vec::new();

    let mut health = request_json(
        &client,
        &args,
        &mut steps,
        "health",
        Method::GET,
        "/v1/health",
        None,
    )
    .await?;
    if string_field(&health, "status")? != "ok" {
        return Err(format!("health status is not ok: {health}"));
    }
    if let Some(marker) = explicit_production_marker(&health) {
        return Err(format!(
            "refusing to seed a target marked as production/mainnet ({marker})"
        ));
    }
    let positive_dev_marker = safe_environment_marker(&health);
    if !positive_dev_marker && !args.explicit_dev_ack {
        return Err(
            "health has no positive dev/demo marker; refusing to seed without \
             --i-know-this-is-dev"
                .to_string(),
        );
    }

    // A fresh chain learns its genesis hash after height 1. Poll the same
    // guarded endpoint briefly so callers do not need a separate warm-up race.
    let mut attempts = 0;
    while health.get("genesis_hash").and_then(Value::as_str).is_none() {
        attempts += 1;
        if attempts >= 30 {
            return Err("health did not expose genesis_hash within 30 seconds".to_string());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        let response = client
            .get(format!("{}/v1/health", args.base_url))
            .send()
            .await
            .map_err(|error| format!("poll health for genesis_hash: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "poll health for genesis_hash returned HTTP {}",
                response.status().as_u16()
            ));
        }
        health = response
            .json()
            .await
            .map_err(|error| format!("decode health while polling genesis_hash: {error}"))?;
    }
    let genesis_hex = string_field(&health, "genesis_hash")?;
    let genesis_bytes = hex::decode(genesis_hex)
        .map_err(|error| format!("health genesis_hash is not hex: {error}"))?;
    let genesis_hash: [u8; 32] = genesis_bytes.try_into().map_err(|bytes: Vec<u8>| {
        format!("health genesis_hash must be 32 bytes, got {}", bytes.len())
    })?;

    let market_name = if args.run_id == 0 {
        MARKET_NAME.to_string()
    } else {
        format!("{MARKET_NAME} run {}", args.run_id)
    };
    let market = request_json(
        &client,
        &args,
        &mut steps,
        "create_market",
        Method::POST,
        "/v1/markets",
        Some(&json!({"name": market_name})),
    )
    .await?;
    let market_id = u32::try_from(u64_field(&market, "market_id")?)
        .map_err(|_| format!("market_id does not fit u32: {market}"))?;
    if string_field(&market, "name")? != market_name {
        return Err(format!("create_market returned the wrong name: {market}"));
    }

    let yes_key = signing_key(&key_seed(args.run_id, 1)?)?;
    let no_key = signing_key(&key_seed(args.run_id, 2)?)?;
    let yes_nonce = order_nonce(args.run_id, 1)?;
    let no_nonce = order_nonce(args.run_id, 2)?;
    let fixtures = [
        ("buy_yes", &yes_key, yes_nonce),
        ("buy_no", &no_key, no_nonce),
    ];
    let mut account_summaries = Vec::with_capacity(fixtures.len());
    for (role, key, nonce) in fixtures {
        let public_key_hex = public_key_hex(key);
        let account = request_json(
            &client,
            &args,
            &mut steps,
            "create_account",
            Method::POST,
            "/v1/accounts",
            Some(&json!({
                "initial_balance_nanos": 0,
                "initial_key": {"public_key_hex": public_key_hex}
            })),
        )
        .await?;
        let account_id = u64_field(&account, "account_id")?;
        if u64_field(&account, "balance_nanos")? != 0 {
            return Err(format!("new account was not zero-balanced: {account}"));
        }

        let fund_path = format!("/v1/accounts/{account_id}/fund");
        let funded = request_json(
            &client,
            &args,
            &mut steps,
            "fund_account",
            Method::POST,
            &fund_path,
            Some(&json!({"amount_nanos": ACCOUNT_FUNDING_NANOS})),
        )
        .await?;
        if u64_field(&funded, "account_id")? != account_id
            || u64_field(&funded, "balance_nanos")? != ACCOUNT_FUNDING_NANOS
        {
            return Err(format!(
                "fund_account returned an unexpected account: {funded}"
            ));
        }
        account_summaries.push(AccountSummary {
            role,
            account_id,
            public_key_hex,
            funded_balance_nanos: ACCOUNT_FUNDING_NANOS,
            order_nonce: nonce,
        });
    }

    let order_fixtures = [
        (
            "BuyYes",
            &yes_key,
            [1, 0],
            YES_LIMIT_NANOS,
            YES_QUANTITY,
            yes_nonce,
            account_summaries[0].account_id,
        ),
        (
            "BuyNo",
            &no_key,
            [0, 1],
            NO_LIMIT_NANOS,
            NO_QUANTITY,
            no_nonce,
            account_summaries[1].account_id,
        ),
    ];
    let mut order_summaries = Vec::with_capacity(order_fixtures.len());
    for (side, key, payoffs, limit, quantity, nonce, account_id) in order_fixtures {
        let body = signed_order_body(
            key,
            genesis_hash,
            market_id,
            payoffs,
            limit,
            quantity,
            nonce,
        );
        let accepted = request_json(
            &client,
            &args,
            &mut steps,
            "submit_signed_order",
            Method::POST,
            "/v1/orders/signed",
            Some(&body),
        )
        .await?;
        if accepted.get("accepted").and_then(Value::as_bool) != Some(true) {
            return Err(format!("signed order was not accepted: {accepted}"));
        }
        let ids = accepted
            .get("order_ids")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("order response is missing order_ids: {accepted}"))?;
        if ids.len() != 1 {
            return Err(format!("expected exactly one order id: {accepted}"));
        }
        let order_id = ids[0]
            .as_u64()
            .ok_or_else(|| format!("order id is not a u64: {accepted}"))?;
        order_summaries.push(OrderSummary {
            side,
            order_id,
            account_id,
            limit_price_nanos: limit,
            quantity,
            expected_fill_quantity: MATCHED_VOLUME,
            expected_fill_price_nanos: CLEARING_PRICE_NANOS,
        });
    }

    Ok(SeedSummary {
        schema: "sybil.seed_book.v1",
        fixture_version: format!("{FIXTURE_VERSION}:{}", args.run_id),
        semantics: "single_use_fresh_state",
        account_count: account_summaries.len(),
        guard: GuardSummary {
            health_status: "ok".to_string(),
            positive_dev_marker,
            explicit_dev_ack: args.explicit_dev_ack,
        },
        market: MarketSummary {
            market_id,
            name: market_name,
        },
        accounts: account_summaries,
        orders: order_summaries,
        expected: ExpectedSummary {
            matched_volume: MATCHED_VOLUME,
            total_fill_quantity: MATCHED_VOLUME * 2,
            fill_count: 2,
            yes_price_nanos: CLEARING_PRICE_NANOS,
            no_price_nanos: CLEARING_PRICE_NANOS,
            total_volume_nanos: 1_000_000_000,
            total_welfare_nanos: 100_000_000,
            funded_balance_total_nanos: ACCOUNT_FUNDING_NANOS * 2,
            marked_position_value_nanos: 1_000_000_000,
            post_trade_balance_total_nanos: ACCOUNT_FUNDING_NANOS * 2 - 1_000_000_000,
        },
        http_steps: steps,
    })
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("seed_book: {error}");
            return ExitCode::from(2);
        }
    };
    match run(args).await {
        Ok(summary) => match serde_json::to_string(&summary) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("seed_book: encode summary: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("seed_book: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_run_zero_has_fixed_keys_and_nonces() {
        assert_eq!(key_seed(0, 1).unwrap()[24..], 1u64.to_be_bytes());
        assert_eq!(key_seed(0, 2).unwrap()[24..], 2u64.to_be_bytes());
        assert_eq!(order_nonce(0, 1).unwrap(), 247_000_001);
        assert_eq!(order_nonce(0, 2).unwrap(), 247_000_002);
    }

    #[test]
    fn run_id_derivation_is_deterministic_and_disjoint() {
        let first = key_seed(42, 1).unwrap();
        let second = key_seed(42, 2).unwrap();
        assert_eq!(first, key_seed(42, 1).unwrap());
        assert_ne!(first, second);
        assert_ne!(first, key_seed(43, 1).unwrap());
        assert_eq!(order_nonce(42, 1).unwrap(), 247_000_085);
        assert_eq!(order_nonce(42, 2).unwrap(), 247_000_086);
    }

    #[test]
    fn signed_fixture_order_is_byte_deterministic() {
        let key = signing_key(&key_seed(0, 1).unwrap()).unwrap();
        let first = signed_order_body(&key, [7; 32], 3, [1, 0], 600_000_000, 1_000, 247_000_001);
        let second = signed_order_body(&key, [7; 32], 3, [1, 0], 600_000_000, 1_000, 247_000_001);
        assert_eq!(first, second);
    }

    #[test]
    fn guard_recognizes_safe_and_production_markers() {
        assert!(safe_environment_marker(&json!({"dev_mode": true})));
        assert!(safe_environment_marker(&json!({"network": "devnet"})));
        assert!(!safe_environment_marker(&json!({"status": "ok"})));
        assert_eq!(
            explicit_production_marker(&json!({"network": "mainnet"})),
            Some("network=mainnet".to_string())
        );
    }

    #[test]
    fn u64_field_accepts_json_and_wire_encoded_integers() {
        assert_eq!(u64_field(&json!({"value": 42}), "value"), Ok(42));
        assert_eq!(u64_field(&json!({"value": "42"}), "value"), Ok(42));
    }

    #[test]
    fn u64_field_rejects_non_decimal_and_overflowing_values() {
        assert!(u64_field(&json!({"value": "-1"}), "value").is_err());
        assert!(u64_field(&json!({"value": "18446744073709551616"}), "value").is_err());
        assert!(u64_field(&json!({"value": "42.0"}), "value").is_err());
    }
}
