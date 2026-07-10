use super::*;

impl BlockSequencer {
    pub fn create_market(&mut self, name: String) -> MarketId {
        self.markets.add_binary(name)
    }

    pub fn create_market_with_metadata(
        &mut self,
        name: String,
        metadata: MarketMetadata,
    ) -> MarketId {
        let market_id = self.create_market(name);
        self.set_market_metadata(market_id, metadata);
        market_id
    }

    pub fn create_market_group(
        &mut self,
        name: String,
        market_ids: Vec<MarketId>,
    ) -> (u64, MarketGroup) {
        let group_id = self.market_groups.len() as u64;
        let mut group = MarketGroup::new(&name);
        for market_id in market_ids {
            group.add_market(market_id);
        }
        self.market_groups.push(group.clone());
        (group_id, group)
    }

    /// Add a market to an existing mutually-exclusive group.
    ///
    /// Group membership is a forward-only solver constraint. Existing positions
    /// and MINT inventory are per-market, not per-group-versioned, so extending
    /// a group never rewrites previously settled positions or tries to
    /// reinterpret old complete sets. The next block commits the updated
    /// `market_group/{group_id}` state leaf; future batches solve one joint
    /// simplex over the extended unresolved membership. Duplicate extension of
    /// the same group/member pair is idempotent.
    pub fn extend_market_group(
        &mut self,
        group_id: u64,
        market_id: MarketId,
    ) -> Result<(MarketGroup, bool), SequencerError> {
        let (group_index, should_insert) =
            self.validate_market_group_extension(group_id, market_id)?;
        if !should_insert {
            return Ok((self.market_groups[group_index].clone(), false));
        }

        let group = self
            .market_groups
            .get_mut(group_index)
            .expect("group exists: validated above");
        group.add_market(market_id);
        let updated = group.clone();
        self.record_system_event(SystemEvent::MarketGroupExtended {
            group_id,
            market_id,
        });
        Ok((updated, true))
    }

    pub fn can_extend_market_group(
        &self,
        group_id: u64,
        market_id: MarketId,
    ) -> Result<(), SequencerError> {
        self.validate_market_group_extension(group_id, market_id)
            .map(|_| ())
    }

    fn validate_market_group_extension(
        &self,
        group_id: u64,
        market_id: MarketId,
    ) -> Result<(usize, bool), SequencerError> {
        if self.markets.get(market_id).is_none() {
            return Err(SequencerError::MarketNotFound);
        }
        let status = self.lifecycle.market_status(market_id);
        if !status.is_tradeable() {
            return Err(SequencerError::InvalidMarketState(format!(
                "market {market_id:?} is not tradeable ({})",
                status.as_str()
            )));
        }

        let group_index =
            usize::try_from(group_id).map_err(|_| SequencerError::MarketGroupNotFound)?;
        let Some(group) = self.market_groups.get(group_index) else {
            return Err(SequencerError::MarketGroupNotFound);
        };
        if group.markets.contains(&market_id) {
            return Ok((group_index, false));
        }

        for (existing_group_id, group) in self.market_groups.iter().enumerate() {
            if existing_group_id != group_index && group.markets.contains(&market_id) {
                return Err(SequencerError::MarketAlreadyGrouped {
                    group_id: existing_group_id as u64,
                });
            }
        }

        Ok((group_index, true))
    }

    pub fn set_market_metadata(&mut self, market_id: MarketId, metadata: MarketMetadata) {
        self.lifecycle.set_market_metadata(market_id, metadata);
    }
    pub fn resolve_market(
        &mut self,
        market_id: MarketId,
        payout_nanos: Nanos,
        timestamp_ms: u64,
    ) -> Result<ResolutionRecord, SequencerError> {
        if self.markets.get(market_id).is_none() {
            return Err(SequencerError::MarketNotFound);
        }

        // Lifecycle decides (consults oracle, updates status)
        let action = self
            .lifecycle
            .resolve_market(market_id, payout_nanos, timestamp_ms)?;

        // Sequencer executes the side effects
        self.execute_resolution_action(action)
    }

    /// Resolve a market from a signed attestation via the market's template
    /// policy. Signature verification is done by the caller (the sequencer
    /// actor) before this is called; here the lifecycle re-checks that the
    /// signer is the template's expected feed and then settles.
    pub fn resolve_market_attested(
        &mut self,
        market_id: MarketId,
        signed: &sybil_oracle::SignedAttestation,
        timestamp_ms: u64,
    ) -> Result<ResolutionRecord, SequencerError> {
        if self.markets.get(market_id).is_none() {
            return Err(SequencerError::MarketNotFound);
        }

        let action = self
            .lifecycle
            .resolve_from_attestation(market_id, signed, timestamp_ms)?;

        self.execute_resolution_action(action)
    }

    fn execute_resolution_action(
        &mut self,
        action: sybil_oracle::ResolutionAction,
    ) -> Result<ResolutionRecord, SequencerError> {
        match action {
            sybil_oracle::ResolutionAction::SettleNow {
                market_id,
                payout_nanos,
                record,
            } => {
                let mut pre_settle_positions: Vec<(AccountId, u8, i64)> = Vec::new();
                let affected_accounts: Vec<AccountId> = self
                    .accounts
                    .iter()
                    .filter_map(|(&account_id, account)| {
                        let yes_pos = account.position(market_id, 0);
                        let no_pos = account.position(market_id, 1);
                        if yes_pos != 0 {
                            pre_settle_positions.push((account_id, 0, yes_pos));
                        }
                        if no_pos != 0 {
                            pre_settle_positions.push((account_id, 1, no_pos));
                        }
                        (yes_pos != 0 || no_pos != 0).then_some(account_id)
                    })
                    .collect();
                for account_id in &affected_accounts {
                    self.capture_system_account_baseline(*account_id);
                }
                let affected_accounts =
                    settlement::resolve_market(&mut self.accounts, market_id, payout_nanos);
                self.analytics.apply_resolution(
                    market_id,
                    payout_nanos.0 as i64,
                    pre_settle_positions,
                );
                self.record_system_event(SystemEvent::MarketResolved {
                    market_id,
                    payout_nanos,
                    affected_accounts,
                });
                self.shrink_market_groups_after_resolution(market_id);
                Ok(record)
            }
            sybil_oracle::ResolutionAction::Propose { .. } => Err(SequencerError::OracleError(
                "resolution proposed but not yet settled".to_string(),
            )),
            sybil_oracle::ResolutionAction::Reject { reason } => {
                Err(SequencerError::OracleError(reason))
            }
        }
    }

    /// Register a data feed (e.g. admin key, Polymarket mirror signer). Returns
    /// the assigned [`sybil_oracle::FeedId`]. Idempotent on pubkey.
    pub fn register_feed(
        &mut self,
        pubkey: sybil_oracle::FeedPubkey,
        name: String,
        now_ms: u64,
    ) -> sybil_oracle::FeedId {
        self.lifecycle.register_feed(pubkey, name, now_ms)
    }

    pub fn feed_by_id(&self, id: sybil_oracle::FeedId) -> Option<&sybil_oracle::DataFeed> {
        self.lifecycle.feed_by_id(id)
    }

    pub fn feed_by_pubkey(
        &self,
        pubkey: &sybil_oracle::FeedPubkey,
    ) -> Option<&sybil_oracle::DataFeed> {
        self.lifecycle.feed_by_pubkey(pubkey)
    }

    pub fn install_template(&mut self, template: sybil_oracle::ResolutionTemplate) {
        self.lifecycle.install_template(template);
    }

    /// Remove a resolved member from any mutually-exclusive group. A group with
    /// two or more unresolved members still constrains survivor prices and group
    /// minting; a singleton has no remaining mutual-exclusion surface.
    fn shrink_market_groups_after_resolution(&mut self, market_id: MarketId) {
        let mut groups = Vec::with_capacity(self.market_groups.len());
        for mut group in std::mem::take(&mut self.market_groups) {
            if group.markets.contains(&market_id) {
                group.markets.retain(|&member| member != market_id);
                if group.markets.len() >= 2 {
                    groups.push(group);
                }
            } else {
                groups.push(group);
            }
        }
        self.market_groups = groups;
    }
}
