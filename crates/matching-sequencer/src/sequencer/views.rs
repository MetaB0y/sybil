use super::*;

impl BlockSequencer {
    pub fn height(&self) -> u64 {
        self.height
    }

    pub fn genesis_hash(&self) -> Option<[u8; 32]> {
        self.genesis_hash
    }

    pub fn order_ttl_blocks(&self) -> u64 {
        self.config.order_ttl_blocks
    }

    /// Snapshot of all state needed to persist the most recently produced block.
    ///
    /// Call this on the `next_sequencer` returned by `prepare_block` (or on a
    /// live `BlockSequencer` after `produce_block`) — it panics if no block has
    /// been produced yet, since there's no header to associate the snapshot with.
    pub fn snapshot(&self) -> SequencerSnapshot<'_> {
        let header = self
            .last_header
            .as_ref()
            .expect("snapshot called before any block was produced");
        SequencerSnapshot {
            accounts: &self.accounts,
            markets: &self.markets,
            market_groups: &self.market_groups,
            lifecycle: &self.lifecycle,
            header,
            genesis_hash: self
                .genesis_hash
                .expect("snapshot called before genesis hash was available"),
            next_order_id: self.next_order_id,
            pubkey_registry: &self.pubkey_registry,
            analytics: self.analytics.snapshot(),
            resting_orders: self.order_book.snapshot(),
            bridge_state: &self.bridge,
        }
    }

    /// Open-batch unique placers for a market — non-persistent computation
    /// over the resting book plus pending bundles touching `market_id`.
    /// Excludes MM-constrained bundles and `AccountId::MINT`.
    pub fn open_batch_unique_placers(&self, market_id: MarketId) -> u32 {
        let mut placers: HashSet<AccountId> = HashSet::new();
        for (order, account_id) in self.order_book.resting_orders() {
            if account_id != AccountId::MINT && order.active_markets().any(|m| m == market_id) {
                placers.insert(account_id);
            }
        }
        for bundle in &self.pending_bundles {
            if bundle.mm_constraint.is_some() || bundle.account_id == AccountId::MINT {
                continue;
            }
            let touches = bundle
                .orders
                .iter()
                .any(|o| o.active_markets().any(|m| m == market_id));
            if touches {
                placers.insert(bundle.account_id);
            }
        }
        placers.len() as u32
    }

    pub fn markets(&self) -> &MarketSet {
        &self.markets
    }

    pub fn markets_mut(&mut self) -> &mut MarketSet {
        &mut self.markets
    }

    pub fn market_groups(&self) -> &[MarketGroup] {
        &self.market_groups
    }

    pub fn market_lifecycle(&self) -> &MarketLifecycle {
        &self.lifecycle
    }

    pub fn market_groups_mut(&mut self) -> &mut Vec<MarketGroup> {
        &mut self.market_groups
    }
    pub fn last_header(&self) -> Option<&BlockHeader> {
        self.last_header.as_ref()
    }

    pub fn next_order_id(&self) -> u64 {
        self.next_order_id
    }

    pub fn pubkey_registry(
        &self,
    ) -> &HashMap<crate::crypto::PublicKey, crate::crypto::RegisteredPubkey> {
        &self.pubkey_registry
    }

    /// Get the oracle-tracked status for a market. Returns `Active` if not explicitly set.
    pub fn market_status(&self, id: MarketId) -> MarketStatus {
        self.lifecycle.market_status(id)
    }

    pub fn market_statuses(&self) -> &HashMap<MarketId, MarketStatus> {
        self.lifecycle.market_statuses()
    }

    pub fn market_metadata(&self, market_id: MarketId) -> Option<&MarketMetadata> {
        self.lifecycle.market_metadata(market_id)
    }

    pub fn market_metadata_all(&self) -> &HashMap<MarketId, MarketMetadata> {
        self.lifecycle.market_metadata_all()
    }

    pub fn analytics(&self) -> &AnalyticsState {
        &self.analytics
    }

    /// Width of the band the LiquidityTracker is currently scoring against.
    /// Pulled from the live config, not from the snapshot — that's the band
    /// the next `record_block` will apply.
    pub fn liquidity_band_nanos(&self) -> u64 {
        self.config.liquidity_band_nanos
    }

    pub fn open_orders_for_account(&self, account_id: AccountId) -> usize {
        self.order_book.orders_for_account(account_id)
    }

    /// Balance committed to live resting orders for this account.
    pub fn reserved_balance_nanos(&self, account_id: AccountId) -> i64 {
        self.order_book.reserved_balance(account_id)
    }

    pub fn pending_bundles_for_account(&self, account_id: AccountId) -> usize {
        self.pending_bundles
            .iter()
            .filter(|submission| submission.account_id == account_id)
            .count()
    }

    pub fn pending_non_mm_orders_for_account(&self, account_id: AccountId) -> usize {
        self.pending_bundles
            .iter()
            .filter(|submission| {
                submission.account_id == account_id && submission.mm_constraint.is_none()
            })
            .map(|submission| submission.orders.len())
            .sum()
    }
    pub fn portfolio_summary(
        &self,
        account_id: AccountId,
    ) -> Result<crate::portfolio::PortfolioSummary, SequencerError> {
        let account = self.accounts.get(account_id).ok_or({
            SequencerError::Rejected(Rejection {
                order_id: 0,
                account_id,
                reason: RejectionReason::AccountNotFound,
            })
        })?;
        Ok(crate::portfolio::compute_portfolio(
            account,
            self.analytics.last_mark_prices(),
            self.analytics.first_deposit_ms(account_id).unwrap_or(0),
            self.analytics.total_fills(account_id),
            self.analytics.cost_basis_tracker(),
        ))
    }

    /// All-time leaderboard inputs for every ranked account, computed from
    /// live in-memory state in a single pass. The API layers 7d/30d windowing
    /// on top using opening anchors from the history service; the sequencer
    /// never scans historical storage for this read.
    ///
    /// The system MINT account, never-funded accounts, accounts that have never
    /// filled, and accounts without an opt-in display name are excluded.
    /// Publishing a display name is explicit consent to publish the associated
    /// leaderboard financial row; clearing it removes the account from future
    /// reads. Funding alone is onboarding state, not leaderboard activity.
    /// `markets_traded` counts distinct markets with a currently open position
    /// (exact lifetime distinct-markets-traded would require a per-account fill
    /// history scan, which is not retained cheaply — see SYB-59).
    pub fn leaderboard_bases(&self) -> Vec<LeaderboardBase> {
        self.accounts
            .iter()
            .filter(|(id, _)| **id != AccountId::MINT)
            .filter_map(|(id, account)| {
                if account.total_deposited <= 0 || self.analytics.total_fills(*id) == 0 {
                    return None;
                }
                let display_name = account.profile.display_name.clone()?;
                let summary = self.portfolio_summary(*id).ok()?;
                let markets: HashSet<MarketId> = account
                    .positions
                    .iter()
                    .filter(|&(_, &qty)| qty != 0)
                    .map(|(&(market_id, _), _)| market_id)
                    .collect();
                Some(LeaderboardBase {
                    account_id: *id,
                    display_name,
                    avatar_seed: account.profile.avatar_seed.clone(),
                    pnl_nanos: summary.pnl_nanos,
                    equity_nanos: summary.portfolio_value_nanos,
                    deposited_nanos: summary.total_deposited_nanos,
                    markets_traded: markets.len() as u32,
                })
            })
            .collect()
    }

    pub fn bridge_state(&self) -> &BridgeState {
        &self.bridge
    }

    pub fn order_book(&self) -> &OrderBook {
        &self.order_book
    }

    pub fn bridge_account_key(&self, account_id: AccountId) -> Option<[u8; 32]> {
        self.accounts
            .get(account_id)
            .map(|_| account_key(account_id))
    }

    pub fn bridge_account_id_by_key(&self, key: [u8; 32]) -> Option<AccountId> {
        self.accounts.iter().find_map(|(&account_id, _)| {
            if account_key(account_id) == key {
                Some(account_id)
            } else {
                None
            }
        })
    }

    pub fn default_bridge_withdrawal_expiry_height(&self) -> u64 {
        self.bridge
            .observed_l1_height
            .saturating_add(DEFAULT_WITHDRAWAL_EXPIRY_BLOCKS)
    }

    pub fn bridge_withdrawal(&self, withdrawal_id: u64) -> Option<&WithdrawalLeaf> {
        self.bridge.withdrawals.get(&withdrawal_id)
    }
    pub(crate) fn solver(&self) -> Arc<dyn Solver> {
        Arc::clone(&self.solver)
    }

    /// Whether a template with this id has been installed. Used by the API
    /// layer to reject market-creation requests that reference a missing
    /// template, instead of deferring the error until resolve time.
    pub fn template_exists(&self, id: &str) -> bool {
        self.lifecycle.templates().get_str(id).is_some()
    }
}
