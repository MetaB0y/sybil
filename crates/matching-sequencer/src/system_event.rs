use matching_engine::{MarketId, Nanos};

use crate::account::AccountId;
use crate::bridge::{L1Deposit, WithdrawalLeaf};

/// System state changes applied outside the matching pipeline.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum SystemEvent {
    CreateAccount {
        account_id: AccountId,
        initial_balance: i64,
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
    MarketResolved {
        market_id: MarketId,
        payout_nanos: Nanos,
        affected_accounts: Vec<AccountId>,
    },
}
