use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

pub const MAX_MARKETS_PER_ORDER: usize = 5;
pub const MAX_STATES: usize = 32;

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
struct CancelRequest {
    account_id: u64,
    order_id: u64,
    nonce: u64,
}

/// Canonical, stable byte layout of a bridge withdrawal request. This mirrors
/// `matching_sequencer::bridge::BridgeWithdrawalRequest` without importing it.
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

/// Canonical, stable byte layout of a resolution attestation. Mirrors
/// `sybil_oracle::ResolutionAttestation` without importing it (keeps this
/// crate dependency-light).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct ResolutionAttestation {
    pub market_id: MarketId,
    pub payout_nanos: u64,
    pub nonce: u64,
}

pub fn canonical_order_bytes(order: &Order) -> Vec<u8> {
    borsh::to_vec(order).expect("canonical order serialization should not fail")
}

pub fn canonical_cancel_bytes(account_id: u64, order_id: u64, nonce: u64) -> Vec<u8> {
    borsh::to_vec(&CancelRequest {
        account_id,
        order_id,
        nonce,
    })
    .expect("canonical cancel serialization should not fail")
}

pub fn canonical_attestation_bytes(att: &ResolutionAttestation) -> Vec<u8> {
    borsh::to_vec(att).expect("canonical attestation serialization should not fail")
}

pub fn canonical_bridge_withdrawal_bytes(request: &BridgeWithdrawalRequest) -> Vec<u8> {
    borsh::to_vec(request).expect("canonical bridge withdrawal serialization should not fail")
}

#[cfg(test)]
mod tests {
    use super::*;

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
        insta::assert_snapshot!("buy_yes", hex::encode(canonical_order_bytes(&order)));
    }

    #[test]
    fn sell_yes_snapshot() {
        let order = order_with(&[7], &[-1, 0], 425_000_000, 3, None);
        insta::assert_snapshot!("sell_yes", hex::encode(canonical_order_bytes(&order)));
    }

    #[test]
    fn spread_snapshot() {
        let order = order_with(&[3, 9], &[0, -1, 1, 0], 125_000_000, 5, None);
        insta::assert_snapshot!("spread", hex::encode(canonical_order_bytes(&order)));
    }

    #[test]
    fn bundle_snapshot() {
        let order = order_with(&[1, 2, 4], &[0, 0, 0, 0, 0, 0, 0, 1], 300_000_000, 2, None);
        insta::assert_snapshot!("bundle", hex::encode(canonical_order_bytes(&order)));
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
            hex::encode(canonical_bridge_withdrawal_bytes(&request))
        );
    }

    #[test]
    fn cancel_snapshot() {
        insta::assert_snapshot!("cancel", hex::encode(canonical_cancel_bytes(7, 42, 11)));
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
        insta::assert_snapshot!("conditional", hex::encode(canonical_order_bytes(&order)));
    }
}
