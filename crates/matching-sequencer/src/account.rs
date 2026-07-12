use std::collections::HashMap;

use matching_engine::MarketId;

/// Lifetime cap on read API-key records retained by one account. Revoked
/// records deliberately count toward this cap: keeping tombstones preserves
/// stable ids/audit history while making add/revoke churn permanently bounded.
pub const MAX_API_KEYS_PER_ACCOUNT: usize = 64;
/// Read-key labels are metadata, not an unbounded account-storage channel.
pub const MAX_API_KEY_LABEL_BYTES: usize = 128;
/// Signing-key labels share the read-key metadata budget. This is a UTF-8 byte
/// limit; callers must not trim or normalize labels before applying it.
pub const MAX_SIGNING_KEY_LABEL_BYTES: usize = MAX_API_KEY_LABEL_BYTES;
/// Admission ceiling for the complete MessagePack-encoded recovery account.
/// qMDB accepts 1 MiB values; 256 KiB leaves 75% headroom for codec/schema
/// overhead and future account fields.
pub const MAX_SERIALIZED_ACCOUNT_BYTES: usize = 1 << 18;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AccountId(pub u64);

impl AccountId {
    /// Reserved system account for minting/burning operations.
    /// Holds protocol counterparty positions derived from MINT adjustments.
    pub const MINT: AccountId = AccountId(u64::MAX);
}

/// Optional, opt-in account profile metadata (SYB-60).
///
/// Set/cleared only via a P256-signed, nonce-protected mutation (see
/// `sequencer::set_profile`). Purely descriptive: it never affects balances,
/// matching, or settlement. A non-empty `display_name` is explicit consent to
/// publish the account's leaderboard financial row; clearing it withdraws that
/// account from future leaderboard reads.
#[derive(Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AccountProfile {
    /// Optional display name (validated for length/charset at the API edge).
    #[serde(default)]
    pub display_name: Option<String>,
    /// Deterministic identicon seed. There is no image upload — the seed is the
    /// only stored avatar state and the client renders the identicon from it.
    #[serde(default)]
    pub avatar_seed: Option<String>,
}

impl AccountProfile {
    pub fn is_empty(&self) -> bool {
        self.display_name.is_none() && self.avatar_seed.is_none()
    }
}

/// A read-scoped bearer API key (SYB-60).
///
/// SECURITY MODEL: bearer tokens are READ-ONLY. Sybil authenticates every
/// mutating action (orders, cancels, withdrawals, profile/key changes) with a
/// P256 signature over canonical bytes plus a replay nonce. A bearer token
/// deliberately cannot place orders/cancels/withdrawals — doing so would bypass
/// the signing model and its replay protection. To give an agent trade
/// authority you register an additional P256 key (scope `Agent`) that signs
/// like any other key; see `RegisteredPubkey`.
///
/// Only the blake3 hash of the token is stored (never the plaintext), so a
/// database or WAL leak cannot recover live tokens.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ApiKeyRecord {
    /// Stable id used to reference the key for revocation (derived from the
    /// first 8 bytes of the token hash — the plaintext is never needed again).
    pub id: u64,
    /// blake3(token) — the only representation of the secret kept at rest.
    pub hash: [u8; 32],
    /// Optional human label, e.g. "grafana" or "read-bot".
    #[serde(default)]
    pub label: Option<String>,
    pub created_at_ms: u64,
    /// `Some(ts)` once revoked; a revoked key is rejected by the bearer
    /// extractor but retained for audit/listing.
    #[serde(default)]
    pub revoked_at_ms: Option<u64>,
}

impl ApiKeyRecord {
    pub fn is_active(&self) -> bool {
        self.revoked_at_ms.is_none()
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Account {
    pub id: AccountId,
    /// Balance in nanos, signed (can go negative during settlement if needed)
    pub balance: i64,
    /// Positions: (market, outcome_idx) -> signed quantity
    pub positions: HashMap<(MarketId, u8), i64>,
    /// Total amount deposited (initial balance + all fund_account calls).
    /// Used for PnL calculation: PnL = portfolio_value - total_deposited.
    pub total_deposited: i64,
    /// Highest accepted signed-action nonce for replay protection.
    #[serde(default)]
    pub last_nonce: u64,
    #[serde(default)]
    pub events_digest: [u8; 32],
    pub keys_digest: [u8; 32],
    /// Optional, signed opt-in profile metadata (SYB-60).
    #[serde(default)]
    pub profile: AccountProfile,
    /// Read-scoped bearer API keys (SYB-60). Hashes only; never plaintext.
    #[serde(default)]
    pub api_keys: Vec<ApiKeyRecord>,
    /// Monotonic counter backing `ApiKeyRecord::id` (never reused, so revoked
    /// ids stay stable even as keys are added/removed).
    #[serde(default)]
    pub next_api_key_id: u64,
}

impl Account {
    pub fn new(id: AccountId, balance: i64) -> Self {
        Self {
            id,
            balance,
            positions: HashMap::new(),
            total_deposited: balance,
            last_nonce: 0,
            events_digest: [0u8; 32],
            keys_digest: sybil_verifier::empty_account_keys_digest(id.0),
            profile: AccountProfile::default(),
            api_keys: Vec::new(),
            next_api_key_id: 0,
        }
    }

    pub fn position(&self, market: MarketId, outcome: u8) -> i64 {
        self.positions.get(&(market, outcome)).copied().unwrap_or(0)
    }
}

#[derive(Clone, Default)]
pub struct AccountStore {
    accounts: HashMap<AccountId, Account>,
    next_id: u64,
}

impl AccountStore {
    pub fn new() -> Self {
        let mut store = Self::default();
        // Create the system mint account (zero balance, protocol counterparty positions).
        store
            .accounts
            .insert(AccountId::MINT, Account::new(AccountId::MINT, 0));
        store
    }

    pub fn create_account(&mut self, balance: i64) -> AccountId {
        let id = AccountId(self.next_id);
        self.next_id += 1;
        self.accounts.insert(id, Account::new(id, balance));
        id
    }

    pub fn get(&self, id: AccountId) -> Option<&Account> {
        self.accounts.get(&id)
    }

    pub fn get_mut(&mut self, id: AccountId) -> Option<&mut Account> {
        self.accounts.get_mut(&id)
    }

    pub fn total_balance(&self) -> i64 {
        self.accounts.values().map(|a| a.balance).sum()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&AccountId, &Account)> {
        self.accounts.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&AccountId, &mut Account)> {
        self.accounts.iter_mut()
    }

    /// Next account ID that will be assigned.
    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    /// Restore from persisted state.
    pub fn restore(accounts: HashMap<AccountId, Account>, next_id: u64) -> Self {
        Self { accounts, next_id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_account_has_zero_events_digest() {
        let account = Account::new(AccountId(7), 100);
        assert_eq!(account.events_digest, [0u8; 32]);
    }

    #[test]
    fn test_new_account_has_empty_key_set_digest() {
        let account = Account::new(AccountId(7), 100);
        assert_eq!(
            account.keys_digest,
            sybil_verifier::empty_account_keys_digest(7)
        );
        assert_ne!(account.keys_digest, [0u8; 32]);
    }
}
