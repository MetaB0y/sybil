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

/// Canonical deployment- and state-bound bytes authorized by an escape claim.
///
/// This deliberately follows the fixed-width, little-endian key-operation
/// convention rather than a transport serializer. It is shared by host tools
/// and both OpenVM guests so claim authorization has one byte definition.
#[allow(clippy::too_many_arguments)] // Frozen claim layout; a wrapper would duplicate its shape.
pub fn canonical_escape_claim_bytes(
    genesis_hash: [u8; 32],
    chain_id: u64,
    vault_address: [u8; 20],
    state_root: [u8; 32],
    height: u64,
    account_id: u64,
    recipient: [u8; 20],
    amount_token_units: u64,
) -> Vec<u8> {
    const DOMAIN: &[u8] = b"sybil/escape-claim/v1";
    let mut bytes = Vec::with_capacity(DOMAIN.len() + 32 + 8 + 20 + 32 + 8 + 8 + 20 + 8);
    bytes.extend_from_slice(DOMAIN);
    bytes.extend_from_slice(&genesis_hash);
    bytes.extend_from_slice(&chain_id.to_le_bytes());
    bytes.extend_from_slice(&vault_address);
    bytes.extend_from_slice(&state_root);
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&account_id.to_le_bytes());
    bytes.extend_from_slice(&recipient);
    bytes.extend_from_slice(&amount_token_units.to_le_bytes());
    bytes
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

        assert_ne!(base, account_keys_digest(43, [raw_key(0x11)]));
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

    #[test]
    fn webauthn_envelope_limits_are_protocol_constants() {
        assert_eq!(MAX_WEBAUTHN_AUTHENTICATOR_DATA_BYTES, 512);
        assert_eq!(MAX_WEBAUTHN_CLIENT_DATA_JSON_BYTES, 2_048);
    }

    #[test]
    fn key_operation_bytes_pin_domains_layout_and_all_fields() {
        let genesis = [0x11; 32];
        let key = webauthn_key(0x22);
        let keys_digest = [0x33; 32];
        let events_digest = [0x44; 32];
        let registration =
            canonical_key_registration_bytes(genesis, 7, &key, keys_digest, events_digest);
        let revocation =
            canonical_key_revocation_bytes(genesis, 7, &key, keys_digest, events_digest);

        for (bytes, domain) in [
            (
                registration.as_slice(),
                b"sybil/keyop/register/v1".as_slice(),
            ),
            (revocation.as_slice(), b"sybil/keyop/revoke/v1".as_slice()),
        ] {
            assert_eq!(&bytes[..domain.len()], domain);
            let mut offset = domain.len();
            assert_eq!(&bytes[offset..offset + 32], &genesis);
            offset += 32;
            assert_eq!(&bytes[offset..offset + 8], &7u64.to_le_bytes());
            offset += 8;
            assert_eq!(bytes[offset], key.auth_scheme);
            offset += 1;
            assert_eq!(&bytes[offset..offset + 33], &key.pubkey_sec1);
            offset += 33;
            assert_eq!(
                &bytes[offset..offset + 4],
                &key.capability_mask.to_le_bytes()
            );
            offset += 4;
            assert_eq!(&bytes[offset..offset + 32], &keys_digest);
            offset += 32;
            assert_eq!(&bytes[offset..offset + 32], &events_digest);
            offset += 32;
            assert_eq!(offset, bytes.len());
        }
        assert_ne!(registration, revocation);

        assert_ne!(
            registration,
            canonical_key_registration_bytes([0x12; 32], 7, &key, keys_digest, events_digest)
        );
        assert_ne!(
            registration,
            canonical_key_registration_bytes(genesis, 8, &key, keys_digest, events_digest)
        );
        assert_ne!(
            registration,
            canonical_key_registration_bytes(
                genesis,
                7,
                &raw_key(0x22),
                keys_digest,
                events_digest
            )
        );
        let mut different_pubkey = key;
        different_pubkey.pubkey_sec1[1] ^= 1;
        assert_ne!(
            registration,
            canonical_key_registration_bytes(
                genesis,
                7,
                &different_pubkey,
                keys_digest,
                events_digest
            )
        );
        let mut different_capability = key;
        different_capability.capability_mask ^= 1;
        assert_ne!(
            registration,
            canonical_key_registration_bytes(
                genesis,
                7,
                &different_capability,
                keys_digest,
                events_digest
            )
        );
        assert_ne!(
            registration,
            canonical_key_registration_bytes(genesis, 7, &key, [0x34; 32], events_digest)
        );
        assert_ne!(
            registration,
            canonical_key_registration_bytes(genesis, 7, &key, keys_digest, [0x45; 32])
        );
    }

    #[test]
    fn escape_claim_bytes_pin_frozen_layout() {
        let bytes = canonical_escape_claim_bytes(
            [0x11; 32],
            0x0102_0304_0506_0708,
            [0x22; 20],
            [0x33; 32],
            0x1112_1314_1516_1718,
            0x2122_2324_2526_2728,
            [0x44; 20],
            0x3132_3334_3536_3738,
        );
        assert_eq!(&bytes[..21], b"sybil/escape-claim/v1");
        assert_eq!(&bytes[21..53], &[0x11; 32]);
        assert_eq!(&bytes[53..61], &0x0102_0304_0506_0708u64.to_le_bytes());
        assert_eq!(&bytes[61..81], &[0x22; 20]);
        assert_eq!(&bytes[81..113], &[0x33; 32]);
        assert_eq!(&bytes[113..121], &0x1112_1314_1516_1718u64.to_le_bytes());
        assert_eq!(&bytes[121..129], &0x2122_2324_2526_2728u64.to_le_bytes());
        assert_eq!(&bytes[129..149], &[0x44; 20]);
        assert_eq!(&bytes[149..157], &0x3132_3334_3536_3738u64.to_le_bytes());
    }
}
