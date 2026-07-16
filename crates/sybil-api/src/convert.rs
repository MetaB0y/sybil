use std::collections::HashMap;

use matching_engine::{
    MarketId, MarketSet, NANOS_PER_DOLLAR, Nanos, Order, Qty, outcome_buy, outcome_sell,
};
use matching_sequencer::Account;
use matching_sequencer::block::SealedBlock;
use matching_sequencer::error::Rejection;

use crate::types::request::{
    BridgeWithdrawalL1Status as ApiBridgeWithdrawalL1Status, OrderSpec, SignedOrderData,
    TimeInForce as ApiTimeInForce,
};
use crate::types::response::*;

fn system_event_to_response(event: &matching_sequencer::SystemEvent) -> SystemEventResponse {
    match event {
        matching_sequencer::SystemEvent::CreateAccount {
            account_id,
            initial_balance,
            ..
        } => SystemEventResponse::CreateAccount {
            account_id: account_id.0,
            initial_balance_nanos: *initial_balance,
        },
        matching_sequencer::SystemEvent::Deposit { account_id, amount } => {
            SystemEventResponse::Deposit {
                account_id: account_id.0,
                amount_nanos: *amount,
            }
        }
        matching_sequencer::SystemEvent::L1Deposit {
            account_id,
            amount,
            deposit,
        } => SystemEventResponse::L1Deposit {
            account_id: account_id.0,
            amount_nanos: *amount,
            deposit_id: deposit.deposit_id,
            deposit_root_hex: hex::encode(deposit.deposit_root),
            sybil_account_key_hex: hex::encode(deposit.sybil_account_key),
        },
        matching_sequencer::SystemEvent::WithdrawalCreated {
            account_id,
            amount,
            withdrawal,
        } => SystemEventResponse::WithdrawalCreated {
            account_id: account_id.0,
            amount_nanos: *amount,
            withdrawal_id: withdrawal.withdrawal_id,
            nullifier_hex: hex::encode(withdrawal.nullifier),
        },
        matching_sequencer::SystemEvent::WithdrawalRefunded {
            account_id,
            withdrawal_id,
            amount,
            reason,
        } => SystemEventResponse::WithdrawalRefunded {
            account_id: account_id.0,
            amount_nanos: *amount,
            withdrawal_id: *withdrawal_id,
            reason: match reason {
                matching_sequencer::WithdrawalRefundReason::L1Cancelled => {
                    "l1_cancelled".to_string()
                }
                matching_sequencer::WithdrawalRefundReason::L1Expired { .. } => {
                    "l1_expired".to_string()
                }
            },
        },
        matching_sequencer::SystemEvent::WithdrawalFinalized {
            account_id,
            withdrawal_id,
            amount,
        } => SystemEventResponse::WithdrawalFinalized {
            account_id: account_id.0,
            amount_nanos: *amount,
            withdrawal_id: *withdrawal_id,
        },
        matching_sequencer::SystemEvent::L1BlockObserved { height } => {
            SystemEventResponse::L1BlockObserved { height: *height }
        }
        matching_sequencer::SystemEvent::MarketResolved {
            market_id,
            payout_nanos,
            affected_accounts,
        } => SystemEventResponse::MarketResolved {
            market_id: market_id.0,
            payout_nanos: payout_nanos.0,
            affected_accounts: affected_accounts.iter().map(|id| id.0).collect(),
        },
        matching_sequencer::SystemEvent::OrderCancelled {
            account_id,
            order_id,
            market_ids,
            side,
            remaining_quantity,
        } => SystemEventResponse::OrderCancelled {
            account_id: account_id.0,
            order_id: *order_id,
            market_ids: market_ids.iter().map(|m| m.0).collect(),
            side: side.to_string(),
            remaining_quantity: *remaining_quantity,
        },
        matching_sequencer::SystemEvent::MarketGroupExtended {
            group_id,
            market_id,
        } => SystemEventResponse::MarketGroupExtended {
            group_id: *group_id,
            market_id: market_id.0,
        },
        matching_sequencer::SystemEvent::KeyRegistered {
            account_id, key, ..
        } => SystemEventResponse::KeyRegistered {
            account_id: account_id.0,
            public_key_hex: hex::encode(key.pubkey_sec1),
            auth_scheme: key.auth_scheme,
            capability_mask: key.capability_mask,
        },
        matching_sequencer::SystemEvent::KeyRevoked {
            account_id, key, ..
        } => SystemEventResponse::KeyRevoked {
            account_id: account_id.0,
            public_key_hex: hex::encode(key.pubkey_sec1),
            auth_scheme: key.auth_scheme,
            capability_mask: key.capability_mask,
        },
        matching_sequencer::SystemEvent::ClientActionAuthorized(action) => {
            let (account_id, action_name, order_id, nonce, authorization) = match action {
                matching_sequencer::ClientActionWitness::Order {
                    account_id,
                    order,
                    nonce,
                    authorization,
                } => (*account_id, "order", order.id, *nonce, authorization),
                matching_sequencer::ClientActionWitness::Cancel {
                    account_id,
                    order_id,
                    nonce,
                    authorization,
                } => (*account_id, "cancel", *order_id, *nonce, authorization),
            };
            SystemEventResponse::ClientActionAuthorized {
                account_id,
                action: action_name.to_string(),
                order_id,
                nonce,
                public_key_hex: hex::encode(authorization.signer_pubkey()),
                auth_scheme: authorization.signer_auth_scheme(),
            }
        }
        matching_sequencer::SystemEvent::DepositQuarantined { amount, deposit } => {
            SystemEventResponse::DepositQuarantined {
                amount_nanos: *amount,
                deposit_id: deposit.deposit_id,
                deposit_root_hex: hex::encode(deposit.deposit_root),
                sybil_account_key_hex: hex::encode(deposit.sybil_account_key),
            }
        }
        matching_sequencer::SystemEvent::QuarantineClaimed {
            account_id,
            amount,
            sybil_account_key,
        } => SystemEventResponse::QuarantineClaimed {
            account_id: account_id.0,
            amount_nanos: *amount,
            sybil_account_key_hex: hex::encode(sybil_account_key),
        },
    }
}

/// Split gross balance into reserved and spendable components defensively.
pub fn account_balance_breakdown(balance_nanos: i64, reserved_balance_nanos: i64) -> (i64, i64) {
    let reserved_balance_nanos = reserved_balance_nanos.max(0);
    let available_balance_nanos = balance_nanos.saturating_sub(reserved_balance_nanos).max(0);
    (available_balance_nanos, reserved_balance_nanos)
}

/// Convert an Account to an AccountResponse.
pub fn account_to_response(account: &Account, reserved_balance_nanos: i64) -> AccountResponse {
    let positions: Vec<PositionResponse> = account
        .positions
        .iter()
        .filter(|&(_, &qty)| qty != 0)
        .map(|(&(market_id, outcome), &qty)| PositionResponse {
            market_id: market_id.0,
            outcome: if outcome == 0 {
                "YES".to_string()
            } else {
                "NO".to_string()
            },
            quantity: qty,
        })
        .collect();

    let (available_balance_nanos, reserved_balance_nanos) =
        account_balance_breakdown(account.balance, reserved_balance_nanos);

    AccountResponse {
        account_id: account.id.0,
        balance_nanos: account.balance,
        available_balance_nanos,
        reserved_balance_nanos,
        keys_digest_hex: hex::encode(account.keys_digest),
        events_digest_hex: hex::encode(account.events_digest),
        positions,
        display_name: account.profile.display_name.clone(),
        avatar_seed: account.profile.avatar_seed.clone(),
    }
}

pub fn bridge_withdrawal_to_response(
    withdrawal: &matching_sequencer::WithdrawalLeaf,
) -> BridgeWithdrawalResponse {
    BridgeWithdrawalResponse {
        withdrawal_id: withdrawal.withdrawal_id,
        account_id: withdrawal.account_id.0,
        recipient_hex: hex::encode(withdrawal.recipient),
        token_hex: hex::encode(withdrawal.token_address),
        amount_token_units: withdrawal.amount_token_units,
        amount_nanos: withdrawal.amount_nanos,
        expiry_height: withdrawal.expiry_height,
        nullifier_hex: hex::encode(withdrawal.nullifier),
        withdrawal_leaf_hex: hex::encode(matching_sequencer::bridge::withdrawal_leaf_bytes(
            withdrawal,
        )),
        withdrawal_leaf_digest_hex: hex::encode(
            matching_sequencer::bridge::withdrawal_leaf_digest(withdrawal),
        ),
        created_at_height: withdrawal.created_at_height,
        l1_status: l1_withdrawal_status_to_response(withdrawal.l1_status),
        l1_requested_at_unix: withdrawal.l1_requested_at_unix,
        l1_executable_at_unix: withdrawal.l1_executable_at_unix,
        l1_finalized_at_unix: withdrawal.l1_finalized_at_unix,
        l1_cancelled_at_unix: withdrawal.l1_cancelled_at_unix,
        l1_tx_hash_hex: withdrawal.l1_tx_hash.map(hex::encode),
    }
}

fn l1_withdrawal_status_to_response(
    status: matching_sequencer::L1WithdrawalStatus,
) -> ApiBridgeWithdrawalL1Status {
    match status {
        matching_sequencer::L1WithdrawalStatus::NotRequested => {
            ApiBridgeWithdrawalL1Status::NotRequested
        }
        matching_sequencer::L1WithdrawalStatus::Queued => ApiBridgeWithdrawalL1Status::Queued,
        matching_sequencer::L1WithdrawalStatus::Finalized => ApiBridgeWithdrawalL1Status::Finalized,
        matching_sequencer::L1WithdrawalStatus::Cancelled => ApiBridgeWithdrawalL1Status::Cancelled,
        matching_sequencer::L1WithdrawalStatus::Refunded => ApiBridgeWithdrawalL1Status::Refunded,
    }
}

fn bridge_block_to_response(block: &SealedBlock) -> BridgeBlockResponse {
    BridgeBlockResponse {
        deposit_count: block.canonical.bridge.deposit_count,
        deposit_root_hex: hex::encode(block.canonical.bridge.deposit_root),
        consumed_deposits: block
            .canonical
            .bridge
            .consumed_deposits
            .iter()
            .map(|deposit| BridgeDepositEventResponse {
                deposit_id: deposit.deposit_id,
                account_id: deposit.account_id.map(|account_id| account_id.0),
                amount_token_units: deposit.amount_token_units,
                deposit_root_hex: hex::encode(deposit.deposit_root),
            })
            .collect(),
        withdrawal_leaves: block
            .canonical
            .bridge
            .withdrawal_leaves
            .iter()
            .map(bridge_withdrawal_to_response)
            .collect(),
    }
}

fn derived_view_sidecar_to_response(
    sidecar: &matching_sequencer::DerivedViewSidecar,
) -> DerivedViewSidecarResponse {
    DerivedViewSidecarResponse {
        provenance: "derived_unproven".to_string(),
        removed_orders: sidecar
            .removed_orders
            .iter()
            .map(|removed| RemovedOrderViewResponse {
                order_id: removed.order_id,
                account_id: removed.account_id,
                phase: removed_order_phase(removed.phase).to_string(),
                exit_reason: removed_order_exit_reason(removed.exit_reason).to_string(),
                has_been_matched: removed.has_been_matched,
                reserved_balance_released: removed.reserved_balance_released,
                reserved_positions_released: removed
                    .reserved_positions_released
                    .iter()
                    .map(
                        |(market_id, outcome, quantity)| ReservedPositionReleaseResponse {
                            market_id: market_id.0,
                            outcome: *outcome,
                            quantity: *quantity,
                        },
                    )
                    .collect(),
                active_markets: removed
                    .active_markets
                    .iter()
                    .map(|market_id| market_id.0)
                    .collect(),
                rejection_reason: removed
                    .rejection_reason
                    .as_ref()
                    .map(|reason| reason.code().to_string()),
            })
            .collect(),
        admits: sidecar
            .admits
            .iter()
            .map(|admit| AdmitTimingViewResponse {
                order_id: admit.order_id,
                account_id: admit.account_id,
                admit_height: admit.admit_height,
                admit_timestamp_ms: admit.admit_timestamp_ms,
                is_new: admit.is_new,
                is_mm: admit.is_mm,
            })
            .collect(),
        rejection_history: sidecar
            .rejection_history
            .iter()
            .map(|rejection| RejectedOrderViewResponse {
                order_id: rejection.order_id,
                account_id: rejection.account_id,
                reason: rejection.reason.code().to_string(),
            })
            .collect(),
    }
}

fn removed_order_phase(phase: matching_sequencer::RemovedOrderPhase) -> &'static str {
    match phase {
        matching_sequencer::RemovedOrderPhase::BlockStartExpire => "block_start_expire",
        matching_sequencer::RemovedOrderPhase::BlockStartRevalidate => "block_start_revalidate",
        matching_sequencer::RemovedOrderPhase::PostSolve => "post_solve",
    }
}

fn removed_order_exit_reason(reason: matching_sequencer::RemovedOrderExitReason) -> &'static str {
    match reason {
        matching_sequencer::RemovedOrderExitReason::Expired => "expired",
        matching_sequencer::RemovedOrderExitReason::RevalidateInsufficientBalance => {
            "revalidate_insufficient_balance"
        }
        matching_sequencer::RemovedOrderExitReason::RevalidateInsufficientPosition => {
            "revalidate_insufficient_position"
        }
        matching_sequencer::RemovedOrderExitReason::RevalidateMarketInactive => {
            "revalidate_market_inactive"
        }
        matching_sequencer::RemovedOrderExitReason::RevalidateAccountGone => {
            "revalidate_account_gone"
        }
        matching_sequencer::RemovedOrderExitReason::RevalidateAccountInsolvent => {
            "revalidate_account_insolvent"
        }
        matching_sequencer::RemovedOrderExitReason::RevalidateRejected => "revalidate_rejected",
        matching_sequencer::RemovedOrderExitReason::Filled => "filled",
        matching_sequencer::RemovedOrderExitReason::Settled => "settled",
    }
}

/// Convert a sealed block to the authenticated service response. This contains
/// account-attributed data and must not be called from public routes.
pub fn block_to_response(block: &SealedBlock) -> BlockResponse {
    let fills = block
        .canonical
        .fills
        .iter()
        .map(|f| FillResponse {
            order_id: f.order_id,
            fill_qty: f.fill_qty.0,
            fill_price_nanos: f.fill_price.0,
            account_id: f.account_id,
        })
        .collect();

    let clearing_prices_nanos: HashMap<String, Vec<u64>> = block
        .canonical
        .clearing_prices
        .iter()
        .map(|(mid, prices)| (mid.0.to_string(), prices.iter().map(|n| n.0).collect()))
        .collect();

    let rejections = block
        .canonical
        .rejections
        .iter()
        .map(rejection_to_response)
        .collect();
    let system_events = block
        .canonical
        .system_events
        .iter()
        .map(system_event_to_response)
        .collect();

    // Union the keys from placers_by_market, volume_by_market, and
    // orders_by_market — any of the three can be empty on its own (a
    // block with carried fills but no fresh admits has zero placers but
    // non-zero volume; a block whose only book activity is expiries has
    // matched/unmatched but no fresh admits).
    let mut by_market: HashMap<String, BlockMarketStats> = HashMap::new();
    for (mid, count) in &block.analytics.placers_by_market {
        by_market.entry(mid.0.to_string()).or_default().placers = *count;
    }
    for (mid, vol) in &block.analytics.volume_by_market {
        by_market.entry(mid.0.to_string()).or_default().volume_nanos = *vol;
    }
    for (mid, stats) in &block.analytics.orders_by_market {
        let entry = by_market.entry(mid.0.to_string()).or_default();
        entry.placed = stats.placed as u32;
        entry.matched = stats.matched as u32;
        entry.unmatched = stats.unmatched as u32;
    }
    for (mid, welfare) in &block.analytics.welfare_by_market {
        by_market
            .entry(mid.0.to_string())
            .or_default()
            .welfare_nanos = *welfare;
    }

    BlockResponse {
        height: block.canonical.header.height,
        parent_hash: hex::encode(block.canonical.header.parent_hash),
        state_root: hex::encode(block.canonical.header.state_root),
        events_root: hex::encode(block.canonical.header.events_root),
        order_count: block.canonical.header.order_count,
        fill_count: block.canonical.header.fill_count,
        timestamp_ms: block.canonical.header.timestamp_ms,
        system_events,
        fills,
        clearing_prices_nanos,
        rejections,
        bridge: bridge_block_to_response(block),
        total_welfare_nanos: block.analytics.total_welfare,
        total_volume_nanos: block.analytics.total_volume,
        orders_filled: block.analytics.orders_filled,
        unique_placers: block.analytics.unique_placers,
        by_market,
        derived_view_sidecar: derived_view_sidecar_to_response(&block.derived_view_sidecar),
    }
}

/// Convert a sealed block to the allowlisted public market tape. The return
/// type makes it impossible for a public handler to accidentally serialize
/// account-attributed canonical rows.
pub fn public_block_to_response(block: &SealedBlock) -> PublicBlockResponse {
    let full = block_to_response(block);
    let resolved_market_ids = block
        .canonical
        .system_events
        .iter()
        .filter_map(|event| match event {
            matching_sequencer::SystemEvent::MarketResolved { market_id, .. } => Some(market_id.0),
            _ => None,
        })
        .collect();

    PublicBlockResponse {
        height: full.height,
        parent_hash: full.parent_hash,
        state_root: full.state_root,
        events_root: full.events_root,
        order_count: full.order_count,
        fill_count: full.fill_count,
        rejection_count: full.rejections.len() as u32,
        timestamp_ms: full.timestamp_ms,
        clearing_prices_nanos: full.clearing_prices_nanos,
        bridge: PublicBridgeBlockResponse {
            deposit_count: full.bridge.deposit_count,
            deposit_root_hex: full.bridge.deposit_root_hex,
        },
        resolved_market_ids,
        total_welfare_nanos: full.total_welfare_nanos,
        total_volume_nanos: full.total_volume_nanos,
        orders_filled: full.orders_filled,
        unique_placers: full.unique_placers,
        by_market: full.by_market,
    }
}

fn rejection_to_response(r: &Rejection) -> RejectionResponse {
    RejectionResponse {
        order_id: r.order_id,
        account_id: r.account_id.0,
        reason: format!("{:?}", r.reason),
    }
}

/// Convert market prices map to response format.
pub fn prices_to_response(prices: &HashMap<MarketId, Vec<Nanos>>) -> MarketPricesResponse {
    let mut map = HashMap::new();
    for (mid, ps) in prices {
        let yes_price_nanos = ps.first().map(|n| n.0).unwrap_or(NANOS_PER_DOLLAR / 2);
        let no_price_nanos = ps.get(1).map(|n| n.0).unwrap_or(NANOS_PER_DOLLAR / 2);
        map.insert(
            mid.0.to_string(),
            MarketPriceResponse {
                yes_price_nanos,
                no_price_nanos,
            },
        );
    }
    MarketPricesResponse { prices: map }
}

/// Convert an OrderSpec from the API into an internal Order.
pub fn order_spec_to_order(
    spec: &OrderSpec,
    markets: &MarketSet,
) -> Result<Order, OrderSpecConversionError> {
    let order = match spec {
        OrderSpec::BuyYes {
            market_id,
            limit_price_nanos,
            quantity,
        } => {
            let mid = MarketId::new(*market_id);
            validate_market(mid, markets)?;
            validate_price_nanos(*limit_price_nanos)?;
            outcome_buy(markets, 0, mid, 0, *limit_price_nanos, *quantity)
        }
        OrderSpec::BuyNo {
            market_id,
            limit_price_nanos,
            quantity,
        } => {
            let mid = MarketId::new(*market_id);
            validate_market(mid, markets)?;
            validate_price_nanos(*limit_price_nanos)?;
            outcome_buy(markets, 0, mid, 1, *limit_price_nanos, *quantity)
        }
        OrderSpec::SellYes {
            market_id,
            limit_price_nanos,
            quantity,
        } => {
            let mid = MarketId::new(*market_id);
            validate_market(mid, markets)?;
            validate_price_nanos(*limit_price_nanos)?;
            outcome_sell(markets, 0, mid, 0, *limit_price_nanos, *quantity)
        }
        OrderSpec::SellNo {
            market_id,
            limit_price_nanos,
            quantity,
        } => {
            let mid = MarketId::new(*market_id);
            validate_market(mid, markets)?;
            validate_price_nanos(*limit_price_nanos)?;
            outcome_sell(markets, 0, mid, 1, *limit_price_nanos, *quantity)
        }
    };
    validate_public_order_shape(&order)?;
    Ok(order)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderSpecConversionError {
    MarketNotFound(MarketId),
    Invalid(String),
}

impl From<String> for OrderSpecConversionError {
    fn from(error: String) -> Self {
        Self::Invalid(error)
    }
}

impl std::fmt::Display for OrderSpecConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MarketNotFound(market_id) => write!(f, "Market {} not found", market_id.0),
            Self::Invalid(error) => f.write_str(error),
        }
    }
}

/// Convert a SignedOrderData to an internal Order.
pub fn signed_order_data_to_order(data: &SignedOrderData) -> Result<Order, String> {
    if data.market_ids.len() != 1 {
        return Err("Signed orders must span exactly one market".to_string());
    }
    if data.payoffs.len() != 2 {
        return Err("Signed orders must provide exactly two binary payoff entries".to_string());
    }
    validate_price_nanos(data.limit_price_nanos)?;

    let mut order = Order::new(0);
    order.markets[0] = MarketId::new(data.market_ids[0]);
    order.num_markets = 1;
    order.num_states = 2;
    order.payoffs[0] = data.payoffs[0];
    order.payoffs[1] = data.payoffs[1];
    order.limit_price = Nanos(data.limit_price_nanos);
    order.max_fill = Qty(data.max_fill);

    validate_public_order_shape(&order)?;

    Ok(order)
}

pub fn apply_time_in_force(
    order: &mut Order,
    time_in_force: ApiTimeInForce,
    expires_at_block: Option<u64>,
    ioc_expires_at_block: Option<u64>,
) -> Result<(), String> {
    order.expires_at_block = match time_in_force {
        ApiTimeInForce::Gtc => {
            if expires_at_block.is_some() {
                return Err("expires_at_block is not valid for GTC orders".to_string());
            }
            None
        }
        ApiTimeInForce::Ioc => {
            let expiry = expires_at_block
                .or(ioc_expires_at_block)
                .ok_or_else(|| "IOC orders require a resolved expires_at_block".to_string())?;
            Some(expiry)
        }
        ApiTimeInForce::Gtd => {
            let Some(expires_at_block) = expires_at_block else {
                return Err("GTD orders require expires_at_block".to_string());
            };
            Some(expires_at_block)
        }
    };

    Ok(())
}

fn validate_market(mid: MarketId, markets: &MarketSet) -> Result<(), OrderSpecConversionError> {
    if markets.get(mid).is_none() {
        return Err(OrderSpecConversionError::MarketNotFound(mid));
    }
    Ok(())
}

fn validate_price_nanos(price_nanos: u64) -> Result<(), String> {
    if price_nanos > NANOS_PER_DOLLAR {
        return Err(format!(
            "Price must be between 0 and {} nanos, got {}",
            NANOS_PER_DOLLAR, price_nanos
        ));
    }
    Ok(())
}

fn validate_public_order_shape(order: &Order) -> Result<(), String> {
    order
        .validate_binary_one_hot()
        .map_err(|reason| format!("unsupported order shape: {reason}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching_engine::MarketSet;
    use matching_sequencer::AccountId;

    fn make_markets() -> MarketSet {
        let mut ms = MarketSet::new();
        ms.add_binary("Market A");
        ms.add_binary("Market B");
        ms
    }

    #[test]
    fn test_buy_yes_conversion() {
        let ms = make_markets();
        let spec = OrderSpec::BuyYes {
            market_id: 0,
            limit_price_nanos: 550_000_000,
            quantity: 10,
        };
        let order = order_spec_to_order(&spec, &ms).unwrap();
        assert_eq!(order.num_markets, 1);
        assert_eq!(order.markets[0], MarketId::new(0));
        assert_eq!(order.payoffs[0], 1); // YES payoff
        assert_eq!(order.payoffs[1], 0); // NO payoff
        assert_eq!(order.max_fill, Qty(10));
    }

    #[test]
    fn test_sell_yes_conversion() {
        let ms = make_markets();
        let spec = OrderSpec::SellYes {
            market_id: 0,
            limit_price_nanos: 600_000_000,
            quantity: 5,
        };
        let order = order_spec_to_order(&spec, &ms).unwrap();
        assert_eq!(order.payoffs[0], -1); // Selling YES
        assert_eq!(order.max_fill, Qty(5));
    }

    #[test]
    fn test_invalid_market_rejected() {
        let ms = make_markets();
        let spec = OrderSpec::BuyYes {
            market_id: 99,
            limit_price_nanos: 550_000_000,
            quantity: 10,
        };
        assert!(order_spec_to_order(&spec, &ms).is_err());
    }

    #[test]
    fn test_oversized_quantity_rejected() {
        let ms = make_markets();
        let spec = OrderSpec::BuyYes {
            market_id: 0,
            limit_price_nanos: 500_000_000,
            quantity: matching_engine::MAX_ORDER_QTY + 1,
        };

        assert!(order_spec_to_order(&spec, &ms).is_err());
    }

    #[test]
    fn test_invalid_price_rejected() {
        let ms = make_markets();
        let spec = OrderSpec::BuyYes {
            market_id: 0,
            limit_price_nanos: 1_500_000_000, // > NANOS_PER_DOLLAR
            quantity: 10,
        };
        assert!(order_spec_to_order(&spec, &ms).is_err());
    }

    #[test]
    fn test_signed_non_one_hot_payoff_rejected() {
        let data = SignedOrderData {
            market_ids: vec![0],
            payoffs: vec![2, 0],
            limit_price_nanos: 500_000_000,
            max_fill: 10,
        };

        assert!(signed_order_data_to_order(&data).is_err());
    }

    #[test]
    fn test_signed_multi_market_payoff_rejected() {
        let data = SignedOrderData {
            market_ids: vec![0, 1],
            payoffs: vec![1, 0, 0, 0],
            limit_price_nanos: 500_000_000,
            max_fill: 10,
        };

        assert!(signed_order_data_to_order(&data).is_err());
    }

    #[test]
    fn test_signed_oversized_quantity_rejected() {
        let data = SignedOrderData {
            market_ids: vec![0],
            payoffs: vec![1, 0],
            limit_price_nanos: 500_000_000,
            max_fill: matching_engine::MAX_ORDER_QTY + 1,
        };

        assert!(signed_order_data_to_order(&data).is_err());
    }

    #[test]
    fn test_account_to_response() {
        let mut account = Account::new(AccountId(42), 100 * NANOS_PER_DOLLAR as i64);
        account.positions.insert((MarketId::new(0), 0), 10);

        let resp = account_to_response(&account, 25 * NANOS_PER_DOLLAR as i64);
        assert_eq!(resp.account_id, 42);
        assert_eq!(resp.balance_nanos, 100 * NANOS_PER_DOLLAR as i64);
        assert_eq!(resp.available_balance_nanos, 75 * NANOS_PER_DOLLAR as i64);
        assert_eq!(resp.reserved_balance_nanos, 25 * NANOS_PER_DOLLAR as i64);
        assert_eq!(resp.positions.len(), 1);
    }

    #[test]
    fn test_account_balance_breakdown_clamps_available_at_zero() {
        assert_eq!(account_balance_breakdown(10, 12), (0, 12));
    }
}
