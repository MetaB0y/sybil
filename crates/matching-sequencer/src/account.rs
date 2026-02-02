use std::collections::HashMap;

use matching_engine::MarketId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AccountId(pub u64);

#[derive(Clone)]
pub struct Account {
    pub id: AccountId,
    /// Balance in nanos, signed (can go negative during settlement if needed)
    pub balance: i64,
    /// Positions: (market, outcome_idx) -> signed quantity
    pub positions: HashMap<(MarketId, u8), i64>,
}

impl Account {
    pub fn new(id: AccountId, balance: i64) -> Self {
        Self {
            id,
            balance,
            positions: HashMap::new(),
        }
    }

    pub fn position(&self, market: MarketId, outcome: u8) -> i64 {
        self.positions.get(&(market, outcome)).copied().unwrap_or(0)
    }
}

#[derive(Default)]
pub struct AccountStore {
    accounts: HashMap<AccountId, Account>,
    next_id: u64,
}

impl AccountStore {
    pub fn new() -> Self {
        Self::default()
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
}
