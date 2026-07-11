//! Canonical account signing-key set digest.

use sha2::{Digest as _, Sha256};

use crate::KeyRecord;

pub const ACCOUNT_KEYS_DIGEST_DOMAIN: &[u8] = b"sybil/state/account-keys-digest/v2";
pub const MAX_KEYS_PER_ACCOUNT: usize = 16;
pub const MAX_KEY_OPS_PER_BLOCK: usize = 64;
pub const MAX_WEBAUTHN_AUTHENTICATOR_DATA_BYTES: usize = 512;
pub const MAX_WEBAUTHN_CLIENT_DATA_JSON_BYTES: usize = 2 * 1024;
pub type AccountKeyDigestRecord = KeyRecord;

pub fn account_keys_digest<I>(account_id: u64, keys: I) -> [u8; 32]
where
    I: IntoIterator<Item = KeyRecord>,
{
    let mut keys: Vec<KeyRecord> = keys.into_iter().collect();
    keys.sort_by_key(KeyRecord::canonical_sort_key);

    let mut hasher = Sha256::new();
    hasher.update(ACCOUNT_KEYS_DIGEST_DOMAIN);
    hasher.update(account_id.to_le_bytes());
    hasher.update((keys.len() as u64).to_le_bytes());
    for key in keys {
        hasher.update([key.auth_scheme]);
        hasher.update(key.pubkey_sec1);
        hasher.update(key.capability_mask.to_le_bytes());
    }
    hasher.finalize().into()
}

pub fn empty_account_keys_digest(account_id: u64) -> [u8; 32] {
    account_keys_digest(account_id, [])
}

pub fn canonical_key_registration_bytes(
    genesis_hash: [u8; 32],
    account_id: u64,
    key: &KeyRecord,
    bound_keys_digest: [u8; 32],
    bound_events_digest: [u8; 32],
) -> Vec<u8> {
    canonical_key_op_bytes(
        b"sybil/keyop/register/v1",
        genesis_hash,
        account_id,
        key,
        bound_keys_digest,
        bound_events_digest,
    )
}

pub fn canonical_key_revocation_bytes(
    genesis_hash: [u8; 32],
    account_id: u64,
    key: &KeyRecord,
    bound_keys_digest: [u8; 32],
    bound_events_digest: [u8; 32],
) -> Vec<u8> {
    canonical_key_op_bytes(
        b"sybil/keyop/revoke/v1",
        genesis_hash,
        account_id,
        key,
        bound_keys_digest,
        bound_events_digest,
    )
}

fn canonical_key_op_bytes(
    domain: &[u8],
    genesis_hash: [u8; 32],
    account_id: u64,
    key: &KeyRecord,
    bound_keys_digest: [u8; 32],
    bound_events_digest: [u8; 32],
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(domain.len() + 32 + 8 + 38 + 32 + 32);
    bytes.extend_from_slice(domain);
    bytes.extend_from_slice(&genesis_hash);
    bytes.extend_from_slice(&account_id.to_le_bytes());
    bytes.push(key.auth_scheme);
    bytes.extend_from_slice(&key.pubkey_sec1);
    bytes.extend_from_slice(&key.capability_mask.to_le_bytes());
    bytes.extend_from_slice(&bound_keys_digest);
    bytes.extend_from_slice(&bound_events_digest);
    bytes
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
            capability_mask: KeyRecord::FULL_CAPABILITY_MASK,
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
    fn key_digest_sorts_by_pubkey_then_auth_scheme() {
        let sorted = account_keys_digest(42, [raw_key(0x11), webauthn_key(0x01)]);
        let unsorted = account_keys_digest(42, [webauthn_key(0x01), raw_key(0x11)]);

        assert_eq!(sorted, unsorted);
    }

    #[test]
    fn key_digest_is_sensitive_to_scheme_pubkey_and_count() {
        let base = account_keys_digest(42, [raw_key(0x11)]);

        assert_ne!(base, account_keys_digest(42, [webauthn_key(0x11)]));
        assert_ne!(base, account_keys_digest(42, [raw_key(0x12)]));
        let mut restricted = raw_key(0x11);
        restricted.capability_mask = 1;
        assert_ne!(base, account_keys_digest(42, [restricted]));
        assert_ne!(
            base,
            account_keys_digest(42, [raw_key(0x11), raw_key(0x12)])
        );
    }
}
