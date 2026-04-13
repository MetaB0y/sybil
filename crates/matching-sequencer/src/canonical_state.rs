use sybil_verifier::AccountSnapshot;

use crate::account::{Account, AccountStore};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalState {
    accounts: Vec<AccountSnapshot>,
}

impl CanonicalState {
    pub fn from_accounts(accounts: &AccountStore) -> Self {
        Self::from_snapshot_iter(
            accounts
                .iter()
                .map(|(_, account)| snapshot_account(account)),
        )
    }

    pub fn from_snapshot_iter<I>(snapshots: I) -> Self
    where
        I: IntoIterator<Item = AccountSnapshot>,
    {
        let mut accounts: Vec<_> = snapshots.into_iter().map(canonicalize_snapshot).collect();
        accounts.sort_by_key(|snapshot| snapshot.id);
        Self { accounts }
    }

    pub fn as_snapshots(&self) -> &[AccountSnapshot] {
        &self.accounts
    }

    pub fn into_snapshots(self) -> Vec<AccountSnapshot> {
        self.accounts
    }

    pub fn state_root(&self) -> [u8; 32] {
        sybil_verifier::block::compute_state_root(&self.accounts)
    }
}

pub fn snapshot_account(account: &Account) -> AccountSnapshot {
    canonicalize_snapshot(AccountSnapshot {
        id: account.id.0,
        balance: account.balance,
        total_deposited: account.total_deposited,
        positions: account
            .positions
            .iter()
            .map(|(&(market, outcome), &qty)| (market, outcome, qty))
            .collect(),
        events_digest: account.events_digest,
    })
}

fn canonicalize_snapshot(mut snapshot: AccountSnapshot) -> AccountSnapshot {
    snapshot.positions.retain(|(_, _, qty)| *qty != 0);
    snapshot
        .positions
        .sort_by_key(|&(market, outcome, _)| (market.0, outcome));
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountStore;
    use matching_engine::MarketId;

    #[test]
    fn test_snapshot_account_filters_zero_positions() {
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100);
        let account = accounts.get_mut(aid).unwrap();
        account.positions.insert((MarketId::new(7), 0), 0);
        account.positions.insert((MarketId::new(7), 1), 3);

        let snapshot = snapshot_account(account);

        assert_eq!(snapshot.positions, vec![(MarketId::new(7), 1, 3)]);
    }

    #[test]
    fn test_canonical_state_sorts_accounts_and_positions() {
        let state = CanonicalState::from_snapshot_iter([
            AccountSnapshot {
                id: 2,
                balance: 20,
                total_deposited: 20,
                positions: vec![(MarketId::new(3), 1, 1), (MarketId::new(1), 0, 2)],
                events_digest: [2u8; 32],
            },
            AccountSnapshot {
                id: 1,
                balance: 10,
                total_deposited: 10,
                positions: vec![(MarketId::new(9), 0, 0), (MarketId::new(4), 1, 5)],
                events_digest: [1u8; 32],
            },
        ]);

        assert_eq!(state.as_snapshots()[0].id, 1);
        assert_eq!(state.as_snapshots()[1].id, 2);
        assert_eq!(
            state.as_snapshots()[0].positions,
            vec![(MarketId::new(4), 1, 5)]
        );
        assert_eq!(
            state.as_snapshots()[1].positions,
            vec![(MarketId::new(1), 0, 2), (MarketId::new(3), 1, 1)]
        );
    }
}
