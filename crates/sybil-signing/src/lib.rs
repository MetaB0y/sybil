use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

pub const MAX_MARKETS_PER_ORDER: usize = 5;
pub const MAX_STATES: usize = 32;
pub type GenesisHash = [u8; 32];

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct MarketId(pub u32);

impl MarketId {
    pub const NONE: Self = Self(u32::MAX);
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub enum ConditionDir {
    Above,
    Below,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct PriceCondition {
    pub market: MarketId,
    pub threshold: u64,
    pub direction: ConditionDir,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Order {
    pub markets: [MarketId; MAX_MARKETS_PER_ORDER],
    pub num_markets: u8,
    pub payoffs: [i8; MAX_STATES],
    pub num_states: u8,
    pub limit_price: u64,
    pub max_fill: u64,
    pub condition: Option<PriceCondition>,
    pub expires_at_block: Option<u64>,
    pub nonce: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct OrderRequest {
    genesis_hash: GenesisHash,
    order: Order,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct CancelRequest {
    genesis_hash: GenesisHash,
    account_id: u64,
    order_id: u64,
    nonce: u64,
}

/// Canonical, stable byte layout of an account profile update (SYB-60).
///
/// `display_name`/`avatar_seed` are `None` to clear. Both the set and clear
/// intents are covered by the signature so a relay cannot forge either.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct ProfileUpdate {
    account_id: u64,
    display_name: Option<String>,
    avatar_seed: Option<String>,
    nonce: u64,
}

/// Canonical, stable byte layout of a signing-key revocation (SYB-60).
///
/// `target_pubkey` is the 33-byte compressed SEC1 point of the key being
/// revoked. Covering it by signature prevents a relay from redirecting the
/// revocation at a different key. Like orders/cancels (SYB-224) and key
/// registrations (SYB-229), the payload is domain-separated by `genesis_hash`
/// (SYB-231) so a revocation signature cannot be replayed onto a different
/// chain/genesis after a fresh-genesis redeploy.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct KeyRevocation {
    genesis_hash: GenesisHash,
    account_id: u64,
    target_pubkey: Vec<u8>,
    nonce: u64,
}

/// Canonical, stable byte layout of a signing-key registration (SYB-229).
///
/// A signed key registration is required whenever the target account already
/// has at least one registered key (the first key is bootstrapped over the
/// service tier). Like orders/cancels (SYB-224), the payload is domain-separated
/// by `genesis_hash` so a registration signature cannot be replayed onto a
/// different chain/genesis. `new_key_auth_scheme` is `0` for raw P256 and `1`
/// for WebAuthn. Both `new_key_pubkey` and `signer_pubkey` are 33-byte
/// compressed SEC1 points; covering both by signature binds the new key to the
/// authorizing signer so a relay can neither swap in a different key nor
/// redirect the authorization.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct KeyRegistration {
    genesis_hash: GenesisHash,
    account_id: u64,
    new_key_auth_scheme: u8,
    new_key_pubkey: Vec<u8>,
    signer_pubkey: Vec<u8>,
    nonce: u64,
}

/// Canonical, stable byte layout of a read API-key creation (SYB-60).
///
/// The bearer token itself is server-generated entropy and is deliberately NOT
/// part of the signed payload; the signature authorizes the *creation intent*
/// (and burns a replay nonce) while the token material never touches signed or
/// persisted bytes in plaintext.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct ApiKeyCreate {
    account_id: u64,
    label: Option<String>,
    nonce: u64,
}

/// Canonical, stable byte layout of a read API-key revocation (SYB-60).
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct ApiKeyRevoke {
    account_id: u64,
    api_key_id: u64,
    nonce: u64,
}

/// Canonical, stable logical fields of a bridge withdrawal request. The signed
/// bytes wrap this value with `genesis_hash`, matching orders and cancellations.
/// This mirrors `matching_sequencer::bridge::BridgeWithdrawalRequest` without
/// importing it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct BridgeWithdrawalRequest {
    pub account_id: u64,
    pub chain_id: u64,
    pub vault_address: [u8; 20],
    pub recipient: [u8; 20],
    pub token_address: [u8; 20],
    pub amount_token_units: u64,
    pub expiry_height: u64,
    pub nonce: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct BridgeWithdrawalSigningRequest {
    genesis_hash: GenesisHash,
    request: BridgeWithdrawalRequest,
}

/// Canonical, stable byte layout of a resolution attestation. Mirrors
/// `sybil_oracle::ResolutionAttestation` without importing it (keeps this
/// crate dependency-light).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct ResolutionAttestation {
    pub market_id: MarketId,
    pub payout_nanos: u64,
    pub nonce: u64,
}

pub fn canonical_order_bytes(order: &Order, genesis_hash: GenesisHash) -> Vec<u8> {
    borsh::to_vec(&OrderRequest {
        genesis_hash,
        order: order.clone(),
    })
    .expect("canonical order serialization should not fail")
}

pub fn canonical_cancel_bytes(
    account_id: u64,
    order_id: u64,
    nonce: u64,
    genesis_hash: GenesisHash,
) -> Vec<u8> {
    borsh::to_vec(&CancelRequest {
        genesis_hash,
        account_id,
        order_id,
        nonce,
    })
    .expect("canonical cancel serialization should not fail")
}

pub fn canonical_attestation_bytes(att: &ResolutionAttestation) -> Vec<u8> {
    borsh::to_vec(att).expect("canonical attestation serialization should not fail")
}

pub fn canonical_bridge_withdrawal_bytes(
    request: &BridgeWithdrawalRequest,
    genesis_hash: GenesisHash,
) -> Vec<u8> {
    borsh::to_vec(&BridgeWithdrawalSigningRequest {
        genesis_hash,
        request: request.clone(),
    })
    .expect("canonical bridge withdrawal serialization should not fail")
}

/// Canonical bytes for a signed account-profile update (SYB-60).
pub fn canonical_profile_update_bytes(
    account_id: u64,
    display_name: Option<&str>,
    avatar_seed: Option<&str>,
    nonce: u64,
) -> Vec<u8> {
    borsh::to_vec(&ProfileUpdate {
        account_id,
        display_name: display_name.map(str::to_owned),
        avatar_seed: avatar_seed.map(str::to_owned),
        nonce,
    })
    .expect("canonical profile update serialization should not fail")
}

/// Canonical bytes for a signed signing-key revocation (SYB-60).
///
/// `target_pubkey` must be the 33-byte compressed SEC1 encoding of the key
/// being revoked. Domain-separated by `genesis_hash` (SYB-231), mirroring
/// orders/cancels (SYB-224) and key registrations (SYB-229).
pub fn canonical_key_revocation_bytes(
    genesis_hash: GenesisHash,
    account_id: u64,
    target_pubkey: &[u8],
    nonce: u64,
) -> Vec<u8> {
    borsh::to_vec(&KeyRevocation {
        genesis_hash,
        account_id,
        target_pubkey: target_pubkey.to_vec(),
        nonce,
    })
    .expect("canonical key revocation serialization should not fail")
}

/// Canonical bytes for a signed signing-key registration (SYB-229).
///
/// `new_key_auth_scheme` is `0` for raw P256 and `1` for WebAuthn. Both
/// `new_key_pubkey` and `signer_pubkey` must be the 33-byte compressed SEC1
/// encodings of the respective keys.
pub fn canonical_key_registration_bytes(
    genesis_hash: GenesisHash,
    account_id: u64,
    new_key_auth_scheme: u8,
    new_key_pubkey: &[u8],
    signer_pubkey: &[u8],
    nonce: u64,
) -> Vec<u8> {
    borsh::to_vec(&KeyRegistration {
        genesis_hash,
        account_id,
        new_key_auth_scheme,
        new_key_pubkey: new_key_pubkey.to_vec(),
        signer_pubkey: signer_pubkey.to_vec(),
        nonce,
    })
    .expect("canonical key registration serialization should not fail")
}

/// Canonical bytes for a signed read API-key creation (SYB-60).
pub fn canonical_api_key_create_bytes(account_id: u64, label: Option<&str>, nonce: u64) -> Vec<u8> {
    borsh::to_vec(&ApiKeyCreate {
        account_id,
        label: label.map(str::to_owned),
        nonce,
    })
    .expect("canonical api key create serialization should not fail")
}

/// Canonical bytes for a signed read API-key revocation (SYB-60).
pub fn canonical_api_key_revoke_bytes(account_id: u64, api_key_id: u64, nonce: u64) -> Vec<u8> {
    borsh::to_vec(&ApiKeyRevoke {
        account_id,
        api_key_id,
        nonce,
    })
    .expect("canonical api key revoke serialization should not fail")
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENESIS_HASH: GenesisHash = [0xab; 32];

    fn order_with(
        markets: &[u32],
        payoffs: &[i8],
        limit_price: u64,
        max_fill: u64,
        condition: Option<PriceCondition>,
    ) -> Order {
        let mut order = Order {
            markets: [MarketId::NONE; MAX_MARKETS_PER_ORDER],
            num_markets: markets.len() as u8,
            payoffs: [0; MAX_STATES],
            num_states: (1usize << markets.len()) as u8,
            limit_price,
            max_fill,
            condition,
            expires_at_block: None,
            nonce: 7,
        };

        for (idx, market) in markets.iter().copied().enumerate() {
            order.markets[idx] = MarketId(market);
        }

        for (idx, payoff) in payoffs.iter().copied().enumerate() {
            order.payoffs[idx] = payoff;
        }

        order
    }

    #[test]
    fn buy_yes_snapshot() {
        let order = order_with(&[7], &[1, 0], 550_000_000, 10, None);
        insta::assert_snapshot!(
            "buy_yes",
            hex::encode(canonical_order_bytes(&order, GENESIS_HASH))
        );
    }

    #[test]
    fn sell_yes_snapshot() {
        let order = order_with(&[7], &[-1, 0], 425_000_000, 3, None);
        insta::assert_snapshot!(
            "sell_yes",
            hex::encode(canonical_order_bytes(&order, GENESIS_HASH))
        );
    }

    #[test]
    fn spread_snapshot() {
        let order = order_with(&[3, 9], &[0, -1, 1, 0], 125_000_000, 5, None);
        insta::assert_snapshot!(
            "spread",
            hex::encode(canonical_order_bytes(&order, GENESIS_HASH))
        );
    }

    #[test]
    fn bundle_snapshot() {
        let order = order_with(&[1, 2, 4], &[0, 0, 0, 0, 0, 0, 0, 1], 300_000_000, 2, None);
        insta::assert_snapshot!(
            "bundle",
            hex::encode(canonical_order_bytes(&order, GENESIS_HASH))
        );
    }

    #[test]
    fn attestation_snapshot() {
        let att = ResolutionAttestation {
            market_id: MarketId(7),
            payout_nanos: 1_000_000_000,
            nonce: 1_700_000_000_000,
        };
        insta::assert_snapshot!(
            "attestation",
            hex::encode(canonical_attestation_bytes(&att))
        );
    }

    #[test]
    fn bridge_withdrawal_snapshot() {
        let request = BridgeWithdrawalRequest {
            account_id: 9,
            chain_id: 31_337,
            vault_address: [0x11; 20],
            recipient: [0x22; 20],
            token_address: [0x33; 20],
            amount_token_units: 42_000_000,
            expiry_height: 123_456,
            nonce: 9,
        };
        insta::assert_snapshot!(
            "bridge_withdrawal",
            hex::encode(canonical_bridge_withdrawal_bytes(&request, GENESIS_HASH))
        );
    }

    #[test]
    fn cancel_snapshot() {
        insta::assert_snapshot!(
            "cancel",
            hex::encode(canonical_cancel_bytes(7, 42, 11, GENESIS_HASH))
        );
    }

    #[test]
    fn profile_update_bytes_deterministic_and_field_covering() {
        let a = canonical_profile_update_bytes(7, Some("alice"), Some("seed-1"), 11);
        let b = canonical_profile_update_bytes(7, Some("alice"), Some("seed-1"), 11);
        assert_eq!(a, b, "encoding must be deterministic");
        // Clearing (None) differs from setting, and each field is covered.
        assert_ne!(a, canonical_profile_update_bytes(7, None, None, 11));
        assert_ne!(
            a,
            canonical_profile_update_bytes(7, Some("bob"), Some("seed-1"), 11)
        );
        assert_ne!(
            a,
            canonical_profile_update_bytes(7, Some("alice"), Some("seed-2"), 11)
        );
        assert_ne!(
            a,
            canonical_profile_update_bytes(7, Some("alice"), Some("seed-1"), 12)
        );
        assert_ne!(
            a,
            canonical_profile_update_bytes(8, Some("alice"), Some("seed-1"), 11)
        );
    }

    #[test]
    fn key_registration_snapshot() {
        // Stable vector mirrored by the TS canonical encoder parity fixtures
        // (frontend/web/src/lib/auth/__tests__/canonical-settings.test.ts).
        let new_key = vec![0x02u8; 33];
        let signer = vec![0x03u8; 33];
        insta::assert_snapshot!(
            "key_registration",
            hex::encode(canonical_key_registration_bytes(
                GENESIS_HASH,
                7,
                1,
                &new_key,
                &signer,
                42
            ))
        );
    }

    #[test]
    fn key_registration_bytes_cover_all_fields() {
        let new_key = [0x02u8; 33];
        let signer = [0x03u8; 33];
        let a = canonical_key_registration_bytes(GENESIS_HASH, 7, 0, &new_key, &signer, 42);
        assert_eq!(
            a,
            canonical_key_registration_bytes(GENESIS_HASH, 7, 0, &new_key, &signer, 42),
            "encoding must be deterministic"
        );
        // Every field is signature-covered.
        assert_ne!(
            a,
            canonical_key_registration_bytes([0xcd; 32], 7, 0, &new_key, &signer, 42),
            "genesis_hash must be covered"
        );
        assert_ne!(
            a,
            canonical_key_registration_bytes(GENESIS_HASH, 8, 0, &new_key, &signer, 42)
        );
        assert_ne!(
            a,
            canonical_key_registration_bytes(GENESIS_HASH, 7, 1, &new_key, &signer, 42),
            "new_key_auth_scheme must be covered"
        );
        assert_ne!(
            a,
            canonical_key_registration_bytes(GENESIS_HASH, 7, 0, &[0x04; 33], &signer, 42)
        );
        assert_ne!(
            a,
            canonical_key_registration_bytes(GENESIS_HASH, 7, 0, &new_key, &[0x05; 33], 42),
            "signer_pubkey must be covered"
        );
        assert_ne!(
            a,
            canonical_key_registration_bytes(GENESIS_HASH, 7, 0, &new_key, &signer, 43)
        );
    }

    #[test]
    fn key_revocation_snapshot() {
        // Stable vector mirrored by the TS canonical encoder parity fixtures
        // (frontend/web/src/lib/auth/__tests__/canonical-settings.test.ts).
        let target = vec![0x02u8; 33];
        insta::assert_snapshot!(
            "key_revocation",
            hex::encode(canonical_key_revocation_bytes(GENESIS_HASH, 7, &target, 42))
        );
    }

    #[test]
    fn key_revocation_bytes_cover_genesis_target_and_nonce() {
        let a = canonical_key_revocation_bytes(GENESIS_HASH, 3, &[0x02; 33], 5);
        assert_eq!(
            a,
            canonical_key_revocation_bytes(GENESIS_HASH, 3, &[0x02; 33], 5)
        );
        // genesis_hash is signature-covered (SYB-231) so a captured revocation
        // cannot replay against a fresh-genesis redeploy.
        assert_ne!(
            a,
            canonical_key_revocation_bytes([0xcd; 32], 3, &[0x02; 33], 5),
            "genesis_hash must be covered"
        );
        assert_ne!(
            a,
            canonical_key_revocation_bytes(GENESIS_HASH, 3, &[0x03; 33], 5)
        );
        assert_ne!(
            a,
            canonical_key_revocation_bytes(GENESIS_HASH, 3, &[0x02; 33], 6)
        );
        assert_ne!(
            a,
            canonical_key_revocation_bytes(GENESIS_HASH, 4, &[0x02; 33], 5)
        );
    }

    #[test]
    fn api_key_bytes_cover_all_fields() {
        let create = canonical_api_key_create_bytes(9, Some("grafana"), 2);
        assert_eq!(
            create,
            canonical_api_key_create_bytes(9, Some("grafana"), 2)
        );
        assert_ne!(create, canonical_api_key_create_bytes(9, None, 2));
        assert_ne!(
            create,
            canonical_api_key_create_bytes(9, Some("grafana"), 3)
        );

        let revoke = canonical_api_key_revoke_bytes(9, 42, 2);
        assert_eq!(revoke, canonical_api_key_revoke_bytes(9, 42, 2));
        assert_ne!(revoke, canonical_api_key_revoke_bytes(9, 43, 2));
        assert_ne!(revoke, canonical_api_key_revoke_bytes(9, 42, 3));
    }

    #[test]
    fn conditional_snapshot() {
        let order = order_with(
            &[5],
            &[1, 0],
            610_000_000,
            9,
            Some(PriceCondition {
                market: MarketId(11),
                threshold: 490_000_000,
                direction: ConditionDir::Above,
            }),
        );
        insta::assert_snapshot!(
            "conditional",
            hex::encode(canonical_order_bytes(&order, GENESIS_HASH))
        );
    }
}
