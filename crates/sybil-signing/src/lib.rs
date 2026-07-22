use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

pub const MAX_MARKETS_PER_ORDER: usize = 5;
pub const MAX_STATES: usize = 32;
pub type GenesisHash = [u8; 32];

pub const ORDER_DOMAIN: &[u8] = b"sybil/signing/order/v1";
pub const CANCEL_DOMAIN: &[u8] = b"sybil/signing/cancel/v1";
pub const MM_BUNDLE_DOMAIN: &[u8] = b"sybil/signing/mm-bundle/v1";
pub const PROFILE_UPDATE_DOMAIN: &[u8] = b"sybil/signing/profile-update/v1";
pub const API_KEY_CREATE_DOMAIN: &[u8] = b"sybil/signing/read-api-key-create/v1";
pub const API_KEY_REVOKE_DOMAIN: &[u8] = b"sybil/signing/read-api-key-revoke/v1";
pub const BRIDGE_WITHDRAWAL_DOMAIN: &[u8] = b"sybil/signing/bridge-withdrawal/v1";
pub const RESOLUTION_ATTESTATION_DOMAIN: &[u8] = b"sybil/signing/resolution-attestation/v1";

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

/// Side of one order in a signed market-maker bundle.
///
/// This is signed explicitly because shared-budget capital depends on the
/// side. Admission also derives the side from the payoff vector and rejects a
/// mismatch, so the signed value cannot be used to relabel an order.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub enum MmSide {
    SellYes,
    BuyYes,
    SellNo,
    BuyNo,
}

/// Canonical order body inside an atomic MM bundle.
///
/// Sequencer-assigned order ids and the bundle-level replay nonce are
/// deliberately absent. `side` is part of the signed economic intent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct MmBundleOrder {
    pub markets: [MarketId; MAX_MARKETS_PER_ORDER],
    pub num_markets: u8,
    pub payoffs: [i8; MAX_STATES],
    pub num_states: u8,
    pub limit_price: u64,
    pub max_fill: u64,
    pub condition: Option<PriceCondition>,
    pub expires_at_block: Option<u64>,
    pub side: MmSide,
}

/// Signed economic fields of one all-or-nothing flash-liquidity bundle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct MmBundle {
    pub account_id: u64,
    pub bundle_id: [u8; 32],
    pub revision: u64,
    pub orders: Vec<MmBundleOrder>,
    pub max_capital: u64,
    pub nonce: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct OrderRequest {
    genesis_hash: GenesisHash,
    order: Order,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct MmBundleRequest {
    genesis_hash: GenesisHash,
    bundle: MmBundle,
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
    genesis_hash: GenesisHash,
    account_id: u64,
    display_name: Option<String>,
    avatar_seed: Option<String>,
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
    genesis_hash: GenesisHash,
    account_id: u64,
    label: Option<String>,
    nonce: u64,
}

/// Canonical, stable byte layout of a read API-key revocation (SYB-60).
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct ApiKeyRevoke {
    genesis_hash: GenesisHash,
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

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct ResolutionAttestationSigningRequest {
    genesis_hash: GenesisHash,
    attestation: ResolutionAttestation,
}

fn domain_separated_bytes<T: BorshSerialize>(domain: &[u8], value: &T) -> Vec<u8> {
    let payload = borsh::to_vec(value).expect("canonical serialization should not fail");
    let mut bytes = Vec::with_capacity(domain.len() + payload.len());
    bytes.extend_from_slice(domain);
    bytes.extend_from_slice(&payload);
    bytes
}

pub fn canonical_order_bytes(order: &Order, genesis_hash: GenesisHash) -> Vec<u8> {
    domain_separated_bytes(
        ORDER_DOMAIN,
        &OrderRequest {
            genesis_hash,
            order: order.clone(),
        },
    )
}

pub fn canonical_mm_bundle_bytes(bundle: &MmBundle, genesis_hash: GenesisHash) -> Vec<u8> {
    domain_separated_bytes(
        MM_BUNDLE_DOMAIN,
        &MmBundleRequest {
            genesis_hash,
            bundle: bundle.clone(),
        },
    )
}

pub fn canonical_cancel_bytes(
    account_id: u64,
    order_id: u64,
    nonce: u64,
    genesis_hash: GenesisHash,
) -> Vec<u8> {
    domain_separated_bytes(
        CANCEL_DOMAIN,
        &CancelRequest {
            genesis_hash,
            account_id,
            order_id,
            nonce,
        },
    )
}

pub fn canonical_attestation_bytes(
    att: &ResolutionAttestation,
    genesis_hash: GenesisHash,
) -> Vec<u8> {
    domain_separated_bytes(
        RESOLUTION_ATTESTATION_DOMAIN,
        &ResolutionAttestationSigningRequest {
            genesis_hash,
            attestation: att.clone(),
        },
    )
}

pub fn canonical_bridge_withdrawal_bytes(
    request: &BridgeWithdrawalRequest,
    genesis_hash: GenesisHash,
) -> Vec<u8> {
    domain_separated_bytes(
        BRIDGE_WITHDRAWAL_DOMAIN,
        &BridgeWithdrawalSigningRequest {
            genesis_hash,
            request: request.clone(),
        },
    )
}

/// Canonical bytes for a signed account-profile update (SYB-60).
pub fn canonical_profile_update_bytes(
    account_id: u64,
    display_name: Option<&str>,
    avatar_seed: Option<&str>,
    nonce: u64,
    genesis_hash: GenesisHash,
) -> Vec<u8> {
    domain_separated_bytes(
        PROFILE_UPDATE_DOMAIN,
        &ProfileUpdate {
            genesis_hash,
            account_id,
            display_name: display_name.map(str::to_owned),
            avatar_seed: avatar_seed.map(str::to_owned),
            nonce,
        },
    )
}

/// Canonical bytes for a signed read API-key creation (SYB-60).
pub fn canonical_api_key_create_bytes(
    account_id: u64,
    label: Option<&str>,
    nonce: u64,
    genesis_hash: GenesisHash,
) -> Vec<u8> {
    domain_separated_bytes(
        API_KEY_CREATE_DOMAIN,
        &ApiKeyCreate {
            genesis_hash,
            account_id,
            label: label.map(str::to_owned),
            nonce,
        },
    )
}

/// Canonical bytes for a signed read API-key revocation (SYB-60).
pub fn canonical_api_key_revoke_bytes(
    account_id: u64,
    api_key_id: u64,
    nonce: u64,
    genesis_hash: GenesisHash,
) -> Vec<u8> {
    domain_separated_bytes(
        API_KEY_REVOKE_DOMAIN,
        &ApiKeyRevoke {
            genesis_hash,
            account_id,
            api_key_id,
            nonce,
        },
    )
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
    fn atomic_mm_bundle_snapshot() {
        let bundle = MmBundle {
            account_id: 42,
            bundle_id: [0x11; 32],
            revision: 0,
            orders: vec![
                MmBundleOrder {
                    markets: [
                        MarketId(7),
                        MarketId::NONE,
                        MarketId::NONE,
                        MarketId::NONE,
                        MarketId::NONE,
                    ],
                    num_markets: 1,
                    payoffs: {
                        let mut payoffs = [0; MAX_STATES];
                        payoffs[0] = 1;
                        payoffs
                    },
                    num_states: 2,
                    limit_price: 510_000_000,
                    max_fill: 1_000,
                    condition: None,
                    expires_at_block: Some(9),
                    side: MmSide::BuyYes,
                },
                MmBundleOrder {
                    markets: [
                        MarketId(8),
                        MarketId::NONE,
                        MarketId::NONE,
                        MarketId::NONE,
                        MarketId::NONE,
                    ],
                    num_markets: 1,
                    payoffs: {
                        let mut payoffs = [0; MAX_STATES];
                        payoffs[0] = -1;
                        payoffs
                    },
                    num_states: 2,
                    limit_price: 490_000_000,
                    max_fill: 2_000,
                    condition: None,
                    expires_at_block: Some(9),
                    side: MmSide::SellYes,
                },
            ],
            max_capital: 3_000_000_000,
            nonce: 17,
        };

        insta::assert_snapshot!(
            "atomic_mm_bundle",
            hex::encode(canonical_mm_bundle_bytes(&bundle, GENESIS_HASH))
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
            hex::encode(canonical_attestation_bytes(&att, GENESIS_HASH))
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
        let a = canonical_profile_update_bytes(7, Some("alice"), Some("seed-1"), 11, GENESIS_HASH);
        let b = canonical_profile_update_bytes(7, Some("alice"), Some("seed-1"), 11, GENESIS_HASH);
        assert_eq!(a, b, "encoding must be deterministic");
        // Clearing (None) differs from setting, and each field is covered.
        assert_ne!(
            a,
            canonical_profile_update_bytes(7, None, None, 11, GENESIS_HASH)
        );
        assert_ne!(
            a,
            canonical_profile_update_bytes(7, Some("bob"), Some("seed-1"), 11, GENESIS_HASH)
        );
        assert_ne!(
            a,
            canonical_profile_update_bytes(7, Some("alice"), Some("seed-2"), 11, GENESIS_HASH)
        );
        assert_ne!(
            a,
            canonical_profile_update_bytes(7, Some("alice"), Some("seed-1"), 12, GENESIS_HASH)
        );
        assert_ne!(
            a,
            canonical_profile_update_bytes(8, Some("alice"), Some("seed-1"), 11, GENESIS_HASH)
        );
        assert_ne!(
            a,
            canonical_profile_update_bytes(7, Some("alice"), Some("seed-1"), 11, [0xcd; 32]),
            "genesis_hash must be covered"
        );
    }

    #[test]
    fn api_key_bytes_cover_all_fields() {
        let create = canonical_api_key_create_bytes(9, Some("grafana"), 2, GENESIS_HASH);
        assert_eq!(
            create,
            canonical_api_key_create_bytes(9, Some("grafana"), 2, GENESIS_HASH)
        );
        assert_ne!(
            create,
            canonical_api_key_create_bytes(9, None, 2, GENESIS_HASH)
        );
        assert_ne!(
            create,
            canonical_api_key_create_bytes(9, Some("grafana"), 3, GENESIS_HASH)
        );
        assert_ne!(
            create,
            canonical_api_key_create_bytes(9, Some("grafana"), 2, [0xcd; 32])
        );

        let revoke = canonical_api_key_revoke_bytes(9, 42, 2, GENESIS_HASH);
        assert_eq!(
            revoke,
            canonical_api_key_revoke_bytes(9, 42, 2, GENESIS_HASH)
        );
        assert_ne!(
            revoke,
            canonical_api_key_revoke_bytes(9, 43, 2, GENESIS_HASH)
        );
        assert_ne!(
            revoke,
            canonical_api_key_revoke_bytes(9, 42, 3, GENESIS_HASH)
        );
        assert_ne!(revoke, canonical_api_key_revoke_bytes(9, 42, 2, [0xcd; 32]));
    }

    #[test]
    fn action_domains_are_unique_and_explicit() {
        let domains = [
            ORDER_DOMAIN,
            CANCEL_DOMAIN,
            PROFILE_UPDATE_DOMAIN,
            API_KEY_CREATE_DOMAIN,
            API_KEY_REVOKE_DOMAIN,
            BRIDGE_WITHDRAWAL_DOMAIN,
            RESOLUTION_ATTESTATION_DOMAIN,
        ];
        for (index, domain) in domains.iter().enumerate() {
            assert!(domain.ends_with(b"/v1"));
            assert!(
                domains[..index].iter().all(|other| other != domain),
                "duplicate signed-action domain: {}",
                String::from_utf8_lossy(domain)
            );
        }
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
