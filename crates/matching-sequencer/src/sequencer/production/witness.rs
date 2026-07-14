use super::*;

pub(super) fn bridge_block_data(
    system_events: &[SystemEvent],
    bridge_state: &BridgeState,
) -> BridgeBlockData {
    let mut consumed_deposits = Vec::new();
    let mut withdrawal_leaves = Vec::new();
    for event in system_events {
        match event {
            SystemEvent::L1Deposit { deposit, .. }
            | SystemEvent::DepositQuarantined { deposit, .. } => {
                consumed_deposits.push(deposit.clone())
            }
            SystemEvent::WithdrawalCreated { withdrawal, .. } => {
                withdrawal_leaves.push(withdrawal.clone());
            }
            SystemEvent::CreateAccount { .. }
            | SystemEvent::KeyRegistered { .. }
            | SystemEvent::KeyRevoked { .. }
            | SystemEvent::ClientActionAuthorized(..)
            | SystemEvent::QuarantineClaimed { .. }
            | SystemEvent::Deposit { .. }
            | SystemEvent::WithdrawalRefunded { .. }
            | SystemEvent::WithdrawalFinalized { .. }
            | SystemEvent::L1BlockObserved { .. }
            | SystemEvent::MarketResolved { .. }
            | SystemEvent::OrderCancelled { .. }
            | SystemEvent::MarketGroupExtended { .. }
            | SystemEvent::CompleteSetCollateralized { .. }
            | SystemEvent::CompleteSetRedeemed { .. }
            | SystemEvent::LiquidityUniverseActivated { .. } => {}
        }
    }
    BridgeBlockData {
        deposit_count: bridge_state.deposit_cursor,
        deposit_root: bridge_state.deposit_root,
        consumed_deposits,
        withdrawal_leaves,
    }
}

pub(crate) fn l1_deposit_witness(deposit: &L1Deposit) -> L1DepositWitness {
    L1DepositWitness {
        deposit_id: deposit.deposit_id,
        chain_id: deposit.chain_id,
        vault_address: deposit.vault_address,
        token_address: deposit.token_address,
        sender: deposit.sender,
        sybil_account_key: deposit.sybil_account_key,
        amount_token_units: deposit.amount_token_units,
        deposit_root: deposit.deposit_root,
    }
}

/// Classify an order's side from its payoff structure.
pub(crate) fn classify_order_side(order: &Order) -> &'static str {
    if order.num_markets != 1 || order.num_states != 2 {
        return if order.is_seller() { "Sell" } else { "Custom" };
    }
    // Binary market: state 0 = YES wins, state 1 = NO wins
    let p0 = order.payoffs[0]; // payoff when YES
    let p1 = order.payoffs[1]; // payoff when NO
    match (p0, p1) {
        (1, 0) => "BuyYes",
        (0, 1) => "BuyNo",
        (-1, 0) => "SellYes",
        (0, -1) => "SellNo",
        _ if order.is_seller() => "Sell",
        _ => "Custom",
    }
}

pub(super) fn verifier_failures(
    verification: &sybil_verifier::VerificationResult,
) -> Vec<VerifierFailure> {
    verification
        .violations
        .iter()
        .map(|violation| VerifierFailure {
            kind: format!("{:?}", violation.kind),
            details: violation.details.clone(),
        })
        .collect()
}

/// Build the witness state snapshots around the system-event boundary.
///
/// `pre_state` represents block-start state, so accounts touched by pending
/// system events use their captured baseline. Created accounts are omitted.
/// `post_system_state` is the live account store after system events.
pub(crate) fn build_witness_phase_snapshots(
    accounts: &AccountStore,
    system_account_baselines: &HashMap<AccountId, Option<Account>>,
) -> (Vec<AccountSnapshot>, Vec<AccountSnapshot>) {
    let pre_state =
        CanonicalState::from_snapshot_iter(accounts.iter().filter_map(|(account_id, account)| {
            match system_account_baselines.get(account_id) {
                Some(Some(baseline)) => Some(snapshot_account(baseline)),
                Some(None) => None,
                None => Some(snapshot_account(account)),
            }
        }))
        .into_snapshots();

    let post_system_state = CanonicalState::from_accounts(accounts).into_snapshots();
    (pre_state, post_system_state)
}

pub(super) fn convert_rejection_reason(r: &RejectionReason) -> sybil_verifier::RejectionReason {
    match r {
        RejectionReason::InsufficientBalance {
            required,
            available,
        } => sybil_verifier::RejectionReason::InsufficientBalance {
            required: *required,
            available: *available,
        },
        RejectionReason::InsufficientPosition {
            market,
            outcome,
            required,
            available,
        } => sybil_verifier::RejectionReason::InsufficientPosition {
            market: *market,
            outcome: *outcome,
            required: *required,
            available: *available,
        },
        RejectionReason::AccountNotFound => sybil_verifier::RejectionReason::AccountNotFound,
        RejectionReason::CompleteSetFormation => {
            sybil_verifier::RejectionReason::CompleteSetFormation
        }
        RejectionReason::InvalidOrder(reason) => {
            sybil_verifier::RejectionReason::InvalidOrder(reason.clone())
        }
        RejectionReason::Expired {
            current_block,
            expires_at_block,
        } => sybil_verifier::RejectionReason::Expired {
            current_block: *current_block,
            expires_at_block: *expires_at_block,
        },
    }
}

pub(super) fn convert_system_event(event: &SystemEvent) -> SystemEventWitness {
    match event {
        SystemEvent::CreateAccount {
            account_id,
            initial_balance,
            initial_keys,
        } => SystemEventWitness::CreateAccount {
            account_id: account_id.0,
            initial_balance: *initial_balance,
            initial_keys: initial_keys.clone(),
        },
        SystemEvent::Deposit { account_id, amount } => SystemEventWitness::Deposit {
            account_id: account_id.0,
            amount: *amount,
        },
        SystemEvent::L1Deposit {
            account_id,
            amount,
            deposit,
        } => SystemEventWitness::L1Deposit {
            account_id: account_id.0,
            amount: *amount,
            deposit_id: deposit.deposit_id,
            deposit_root: deposit.deposit_root,
            sybil_account_key: deposit.sybil_account_key,
        },
        SystemEvent::WithdrawalCreated {
            account_id,
            amount,
            withdrawal,
        } => SystemEventWitness::WithdrawalCreated {
            account_id: account_id.0,
            amount: *amount,
            withdrawal_id: withdrawal.withdrawal_id,
            recipient: withdrawal.recipient,
            token: withdrawal.token_address,
            amount_token_units: withdrawal.amount_token_units,
            expiry_height: withdrawal.expiry_height,
            nullifier: withdrawal.nullifier,
        },
        SystemEvent::WithdrawalRefunded {
            account_id,
            withdrawal_id,
            amount,
            reason,
        } => SystemEventWitness::WithdrawalRefunded {
            account_id: account_id.0,
            withdrawal_id: *withdrawal_id,
            amount: *amount,
            reason: match reason {
                crate::bridge::WithdrawalRefundReason::L1Cancelled => {
                    sybil_verifier::WithdrawalRefundReasonWitness::L1Cancelled
                }
                crate::bridge::WithdrawalRefundReason::L1Expired { observed_l1_height } => {
                    sybil_verifier::WithdrawalRefundReasonWitness::L1Expired {
                        observed_l1_height: *observed_l1_height,
                    }
                }
            },
        },
        SystemEvent::WithdrawalFinalized {
            account_id,
            withdrawal_id,
            amount,
        } => SystemEventWitness::WithdrawalFinalized {
            account_id: account_id.0,
            withdrawal_id: *withdrawal_id,
            amount: *amount,
        },
        SystemEvent::L1BlockObserved { height } => {
            SystemEventWitness::L1BlockObserved { height: *height }
        }
        SystemEvent::MarketResolved {
            market_id,
            payout_nanos,
            affected_accounts,
        } => SystemEventWitness::MarketResolved {
            market_id: *market_id,
            payout_nanos: *payout_nanos,
            affected_accounts: affected_accounts.iter().map(|id| id.0).collect(),
        },
        SystemEvent::OrderCancelled {
            account_id,
            order_id,
            market_ids,
            side,
            remaining_quantity,
        } => SystemEventWitness::OrderCancelled {
            account_id: account_id.0,
            order_id: *order_id,
            market_ids: market_ids.clone(),
            side: *side,
            remaining_quantity: *remaining_quantity,
        },
        SystemEvent::MarketGroupExtended {
            group_id,
            market_id,
        } => SystemEventWitness::MarketGroupExtended {
            group_id: *group_id,
            market_id: *market_id,
        },
        SystemEvent::KeyRegistered {
            account_id,
            key,
            authorization,
        } => SystemEventWitness::KeyRegistered {
            account_id: account_id.0,
            key: *key,
            authorization: authorization.clone(),
        },
        SystemEvent::KeyRevoked {
            account_id,
            key,
            authorization,
        } => SystemEventWitness::KeyRevoked {
            account_id: account_id.0,
            key: *key,
            authorization: authorization.clone(),
        },
        SystemEvent::DepositQuarantined { amount, deposit } => {
            SystemEventWitness::DepositQuarantined {
                amount: *amount,
                deposit_id: deposit.deposit_id,
                deposit_root: deposit.deposit_root,
                sybil_account_key: deposit.sybil_account_key,
            }
        }
        SystemEvent::QuarantineClaimed {
            account_id,
            amount,
            sybil_account_key,
        } => SystemEventWitness::QuarantineClaimed {
            account_id: account_id.0,
            amount: *amount,
            sybil_account_key: *sybil_account_key,
        },
        SystemEvent::ClientActionAuthorized(action) => {
            SystemEventWitness::ClientActionAuthorized(action.clone())
        }
        SystemEvent::CompleteSetCollateralized {
            account_id,
            market_id,
            quantity,
        } => SystemEventWitness::CompleteSetCollateralized {
            account_id: account_id.0,
            market_id: *market_id,
            quantity: *quantity,
        },
        SystemEvent::CompleteSetRedeemed {
            account_id,
            market_id,
            quantity,
        } => SystemEventWitness::CompleteSetRedeemed {
            account_id: account_id.0,
            market_id: *market_id,
            quantity: *quantity,
        },
        SystemEvent::LiquidityUniverseActivated {
            generation,
            policy_digest,
            activated_at_height,
            market_ids,
        } => SystemEventWitness::LiquidityUniverseActivated {
            generation: *generation,
            policy_digest: *policy_digest,
            activated_at_height: *activated_at_height,
            market_ids: market_ids.clone(),
        },
    }
}

impl BlockSequencer {
    pub(super) fn assemble_witness_artifacts(
        &self,
        input: WitnessAssemblyInput<'_>,
    ) -> WitnessArtifacts {
        let WitnessAssemblyInput {
            post_state,
            order_count,
            timestamp_ms,
            previous_header,
            witness_orders,
            witness_rejections,
            system_events,
            fills,
            clearing_prices,
            total_welfare,
            minting_cost,
            problem,
            pre_state,
            pre_state_sidecar,
            pre_deposit_frontier,
            post_system_state,
            resolved_markets,
        } = input;

        let state_sidecar = state_sidecar_snapshot(
            &self.bridge,
            &self.order_book,
            &self.markets,
            &self.market_groups,
            &self.lifecycle,
            self.analytics.last_clearing_prices(),
            &self.liquidity_universe,
        );
        let system_event_witnesses: Vec<SystemEventWitness> =
            system_events.iter().map(convert_system_event).collect();
        let new_deposits = system_events
            .iter()
            .filter_map(|event| match event {
                SystemEvent::L1Deposit { deposit, .. }
                | SystemEvent::DepositQuarantined { deposit, .. } => {
                    Some(l1_deposit_witness(deposit))
                }
                SystemEvent::CreateAccount { .. }
                | SystemEvent::KeyRegistered { .. }
                | SystemEvent::KeyRevoked { .. }
                | SystemEvent::ClientActionAuthorized(..)
                | SystemEvent::QuarantineClaimed { .. }
                | SystemEvent::Deposit { .. }
                | SystemEvent::WithdrawalCreated { .. }
                | SystemEvent::WithdrawalRefunded { .. }
                | SystemEvent::WithdrawalFinalized { .. }
                | SystemEvent::L1BlockObserved { .. }
                | SystemEvent::MarketResolved { .. }
                | SystemEvent::OrderCancelled { .. }
                | SystemEvent::MarketGroupExtended { .. }
                | SystemEvent::CompleteSetCollateralized { .. }
                | SystemEvent::CompleteSetRedeemed { .. }
                | SystemEvent::LiquidityUniverseActivated { .. } => None,
            })
            .collect();
        let events_root = sybil_verifier::event_commitment::events_root_from_event_bytes(
            &sybil_verifier::event_commitment::event_leaf_values(
                &system_event_witnesses,
                &witness_orders,
                &witness_rejections,
                fills,
            ),
        );
        let header = BlockHeader {
            height: self.height,
            parent_hash: self
                .last_header
                .as_ref()
                .map(hash_header)
                .unwrap_or([0u8; 32]),
            state_root: sybil_verifier::block::compute_state_root_with_sidecar(
                post_state.as_snapshots(),
                &state_sidecar,
            ),
            events_root,
            order_count,
            fill_count: fills.len() as u32,
            timestamp_ms,
        };

        let witness = BlockWitness {
            header: header.to_witness_header(),
            previous_header,
            genesis_hash: self.genesis_hash.unwrap_or_else(|| hash_header(&header)),
            orders: witness_orders,
            rejections: witness_rejections,
            system_events: system_event_witnesses,
            deposit_accumulator: sybil_verifier::DepositAccumulatorWitness {
                pre_frontier: pre_deposit_frontier,
                pre_count: pre_state_sidecar.bridge.deposit_cursor,
                new_deposits,
            },
            fills: fills.to_vec(),
            clearing_prices: clearing_prices.clone(),
            total_welfare,
            minting_cost,
            mm_constraints: problem.mm_constraints.clone(),
            market_groups: problem.market_groups.clone(),
            pre_state,
            post_system_state,
            post_state: post_state.into_snapshots(),
            account_keys: account_key_sets(&self.accounts, &self.pubkey_registry),
            state_sidecar,
            pre_state_sidecar,
            resolved_markets,
        };

        WitnessArtifacts { header, witness }
    }
}

fn account_key_sets(
    accounts: &AccountStore,
    registry: &HashMap<crate::crypto::PublicKey, crate::crypto::RegisteredPubkey>,
) -> Vec<(u64, Vec<sybil_verifier::KeyRecord>)> {
    let mut sets = Vec::new();
    for (account_id, _) in accounts.iter() {
        let mut keys: Vec<_> = registry
            .iter()
            .filter(|(_, registered)| registered.account_id == *account_id)
            .map(|(pubkey, registered)| crate::digest::key_record(pubkey, registered))
            .collect();
        if keys.is_empty() {
            continue;
        }
        keys.sort_by_key(sybil_verifier::KeyRecord::canonical_sort_key);
        sets.push((account_id.0, keys));
    }
    sets.sort_by_key(|(account_id, _)| *account_id);
    sets
}
