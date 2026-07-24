use super::*;

impl BlockSequencer {
    pub const MAX_MARKET_CREATION_KEY_BYTES: usize =
        matching_engine::MAX_OPERATOR_CREATION_KEY_BYTES;

    pub fn create_market(&mut self, name: String) -> MarketId {
        self.markets.add_binary(name)
    }

    pub fn create_market_with_metadata(
        &mut self,
        name: String,
        metadata: MarketMetadata,
    ) -> Result<MarketId, SequencerError> {
        if let Some(market_id) = self.existing_market_for_creation(&name, &metadata)? {
            return Ok(market_id);
        }
        let market_id = self.create_market(name);
        self.set_market_metadata(market_id, metadata);
        Ok(market_id)
    }

    pub(crate) fn existing_market_for_creation(
        &self,
        name: &str,
        metadata: &MarketMetadata,
    ) -> Result<Option<MarketId>, SequencerError> {
        let Some(key) = metadata.creation_key.as_deref() else {
            return Ok(None);
        };
        validate_market_creation_key(key)?;

        let Some((&market_id, existing_metadata)) = self
            .lifecycle
            .market_metadata_all()
            .iter()
            .find(|(_, existing)| existing.creation_key.as_deref() == Some(key))
        else {
            return Ok(None);
        };
        let existing_market = self.markets.get(market_id).ok_or_else(|| {
            SequencerError::Persistence(format!(
                "market creation key {key:?} references missing market {}",
                market_id.0
            ))
        })?;
        if existing_market.name == name && existing_metadata.same_creation_fields(metadata) {
            return Ok(Some(market_id));
        }
        Err(SequencerError::MarketCreationKeyConflict {
            key: key.to_string(),
            existing_market_id: market_id,
        })
    }

    pub fn create_market_group(
        &mut self,
        name: String,
        market_ids: Vec<MarketId>,
    ) -> (u64, MarketGroup) {
        self.create_market_group_with_key(name, None, market_ids)
            .expect("unkeyed market group creation requires valid ungrouped active markets")
    }

    pub fn create_market_group_with_key(
        &mut self,
        name: String,
        creation_key: Option<String>,
        mut market_ids: Vec<MarketId>,
    ) -> Result<(u64, MarketGroup), SequencerError> {
        market_ids.sort_by_key(|market_id| market_id.0);
        market_ids.dedup();
        if let Some(existing) =
            self.can_create_market_group(&name, creation_key.as_deref(), &market_ids)?
        {
            return Ok(existing);
        }

        let group_id = self.market_groups.len() as u64;
        let mut group = MarketGroup::new(&name);
        group.creation_key = creation_key;
        for market_id in market_ids {
            group.add_market(market_id);
        }
        self.market_groups.push(group.clone());
        Ok((group_id, group))
    }

    pub(crate) fn can_create_market_group(
        &self,
        name: &str,
        creation_key: Option<&str>,
        market_ids: &[MarketId],
    ) -> Result<Option<(u64, MarketGroup)>, SequencerError> {
        if let Some(existing) =
            self.existing_market_group_for_creation(name, creation_key, market_ids)?
        {
            return Ok(Some(existing));
        }

        for &market_id in market_ids {
            if self.markets.get(market_id).is_none() {
                return Err(SequencerError::MarketNotFound { market_id });
            }
            let status = self.lifecycle.market_status(market_id);
            if !status.is_tradeable() {
                return Err(SequencerError::MarketNotTradeable {
                    market_id,
                    status: status.as_str().to_string(),
                });
            }
            if let Some((group_id, _)) = self
                .market_groups
                .iter()
                .enumerate()
                .find(|(_, group)| group.markets.contains(&market_id))
            {
                return Err(SequencerError::MarketAlreadyGrouped {
                    group_id: group_id as u64,
                });
            }
        }

        Ok(None)
    }

    pub(crate) fn existing_market_group_for_creation(
        &self,
        name: &str,
        creation_key: Option<&str>,
        market_ids: &[MarketId],
    ) -> Result<Option<(u64, MarketGroup)>, SequencerError> {
        let Some(key) = creation_key else {
            return Ok(None);
        };
        validate_market_group_creation_key(key)?;

        let Some((existing_group_id, existing)) = self
            .market_groups
            .iter()
            .enumerate()
            .find(|(_, group)| group.creation_key.as_deref() == Some(key))
        else {
            return Ok(None);
        };

        let mut existing_markets = existing.markets.clone();
        existing_markets.sort_by_key(|market_id| market_id.0);
        existing_markets.dedup();
        let mut requested_markets = market_ids.to_vec();
        requested_markets.sort_by_key(|market_id| market_id.0);
        requested_markets.dedup();
        if existing.name == name && existing_markets == requested_markets {
            return Ok(Some((existing_group_id as u64, existing.clone())));
        }
        Err(SequencerError::MarketGroupCreationKeyConflict {
            key: key.to_string(),
            existing_group_id: existing_group_id as u64,
        })
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
            return Err(SequencerError::MarketNotFound { market_id });
        }
        let status = self.lifecycle.market_status(market_id);
        if !status.is_tradeable() {
            return Err(SequencerError::MarketNotTradeable {
                market_id,
                status: status.as_str().to_string(),
            });
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
            return Err(SequencerError::MarketNotFound { market_id });
        }

        let record = self
            .lifecycle
            .resolve_market(market_id, payout_nanos, timestamp_ms)?;
        self.execute_resolution(market_id, record)
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
            return Err(SequencerError::MarketNotFound { market_id });
        }

        let record = self
            .lifecycle
            .resolve_from_attestation(market_id, signed, timestamp_ms)?;
        self.execute_resolution(market_id, record)
    }

    fn execute_resolution(
        &mut self,
        market_id: MarketId,
        record: ResolutionRecord,
    ) -> Result<ResolutionRecord, SequencerError> {
        let payout_nanos = record.payout_nanos;
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
        self.analytics
            .apply_resolution(market_id, payout_nanos.0 as i64, pre_settle_positions);
        self.record_system_event(SystemEvent::MarketResolved {
            market_id,
            payout_nanos,
            affected_accounts,
        });
        self.shrink_market_groups_after_resolution(market_id);
        Ok(record)
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

fn validate_market_creation_key(key: &str) -> Result<(), SequencerError> {
    validate_creation_key_shape(key).map_err(SequencerError::InvalidMarketCreationKey)
}

fn validate_market_group_creation_key(key: &str) -> Result<(), SequencerError> {
    validate_creation_key_shape(key).map_err(SequencerError::InvalidMarketGroupCreationKey)
}

fn validate_creation_key_shape(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("key must not be empty".to_string());
    }
    if key.len() > BlockSequencer::MAX_MARKET_CREATION_KEY_BYTES {
        return Err(format!(
            "key is {} bytes; maximum is {}",
            key.len(),
            BlockSequencer::MAX_MARKET_CREATION_KEY_BYTES
        ));
    }
    if !matching_engine::operator_creation_key_is_valid(key) {
        return Err("key must use ASCII letters, digits, '-', '_', ':', '.', or '/'".to_string());
    }
    Ok(())
}
