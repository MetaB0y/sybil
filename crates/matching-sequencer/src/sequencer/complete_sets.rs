use matching_engine::{MarketId, Qty, collateralize_complete_set, redeem_complete_set};

use super::*;

impl BlockSequencer {
    pub fn apply_complete_set_inventory_actions(
        &mut self,
        account_id: AccountId,
        actions: &[crate::CompleteSetInventoryAction],
    ) -> Result<(), SequencerError> {
        for action in actions {
            if action.collateralize {
                self.collateralize_complete_set(account_id, action.market_id, action.quantity)?;
            } else {
                self.redeem_complete_set(account_id, action.market_id, action.quantity)?;
            }
        }
        Ok(())
    }

    pub fn collateralize_complete_set(
        &mut self,
        account_id: AccountId,
        market_id: MarketId,
        quantity: Qty,
    ) -> Result<(), SequencerError> {
        self.apply_complete_set_action(account_id, market_id, quantity, true)
    }

    pub fn redeem_complete_set(
        &mut self,
        account_id: AccountId,
        market_id: MarketId,
        quantity: Qty,
    ) -> Result<(), SequencerError> {
        self.apply_complete_set_action(account_id, market_id, quantity, false)
    }

    fn apply_complete_set_action(
        &mut self,
        account_id: AccountId,
        market_id: MarketId,
        quantity: Qty,
        collateralize: bool,
    ) -> Result<(), SequencerError> {
        if quantity == Qty::ZERO {
            return Err(SequencerError::CompleteSetInvalidQuantity);
        }
        if self.markets.get(market_id).is_none() {
            return Err(SequencerError::MarketNotFound);
        }
        if !self.market_status(market_id).is_tradeable() {
            return Err(SequencerError::InvalidMarketState(format!(
                "market {} is not tradeable",
                market_id.0
            )));
        }
        if !self.liquidity_universe.permits(market_id) {
            return Err(SequencerError::InvalidMarketState(format!(
                "market {} is not in active liquidity universe generation {}",
                market_id.0, self.liquidity_universe.generation
            )));
        }
        let delta = if collateralize {
            collateralize_complete_set(quantity)
        } else {
            redeem_complete_set(quantity)
        }
        .map_err(|_| SequencerError::CompleteSetArithmetic)?;

        let account = self
            .accounts
            .get(account_id)
            .ok_or(SequencerError::CompleteSetAccountNotFound)?;
        if collateralize && account.balance < -delta.balance_delta {
            return Err(SequencerError::CompleteSetInsufficientCash {
                required: -delta.balance_delta,
                available: account.balance,
            });
        }
        if !collateralize {
            let required =
                i64::try_from(quantity.0).map_err(|_| SequencerError::CompleteSetArithmetic)?;
            let yes_available = account.position(market_id, 0);
            let no_available = account.position(market_id, 1);
            if yes_available < required || no_available < required {
                return Err(SequencerError::CompleteSetInsufficientInventory {
                    required,
                    yes_available,
                    no_available,
                });
            }
        }

        self.capture_system_account_baseline(account_id);
        let account = self
            .accounts
            .get_mut(account_id)
            .expect("account existence checked above");
        account.balance = account
            .balance
            .checked_add(delta.balance_delta)
            .ok_or(SequencerError::CompleteSetArithmetic)?;
        for (outcome, position_delta) in [(0, delta.yes_delta), (1, delta.no_delta)] {
            let position = account.positions.entry((market_id, outcome)).or_insert(0);
            *position = position
                .checked_add(position_delta)
                .ok_or(SequencerError::CompleteSetArithmetic)?;
            if *position == 0 {
                account.positions.remove(&(market_id, outcome));
            }
        }

        let event = if collateralize {
            SystemEvent::CompleteSetCollateralized {
                account_id,
                market_id,
                quantity: quantity.0,
            }
        } else {
            SystemEvent::CompleteSetRedeemed {
                account_id,
                market_id,
                quantity: quantity.0,
            }
        };
        self.record_system_event(event);
        Ok(())
    }
}
