//! P256 signing helper for the post-deploy smoke test (SYB-223).
//!
//! Canonical signing bytes have exactly ONE home: the `sybil-signing` crate.
//! This example deliberately does NOT reimplement any byte layout — it builds
//! the canonical structs and calls `sybil_signing::canonical_*_bytes`, then
//! signs with P256 producing the exact encodings the server verifies:
//!   - `signer_pubkey_hex`  = compressed SEC1 point (33 bytes), hex
//!   - `signature_hex`      = raw ECDSA r||s (64 bytes), hex  (`Signature::from_slice`)
//!
//! The order path mirrors the server's `signed_order_data_to_order` plus
//! `apply_time_in_force(Gtc, ..)`: a single-market binary order with
//! `num_markets = 1`, `num_states = 2`, and `expires_at_block = None`.
//!
//! All subcommands emit a single JSON object to stdout so the shell smoke
//! script can merge the fields into the REST body with `jq`.
//!
//! Usage:
//!   cargo run -p sybil-client --example smoke_sign -- keygen
//!     -> {"private_key_hex":..,"public_key_hex":..}
//!   cargo run -p sybil-client --example smoke_sign -- order \
//!       --priv HEX --market N --nonce N --genesis-hash HEX32 \
//!       [--price NANOS] [--qty UNITS] [--payoffs a,b]
//!     -> {"signer_pubkey_hex":..,"signature_hex":..}
//!   cargo run -p sybil-client --example smoke_sign -- cancel \
//!       --priv HEX --account N --order N --nonce N --genesis-hash HEX32
//!     -> {"signer_pubkey_hex":..,"signature_hex":..}
//!   cargo run -p sybil-client --example smoke_sign -- withdrawal \
//!       --priv HEX --account N --chain-id N --vault HEX20 --recipient HEX20 \
//!       --token HEX20 --amount N --expiry N --nonce N
//!     -> {"signer_pubkey_hex":..,"signature_hex":..}

use std::collections::HashMap;
use std::process::exit;

use getrandom::SysRng;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::rand_core::UnwrapErr;
use p256::elliptic_curve::Generate;
use sybil_signing::{
    canonical_bridge_withdrawal_bytes, canonical_cancel_bytes, canonical_order_bytes,
    BridgeWithdrawalRequest, MarketId, Order, MAX_MARKETS_PER_ORDER, MAX_STATES,
};

fn die(msg: &str) -> ! {
    eprintln!("smoke_sign: {msg}");
    exit(2);
}

/// Parse `--flag value` pairs into a map. The leading token is the subcommand.
fn parse_flags(args: &[String]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        let key = &args[i];
        let Some(name) = key.strip_prefix("--") else {
            die(&format!("expected --flag, got {key:?}"));
        };
        let Some(value) = args.get(i + 1) else {
            die(&format!("flag --{name} is missing a value"));
        };
        map.insert(name.to_string(), value.clone());
        i += 2;
    }
    map
}

fn req<'a>(flags: &'a HashMap<String, String>, name: &str) -> &'a str {
    flags
        .get(name)
        .map(String::as_str)
        .unwrap_or_else(|| die(&format!("missing required flag --{name}")))
}

fn req_u64(flags: &HashMap<String, String>, name: &str) -> u64 {
    req(flags, name)
        .parse()
        .unwrap_or_else(|_| die(&format!("--{name} must be a u64")))
}

fn opt_u64(flags: &HashMap<String, String>, name: &str, default: u64) -> u64 {
    match flags.get(name) {
        Some(v) => v
            .parse()
            .unwrap_or_else(|_| die(&format!("--{name} must be a u64"))),
        None => default,
    }
}

fn parse_addr20(value: &str, name: &str) -> [u8; 20] {
    let stripped = value.strip_prefix("0x").unwrap_or(value);
    let bytes = hex::decode(stripped).unwrap_or_else(|_| die(&format!("--{name} must be hex")));
    bytes
        .try_into()
        .unwrap_or_else(|_: Vec<u8>| die(&format!("--{name} must be 20 bytes")))
}

fn parse_hash32(value: &str, name: &str) -> [u8; 32] {
    let stripped = value.strip_prefix("0x").unwrap_or(value);
    let bytes = hex::decode(stripped).unwrap_or_else(|_| die(&format!("--{name} must be hex")));
    bytes
        .try_into()
        .unwrap_or_else(|_: Vec<u8>| die(&format!("--{name} must be 32 bytes")))
}

/// Load a signing key from a 32-byte hex private scalar.
fn key_from_hex(priv_hex: &str) -> SigningKey {
    let stripped = priv_hex.strip_prefix("0x").unwrap_or(priv_hex);
    let bytes = hex::decode(stripped).unwrap_or_else(|_| die("--priv must be hex"));
    SigningKey::from_slice(&bytes).unwrap_or_else(|_| die("--priv is not a valid P256 scalar"))
}

/// Compressed SEC1 public-key bytes for a signing key (matches the server's
/// `VerifyingKey::from_sec1_point` round-trip).
fn pubkey_hex(key: &SigningKey) -> String {
    hex::encode(key.verifying_key().to_sec1_point(true).as_bytes())
}

/// Raw 64-byte r||s signature over `msg`, hex-encoded (server uses
/// `Signature::from_slice`).
fn sign_hex(key: &SigningKey, msg: &[u8]) -> String {
    let signature: Signature = key.sign(msg);
    hex::encode(signature.to_bytes())
}

/// Emit `{"signer_pubkey_hex":..,"signature_hex":..}`.
fn emit_signed(key: &SigningKey, msg: &[u8]) {
    println!(
        "{{\"signer_pubkey_hex\":\"{}\",\"signature_hex\":\"{}\"}}",
        pubkey_hex(key),
        sign_hex(key, msg),
    );
}

fn cmd_keygen() {
    let key = SigningKey::generate_from_rng(&mut UnwrapErr(SysRng));
    println!(
        "{{\"private_key_hex\":\"{}\",\"public_key_hex\":\"{}\"}}",
        hex::encode(key.to_bytes()),
        pubkey_hex(&key),
    );
}

fn cmd_order(flags: &HashMap<String, String>) {
    let key = key_from_hex(req(flags, "priv"));
    let market = req_u64(flags, "market") as u32;
    let nonce = req_u64(flags, "nonce");
    let genesis_hash = parse_hash32(req(flags, "genesis-hash"), "genesis-hash");
    let price = opt_u64(flags, "price", 500_000_000); // $0.50
    let qty = opt_u64(flags, "qty", 1_000); // 1 share
    let (p0, p1) = match flags.get("payoffs") {
        Some(raw) => {
            let parts: Vec<&str> = raw.split(',').collect();
            if parts.len() != 2 {
                die("--payoffs must be exactly two comma-separated i8 values, e.g. 1,0");
            }
            (
                parts[0].parse().unwrap_or_else(|_| die("bad --payoffs")),
                parts[1].parse().unwrap_or_else(|_| die("bad --payoffs")),
            )
        }
        None => (1i8, 0i8),
    };

    let mut markets = [MarketId::NONE; MAX_MARKETS_PER_ORDER];
    markets[0] = MarketId(market);
    let mut payoffs = [0i8; MAX_STATES];
    payoffs[0] = p0;
    payoffs[1] = p1;

    let order = Order {
        markets,
        num_markets: 1,
        payoffs,
        num_states: 2,
        limit_price: price,
        max_fill: qty,
        condition: None,
        expires_at_block: None,
        nonce,
    };
    emit_signed(&key, &canonical_order_bytes(&order, genesis_hash));
}

fn cmd_cancel(flags: &HashMap<String, String>) {
    let key = key_from_hex(req(flags, "priv"));
    let account = req_u64(flags, "account");
    let order = req_u64(flags, "order");
    let nonce = req_u64(flags, "nonce");
    let genesis_hash = parse_hash32(req(flags, "genesis-hash"), "genesis-hash");
    emit_signed(
        &key,
        &canonical_cancel_bytes(account, order, nonce, genesis_hash),
    );
}

fn cmd_withdrawal(flags: &HashMap<String, String>) {
    let key = key_from_hex(req(flags, "priv"));
    let request = BridgeWithdrawalRequest {
        account_id: req_u64(flags, "account"),
        chain_id: req_u64(flags, "chain-id"),
        vault_address: parse_addr20(req(flags, "vault"), "vault"),
        recipient: parse_addr20(req(flags, "recipient"), "recipient"),
        token_address: parse_addr20(req(flags, "token"), "token"),
        amount_token_units: req_u64(flags, "amount"),
        expiry_height: req_u64(flags, "expiry"),
        nonce: req_u64(flags, "nonce"),
    };
    emit_signed(&key, &canonical_bridge_withdrawal_bytes(&request));
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(subcommand) = args.first() else {
        die("usage: smoke_sign <keygen|order|cancel|withdrawal> [flags]");
    };
    let flags = parse_flags(&args[1..]);
    match subcommand.as_str() {
        "keygen" => cmd_keygen(),
        "order" => cmd_order(&flags),
        "cancel" => cmd_cancel(&flags),
        "withdrawal" => cmd_withdrawal(&flags),
        other => die(&format!("unknown subcommand {other:?}")),
    }
}
