//! Witness types consumed by the verifier.
//!
//! The sequencer builds a [`BlockWitness`] after each block. The verifier
//! (and future ZK circuit) takes it as input and checks every constraint.

use std::collections::HashMap;

use matching_engine::{Fill, MarketGroup, MarketId, MmConstraint, Nanos, Order};

/// Everything the verifier needs to check a single block.
///
/// Built by the sequencer, consumed by the verifier. A future ZK circuit
/// takes this as its public/private input.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BlockWitness {
    /// Block header being verified.
    pub header: WitnessBlockHeader,
    /// Previous block header (`None` for genesis).
    pub previous_header: Option<WitnessBlockHeader>,

    // -- Orders --
    /// Orders accepted into this batch (with account mapping).
    pub orders: Vec<WitnessOrder>,
    /// Orders rejected (with reasons).
    pub rejections: Vec<WitnessRejection>,
    /// System state changes applied between blocks.
    pub system_events: Vec<SystemEventWitness>,

    // -- Solver output --
    pub fills: Vec<Fill>,
    /// Clearing prices per market: `market_id -> [price_outcome_0, price_outcome_1, ...]`.
    pub clearing_prices: HashMap<MarketId, Vec<Nanos>>,
    pub total_welfare: i64,
    /// Minting cost not captured in fill-level welfare (MILP only, 0 for heuristics).
    pub minting_cost: i64,

    // -- Constraints --
    pub mm_constraints: Vec<MmConstraint>,
    pub market_groups: Vec<MarketGroup>,

    // -- Account state --
    /// Account snapshots at block start, before any system events, sorted by id.
    pub pre_state: Vec<AccountSnapshot>,
    /// Account snapshots after system events and before fills, sorted by id.
    pub post_system_state: Vec<AccountSnapshot>,
    /// Account snapshots *after* settlement, sorted by id.
    pub post_state: Vec<AccountSnapshot>,
    /// Non-account state committed by the header's `state_root`.
    pub state_sidecar: StateSidecarSnapshot,

    /// Markets that are resolved/voided — orders/fills must not reference these.
    pub resolved_markets: Vec<MarketId>,
}

/// Minimal block header stored in the witness.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WitnessBlockHeader {
    pub height: u64,
    pub parent_hash: [u8; 32],
    pub state_root: [u8; 32],
    pub events_root: [u8; 32],
    pub order_count: u32,
    pub fill_count: u32,
    pub timestamp_ms: u64,
}

/// An order together with the account that placed it.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WitnessOrder {
    pub order: Order,
    pub account_id: u64,
    /// Whether this is a market-maker order (skip balance validation).
    pub is_mm: bool,
}

/// A rejected order together with a reason.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WitnessRejection {
    pub order: Order,
    pub account_id: u64,
    pub reason: RejectionReason,
}

/// Reason an order was rejected (mirrors sequencer's `RejectionReason`).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum RejectionReason {
    InsufficientBalance {
        required: i64,
        available: i64,
    },
    InsufficientPosition {
        market: MarketId,
        outcome: u8,
        required: i64,
        available: i64,
    },
    AccountNotFound,
    /// MM orders form a complete set within a market group (self-trade via minting).
    CompleteSetFormation,
    /// Order time-in-force made it ineligible for the target batch.
    Expired {
        current_block: u64,
        expires_at_block: u64,
    },
}

/// System state change recorded in a block witness.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SystemEventWitness {
    CreateAccount {
        account_id: u64,
        initial_balance: i64,
    },
    Deposit {
        account_id: u64,
        amount: i64,
    },
    L1Deposit {
        account_id: u64,
        amount: i64,
        deposit_id: u64,
        deposit_root: [u8; 32],
        sybil_account_key: [u8; 32],
    },
    WithdrawalCreated {
        account_id: u64,
        amount: i64,
        withdrawal_id: u64,
        recipient: [u8; 20],
        token: [u8; 20],
        amount_token_units: u64,
        expiry_height: u64,
        nullifier: [u8; 32],
    },
    MarketResolved {
        market_id: MarketId,
        payout_nanos: Nanos,
        affected_accounts: Vec<u64>,
    },
}

/// Snapshot of a single account's state at a point in time.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AccountSnapshot {
    pub id: u64,
    pub balance: i64,
    #[serde(default)]
    pub total_deposited: i64,
    /// Sorted by `(market, outcome)`.
    pub positions: Vec<(MarketId, u8, i64)>,
    #[serde(default)]
    pub events_digest: [u8; 32],
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct StateSidecarSnapshot {
    pub bridge: BridgeStateSnapshot,
    pub markets: Vec<MarketSnapshot>,
    pub market_groups: Vec<MarketGroupSnapshot>,
    pub resting_orders: Vec<RestingOrderSnapshot>,
    pub account_reservations: Vec<AccountReservationSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MarketSnapshot {
    pub market_id: MarketId,
    pub name: String,
    pub num_outcomes: u8,
    pub status: MarketStatusSnapshot,
    pub metadata_digest: [u8; 32],
    pub resolution_template: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MarketGroupSnapshot {
    pub group_id: u64,
    pub name: String,
    pub markets: Vec<MarketId>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MarketStatusSnapshot {
    Active,
    Proposed {
        proposal: ResolutionProposalSnapshot,
        challenge_deadline_ms: u64,
    },
    Challenged {
        proposal: ResolutionProposalSnapshot,
        challenge: ChallengeSnapshot,
    },
    Resolved {
        record: ResolutionRecordSnapshot,
    },
    Voided,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ResolutionProposalSnapshot {
    pub id: u64,
    pub market_id: MarketId,
    pub payout_nanos: Nanos,
    pub source: OracleSourceSnapshot,
    pub proposed_at_ms: u64,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChallengeSnapshot {
    pub id: u64,
    pub challenger: u64,
    pub proposal_id: u64,
    pub bond_amount: Nanos,
    pub proposed_payout_nanos: Nanos,
    pub reason: String,
    pub challenged_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ResolutionRecordSnapshot {
    pub market_id: MarketId,
    pub payout_nanos: Nanos,
    pub resolved_by: OracleSourceSnapshot,
    pub resolved_at_ms: u64,
    pub proposal: Option<ResolutionProposalSnapshot>,
    pub challenge: Option<ChallengeSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OracleSourceSnapshot {
    Admin,
    DataFeed(u64),
    AutomatedL0,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BridgeStateSnapshot {
    pub deposit_cursor: u64,
    pub deposit_root: [u8; 32],
    pub next_withdrawal_id: u64,
    pub withdrawals: Vec<WithdrawalSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WithdrawalSnapshot {
    pub withdrawal_id: u64,
    pub account_id: u64,
    pub recipient: [u8; 20],
    pub token: [u8; 20],
    pub amount_token_units: u64,
    pub amount_nanos: u64,
    pub expiry_height: u64,
    pub nullifier: [u8; 32],
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RestingOrderSnapshot {
    pub order: Order,
    pub account_id: u64,
    pub created_at: u64,
    pub expires_at_block: u64,
    pub reserved_balance: i64,
    pub reserved_positions: Vec<(MarketId, u8, i64)>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AccountReservationSnapshot {
    pub account_id: u64,
    pub reserved_balance: i64,
    pub reserved_positions: Vec<(MarketId, u8, i64)>,
}
