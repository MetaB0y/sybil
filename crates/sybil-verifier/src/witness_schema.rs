//! Canonical full-witness byte schema used by `witness_root`.

use std::collections::HashMap;
use std::str;

use matching_engine::{
    ConditionDir, Fill, MarketGroup, MarketId, MmConstraint, MmId, MmSide, Nanos, Order,
    OrderDirection, PriceCondition, Qty, MAX_MARKETS_PER_ORDER, MAX_STATES,
};

use crate::event_schema::{
    append_key_record, fill_leaf_value, order_accepted_leaf_value, order_rejected_leaf_value,
    system_event_leaf_value,
};
use crate::snapshot_schema::{
    append_i64, append_market_id, append_string, append_u32, append_u64, append_witness_account,
    append_witness_pre_state_sidecar, append_witness_state_sidecar,
};
use crate::types::{
    AccountReservationSnapshot, AccountSnapshot, BlockWitness, BridgeStateSnapshot,
    ChallengeSnapshot, DepositAccumulatorWitness, KeyOpAuth, KeyRecord, L1DepositWitness,
    MarketGroupSnapshot, MarketSnapshot, MarketStatusSnapshot, OracleSourceSnapshot,
    RejectionReason, ResolutionProposalSnapshot, ResolutionRecordSnapshot, RestingOrderSnapshot,
    StateSidecarSnapshot, SystemEventWitness, WithdrawalRefundReasonWitness, WithdrawalSnapshot,
    WitnessBlockHeader, WitnessOrder, WitnessRejection,
};

pub const WITNESS_FORMAT_VERSION: u8 = 6;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WitnessDecodeError {
    #[error("unexpected EOF at offset {offset}: needed {needed} bytes, remaining {remaining}")]
    UnexpectedEof {
        offset: usize,
        needed: usize,
        remaining: usize,
    },
    #[error("trailing bytes after canonical witness at offset {offset}: {trailing} bytes")]
    TrailingBytes { offset: usize, trailing: usize },
    #[error("unknown witness format version {0}; only v6 is supported")]
    UnknownVersion(u8),
    #[error("invalid tag for {field} at offset {offset}: {tag}")]
    InvalidTag {
        field: &'static str,
        tag: u8,
        offset: usize,
    },
    #[error("domain mismatch for {field} at offset {offset}")]
    DomainMismatch { field: &'static str, offset: usize },
    #[error("invalid UTF-8 for {field} at offset {offset}")]
    InvalidUtf8 { field: &'static str, offset: usize },
    #[error("count too large for {field}: {count}")]
    CountTooLarge { field: &'static str, count: u64 },
    #[error("invalid value for {field} at offset {offset}: {details}")]
    InvalidValue {
        field: &'static str,
        offset: usize,
        details: &'static str,
    },
    #[error("decoded witness is not in canonical byte order")]
    NonCanonical,
}

pub fn decode_canonical_witness_bytes(bytes: &[u8]) -> Result<BlockWitness, WitnessDecodeError> {
    let mut reader = WitnessReader::new(bytes);
    let version = reader.read_u8()?;
    if version != WITNESS_FORMAT_VERSION {
        return Err(WitnessDecodeError::UnknownVersion(version));
    }

    let header = reader.read_header()?;
    let previous_header = match reader.read_tag("previous_header")? {
        0 => None,
        1 => Some(reader.read_header()?),
        tag => {
            return Err(WitnessDecodeError::InvalidTag {
                field: "previous_header",
                tag,
                offset: reader.offset.saturating_sub(1),
            })
        }
    };

    let orders = reader.read_vec("orders", |reader| reader.read_witness_order())?;
    let rejections = reader.read_vec("rejections", |reader| reader.read_witness_rejection())?;
    let system_events = reader.read_vec("system_events", |reader| reader.read_system_event())?;
    let deposit_accumulator = reader.read_deposit_accumulator()?;
    let fills = reader.read_vec("fills", |reader| reader.read_fill())?;
    let clearing_prices = reader.read_clearing_prices()?;
    let total_welfare = reader.read_i64()?;
    let minting_cost = reader.read_i64()?;
    let mm_constraints = reader.read_vec("mm_constraints", |reader| reader.read_mm_constraint())?;
    let market_groups = reader.read_vec("market_groups", |reader| reader.read_market_group())?;
    let pre_state = reader.read_vec("pre_state", |reader| reader.read_witness_account())?;
    let post_system_state =
        reader.read_vec("post_system_state", |reader| reader.read_witness_account())?;
    let post_state = reader.read_vec("post_state", |reader| reader.read_witness_account())?;
    let account_keys = reader.read_account_keys()?;
    let state_sidecar =
        reader.read_state_sidecar(b"sybil/witness/state-sidecar", "state_sidecar")?;
    let pre_state_sidecar =
        reader.read_state_sidecar(b"sybil/witness/pre-state-sidecar", "pre_state_sidecar")?;
    let resolved_markets = reader.read_vec("resolved_markets", |reader| reader.read_market_id())?;

    if !reader.is_finished() {
        return Err(WitnessDecodeError::TrailingBytes {
            offset: reader.offset,
            trailing: bytes.len() - reader.offset,
        });
    }

    let witness = BlockWitness {
        header,
        previous_header,
        orders,
        rejections,
        system_events,
        deposit_accumulator,
        fills,
        clearing_prices,
        total_welfare,
        minting_cost,
        mm_constraints,
        market_groups,
        pre_state,
        post_system_state,
        post_state,
        account_keys,
        state_sidecar,
        pre_state_sidecar,
        resolved_markets,
    };

    if canonical_witness_bytes(&witness) != bytes {
        return Err(WitnessDecodeError::NonCanonical);
    }

    Ok(witness)
}

struct WitnessReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> WitnessReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn is_finished(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn read_exact<const N: usize>(&mut self) -> Result<[u8; N], WitnessDecodeError> {
        if self.bytes.len().saturating_sub(self.offset) < N {
            return Err(WitnessDecodeError::UnexpectedEof {
                offset: self.offset,
                needed: N,
                remaining: self.bytes.len().saturating_sub(self.offset),
            });
        }
        let mut out = [0u8; N];
        out.copy_from_slice(&self.bytes[self.offset..self.offset + N]);
        self.offset += N;
        Ok(out)
    }

    fn read_domain(
        &mut self,
        expected: &'static [u8],
        field: &'static str,
    ) -> Result<(), WitnessDecodeError> {
        let offset = self.offset;
        if self.bytes.len().saturating_sub(self.offset) < expected.len() {
            return Err(WitnessDecodeError::UnexpectedEof {
                offset: self.offset,
                needed: expected.len(),
                remaining: self.bytes.len().saturating_sub(self.offset),
            });
        }
        let actual = &self.bytes[self.offset..self.offset + expected.len()];
        if actual != expected {
            return Err(WitnessDecodeError::DomainMismatch { field, offset });
        }
        self.offset += expected.len();
        Ok(())
    }

    fn read_u8(&mut self) -> Result<u8, WitnessDecodeError> {
        Ok(self.read_exact::<1>()?[0])
    }

    fn read_tag(&mut self, _field: &'static str) -> Result<u8, WitnessDecodeError> {
        self.read_u8()
    }

    fn read_u32(&mut self) -> Result<u32, WitnessDecodeError> {
        Ok(u32::from_le_bytes(self.read_exact::<4>()?))
    }

    fn read_u64(&mut self) -> Result<u64, WitnessDecodeError> {
        Ok(u64::from_le_bytes(self.read_exact::<8>()?))
    }

    fn read_i64(&mut self) -> Result<i64, WitnessDecodeError> {
        Ok(i64::from_le_bytes(self.read_exact::<8>()?))
    }

    fn read_hash32(&mut self) -> Result<[u8; 32], WitnessDecodeError> {
        self.read_exact::<32>()
    }

    fn read_pubkey33(&mut self) -> Result<[u8; 33], WitnessDecodeError> {
        self.read_exact::<33>()
    }

    fn read_signature64(&mut self) -> Result<[u8; 64], WitnessDecodeError> {
        self.read_exact::<64>()
    }

    fn read_address20(&mut self) -> Result<[u8; 20], WitnessDecodeError> {
        self.read_exact::<20>()
    }

    fn read_market_id(&mut self) -> Result<MarketId, WitnessDecodeError> {
        Ok(MarketId(self.read_u32()?))
    }

    fn read_string(&mut self, field: &'static str) -> Result<String, WitnessDecodeError> {
        let len = self.read_len(field)?;
        if self.bytes.len().saturating_sub(self.offset) < len {
            return Err(WitnessDecodeError::UnexpectedEof {
                offset: self.offset,
                needed: len,
                remaining: self.bytes.len().saturating_sub(self.offset),
            });
        }
        let offset = self.offset;
        let bytes = &self.bytes[self.offset..self.offset + len];
        self.offset += len;
        str::from_utf8(bytes)
            .map(|value| value.to_owned())
            .map_err(|_| WitnessDecodeError::InvalidUtf8 { field, offset })
    }

    fn read_option_string(
        &mut self,
        field: &'static str,
    ) -> Result<Option<String>, WitnessDecodeError> {
        let offset = self.offset;
        match self.read_tag(field)? {
            0 => Ok(None),
            1 => self.read_string(field).map(Some),
            tag => Err(WitnessDecodeError::InvalidTag { field, tag, offset }),
        }
    }

    fn read_len(&mut self, field: &'static str) -> Result<usize, WitnessDecodeError> {
        let count = self.read_u64()?;
        usize::try_from(count).map_err(|_| WitnessDecodeError::CountTooLarge { field, count })
    }

    fn read_bytes(
        &mut self,
        field: &'static str,
        max_len: usize,
    ) -> Result<Vec<u8>, WitnessDecodeError> {
        let offset = self.offset;
        let len = self.read_len(field)?;
        if len > max_len {
            return Err(WitnessDecodeError::InvalidValue {
                field,
                offset,
                details: "length exceeds protocol cap",
            });
        }
        if self.bytes.len().saturating_sub(self.offset) < len {
            return Err(WitnessDecodeError::UnexpectedEof {
                offset: self.offset,
                needed: len,
                remaining: self.bytes.len().saturating_sub(self.offset),
            });
        }
        let bytes = self.bytes[self.offset..self.offset + len].to_vec();
        self.offset += len;
        Ok(bytes)
    }

    fn read_vec<T>(
        &mut self,
        field: &'static str,
        mut read_item: impl FnMut(&mut Self) -> Result<T, WitnessDecodeError>,
    ) -> Result<Vec<T>, WitnessDecodeError> {
        let count = self.read_len(field)?;
        let mut out = Vec::new();
        for _ in 0..count {
            out.push(read_item(self)?);
        }
        Ok(out)
    }

    fn read_header(&mut self) -> Result<WitnessBlockHeader, WitnessDecodeError> {
        Ok(WitnessBlockHeader {
            height: self.read_u64()?,
            parent_hash: self.read_hash32()?,
            state_root: self.read_hash32()?,
            events_root: self.read_hash32()?,
            order_count: self.read_u32()?,
            fill_count: self.read_u32()?,
            timestamp_ms: self.read_u64()?,
        })
    }

    fn read_order(&mut self) -> Result<Order, WitnessDecodeError> {
        let id = self.read_u64()?;
        let num_markets = self.read_u8()?;
        if usize::from(num_markets) > MAX_MARKETS_PER_ORDER {
            return Err(WitnessDecodeError::InvalidValue {
                field: "order.num_markets",
                offset: self.offset.saturating_sub(1),
                details: "exceeds MAX_MARKETS_PER_ORDER",
            });
        }
        let mut markets = [MarketId::NONE; MAX_MARKETS_PER_ORDER];
        for slot in markets.iter_mut().take(num_markets as usize) {
            *slot = self.read_market_id()?;
        }

        let num_states = self.read_u8()?;
        if usize::from(num_states) > MAX_STATES {
            return Err(WitnessDecodeError::InvalidValue {
                field: "order.num_states",
                offset: self.offset.saturating_sub(1),
                details: "exceeds MAX_STATES",
            });
        }
        let mut payoffs = [0i8; MAX_STATES];
        for slot in payoffs.iter_mut().take(num_states as usize) {
            *slot = self.read_u8()? as i8;
        }

        let limit_price = Nanos(self.read_u64()?);
        let max_fill = Qty(self.read_u64()?);
        let condition_offset = self.offset;
        let condition = match self.read_tag("order.condition")? {
            0 => None,
            1 => {
                let market = self.read_market_id()?;
                let threshold = Nanos(self.read_u64()?);
                let dir_offset = self.offset;
                let direction = match self.read_tag("condition.direction")? {
                    0 => ConditionDir::Above,
                    1 => ConditionDir::Below,
                    tag => {
                        return Err(WitnessDecodeError::InvalidTag {
                            field: "condition.direction",
                            tag,
                            offset: dir_offset,
                        })
                    }
                };
                Some(PriceCondition {
                    market,
                    threshold,
                    direction,
                })
            }
            tag => {
                return Err(WitnessDecodeError::InvalidTag {
                    field: "order.condition",
                    tag,
                    offset: condition_offset,
                })
            }
        };
        let expires_offset = self.offset;
        let expires_at_block = match self.read_tag("order.expires_at_block")? {
            0 => None,
            1 => Some(self.read_u64()?),
            tag => {
                return Err(WitnessDecodeError::InvalidTag {
                    field: "order.expires_at_block",
                    tag,
                    offset: expires_offset,
                })
            }
        };

        Ok(Order {
            id,
            markets,
            num_markets,
            payoffs,
            num_states,
            limit_price,
            max_fill,
            condition,
            expires_at_block,
        })
    }

    fn read_witness_order(&mut self) -> Result<WitnessOrder, WitnessDecodeError> {
        self.read_domain(b"sybil/event/order-accepted", "order_accepted")?;
        let account_id = self.read_u64()?;
        let is_mm_offset = self.offset;
        let is_mm = match self.read_tag("order_accepted.is_mm")? {
            0 => false,
            1 => true,
            tag => {
                return Err(WitnessDecodeError::InvalidTag {
                    field: "order_accepted.is_mm",
                    tag,
                    offset: is_mm_offset,
                })
            }
        };
        Ok(WitnessOrder {
            order: self.read_order()?,
            account_id,
            is_mm,
        })
    }

    fn read_witness_rejection(&mut self) -> Result<WitnessRejection, WitnessDecodeError> {
        self.read_domain(b"sybil/event/order-rejected", "order_rejected")?;
        let account_id = self.read_u64()?;
        let order = self.read_order()?;
        let reason = self.read_rejection_reason()?;
        Ok(WitnessRejection {
            order,
            account_id,
            reason,
        })
    }

    fn read_rejection_reason(&mut self) -> Result<RejectionReason, WitnessDecodeError> {
        let offset = self.offset;
        match self.read_tag("rejection_reason")? {
            0 => Ok(RejectionReason::InsufficientBalance {
                required: self.read_i64()?,
                available: self.read_i64()?,
            }),
            1 => Ok(RejectionReason::InsufficientPosition {
                market: self.read_market_id()?,
                outcome: self.read_u8()?,
                required: self.read_i64()?,
                available: self.read_i64()?,
            }),
            2 => Ok(RejectionReason::AccountNotFound),
            3 => Ok(RejectionReason::CompleteSetFormation),
            4 => Ok(RejectionReason::Expired {
                current_block: self.read_u64()?,
                expires_at_block: self.read_u64()?,
            }),
            5 => Ok(RejectionReason::InvalidOrder(
                self.read_u32_len_string("rejection_reason.invalid_order")?,
            )),
            tag => Err(WitnessDecodeError::InvalidTag {
                field: "rejection_reason",
                tag,
                offset,
            }),
        }
    }

    fn read_u32_len_string(&mut self, field: &'static str) -> Result<String, WitnessDecodeError> {
        let len = self.read_u32()? as usize;
        if self.bytes.len().saturating_sub(self.offset) < len {
            return Err(WitnessDecodeError::UnexpectedEof {
                offset: self.offset,
                needed: len,
                remaining: self.bytes.len().saturating_sub(self.offset),
            });
        }
        let offset = self.offset;
        let bytes = &self.bytes[self.offset..self.offset + len];
        self.offset += len;
        str::from_utf8(bytes)
            .map(|value| value.to_owned())
            .map_err(|_| WitnessDecodeError::InvalidUtf8 { field, offset })
    }

    fn read_system_event(&mut self) -> Result<SystemEventWitness, WitnessDecodeError> {
        self.read_domain(b"sybil/event/system", "system_event")?;
        let offset = self.offset;
        match self.read_tag("system_event")? {
            0 => Ok(SystemEventWitness::CreateAccount {
                account_id: self.read_u64()?,
                initial_balance: self.read_i64()?,
                initial_keys: self.read_key_records("system_event.initial_keys")?,
            }),
            1 => Ok(SystemEventWitness::Deposit {
                account_id: self.read_u64()?,
                amount: self.read_i64()?,
            }),
            2 => Ok(SystemEventWitness::L1Deposit {
                account_id: self.read_u64()?,
                amount: self.read_i64()?,
                deposit_id: self.read_u64()?,
                deposit_root: self.read_hash32()?,
                sybil_account_key: self.read_hash32()?,
            }),
            3 => Ok(SystemEventWitness::WithdrawalCreated {
                account_id: self.read_u64()?,
                amount: self.read_i64()?,
                withdrawal_id: self.read_u64()?,
                recipient: self.read_address20()?,
                token: self.read_address20()?,
                amount_token_units: self.read_u64()?,
                expiry_height: self.read_u64()?,
                nullifier: self.read_hash32()?,
            }),
            4 => Ok(SystemEventWitness::MarketResolved {
                market_id: self.read_market_id()?,
                payout_nanos: Nanos(self.read_u64()?),
                affected_accounts: self
                    .read_vec("system_event.affected_accounts", |reader| reader.read_u64())?,
            }),
            5 => Ok(SystemEventWitness::OrderCancelled {
                account_id: self.read_u64()?,
                order_id: self.read_u64()?,
                market_ids: self
                    .read_vec("system_event.market_ids", |reader| reader.read_market_id())?,
                side: self.read_order_direction()?,
                remaining_quantity: self.read_u64()?,
            }),
            6 => Ok(SystemEventWitness::MarketGroupExtended {
                group_id: self.read_u64()?,
                market_id: self.read_market_id()?,
            }),
            7 => {
                let account_id = self.read_u64()?;
                let withdrawal_id = self.read_u64()?;
                let amount = self.read_i64()?;
                let reason_offset = self.offset;
                let reason = match self.read_tag("withdrawal_refund_reason")? {
                    0 => WithdrawalRefundReasonWitness::L1Cancelled,
                    1 => WithdrawalRefundReasonWitness::L1Expired {
                        observed_l1_height: self.read_u64()?,
                    },
                    tag => {
                        return Err(WitnessDecodeError::InvalidTag {
                            field: "withdrawal_refund_reason",
                            tag,
                            offset: reason_offset,
                        })
                    }
                };
                Ok(SystemEventWitness::WithdrawalRefunded {
                    account_id,
                    withdrawal_id,
                    amount,
                    reason,
                })
            }
            8 => Ok(SystemEventWitness::WithdrawalFinalized {
                account_id: self.read_u64()?,
                withdrawal_id: self.read_u64()?,
                amount: self.read_i64()?,
            }),
            9 => Ok(SystemEventWitness::L1BlockObserved {
                height: self.read_u64()?,
            }),
            10 => Ok(SystemEventWitness::KeyRegistered {
                account_id: self.read_u64()?,
                key: self.read_key_record()?,
                authorization: self.read_key_op_auth()?,
            }),
            11 => Ok(SystemEventWitness::KeyRevoked {
                account_id: self.read_u64()?,
                key: self.read_key_record()?,
                authorization: self.read_key_op_auth()?,
            }),
            tag => Err(WitnessDecodeError::InvalidTag {
                field: "system_event",
                tag,
                offset,
            }),
        }
    }

    fn read_key_record(&mut self) -> Result<KeyRecord, WitnessDecodeError> {
        Ok(KeyRecord {
            auth_scheme: self.read_u8()?,
            pubkey_sec1: self.read_pubkey33()?,
            capability_mask: self.read_u32()?,
        })
    }

    fn read_key_records(
        &mut self,
        field: &'static str,
    ) -> Result<Vec<KeyRecord>, WitnessDecodeError> {
        let keys = self.read_vec(field, |reader| reader.read_key_record())?;
        if keys.len() > crate::MAX_KEYS_PER_ACCOUNT {
            return Err(WitnessDecodeError::InvalidValue {
                field,
                offset: self.offset,
                details: "key count exceeds MAX_KEYS_PER_ACCOUNT",
            });
        }
        Ok(keys)
    }

    fn read_key_op_auth(&mut self) -> Result<KeyOpAuth, WitnessDecodeError> {
        let offset = self.offset;
        match self.read_tag("key_op_auth")? {
            0 => Ok(KeyOpAuth::RawP256 {
                signer_pubkey: self.read_pubkey33()?,
                signature: self.read_signature64()?,
            }),
            1 => Ok(KeyOpAuth::WebAuthn {
                signer_pubkey: self.read_pubkey33()?,
                authenticator_data: self.read_bytes(
                    "key_op_auth.authenticator_data",
                    crate::MAX_WEBAUTHN_AUTHENTICATOR_DATA_BYTES,
                )?,
                client_data_json: self.read_bytes(
                    "key_op_auth.client_data_json",
                    crate::MAX_WEBAUTHN_CLIENT_DATA_JSON_BYTES,
                )?,
                signature: self.read_signature64()?,
            }),
            tag => Err(WitnessDecodeError::InvalidTag {
                field: "key_op_auth",
                tag,
                offset,
            }),
        }
    }

    fn read_order_direction(&mut self) -> Result<OrderDirection, WitnessDecodeError> {
        let offset = self.offset;
        match self.read_tag("order_direction")? {
            0 => Ok(OrderDirection::BuyYes),
            1 => Ok(OrderDirection::SellYes),
            2 => Ok(OrderDirection::BuyNo),
            3 => Ok(OrderDirection::SellNo),
            tag => Err(WitnessDecodeError::InvalidTag {
                field: "order_direction",
                tag,
                offset,
            }),
        }
    }

    fn read_deposit_accumulator(
        &mut self,
    ) -> Result<DepositAccumulatorWitness, WitnessDecodeError> {
        self.read_domain(b"sybil/witness/deposit-accumulator", "deposit_accumulator")?;
        let mut pre_frontier = [[0u8; 32]; sybil_l1_protocol::DEPOSIT_TREE_DEPTH];
        for hash in &mut pre_frontier {
            *hash = self.read_hash32()?;
        }
        let pre_count = self.read_u64()?;
        let new_deposits = self.read_vec("deposit_accumulator.new_deposits", |reader| {
            reader.read_l1_deposit()
        })?;
        Ok(DepositAccumulatorWitness {
            pre_frontier,
            pre_count,
            new_deposits,
        })
    }

    fn read_l1_deposit(&mut self) -> Result<L1DepositWitness, WitnessDecodeError> {
        self.read_domain(b"sybil/witness/l1-deposit", "l1_deposit")?;
        Ok(L1DepositWitness {
            deposit_id: self.read_u64()?,
            chain_id: self.read_u64()?,
            vault_address: self.read_address20()?,
            token_address: self.read_address20()?,
            sender: self.read_address20()?,
            sybil_account_key: self.read_hash32()?,
            amount_token_units: self.read_u64()?,
            deposit_root: self.read_hash32()?,
        })
    }

    fn read_fill(&mut self) -> Result<Fill, WitnessDecodeError> {
        self.read_domain(b"sybil/event/fill", "fill")?;
        Ok(Fill {
            order_id: self.read_u64()?,
            fill_qty: Qty(self.read_u64()?),
            fill_price: Nanos(self.read_u64()?),
            account_id: self.read_u64()?,
        })
    }

    fn read_clearing_prices(
        &mut self,
    ) -> Result<HashMap<MarketId, Vec<Nanos>>, WitnessDecodeError> {
        let entries = self.read_vec("clearing_prices", |reader| {
            let market = reader.read_market_id()?;
            let outcome_count = reader.read_u32()? as usize;
            let mut outcomes = Vec::new();
            for _ in 0..outcome_count {
                outcomes.push(Nanos(reader.read_u64()?));
            }
            Ok((market, outcomes))
        })?;
        Ok(entries.into_iter().collect())
    }

    fn read_mm_constraint(&mut self) -> Result<MmConstraint, WitnessDecodeError> {
        let mm_id = MmId(self.read_u64()?);
        let max_capital = Nanos(self.read_u64()?);
        let order_ids = self.read_vec("mm_constraint.order_ids", |reader| reader.read_u64())?;
        let sides = self.read_vec("mm_constraint.order_sides", |reader| {
            let order_id = reader.read_u64()?;
            let side = reader.read_mm_side()?;
            Ok((order_id, side))
        })?;
        Ok(MmConstraint {
            mm_id,
            max_capital,
            order_ids,
            order_sides: sides.into_iter().collect(),
        })
    }

    fn read_mm_side(&mut self) -> Result<MmSide, WitnessDecodeError> {
        let offset = self.offset;
        match self.read_tag("mm_side")? {
            0 => Ok(MmSide::SellYes),
            1 => Ok(MmSide::BuyYes),
            2 => Ok(MmSide::SellNo),
            3 => Ok(MmSide::BuyNo),
            tag => Err(WitnessDecodeError::InvalidTag {
                field: "mm_side",
                tag,
                offset,
            }),
        }
    }

    fn read_market_group(&mut self) -> Result<MarketGroup, WitnessDecodeError> {
        Ok(MarketGroup {
            name: self.read_string("market_group.name")?,
            markets: self.read_vec("market_group.markets", |reader| reader.read_market_id())?,
        })
    }

    fn read_witness_account(&mut self) -> Result<AccountSnapshot, WitnessDecodeError> {
        self.read_domain(b"sybil/witness/account", "account")?;
        Ok(AccountSnapshot {
            id: self.read_u64()?,
            balance: self.read_i64()?,
            total_deposited: self.read_i64()?,
            positions: self.read_positions("account.positions")?,
            events_digest: self.read_hash32()?,
            keys_digest: self.read_hash32()?,
        })
    }

    fn read_account_keys(&mut self) -> Result<Vec<(u64, Vec<KeyRecord>)>, WitnessDecodeError> {
        self.read_domain(b"sybil/witness/account-keys", "account_keys")?;
        self.read_vec("account_keys", |reader| {
            let account_id = reader.read_u64()?;
            let keys = reader.read_key_records("account_keys.keys")?;
            Ok((account_id, keys))
        })
    }

    fn read_positions(
        &mut self,
        field: &'static str,
    ) -> Result<Vec<(MarketId, u8, i64)>, WitnessDecodeError> {
        self.read_vec(field, |reader| {
            Ok((
                reader.read_market_id()?,
                reader.read_u8()?,
                reader.read_i64()?,
            ))
        })
    }

    fn read_state_sidecar(
        &mut self,
        domain: &'static [u8],
        field: &'static str,
    ) -> Result<StateSidecarSnapshot, WitnessDecodeError> {
        self.read_domain(domain, field)?;
        Ok(StateSidecarSnapshot {
            bridge: self.read_bridge()?,
            markets: self.read_vec("sidecar.markets", |reader| reader.read_market_snapshot())?,
            market_groups: self.read_vec("sidecar.market_groups", |reader| {
                reader.read_market_group_snapshot()
            })?,
            resting_orders: self.read_vec("sidecar.resting_orders", |reader| {
                reader.read_resting_order()
            })?,
            account_reservations: self.read_vec("sidecar.account_reservations", |reader| {
                reader.read_account_reservation()
            })?,
        })
    }

    fn read_bridge(&mut self) -> Result<BridgeStateSnapshot, WitnessDecodeError> {
        Ok(BridgeStateSnapshot {
            deposit_cursor: self.read_u64()?,
            deposit_root: self.read_hash32()?,
            observed_l1_height: self.read_u64()?,
            next_withdrawal_id: self.read_u64()?,
            withdrawals: self.read_vec("bridge.withdrawals", |reader| reader.read_withdrawal())?,
        })
    }

    fn read_withdrawal(&mut self) -> Result<WithdrawalSnapshot, WitnessDecodeError> {
        Ok(WithdrawalSnapshot {
            withdrawal_id: self.read_u64()?,
            account_id: self.read_u64()?,
            recipient: self.read_address20()?,
            token: self.read_address20()?,
            amount_token_units: self.read_u64()?,
            amount_nanos: self.read_u64()?,
            expiry_height: self.read_u64()?,
            nullifier: self.read_hash32()?,
        })
    }

    fn read_market_snapshot(&mut self) -> Result<MarketSnapshot, WitnessDecodeError> {
        Ok(MarketSnapshot {
            market_id: self.read_market_id()?,
            name: self.read_string("market.name")?,
            num_outcomes: self.read_u8()?,
            status: self.read_market_status()?,
            metadata_digest: self.read_hash32()?,
            resolution_template: self.read_string("market.resolution_template")?,
        })
    }

    fn read_market_group_snapshot(&mut self) -> Result<MarketGroupSnapshot, WitnessDecodeError> {
        Ok(MarketGroupSnapshot {
            group_id: self.read_u64()?,
            name: self.read_string("sidecar.market_group.name")?,
            markets: self.read_vec("sidecar.market_group.markets", |reader| {
                reader.read_market_id()
            })?,
        })
    }

    fn read_market_status(&mut self) -> Result<MarketStatusSnapshot, WitnessDecodeError> {
        let offset = self.offset;
        match self.read_tag("market.status")? {
            0 => Ok(MarketStatusSnapshot::Active),
            1 => Ok(MarketStatusSnapshot::Proposed {
                proposal: self.read_resolution_proposal()?,
                challenge_deadline_ms: self.read_u64()?,
            }),
            2 => Ok(MarketStatusSnapshot::Challenged {
                proposal: self.read_resolution_proposal()?,
                challenge: self.read_challenge()?,
            }),
            3 => Ok(MarketStatusSnapshot::Resolved {
                record: self.read_resolution_record()?,
            }),
            4 => Ok(MarketStatusSnapshot::Voided),
            tag => Err(WitnessDecodeError::InvalidTag {
                field: "market.status",
                tag,
                offset,
            }),
        }
    }

    fn read_resolution_proposal(
        &mut self,
    ) -> Result<ResolutionProposalSnapshot, WitnessDecodeError> {
        Ok(ResolutionProposalSnapshot {
            id: self.read_u64()?,
            market_id: self.read_market_id()?,
            payout_nanos: Nanos(self.read_u64()?),
            source: self.read_oracle_source()?,
            proposed_at_ms: self.read_u64()?,
            reason: self.read_option_string("resolution_proposal.reason")?,
        })
    }

    fn read_challenge(&mut self) -> Result<ChallengeSnapshot, WitnessDecodeError> {
        Ok(ChallengeSnapshot {
            id: self.read_u64()?,
            challenger: self.read_u64()?,
            proposal_id: self.read_u64()?,
            bond_amount: Nanos(self.read_u64()?),
            proposed_payout_nanos: Nanos(self.read_u64()?),
            reason: self.read_string("challenge.reason")?,
            challenged_at_ms: self.read_u64()?,
        })
    }

    fn read_resolution_record(&mut self) -> Result<ResolutionRecordSnapshot, WitnessDecodeError> {
        Ok(ResolutionRecordSnapshot {
            market_id: self.read_market_id()?,
            payout_nanos: Nanos(self.read_u64()?),
            resolved_by: self.read_oracle_source()?,
            resolved_at_ms: self.read_u64()?,
            proposal: self.read_option("resolution_record.proposal", |reader| {
                reader.read_resolution_proposal()
            })?,
            challenge: self.read_option("resolution_record.challenge", |reader| {
                reader.read_challenge()
            })?,
        })
    }

    fn read_option<T>(
        &mut self,
        field: &'static str,
        read_item: impl FnOnce(&mut Self) -> Result<T, WitnessDecodeError>,
    ) -> Result<Option<T>, WitnessDecodeError> {
        let offset = self.offset;
        match self.read_tag(field)? {
            0 => Ok(None),
            1 => read_item(self).map(Some),
            tag => Err(WitnessDecodeError::InvalidTag { field, tag, offset }),
        }
    }

    fn read_oracle_source(&mut self) -> Result<OracleSourceSnapshot, WitnessDecodeError> {
        let offset = self.offset;
        match self.read_tag("oracle_source")? {
            0 => Ok(OracleSourceSnapshot::Admin),
            1 => Ok(OracleSourceSnapshot::DataFeed(self.read_u64()?)),
            2 => Ok(OracleSourceSnapshot::AutomatedL0),
            tag => Err(WitnessDecodeError::InvalidTag {
                field: "oracle_source",
                tag,
                offset,
            }),
        }
    }

    fn read_resting_order(&mut self) -> Result<RestingOrderSnapshot, WitnessDecodeError> {
        Ok(RestingOrderSnapshot {
            order: self.read_order()?,
            account_id: self.read_u64()?,
            created_at: self.read_u64()?,
            expires_at_block: self.read_u64()?,
            reserved_balance: self.read_i64()?,
            reserved_positions: self.read_positions("resting_order.reserved_positions")?,
        })
    }

    fn read_account_reservation(
        &mut self,
    ) -> Result<AccountReservationSnapshot, WitnessDecodeError> {
        Ok(AccountReservationSnapshot {
            account_id: self.read_u64()?,
            reserved_balance: self.read_i64()?,
            reserved_positions: self.read_positions("account_reservation.reserved_positions")?,
        })
    }
}

pub fn canonical_witness_bytes(witness: &BlockWitness) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(WITNESS_FORMAT_VERSION);
    append_header(&mut out, &witness.header);
    match &witness.previous_header {
        Some(previous) => {
            out.push(1);
            append_header(&mut out, previous);
        }
        None => out.push(0),
    }

    let mut orders: Vec<_> = witness.orders.iter().collect();
    orders.sort_by_key(|order| order.order.id);
    append_u64(&mut out, orders.len() as u64);
    for order in orders {
        out.extend_from_slice(&order_accepted_leaf_value(order));
    }

    let mut rejections: Vec<_> = witness.rejections.iter().collect();
    rejections.sort_by_key(|rejection| rejection.order.id);
    append_u64(&mut out, rejections.len() as u64);
    for rejection in rejections {
        out.extend_from_slice(&order_rejected_leaf_value(rejection));
    }

    append_u64(&mut out, witness.system_events.len() as u64);
    for event in &witness.system_events {
        out.extend_from_slice(&system_event_leaf_value(event));
    }

    append_deposit_accumulator(&mut out, &witness.deposit_accumulator);

    append_u64(&mut out, witness.fills.len() as u64);
    for fill in &witness.fills {
        out.extend_from_slice(&fill_leaf_value(fill));
    }

    append_clearing_prices(&mut out, &witness.clearing_prices);
    append_i64(&mut out, witness.total_welfare);
    append_i64(&mut out, witness.minting_cost);
    append_mm_constraints(&mut out, &witness.mm_constraints);
    append_market_groups(&mut out, &witness.market_groups);
    append_account_section(&mut out, &witness.pre_state);
    append_account_section(&mut out, &witness.post_system_state);
    append_account_section(&mut out, &witness.post_state);
    append_account_keys(&mut out, &witness.account_keys);
    append_witness_state_sidecar(&mut out, &witness.state_sidecar);
    append_witness_pre_state_sidecar(&mut out, &witness.pre_state_sidecar);

    let mut resolved_markets = witness.resolved_markets.clone();
    resolved_markets.sort_by_key(|market| market.0);
    append_u64(&mut out, resolved_markets.len() as u64);
    for market in resolved_markets {
        append_market_id(&mut out, market);
    }

    out
}

fn append_account_keys(out: &mut Vec<u8>, account_keys: &[(u64, Vec<KeyRecord>)]) {
    out.extend_from_slice(b"sybil/witness/account-keys");
    let mut account_keys = account_keys.to_vec();
    account_keys.sort_by_key(|(account_id, _)| *account_id);
    append_u64(out, account_keys.len() as u64);
    for (account_id, mut keys) in account_keys {
        append_u64(out, account_id);
        keys.sort_by_key(KeyRecord::canonical_sort_key);
        append_u64(out, keys.len() as u64);
        for key in &keys {
            append_key_record(out, key);
        }
    }
}

fn append_header(out: &mut Vec<u8>, header: &WitnessBlockHeader) {
    append_u64(out, header.height);
    out.extend_from_slice(&header.parent_hash);
    out.extend_from_slice(&header.state_root);
    out.extend_from_slice(&header.events_root);
    append_u32(out, header.order_count);
    append_u32(out, header.fill_count);
    append_u64(out, header.timestamp_ms);
}

fn append_clearing_prices(out: &mut Vec<u8>, clearing_prices: &HashMap<MarketId, Vec<Nanos>>) {
    let mut prices: Vec<_> = clearing_prices.iter().collect();
    prices.sort_by_key(|(market, _)| market.0);
    append_u64(out, prices.len() as u64);
    for (market, outcomes) in prices {
        append_market_id(out, *market);
        append_u32(out, outcomes.len() as u32);
        for price in outcomes {
            append_u64(out, price.0);
        }
    }
}

fn append_mm_constraints(out: &mut Vec<u8>, constraints: &[MmConstraint]) {
    let mut constraints: Vec<_> = constraints.iter().collect();
    constraints.sort_by_key(|constraint| constraint.mm_id.0);
    append_u64(out, constraints.len() as u64);
    for constraint in constraints {
        append_u64(out, constraint.mm_id.0);
        append_u64(out, constraint.max_capital.0);

        let mut order_ids = constraint.order_ids.clone();
        order_ids.sort_unstable();
        append_u64(out, order_ids.len() as u64);
        for order_id in order_ids {
            append_u64(out, order_id);
        }

        let mut sides: Vec<_> = constraint.order_sides.iter().collect();
        sides.sort_by_key(|(order_id, _)| **order_id);
        append_u64(out, sides.len() as u64);
        for (order_id, side) in sides {
            append_u64(out, *order_id);
            append_mm_side(out, *side);
        }
    }
}

fn append_mm_side(out: &mut Vec<u8>, side: MmSide) {
    out.push(match side {
        MmSide::SellYes => 0,
        MmSide::BuyYes => 1,
        MmSide::SellNo => 2,
        MmSide::BuyNo => 3,
    });
}

fn append_market_groups(out: &mut Vec<u8>, groups: &[MarketGroup]) {
    let mut groups: Vec<_> = groups.iter().collect();
    groups.sort_by(|left, right| {
        let left_first = left
            .markets
            .iter()
            .map(|market| market.0)
            .min()
            .unwrap_or(u32::MAX);
        let right_first = right
            .markets
            .iter()
            .map(|market| market.0)
            .min()
            .unwrap_or(u32::MAX);
        left_first
            .cmp(&right_first)
            .then(left.name.cmp(&right.name))
    });
    append_u64(out, groups.len() as u64);
    for group in groups {
        append_string(out, &group.name);
        let mut markets = group.markets.clone();
        markets.sort_by_key(|market| market.0);
        append_u64(out, markets.len() as u64);
        for market in markets {
            append_market_id(out, market);
        }
    }
}

fn append_account_section(out: &mut Vec<u8>, accounts: &[AccountSnapshot]) {
    let mut accounts: Vec<_> = accounts.iter().collect();
    accounts.sort_by_key(|account| account.id);
    append_u64(out, accounts.len() as u64);
    for account in accounts {
        append_witness_account(out, account);
    }
}

fn append_deposit_accumulator(out: &mut Vec<u8>, accumulator: &DepositAccumulatorWitness) {
    out.extend_from_slice(b"sybil/witness/deposit-accumulator");
    for hash in accumulator.pre_frontier {
        out.extend_from_slice(&hash);
    }
    append_u64(out, accumulator.pre_count);
    append_u64(out, accumulator.new_deposits.len() as u64);
    for deposit in &accumulator.new_deposits {
        out.extend_from_slice(b"sybil/witness/l1-deposit");
        append_u64(out, deposit.deposit_id);
        append_u64(out, deposit.chain_id);
        out.extend_from_slice(&deposit.vault_address);
        out.extend_from_slice(&deposit.token_address);
        out.extend_from_slice(&deposit.sender);
        out.extend_from_slice(&deposit.sybil_account_key);
        append_u64(out, deposit.amount_token_units);
        out.extend_from_slice(&deposit.deposit_root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DepositAccumulatorWitness, MarketStatusSnapshot, OracleSourceSnapshot,
        ResolutionRecordSnapshot, StateSidecarSnapshot, WitnessBlockHeader,
    };
    use matching_engine::{ConditionDir, MmId, OrderDirection, PriceCondition};

    #[test]
    fn canonical_witness_bytes_are_stable_for_empty_witness() {
        let witness = BlockWitness {
            header: WitnessBlockHeader {
                height: 1,
                parent_hash: [0u8; 32],
                state_root: [1u8; 32],
                events_root: [2u8; 32],
                order_count: 0,
                fill_count: 0,
                timestamp_ms: 1000,
            },
            previous_header: None,
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            deposit_accumulator: DepositAccumulatorWitness::default(),
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state: vec![],
            account_keys: vec![],
            state_sidecar: StateSidecarSnapshot::default(),
            pre_state_sidecar: StateSidecarSnapshot::default(),
            resolved_markets: vec![],
        };

        let bytes = canonical_witness_bytes(&witness);
        assert_eq!(bytes[0], WITNESS_FORMAT_VERSION);
        assert_eq!(bytes.len(), 1583);
    }

    #[test]
    fn decode_round_trips_empty_witness_bytes() {
        let witness = BlockWitness {
            header: WitnessBlockHeader {
                height: 1,
                parent_hash: [0u8; 32],
                state_root: [1u8; 32],
                events_root: [2u8; 32],
                order_count: 0,
                fill_count: 0,
                timestamp_ms: 1000,
            },
            previous_header: None,
            orders: vec![],
            rejections: vec![],
            system_events: vec![],
            deposit_accumulator: DepositAccumulatorWitness::default(),
            fills: vec![],
            clearing_prices: HashMap::new(),
            total_welfare: 0,
            minting_cost: 0,
            mm_constraints: vec![],
            market_groups: vec![],
            pre_state: vec![],
            post_system_state: vec![],
            post_state: vec![],
            account_keys: vec![],
            state_sidecar: StateSidecarSnapshot::default(),
            pre_state_sidecar: StateSidecarSnapshot::default(),
            resolved_markets: vec![],
        };

        let bytes = canonical_witness_bytes(&witness);
        let decoded = decode_canonical_witness_bytes(&bytes).unwrap();
        assert_eq!(canonical_witness_bytes(&decoded), bytes);
        assert_witness_eq(&decoded, &witness);
    }

    #[test]
    fn decode_round_trips_representative_witness() {
        let witness = representative_witness();
        let bytes = canonical_witness_bytes(&witness);
        let decoded = decode_canonical_witness_bytes(&bytes).unwrap();

        assert_eq!(canonical_witness_bytes(&decoded), bytes);
        assert_witness_eq(&decoded, &witness);
    }

    #[test]
    fn decode_rejects_unknown_version_and_trailing_bytes() {
        let bytes = canonical_witness_bytes(&representative_witness());

        let mut unknown_version = bytes.clone();
        unknown_version[0] = 2;
        assert!(matches!(
            decode_canonical_witness_bytes(&unknown_version),
            Err(WitnessDecodeError::UnknownVersion(2))
        ));

        let mut trailing = bytes;
        trailing.push(0);
        assert!(matches!(
            decode_canonical_witness_bytes(&trailing),
            Err(WitnessDecodeError::TrailingBytes { .. })
        ));
    }

    #[test]
    fn decode_rejects_corrupt_tags_domains_and_truncation_without_panic() {
        let bytes = canonical_witness_bytes(&representative_witness());

        let mut bad_previous_tag = bytes.clone();
        let previous_tag_offset = 1 + 120;
        bad_previous_tag[previous_tag_offset] = 9;
        assert!(matches!(
            decode_canonical_witness_bytes(&bad_previous_tag),
            Err(WitnessDecodeError::InvalidTag {
                field: "previous_header",
                ..
            })
        ));

        let mut bad_domain = bytes.clone();
        let order_domain_offset = 1 + 120 + 1 + 120 + 8;
        bad_domain[order_domain_offset] ^= 0xff;
        assert!(matches!(
            decode_canonical_witness_bytes(&bad_domain),
            Err(WitnessDecodeError::DomainMismatch {
                field: "order_accepted",
                ..
            })
        ));

        for truncated_len in [0, 1, 120, bytes.len() / 2, bytes.len() - 1] {
            let result = std::panic::catch_unwind(|| {
                decode_canonical_witness_bytes(&bytes[..truncated_len])
            });
            assert!(result.is_ok(), "decoder panicked at len {truncated_len}");
            assert!(
                result.unwrap().is_err(),
                "truncated input unexpectedly decoded at len {truncated_len}"
            );
        }
    }

    #[test]
    fn decode_rejects_noncanonical_section_order() {
        let witness = representative_witness();
        let mut bytes = canonical_witness_bytes(&witness);
        let first_account_id_offset = find_subslice(&bytes, b"sybil/witness/account").unwrap()
            + b"sybil/witness/account".len();
        bytes[first_account_id_offset..first_account_id_offset + 8]
            .copy_from_slice(&9999u64.to_le_bytes());

        assert!(matches!(
            decode_canonical_witness_bytes(&bytes),
            Err(WitnessDecodeError::NonCanonical)
        ));
    }

    fn representative_witness() -> BlockWitness {
        let market_a = MarketId::new(3);
        let market_b = MarketId::new(9);
        let accepted_order = fixture_order(7, market_a, market_b, 610_000_000, Some(77));
        let rejected_order = fixture_order(8, market_b, market_a, 455_000_000, None);
        let deposit_accumulator = deposit_accumulator();
        let post_bridge = BridgeStateSnapshot {
            deposit_cursor: 3,
            deposit_root: deposit_accumulator.new_deposits[0].deposit_root,
            observed_l1_height: 11,
            next_withdrawal_id: 4,
            withdrawals: vec![WithdrawalSnapshot {
                withdrawal_id: 3,
                account_id: 1001,
                recipient: [9u8; 20],
                token: [10u8; 20],
                amount_token_units: 123_000,
                amount_nanos: 456_000,
                expiry_height: 99,
                nullifier: [11u8; 32],
            }],
        };
        let pre_bridge = BridgeStateSnapshot {
            deposit_cursor: 2,
            deposit_root: sybil_l1_protocol::deposit_root_from_frontier(
                &deposit_accumulator.pre_frontier,
                deposit_accumulator.pre_count,
            )
            .unwrap(),
            observed_l1_height: 10,
            next_withdrawal_id: 3,
            withdrawals: vec![],
        };
        let state_sidecar = state_sidecar(accepted_order.clone(), post_bridge);
        let pre_state_sidecar = StateSidecarSnapshot {
            bridge: pre_bridge,
            markets: state_sidecar.markets.clone(),
            market_groups: state_sidecar.market_groups.clone(),
            resting_orders: state_sidecar.resting_orders.clone(),
            account_reservations: state_sidecar.account_reservations.clone(),
        };

        let previous_header = WitnessBlockHeader {
            height: 10,
            parent_hash: [1u8; 32],
            state_root: [2u8; 32],
            events_root: [3u8; 32],
            order_count: 4,
            fill_count: 2,
            timestamp_ms: 1_700_000_000_000,
        };
        let header = WitnessBlockHeader {
            height: 11,
            parent_hash: [4u8; 32],
            state_root: [5u8; 32],
            events_root: [6u8; 32],
            order_count: 2,
            fill_count: 1,
            timestamp_ms: 1_700_000_001_234,
        };

        let mut clearing_prices = HashMap::new();
        clearing_prices.insert(market_a, vec![Nanos(610_000_000), Nanos(390_000_000)]);
        clearing_prices.insert(market_b, vec![Nanos(410_000_000), Nanos(590_000_000)]);

        BlockWitness {
            header,
            previous_header: Some(previous_header),
            orders: vec![WitnessOrder {
                order: accepted_order.clone(),
                account_id: 1001,
                is_mm: false,
            }],
            rejections: vec![WitnessRejection {
                order: rejected_order,
                account_id: 1002,
                reason: RejectionReason::InsufficientBalance {
                    required: 12_345,
                    available: 6_789,
                },
            }],
            system_events: vec![
                SystemEventWitness::L1Deposit {
                    account_id: 1001,
                    amount: 1_000_000,
                    deposit_id: 3,
                    deposit_root: deposit_accumulator.new_deposits[0].deposit_root,
                    sybil_account_key: deposit_accumulator.new_deposits[0].sybil_account_key,
                },
                SystemEventWitness::WithdrawalCreated {
                    account_id: 1001,
                    amount: -456_000,
                    withdrawal_id: 3,
                    recipient: [9u8; 20],
                    token: [10u8; 20],
                    amount_token_units: 123_000,
                    expiry_height: 99,
                    nullifier: [11u8; 32],
                },
                SystemEventWitness::MarketResolved {
                    market_id: market_b,
                    payout_nanos: Nanos(1_000_000_000),
                    affected_accounts: vec![1001, 1002],
                },
                SystemEventWitness::OrderCancelled {
                    account_id: 1001,
                    order_id: 7,
                    market_ids: vec![market_a, market_b],
                    side: OrderDirection::SellNo,
                    remaining_quantity: 321,
                },
                SystemEventWitness::MarketGroupExtended {
                    group_id: 5,
                    market_id: market_b,
                },
            ],
            deposit_accumulator,
            fills: vec![Fill {
                order_id: 7,
                fill_qty: Qty(250),
                fill_price: Nanos(600_000_000),
                account_id: 1001,
            }],
            clearing_prices,
            total_welfare: 12_345,
            minting_cost: -222,
            mm_constraints: vec![MmConstraint::new(MmId::new(12), Nanos(3_000_000_000))
                .with_order(7, MmSide::BuyYes)
                .with_order(8, MmSide::SellNo)],
            market_groups: vec![MarketGroup {
                name: "Weather basket".to_string(),
                markets: vec![market_a, market_b],
            }],
            pre_state: vec![account_snapshot(1001), account_snapshot(1002)],
            post_system_state: vec![account_snapshot(1001), account_snapshot(1002)],
            post_state: vec![account_snapshot(1001), account_snapshot(1002)],
            account_keys: vec![],
            state_sidecar,
            pre_state_sidecar,
            resolved_markets: vec![market_a, market_b],
        }
    }

    fn deposit_accumulator() -> DepositAccumulatorWitness {
        let deposits = [1u64, 2, 3]
            .into_iter()
            .map(|deposit_id| sybil_l1_protocol::DepositLeaf {
                chain_id: 31_337,
                vault_address: [1u8; 20],
                deposit_id,
                token_address: [2u8; 20],
                sender: [deposit_id as u8; 20],
                sybil_account_key: [10 + deposit_id as u8; 32],
                amount_token_units: 1_000 + deposit_id,
            })
            .collect::<Vec<_>>();
        let roots = sybil_l1_protocol::deposit_prefix_roots(&deposits);
        let pre_frontier = sybil_l1_protocol::deposit_frontier_after_prefix(
            &sybil_l1_protocol::empty_deposit_frontier(),
            0,
            &deposits[..2],
        )
        .unwrap();
        let new_deposits = vec![L1DepositWitness {
            deposit_id: deposits[2].deposit_id,
            chain_id: deposits[2].chain_id,
            vault_address: deposits[2].vault_address,
            token_address: deposits[2].token_address,
            sender: deposits[2].sender,
            sybil_account_key: deposits[2].sybil_account_key,
            amount_token_units: deposits[2].amount_token_units,
            deposit_root: roots[2],
        }];
        DepositAccumulatorWitness {
            pre_frontier,
            pre_count: 2,
            new_deposits,
        }
    }

    fn fixture_order(
        id: u64,
        primary: MarketId,
        secondary: MarketId,
        limit_price: u64,
        expires_at_block: Option<u64>,
    ) -> Order {
        let mut order = Order::new(id);
        order.markets[0] = primary;
        order.markets[1] = secondary;
        order.num_markets = 2;
        order.num_states = 4;
        order.payoffs[0] = 0;
        order.payoffs[1] = -1;
        order.payoffs[2] = 1;
        order.payoffs[3] = 0;
        order.limit_price = Nanos(limit_price);
        order.max_fill = Qty(500);
        order.condition = Some(PriceCondition {
            market: secondary,
            threshold: Nanos(500_000_000),
            direction: ConditionDir::Above,
        });
        order.expires_at_block = expires_at_block;
        order
    }

    fn account_snapshot(id: u64) -> AccountSnapshot {
        AccountSnapshot {
            id,
            balance: if id == 1001 { 9_000_000 } else { 7_000_000 },
            total_deposited: if id == 1001 { 10_000_000 } else { 8_000_000 },
            positions: vec![(MarketId::new(3), 0, 25), (MarketId::new(9), 0, -7)],
            events_digest: [id as u8; 32],
            keys_digest: crate::empty_account_keys_digest(id),
        }
    }

    fn state_sidecar(resting_order: Order, bridge: BridgeStateSnapshot) -> StateSidecarSnapshot {
        let record = ResolutionRecordSnapshot {
            market_id: MarketId::new(9),
            payout_nanos: Nanos(1_000_000_000),
            resolved_by: OracleSourceSnapshot::Admin,
            resolved_at_ms: 1_700_000_000_300,
            proposal: None,
            challenge: None,
        };
        StateSidecarSnapshot {
            bridge,
            markets: vec![
                MarketSnapshot {
                    market_id: MarketId::new(3),
                    name: "Wind over 20kt".to_string(),
                    num_outcomes: 2,
                    status: MarketStatusSnapshot::Active,
                    metadata_digest: [13u8; 32],
                    resolution_template: "admin_immediate".to_string(),
                },
                MarketSnapshot {
                    market_id: MarketId::new(9),
                    name: "Rain in London".to_string(),
                    num_outcomes: 2,
                    status: MarketStatusSnapshot::Resolved { record },
                    metadata_digest: [12u8; 32],
                    resolution_template: "admin_immediate".to_string(),
                },
            ],
            market_groups: vec![MarketGroupSnapshot {
                group_id: 5,
                name: "Weather basket".to_string(),
                markets: vec![MarketId::new(3), MarketId::new(9)],
            }],
            resting_orders: vec![RestingOrderSnapshot {
                order: resting_order,
                account_id: 1001,
                created_at: 8,
                expires_at_block: 77,
                reserved_balance: 123_456,
                reserved_positions: vec![(MarketId::new(3), 0, 12)],
            }],
            account_reservations: vec![AccountReservationSnapshot {
                account_id: 1001,
                reserved_balance: 123_456,
                reserved_positions: vec![(MarketId::new(3), 0, 12)],
            }],
        }
    }

    fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    fn assert_witness_eq(left: &BlockWitness, right: &BlockWitness) {
        assert_eq!(
            canonical_witness_bytes(left),
            canonical_witness_bytes(right)
        );
    }
}
