use super::*;

pub(super) fn fill_history_key(account_id: AccountId, record: &AccountFillRecord) -> [u8; 24] {
    let mut key = [0u8; 24];
    key[0..8].copy_from_slice(&account_id.0.to_be_bytes());
    key[8..16].copy_from_slice(&record.block_height.to_be_bytes());
    key[16..24].copy_from_slice(&record.order_id.to_be_bytes());
    key
}

/// Inclusive `[lo, hi]` bounds covering every fill-history key for one account
/// (keys are `account_id || block_height || order_id`, big-endian, so a single
/// account is a contiguous range).
pub(super) fn fill_history_account_bounds(account_id: AccountId) -> ([u8; 24], [u8; 24]) {
    let mut lo = [0u8; 24];
    lo[0..8].copy_from_slice(&account_id.0.to_be_bytes());
    let mut hi = [0xffu8; 24];
    hi[0..8].copy_from_slice(&account_id.0.to_be_bytes());
    (lo, hi)
}

pub(super) fn equity_key(account_id: AccountId, height: u64) -> [u8; 16] {
    let mut k = [0u8; 16];
    k[..8].copy_from_slice(&account_id.0.to_be_bytes());
    k[8..].copy_from_slice(&height.to_be_bytes());
    k
}

pub(super) fn history_event_key(account_id: AccountId, block_height: u64, seq: u64) -> [u8; 24] {
    let mut k = [0u8; 24];
    k[..8].copy_from_slice(&account_id.0.to_be_bytes());
    k[8..16].copy_from_slice(&block_height.to_be_bytes());
    k[16..].copy_from_slice(&seq.to_be_bytes());
    k
}

pub(super) fn price_point_key(market_id: MarketId, height: u64) -> [u8; 12] {
    let mut key = [0u8; 12];
    key[..4].copy_from_slice(&market_id.0.to_be_bytes());
    key[4..].copy_from_slice(&height.to_be_bytes());
    key
}

pub(super) fn price_point_parts_from_key(key: &[u8]) -> Option<(MarketId, u64)> {
    let market_bytes: [u8; 4] = key.get(..4)?.try_into().ok()?;
    let height_bytes: [u8; 8] = key.get(4..12)?.try_into().ok()?;
    Some((
        MarketId(u32::from_be_bytes(market_bytes)),
        u64::from_be_bytes(height_bytes),
    ))
}

pub(super) fn price_point_by_height_key(height: u64, market_id: MarketId) -> [u8; 12] {
    let mut key = [0u8; 12];
    key[..8].copy_from_slice(&height.to_be_bytes());
    key[8..].copy_from_slice(&market_id.0.to_be_bytes());
    key
}

pub(super) fn price_point_by_height_parts_from_key(key: &[u8]) -> Option<(u64, MarketId)> {
    let height_bytes: [u8; 8] = key.get(..8)?.try_into().ok()?;
    let market_bytes: [u8; 4] = key.get(8..12)?.try_into().ok()?;
    Some((
        u64::from_be_bytes(height_bytes),
        MarketId(u32::from_be_bytes(market_bytes)),
    ))
}

pub(super) fn price_point_market_bounds(market_id: MarketId) -> ([u8; 12], [u8; 12]) {
    (
        price_point_key(market_id, 0),
        price_point_key(market_id, u64::MAX),
    )
}

pub(super) fn price_candle_key(
    market_id: MarketId,
    resolution_secs: u32,
    bucket_start_ms: u64,
) -> [u8; 16] {
    let mut key = [0u8; 16];
    key[..4].copy_from_slice(&market_id.0.to_be_bytes());
    key[4..8].copy_from_slice(&resolution_secs.to_be_bytes());
    key[8..].copy_from_slice(&bucket_start_ms.to_be_bytes());
    key
}

pub(super) fn price_candle_parts_from_key(key: &[u8]) -> Option<(MarketId, u32, u64)> {
    let market_bytes: [u8; 4] = key.get(..4)?.try_into().ok()?;
    let resolution_bytes: [u8; 4] = key.get(4..8)?.try_into().ok()?;
    let bucket_bytes: [u8; 8] = key.get(8..16)?.try_into().ok()?;
    Some((
        MarketId(u32::from_be_bytes(market_bytes)),
        u32::from_be_bytes(resolution_bytes),
        u64::from_be_bytes(bucket_bytes),
    ))
}

pub(super) fn price_candle_market_resolution_bounds(
    market_id: MarketId,
    resolution_secs: u32,
) -> ([u8; 16], [u8; 16]) {
    (
        price_candle_key(market_id, resolution_secs, 0),
        price_candle_key(market_id, resolution_secs, u64::MAX),
    )
}

pub(super) fn price_candle_by_resolution_key(
    resolution_secs: u32,
    bucket_start_ms: u64,
    market_id: MarketId,
) -> [u8; 16] {
    let mut key = [0u8; 16];
    key[..4].copy_from_slice(&resolution_secs.to_be_bytes());
    key[4..12].copy_from_slice(&bucket_start_ms.to_be_bytes());
    key[12..].copy_from_slice(&market_id.0.to_be_bytes());
    key
}

pub(super) fn price_candle_by_resolution_parts_from_key(
    key: &[u8],
) -> Option<(u32, u64, MarketId)> {
    let resolution_bytes: [u8; 4] = key.get(..4)?.try_into().ok()?;
    let bucket_bytes: [u8; 8] = key.get(4..12)?.try_into().ok()?;
    let market_bytes: [u8; 4] = key.get(12..16)?.try_into().ok()?;
    Some((
        u32::from_be_bytes(resolution_bytes),
        u64::from_be_bytes(bucket_bytes),
        MarketId(u32::from_be_bytes(market_bytes)),
    ))
}

pub(super) fn price_candle_resolution_bounds(resolution_secs: u32) -> ([u8; 16], [u8; 16]) {
    (
        price_candle_by_resolution_key(resolution_secs, 0, MarketId(0)),
        price_candle_by_resolution_key(resolution_secs, u64::MAX, MarketId(u32::MAX)),
    )
}

pub(super) fn price_candles_min_bucket_key(resolution_secs: u32) -> String {
    format!("{KEY_PRICE_CANDLES_MIN_BUCKET_MS_PREFIX}{resolution_secs}")
}

pub(super) fn parse_price_candles_min_bucket_key(key: &str) -> Option<u32> {
    key.strip_prefix(KEY_PRICE_CANDLES_MIN_BUCKET_MS_PREFIX)?
        .parse()
        .ok()
}

pub(super) fn seq_from_history_event_key(key: &[u8]) -> Option<u64> {
    let seq_bytes: [u8; 8] = key.get(16..24)?.try_into().ok()?;
    Some(u64::from_be_bytes(seq_bytes))
}

pub(super) fn account_id_from_fill_history_key(key: &[u8]) -> Option<AccountId> {
    let account_bytes: [u8; 8] = key.get(0..8)?.try_into().ok()?;
    Some(AccountId(u64::from_be_bytes(account_bytes)))
}

/// Persisted management metadata for a signing key (SYB-60).
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct PubkeyMetaRow {
    #[serde(default)]
    pub(super) label: Option<String>,
    #[serde(default)]
    pub(super) scope: u8,
    #[serde(default)]
    pub(super) created_at_ms: u64,
}

pub(super) fn key_scope_to_store(scope: crate::crypto::KeyScope) -> u8 {
    match scope {
        crate::crypto::KeyScope::Primary => 0,
        crate::crypto::KeyScope::Agent => 1,
        crate::crypto::KeyScope::Custom => 2,
    }
}

pub(super) fn key_scope_from_store(value: u8) -> crate::crypto::KeyScope {
    match value {
        1 => crate::crypto::KeyScope::Agent,
        2 => crate::crypto::KeyScope::Custom,
        _ => crate::crypto::KeyScope::Primary,
    }
}

pub(super) fn account_auth_scheme_to_store(scheme: crate::crypto::AccountAuthScheme) -> u8 {
    match scheme {
        crate::crypto::AccountAuthScheme::RawP256 => 0,
        crate::crypto::AccountAuthScheme::WebAuthn => 1,
    }
}

pub(super) fn account_auth_scheme_from_store(value: u8) -> crate::crypto::AccountAuthScheme {
    match value {
        1 => crate::crypto::AccountAuthScheme::WebAuthn,
        _ => crate::crypto::AccountAuthScheme::RawP256,
    }
}

pub(super) fn parse_hash32(bytes: &[u8], context: &str) -> Result<[u8; 32], StoreError> {
    bytes.try_into().map_err(|_| {
        StoreError::CorruptLayout(format!("{context} must be 32 bytes, got {}", bytes.len()))
    })
}
