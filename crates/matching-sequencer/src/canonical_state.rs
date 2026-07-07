use std::collections::{BTreeMap, HashSet};

use matching_engine::MarketId;
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

    /// Account-only root with the verifier's zero-valued bridge sidecar.
    pub fn state_root(&self) -> [u8; 32] {
        sybil_verifier::block::compute_state_root(&self.accounts)
    }

    pub fn market_position_totals(&self) -> MarketPositionTotals {
        MarketPositionTotals::from_snapshots(&self.accounts)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MarketPositionTotals {
    totals: BTreeMap<MarketId, (i64, i64)>,
}

impl MarketPositionTotals {
    pub fn from_snapshots(accounts: &[AccountSnapshot]) -> Self {
        let mut totals = BTreeMap::new();
        for account in accounts {
            for &(market_id, outcome, qty) in &account.positions {
                let entry = totals.entry(market_id).or_insert((0, 0));
                match outcome {
                    0 => entry.0 += qty,
                    1 => entry.1 += qty,
                    _ => {}
                }
            }
        }
        Self { totals }
    }

    pub fn totals_for(&self, market_id: MarketId) -> (i64, i64) {
        self.totals.get(&market_id).copied().unwrap_or((0, 0))
    }

    pub fn markets(&self) -> HashSet<MarketId> {
        self.totals.keys().copied().collect()
    }

    pub fn minting_inputs(&self) -> Vec<(MarketId, i64, i64)> {
        self.totals
            .iter()
            .map(|(&market_id, &(total_yes, total_no))| (market_id, total_yes, total_no))
            .collect()
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
        keys_digest: account.keys_digest,
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
                keys_digest: sybil_verifier::empty_account_keys_digest(2),
            },
            AccountSnapshot {
                id: 1,
                balance: 10,
                total_deposited: 10,
                positions: vec![(MarketId::new(9), 0, 0), (MarketId::new(4), 1, 5)],
                events_digest: [1u8; 32],
                keys_digest: sybil_verifier::empty_account_keys_digest(1),
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

    #[test]
    fn test_market_position_totals_ignore_zero_and_sum_accounts() {
        let state = CanonicalState::from_snapshot_iter([
            AccountSnapshot {
                id: 2,
                balance: 20,
                total_deposited: 20,
                positions: vec![(MarketId::new(7), 0, 3), (MarketId::new(7), 1, 1)],
                events_digest: [2u8; 32],
                keys_digest: sybil_verifier::empty_account_keys_digest(2),
            },
            AccountSnapshot {
                id: 1,
                balance: 10,
                total_deposited: 10,
                positions: vec![
                    (MarketId::new(7), 0, 0),
                    (MarketId::new(7), 1, 5),
                    (MarketId::new(8), 0, -2),
                ],
                events_digest: [1u8; 32],
                keys_digest: sybil_verifier::empty_account_keys_digest(1),
            },
        ]);

        let totals = state.market_position_totals();

        assert_eq!(totals.totals_for(MarketId::new(7)), (3, 6));
        assert_eq!(totals.totals_for(MarketId::new(8)), (-2, 0));
        assert_eq!(totals.totals_for(MarketId::new(9)), (0, 0));
    }
}
