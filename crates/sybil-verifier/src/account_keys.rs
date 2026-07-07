//! Canonical account signing-key set digest.

use sha2::{Digest as _, Sha256};

pub const ACCOUNT_KEYS_DIGEST_DOMAIN: &[u8] = b"sybil/state/account-keys-digest/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccountKeyDigestRecord {
    pub auth_scheme: u8,
    pub pubkey_sec1: [u8; 33],
}

pub fn account_keys_digest<I>(account_id: u64, keys: I) -> [u8; 32]
where
    I: IntoIterator<Item = AccountKeyDigestRecord>,
{
    let mut keys: Vec<AccountKeyDigestRecord> = keys.into_iter().collect();
    keys.sort_by_key(|record| (record.auth_scheme, record.pubkey_sec1));

    let mut hasher = Sha256::new();
    hasher.update(ACCOUNT_KEYS_DIGEST_DOMAIN);
    hasher.update(account_id.to_le_bytes());
    hasher.update((keys.len() as u64).to_le_bytes());
    for key in keys {
        hasher.update([key.auth_scheme]);
        hasher.update(key.pubkey_sec1);
    }
    hasher.finalize().into()
}

pub fn empty_account_keys_digest(account_id: u64) -> [u8; 32] {
    account_keys_digest(account_id, [])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_key(byte: u8) -> AccountKeyDigestRecord {
        let mut pubkey_sec1 = [byte; 33];
        pubkey_sec1[0] = 0x02;
        AccountKeyDigestRecord {
            auth_scheme: 0,
            pubkey_sec1,
        }
    }

    fn webauthn_key(byte: u8) -> AccountKeyDigestRecord {
        AccountKeyDigestRecord {
            auth_scheme: 1,
            ..raw_key(byte)
        }
    }

    #[test]
    fn empty_digest_is_domain_and_account_bound_not_zero() {
        assert_ne!(empty_account_keys_digest(7), [0u8; 32]);
        assert_ne!(empty_account_keys_digest(7), empty_account_keys_digest(8));
    }

    #[test]
    fn key_digest_sorts_by_auth_scheme_then_pubkey() {
        let sorted = account_keys_digest(42, [raw_key(0x11), webauthn_key(0x01)]);
        let unsorted = account_keys_digest(42, [webauthn_key(0x01), raw_key(0x11)]);

        assert_eq!(sorted, unsorted);
    }

    #[test]
    fn key_digest_is_sensitive_to_scheme_pubkey_and_count() {
        let base = account_keys_digest(42, [raw_key(0x11)]);

        assert_ne!(base, account_keys_digest(42, [webauthn_key(0x11)]));
        assert_ne!(base, account_keys_digest(42, [raw_key(0x12)]));
        assert_ne!(
            base,
            account_keys_digest(42, [raw_key(0x11), raw_key(0x12)])
        );
    }
}
