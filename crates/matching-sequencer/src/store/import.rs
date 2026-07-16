use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WitnessImportSummary {
    pub height: u64,
    pub state_root: [u8; 32],
    pub genesis_hash: [u8; 32],
    pub accounts: usize,
    pub markets: usize,
    pub market_groups: usize,
    pub resting_orders: usize,
    pub account_reservations: usize,
    pub withdrawals: usize,
    pub deposit_cursor: u64,
    pub next_account_id: u64,
    pub next_market_id: u32,
    pub next_order_id: u64,
    pub next_withdrawal_id: u64,
}

impl Store {
    /// Import a canonical witness as the committed head of an otherwise fresh store.
    ///
    /// This is the disaster-recovery "genesis-from-witness" path: it refuses
    /// any non-empty store, rebuilds core sequencer state from the witness
    /// post-state and sidecar, verifies that the rebuilt state commits to the
    /// witness header root, and then persists through the same redb/qMDB fence
    /// used by normal block commits.
    pub async fn import_witness_genesis(
        &self,
        witness: BlockWitness,
        expect_state_root: Option<[u8; 32]>,
        genesis_hash: Option<[u8; 32]>,
        config: SequencerConfig,
    ) -> Result<WitnessImportSummary, StoreError> {
        self.ensure_import_target_empty()?;
        validate_import_witness_root(&witness, expect_state_root)?;
        let genesis_hash = import_genesis_hash(&witness, genesis_hash)?;

        let restored = restored_state_from_witness(&witness, &config, genesis_hash)?;
        let sequencer = BlockSequencer::try_restore(restored, config)
            .map_err(|error| StoreError::WitnessImport(error.to_string()))?;
        let snapshot = sequencer.snapshot();
        let rebuilt_sidecar = state_sidecar_snapshot_from_resting_orders(
            snapshot.bridge_state,
            &snapshot.resting_orders,
            snapshot.markets,
            snapshot.market_groups,
            snapshot.lifecycle,
            snapshot.analytics.last_clearing_prices,
        );
        let rebuilt_accounts =
            crate::canonical_state::CanonicalState::from_accounts(snapshot.accounts);
        let rebuilt_root = sybil_verifier::block::compute_state_root_with_sidecar(
            rebuilt_accounts.as_snapshots(),
            &rebuilt_sidecar,
        );
        if rebuilt_root != witness.header.state_root {
            return Err(StoreError::WitnessImport(format!(
                "rebuilt sequencer state root {} does not match witness header {}",
                hex32(rebuilt_root),
                hex32(witness.header.state_root)
            )));
        }

        let summary = WitnessImportSummary {
            height: witness.header.height,
            state_root: witness.header.state_root,
            genesis_hash,
            accounts: snapshot.accounts.iter().count(),
            markets: snapshot.markets.len(),
            market_groups: snapshot.market_groups.len(),
            resting_orders: snapshot.resting_orders.len(),
            account_reservations: witness.state_sidecar.account_reservations.len(),
            withdrawals: snapshot.bridge_state.withdrawals.len(),
            deposit_cursor: snapshot.bridge_state.deposit_cursor,
            next_account_id: snapshot.accounts.next_id(),
            next_market_id: snapshot.markets.next_id(),
            next_order_id: snapshot.next_order_id,
            next_withdrawal_id: snapshot.bridge_state.next_withdrawal_id,
        };

        // The imported head is a trusted recovery checkpoint, not a locally
        // replayable transition: the fresh store has no qMDB slot for its
        // `previous_header`. Its first child resumes mandatory proof-job
        // capture using this checkpoint as the pre-state slot.
        self.save_imported_checkpoint(snapshot, &witness).await?;
        Ok(summary)
    }

    fn ensure_import_target_empty(&self) -> Result<(), StoreError> {
        let txn = self.db.begin_read()?;
        if read_recovery_metadata(&txn)?.is_some() {
            return Err(StoreError::WitnessImport(
                "store already has committed recovery metadata".to_string(),
            ));
        }

        let data_rows = [
            ("markets", txn.open_table(MARKETS)?.len()?),
            ("market_meta", txn.open_table(MARKET_META)?.len()?),
            ("market_statuses", txn.open_table(MARKET_STATUSES)?.len()?),
            ("market_groups", txn.open_table(MARKET_GROUPS)?.len()?),
            ("block_headers", txn.open_table(BLOCK_HEADERS)?.len()?),
            (
                "canonical_block_archive",
                txn.open_table(CANONICAL_BLOCK_ARCHIVE)?.len()?,
            ),
            ("block_witnesses", txn.open_table(BLOCK_WITNESSES)?.len()?),
            ("proof_job_outbox", txn.open_table(PROOF_JOB_OUTBOX)?.len()?),
            ("proof_job_acks", txn.open_table(PROOF_JOB_ACKS)?.len()?),
            ("da_artifacts", txn.open_table(DA_ARTIFACTS)?.len()?),
            ("da_manifests", txn.open_table(DA_MANIFESTS)?.len()?),
            ("pubkey_registry", txn.open_table(PUBKEY_REGISTRY)?.len()?),
            ("clearing_prices", txn.open_table(CLEARING_PRICES)?.len()?),
            ("market_volumes", txn.open_table(MARKET_VOLUMES)?.len()?),
            ("resting_orders", txn.open_table(RESTING_ORDERS)?.len()?),
            (
                "acknowledged_writes",
                txn.open_table(ACKNOWLEDGED_WRITES)?.len()?,
            ),
            ("chain_meta", txn.open_table(CHAIN_META)?.len()?),
            ("bridge_state", txn.open_table(BRIDGE_STATE)?.len()?),
        ];
        for (table, rows) in data_rows {
            if rows != 0 {
                return Err(StoreError::WitnessImport(format!(
                    "store is not empty: table `{table}` has {rows} row(s)"
                )));
            }
        }

        Ok(())
    }
}

fn import_err(message: impl Into<String>) -> StoreError {
    StoreError::WitnessImport(message.into())
}

fn hex32(bytes: [u8; 32]) -> String {
    format!("0x{}", hex::encode(bytes))
}

fn witness_header_hash(header: &WitnessBlockHeader) -> [u8; 32] {
    crate::block::hash_header(&block_header_from_witness(header))
}

fn derivable_genesis_hash_from_witness(witness: &BlockWitness) -> Option<[u8; 32]> {
    if witness.header.height == 1 {
        return Some(witness_header_hash(&witness.header));
    }
    witness
        .previous_header
        .as_ref()
        .filter(|header| header.height == 1)
        .map(witness_header_hash)
}

fn import_genesis_hash(
    witness: &BlockWitness,
    provided: Option<[u8; 32]>,
) -> Result<[u8; 32], StoreError> {
    let derived = derivable_genesis_hash_from_witness(witness);
    if let (Some(provided), Some(derived)) = (provided, derived)
        && provided != derived
    {
        return Err(import_err(format!(
            "--genesis-hash {} does not match witness-derived genesis hash {}",
            hex32(provided),
            hex32(derived)
        )));
    }
    if let Some(provided) = provided {
        return Ok(provided);
    }
    derived.ok_or_else(|| {
        import_err(
            "--genesis-hash is required when the imported witness does not include the genesis header",
        )
    })
}

fn validate_import_witness_root(
    witness: &BlockWitness,
    expect_state_root: Option<[u8; 32]>,
) -> Result<(), StoreError> {
    let computed = sybil_verifier::block::compute_state_root_with_sidecar(
        &witness.post_state,
        &witness.state_sidecar,
    );
    if computed != witness.header.state_root {
        return Err(import_err(format!(
            "witness post_state + sidecar root {} does not match header {}",
            hex32(computed),
            hex32(witness.header.state_root)
        )));
    }
    if let Some(expected) = expect_state_root
        && expected != witness.header.state_root
    {
        return Err(import_err(format!(
            "--expect-state-root {} does not match witness header {}",
            hex32(expected),
            hex32(witness.header.state_root)
        )));
    }
    Ok(())
}

fn restored_state_from_witness(
    witness: &BlockWitness,
    config: &SequencerConfig,
    genesis_hash: [u8; 32],
) -> Result<RestoredState, StoreError> {
    let accounts = account_store_from_witness(&witness.post_state)?;
    let pubkey_registry = pubkey_registry_from_witness(witness, &accounts)?;
    let (markets, market_statuses, market_metadata, market_groups) =
        market_state_from_sidecar(&witness.state_sidecar)?;
    let last_clearing_prices = witness
        .state_sidecar
        .markets
        .iter()
        .filter(|market| !market.last_clearing_prices.is_empty())
        .map(|market| (market.market_id, market.last_clearing_prices.clone()))
        .collect::<HashMap<_, _>>();
    let bridge_state = bridge_state_from_witness(witness)?;
    let resting_orders = resting_orders_from_sidecar(&witness.state_sidecar);
    validate_restored_reservations(&resting_orders)
        .map_err(|error| import_err(error.to_string()))?;
    validate_restored_account_reservations(
        &resting_orders,
        &witness.state_sidecar.account_reservations,
    )
    .map_err(|error| import_err(error.to_string()))?;
    let order_book = OrderBook::restore(resting_orders.clone(), config.order_ttl_blocks);
    let restored_resting_orders = order_book.snapshot();
    if restored_resting_orders.len() != resting_orders.len() {
        return Err(import_err(
            "resting order restore dropped one or more sidecar orders",
        ));
    }

    let rebuilt_reservations = reservation_snapshots_from_resting_orders(&restored_resting_orders);
    if rebuilt_reservations != witness.state_sidecar.account_reservations {
        return Err(import_err(
            "resting-order reservation aggregates do not match sidecar account_reservations",
        ));
    }

    let mut lifecycle = MarketLifecycle::new();
    for (&market_id, status) in &market_statuses {
        lifecycle.set_market_status(market_id, status.clone());
    }
    for (&market_id, metadata) in &market_metadata {
        lifecycle.set_market_metadata(market_id, metadata.clone());
    }
    let rebuilt_sidecar = state_sidecar_snapshot_from_resting_orders(
        &bridge_state,
        &restored_resting_orders,
        &markets,
        &market_groups,
        &lifecycle,
        &last_clearing_prices,
    );
    let rebuilt_root = sybil_verifier::block::compute_state_root_with_sidecar(
        crate::canonical_state::CanonicalState::from_accounts(&accounts).as_snapshots(),
        &rebuilt_sidecar,
    );
    if rebuilt_root != witness.header.state_root {
        return Err(import_err(format!(
            "rebuilt import mapping root {} does not match witness header {}",
            hex32(rebuilt_root),
            hex32(witness.header.state_root)
        )));
    }

    Ok(RestoredState {
        accounts,
        markets,
        market_groups,
        market_statuses,
        market_metadata,
        height: witness.header.height,
        last_header: Some(block_header_from_witness(&witness.header)),
        genesis_hash,
        next_order_id: next_order_id_from_witness(witness)?,
        pubkey_registry,
        resting_orders: restored_resting_orders,
        data_feeds: Vec::new(),
        resolution_templates: Vec::new(),
        acknowledged_writes: Vec::new(),
        analytics: empty_import_analytics(last_clearing_prices),
        bridge_state,
    })
}

fn pubkey_registry_from_witness(
    witness: &BlockWitness,
    accounts: &AccountStore,
) -> Result<HashMap<crate::crypto::PublicKey, crate::crypto::RegisteredPubkey>, StoreError> {
    let mut sets: HashMap<u64, Vec<sybil_verifier::KeyRecord>> = HashMap::new();
    for (account_id, keys) in &witness.account_keys {
        if sets.insert(*account_id, keys.clone()).is_some() {
            return Err(import_err(format!(
                "duplicate account {account_id} in witness.account_keys"
            )));
        }
    }

    let mut registry = HashMap::new();
    for (account_id, account) in accounts.iter() {
        let keys = sets.remove(&account_id.0).unwrap_or_default();
        if keys.len() > sybil_verifier::MAX_KEYS_PER_ACCOUNT {
            return Err(import_err(format!(
                "account {} exceeds MAX_KEYS_PER_ACCOUNT",
                account_id.0
            )));
        }
        let digest = sybil_verifier::account_keys_digest(account_id.0, keys.iter().copied());
        if digest != account.keys_digest {
            return Err(import_err(format!(
                "account {} key set does not match committed keys_digest",
                account_id.0
            )));
        }
        for key in keys {
            if key.capability_mask != sybil_verifier::KeyRecord::FULL_CAPABILITY_MASK {
                return Err(import_err("unsupported non-full key capability mask"));
            }
            let auth_scheme = match key.auth_scheme {
                0 => crate::crypto::AccountAuthScheme::RawP256,
                1 => crate::crypto::AccountAuthScheme::WebAuthn,
                other => {
                    return Err(import_err(format!(
                        "unsupported account key auth scheme {other}"
                    )));
                }
            };
            let pubkey = crate::crypto::PublicKey::from_compressed_bytes(&key.pubkey_sec1)
                .ok_or_else(|| import_err("invalid compressed P256 key in witness.account_keys"))?;
            if registry
                .insert(
                    pubkey,
                    crate::crypto::RegisteredPubkey::primary(*account_id, auth_scheme),
                )
                .is_some()
            {
                return Err(import_err(
                    "duplicate pubkey across witness.account_keys accounts",
                ));
            }
        }
    }
    if let Some(account_id) = sets.keys().next() {
        return Err(import_err(format!(
            "witness.account_keys references unknown account {account_id}"
        )));
    }
    Ok(registry)
}

fn account_store_from_witness(accounts: &[AccountSnapshot]) -> Result<AccountStore, StoreError> {
    let mut map = HashMap::new();
    let mut next_account_id = 0u64;
    for snapshot in accounts {
        let account_id = AccountId(snapshot.id);
        if account_id != AccountId::MINT {
            next_account_id = next_account_id.max(
                snapshot
                    .id
                    .checked_add(1)
                    .ok_or_else(|| import_err("account id overflow deriving next_account_id"))?,
            );
        }

        let mut positions = HashMap::new();
        for &(market, outcome, qty) in &snapshot.positions {
            if positions.insert((market, outcome), qty).is_some() {
                return Err(import_err(format!(
                    "duplicate position in account {} for market {} outcome {}",
                    snapshot.id, market.0, outcome
                )));
            }
        }
        let account = Account {
            id: account_id,
            balance: snapshot.balance,
            positions,
            total_deposited: snapshot.total_deposited,
            // Trading nonces are proven state. Seed the broader operational
            // nonce to the same floor so an imported checkpoint cannot replay
            // an already-authorized order or cancellation.
            last_nonce: snapshot.last_trading_nonce,
            last_trading_nonce: snapshot.last_trading_nonce,
            events_digest: snapshot.events_digest,
            keys_digest: snapshot.keys_digest,
            profile: Default::default(),
            api_keys: Vec::new(),
            next_api_key_id: 0,
        };
        if map.insert(account_id, account).is_some() {
            return Err(import_err(format!("duplicate account id {}", snapshot.id)));
        }
    }
    Ok(AccountStore::restore(map, next_account_id))
}

type ImportedMarketState = (
    MarketSet,
    HashMap<MarketId, MarketStatus>,
    HashMap<MarketId, MarketMetadata>,
    Vec<MarketGroup>,
);

fn market_state_from_sidecar(
    sidecar: &StateSidecarSnapshot,
) -> Result<ImportedMarketState, StoreError> {
    let mut market_map = HashMap::new();
    let mut statuses = HashMap::new();
    let mut metadata = HashMap::new();
    let mut next_market_id = 0u32;
    for market in &sidecar.markets {
        if market.num_outcomes != 2 {
            return Err(import_err(format!(
                "market {} has {} outcomes; importer only supports binary markets",
                market.market_id.0, market.num_outcomes
            )));
        }
        next_market_id = next_market_id.max(
            market
                .market_id
                .0
                .checked_add(1)
                .ok_or_else(|| import_err("market id overflow deriving next_market_id"))?,
        );
        if market_map
            .insert(
                market.market_id,
                Market::new(market.market_id, market.name.clone()),
            )
            .is_some()
        {
            return Err(import_err(format!(
                "duplicate market id {}",
                market.market_id.0
            )));
        }
        statuses.insert(
            market.market_id,
            market_status_from_snapshot(&market.status),
        );
        metadata.insert(
            market.market_id,
            MarketMetadata {
                resolution_config: Some(ResolutionConfig {
                    template: market.resolution_template.clone(),
                }),
                committed_metadata_digest: Some(market.metadata_digest),
                ..MarketMetadata::default()
            },
        );
    }

    let mut groups_with_ids = sidecar
        .market_groups
        .iter()
        .map(|group| {
            (
                group.group_id,
                MarketGroup {
                    name: group.name.clone(),
                    markets: group.markets.clone(),
                },
            )
        })
        .collect::<Vec<_>>();
    groups_with_ids.sort_by_key(|(group_id, _)| *group_id);
    for (index, (group_id, _)) in groups_with_ids.iter().enumerate() {
        if *group_id != index as u64 {
            return Err(import_err(format!(
                "market group ids must be contiguous from 0; found group_id {} at index {}",
                group_id, index
            )));
        }
    }
    let groups = groups_with_ids
        .into_iter()
        .map(|(_, group)| group)
        .collect();

    Ok((
        MarketSet::restore(market_map, next_market_id),
        statuses,
        metadata,
        groups,
    ))
}

fn market_status_from_snapshot(status: &MarketStatusSnapshot) -> MarketStatus {
    match status {
        MarketStatusSnapshot::Active => MarketStatus::Active,
        MarketStatusSnapshot::Resolved { record } => MarketStatus::Resolved {
            record: resolution_record_from_snapshot(record),
        },
    }
}

fn resolution_record_from_snapshot(
    record: &sybil_verifier::ResolutionRecordSnapshot,
) -> ResolutionRecord {
    ResolutionRecord {
        payout_nanos: record.payout_nanos,
        resolved_by: oracle_source_from_snapshot(&record.resolved_by),
        resolved_at_ms: record.resolved_at_ms,
    }
}

fn oracle_source_from_snapshot(source: &OracleSourceSnapshot) -> OracleSource {
    match source {
        OracleSourceSnapshot::Admin => OracleSource::Admin,
        OracleSourceSnapshot::DataFeed(feed_id) => OracleSource::DataFeed(FeedId(*feed_id)),
    }
}

fn bridge_state_from_witness(witness: &BlockWitness) -> Result<BridgeState, StoreError> {
    let deposit_frontier = folded_deposit_frontier(
        &witness.deposit_accumulator,
        &witness.pre_state_sidecar,
        &witness.state_sidecar,
    )?;
    let mut withdrawals = BTreeMap::new();
    for withdrawal in &witness.state_sidecar.bridge.withdrawals {
        let leaf = WithdrawalLeaf {
            withdrawal_id: withdrawal.withdrawal_id,
            account_id: AccountId(withdrawal.account_id),
            recipient: withdrawal.recipient,
            token_address: withdrawal.token,
            amount_token_units: withdrawal.amount_token_units,
            amount_nanos: withdrawal.amount_nanos,
            expiry_height: withdrawal.expiry_height,
            nullifier: withdrawal.nullifier,
            created_at_height: witness.header.height,
            l1_status: L1WithdrawalStatus::NotRequested,
            l1_requested_at_unix: None,
            l1_executable_at_unix: None,
            l1_finalized_at_unix: None,
            l1_cancelled_at_unix: None,
            l1_tx_hash: None,
        };
        if withdrawals.insert(leaf.withdrawal_id, leaf).is_some() {
            return Err(import_err(format!(
                "duplicate withdrawal id {}",
                withdrawal.withdrawal_id
            )));
        }
    }

    Ok(BridgeState {
        deposit_cursor: witness.state_sidecar.bridge.deposit_cursor,
        deposit_root: witness.state_sidecar.bridge.deposit_root,
        deposit_frontier,
        observed_l1_height: witness.state_sidecar.bridge.observed_l1_height,
        next_withdrawal_id: witness.state_sidecar.bridge.next_withdrawal_id,
        withdrawals,
        quarantine: witness
            .state_sidecar
            .bridge
            .quarantine
            .iter()
            .map(|entry| (entry.sybil_account_key, entry.amount))
            .collect(),
    })
}

fn folded_deposit_frontier(
    accumulator: &DepositAccumulatorWitness,
    pre_sidecar: &StateSidecarSnapshot,
    post_sidecar: &StateSidecarSnapshot,
) -> Result<sybil_l1_protocol::DepositFrontier, StoreError> {
    if accumulator.pre_count != pre_sidecar.bridge.deposit_cursor {
        return Err(import_err(format!(
            "deposit pre_count {} does not match pre-sidecar cursor {}",
            accumulator.pre_count, pre_sidecar.bridge.deposit_cursor
        )));
    }
    let pre_root = sybil_l1_protocol::deposit_root_from_frontier(
        &accumulator.pre_frontier,
        accumulator.pre_count,
    )
    .ok_or_else(|| import_err("deposit pre-frontier count exceeds tree capacity"))?;
    if pre_root != pre_sidecar.bridge.deposit_root {
        return Err(import_err(format!(
            "deposit pre-frontier root {} does not match pre-sidecar root {}",
            hex32(pre_root),
            hex32(pre_sidecar.bridge.deposit_root)
        )));
    }

    let expected_post_count = accumulator
        .pre_count
        .checked_add(accumulator.new_deposits.len() as u64)
        .ok_or_else(|| import_err("deposit cursor overflow"))?;
    if expected_post_count != post_sidecar.bridge.deposit_cursor {
        return Err(import_err(format!(
            "deposit cursor after fold {} does not match post-sidecar cursor {}",
            expected_post_count, post_sidecar.bridge.deposit_cursor
        )));
    }

    let leaves = accumulator
        .new_deposits
        .iter()
        .enumerate()
        .map(|(index, deposit)| {
            let expected_id = accumulator.pre_count + index as u64 + 1;
            if deposit.deposit_id != expected_id {
                return Err(import_err(format!(
                    "deposit id mismatch at delta index {index}: expected {expected_id}, got {}",
                    deposit.deposit_id
                )));
            }
            Ok(deposit_leaf_from_witness(deposit))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let prefix_roots = sybil_l1_protocol::deposit_frontier_prefix_roots(
        &accumulator.pre_frontier,
        accumulator.pre_count,
        &leaves,
    )
    .ok_or_else(|| import_err("deposit frontier delta exceeds tree capacity"))?;
    for (deposit, root) in accumulator.new_deposits.iter().zip(&prefix_roots) {
        if deposit.deposit_root != *root {
            return Err(import_err(format!(
                "deposit {} root {} does not match folded root {}",
                deposit.deposit_id,
                hex32(deposit.deposit_root),
                hex32(*root)
            )));
        }
    }
    let post_root = prefix_roots.last().copied().unwrap_or(pre_root);
    if post_root != post_sidecar.bridge.deposit_root {
        return Err(import_err(format!(
            "deposit post-frontier root {} does not match post-sidecar root {}",
            hex32(post_root),
            hex32(post_sidecar.bridge.deposit_root)
        )));
    }

    sybil_l1_protocol::deposit_frontier_after_prefix(
        &accumulator.pre_frontier,
        accumulator.pre_count,
        &leaves,
    )
    .ok_or_else(|| import_err("deposit frontier delta exceeds tree capacity"))
}

fn deposit_leaf_from_witness(deposit: &L1DepositWitness) -> sybil_l1_protocol::DepositLeaf {
    sybil_l1_protocol::DepositLeaf {
        chain_id: deposit.chain_id,
        vault_address: deposit.vault_address,
        deposit_id: deposit.deposit_id,
        token_address: deposit.token_address,
        sender: deposit.sender,
        sybil_account_key: deposit.sybil_account_key,
        amount_token_units: deposit.amount_token_units,
    }
}

fn resting_orders_from_sidecar(sidecar: &StateSidecarSnapshot) -> Vec<RestingOrder> {
    sidecar
        .resting_orders
        .iter()
        .map(|snapshot| {
            let reserved_positions = snapshot
                .reserved_positions
                .iter()
                .map(|&(market, outcome, qty)| ((market, outcome), qty))
                .collect();
            // The sidecar does not carry matched provenance (has_been_matched /
            // original_max_fill). A partially-filled remainder legitimately
            // carries a proportionally-scaled reservation that may exceed the
            // admission formula for its remaining size, so infer the matched
            // flag from that divergence; restore validation then applies the
            // lower-bound rule instead of falsely rejecting the exact rule.
            // The original size stays unknown (original_max_fill = current).
            let has_been_matched = crate::validation::validate_order_shape(&snapshot.order)
                .and_then(|()| crate::validation::balance_reservation(&snapshot.order))
                .map(|admission_cost| snapshot.reserved_balance != admission_cost)
                .unwrap_or(false);
            RestingOrder {
                order: snapshot.order.clone(),
                account_id: AccountId(snapshot.account_id),
                created_at: snapshot.created_at,
                expires_at_block: snapshot.expires_at_block,
                reserved_balance: snapshot.reserved_balance,
                reserved_positions,
                has_been_matched,
                original_max_fill: snapshot.order.max_fill.0,
                created_at_ms: 0,
            }
        })
        .collect()
}

fn block_header_from_witness(header: &WitnessBlockHeader) -> BlockHeader {
    BlockHeader {
        height: header.height,
        parent_hash: header.parent_hash,
        state_root: header.state_root,
        events_root: header.events_root,
        order_count: header.order_count,
        fill_count: header.fill_count,
        timestamp_ms: header.timestamp_ms,
    }
}

fn next_order_id_from_witness(witness: &BlockWitness) -> Result<u64, StoreError> {
    let max_id = witness
        .orders
        .iter()
        .map(|order| order.order.id)
        .chain(
            witness
                .rejections
                .iter()
                .map(|rejection| rejection.order.id),
        )
        .chain(
            witness
                .state_sidecar
                .resting_orders
                .iter()
                .map(|resting| resting.order.id),
        )
        .chain(witness.fills.iter().map(|fill| fill.order_id))
        .max();
    match max_id {
        Some(max_id) => max_id
            .checked_add(1)
            .ok_or_else(|| import_err("order id overflow deriving next_order_id")),
        None => Ok(1),
    }
}

fn empty_import_analytics(
    last_clearing_prices: HashMap<MarketId, Vec<Nanos>>,
) -> AnalyticsRestoredState {
    AnalyticsRestoredState {
        last_clearing_prices,
        market_volumes: HashMap::new(),
        trader_tracker: Default::default(),
        rolling_volume: Default::default(),
        rolling_price_anchors: Default::default(),
        liquidity_tracker: Default::default(),
        order_stats_tracker: Default::default(),
        welfare_tracker: Default::default(),
        first_deposit_ms: HashMap::new(),
        fill_total_counts: HashMap::new(),
        cost_basis_tracker: Default::default(),
        next_product_event_seq: 0,
    }
}
