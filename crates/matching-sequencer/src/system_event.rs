use matching_engine::{MarketId, Nanos, OrderDirection};

use crate::account::AccountId;
use crate::bridge::{L1Deposit, WithdrawalLeaf, WithdrawalRefundReason};
use sybil_verifier::{KeyOpAuth, KeyRecord};

/// System state changes applied outside the matching pipeline.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum SystemEvent {
    CreateAccount {
        account_id: AccountId,
        initial_balance: i64,
        initial_keys: Vec<KeyRecord>,
    },
    Deposit {
        account_id: AccountId,
        amount: i64,
    },
    L1Deposit {
        account_id: AccountId,
        amount: i64,
        deposit: L1Deposit,
    },
    WithdrawalCreated {
        account_id: AccountId,
        amount: i64,
        withdrawal: WithdrawalLeaf,
    },
    WithdrawalRefunded {
        account_id: AccountId,
        withdrawal_id: u64,
        amount: i64,
        reason: WithdrawalRefundReason,
    },
    WithdrawalFinalized {
        account_id: AccountId,
        withdrawal_id: u64,
        amount: i64,
    },
    /// Monotonic confirmed L1 scan height used as the withdrawal-expiry clock.
    L1BlockObserved {
        height: u64,
    },
    MarketResolved {
        market_id: MarketId,
        payout_nanos: Nanos,
        affected_accounts: Vec<AccountId>,
    },
    /// A resting order was cancelled by its owner (D1). `market_ids` is the
    /// order's set of active markets; `side` is the categorical direction
    /// w.r.t. `market_ids[0]` (the primary market) as derived by
    /// `matching_engine::derive_order_direction`; `remaining_quantity` is
    /// `max_fill` at cancel time (post any partial fills, so cancels of
    /// partially-filled orders surface the unfilled remainder).
    OrderCancelled {
        account_id: AccountId,
        order_id: u64,
        market_ids: Vec<MarketId>,
        side: OrderDirection,
        remaining_quantity: u64,
    },
    /// A market was added to an existing mutually-exclusive group (SYB-212).
    MarketGroupExtended {
        group_id: u64,
        market_id: MarketId,
    },
    KeyRegistered {
        account_id: AccountId,
        key: KeyRecord,
        authorization: KeyOpAuth,
    },
    KeyRevoked {
        account_id: AccountId,
        key: KeyRecord,
        authorization: KeyOpAuth,
    },
    DepositQuarantined {
        amount: i64,
        deposit: L1Deposit,
    },
    QuarantineClaimed {
        account_id: AccountId,
        amount: i64,
        sybil_account_key: [u8; 32],
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_cancelled_serde_roundtrip() {
        let event = SystemEvent::OrderCancelled {
            account_id: AccountId(42),
            order_id: 1234,
            market_ids: vec![MarketId::new(7), MarketId::new(3)],
            side: OrderDirection::BuyNo,
            remaining_quantity: 9,
        };
        let bytes = rmp_serde::to_vec_named(&event).expect("encode");
        let decoded: SystemEvent = rmp_serde::from_slice(&bytes).expect("decode");
        match decoded {
            SystemEvent::OrderCancelled {
                account_id,
                order_id,
                market_ids,
                side,
                remaining_quantity,
            } => {
                assert_eq!(account_id, AccountId(42));
                assert_eq!(order_id, 1234);
                assert_eq!(market_ids, vec![MarketId::new(7), MarketId::new(3)]);
                assert_eq!(side, OrderDirection::BuyNo);
                assert_eq!(remaining_quantity, 9);
            }
            other => panic!("expected OrderCancelled, got {:?}", other),
        }
    }
}
