use std::collections::HashMap;

use matching_engine::MarketId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AccountId(pub u64);

impl AccountId {
    /// Reserved system account for minting/burning operations.
    /// Holds the counterparty positions from group minting arb orders.
    pub const MINT: AccountId = AccountId(u64::MAX);
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
    #[serde(default)]
    pub events_digest: [u8; 32],
}

impl Account {
    pub fn new(id: AccountId, balance: i64) -> Self {
        Self {
            id,
            balance,
            positions: HashMap::new(),
            total_deposited: balance,
            events_digest: [0u8; 32],
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
        // Create the system mint account (zero balance, holds arb positions).
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
}
