//! Witness types consumed by the verifier.
//!
//! The sequencer builds a [`BlockWitness`] after each block. The verifier
//! (and future ZK circuit) takes it as input and checks every constraint.

use std::collections::HashMap;

use matching_engine::{
    Fill, MarketGroup, MarketId, MmConstraint, MmSide, Nanos, Order, OrderDirection,
};
use sybil_l1_protocol::{DEPOSIT_TREE_DEPTH, DepositFrontier};

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
    /// Chain-instance domain used by validity-critical signed operations.
    /// This private guest input is covered by every key and client-action signature.
    pub genesis_hash: [u8; 32],

    // -- Orders --
    /// Orders accepted into this batch (with account mapping).
    pub orders: Vec<WitnessOrder>,
    /// Orders rejected (with reasons).
    pub rejections: Vec<WitnessRejection>,
    /// System state changes applied between blocks.
    pub system_events: Vec<SystemEventWitness>,
    /// L1 deposit accumulator frontier at block start plus deposits credited
    /// in this block. This replaces the v2 cumulative deposit-log prefix.
    pub deposit_accumulator: DepositAccumulatorWitness,

    // -- Solver output --
    pub fills: Vec<Fill>,
    /// Clearing prices per market: `market_id -> [price_outcome_0, price_outcome_1, ...]`.
    pub clearing_prices: HashMap<MarketId, Vec<Nanos>>,
    pub total_welfare: i64,
    /// Signed complete-set cost: positive for minting, negative for burning.
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
    /// Active signing-key sets at block end. Entries are canonicalized by
    /// account id and each key list by [`KeyRecord::canonical_sort_key`].
    /// Accounts with no active keys may be omitted.
    pub account_keys: Vec<(u64, Vec<KeyRecord>)>,
    /// Non-account state committed by the header's `state_root`.
    pub state_sidecar: StateSidecarSnapshot,
    /// Non-account state at block start, authenticated against the previous
    /// header's `state_root`.
    pub pre_state_sidecar: StateSidecarSnapshot,

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

/// An ordinary client action whose exact authorization envelope is replayed
/// against the running key set and cross-block nonce inside the guest.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ClientActionWitness {
    Order {
        account_id: u64,
        order: Order,
        nonce: u64,
        authorization: ClientActionAuth,
    },
    Cancel {
        account_id: u64,
        order_id: u64,
        nonce: u64,
        authorization: ClientActionAuth,
    },
    /// One signed, all-or-nothing flash-liquidity bundle. Order ids are
    /// sequencer-assigned after signature verification and are excluded from
    /// canonical signing bytes.
    MmBundle {
        account_id: u64,
        bundle_id: [u8; 32],
        revision: u64,
        orders: Vec<Order>,
        order_sides: Vec<MmSide>,
        max_capital: Nanos,
        nonce: u64,
        authorization: ClientActionAuth,
    },
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
    /// The order was rejected as part of one all-or-nothing MM bundle.
    AtomicBundle,
    /// Order shape or quantity is not supported by production admission.
    InvalidOrder(String),
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
        initial_keys: Vec<KeyRecord>,
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
    WithdrawalRefunded {
        account_id: u64,
        withdrawal_id: u64,
        amount: i64,
        reason: WithdrawalRefundReasonWitness,
    },
    WithdrawalFinalized {
        account_id: u64,
        withdrawal_id: u64,
        amount: i64,
    },
    L1BlockObserved {
        height: u64,
    },
    MarketResolved {
        market_id: MarketId,
        payout_nanos: Nanos,
        affected_accounts: Vec<u64>,
    },
    /// A resting order was cancelled by its owner (D1).
    OrderCancelled {
        account_id: u64,
        order_id: u64,
        market_ids: Vec<MarketId>,
        side: OrderDirection,
        remaining_quantity: u64,
    },
    /// A market was added to an existing mutually-exclusive market group.
    MarketGroupExtended {
        group_id: u64,
        market_id: MarketId,
    },
    KeyRegistered {
        account_id: u64,
        key: KeyRecord,
        authorization: KeyOpAuth,
    },
    KeyRevoked {
        account_id: u64,
        key: KeyRecord,
        authorization: KeyOpAuth,
    },
    DepositQuarantined {
        amount: i64,
        deposit_id: u64,
        deposit_root: [u8; 32],
        sybil_account_key: [u8; 32],
    },
    QuarantineClaimed {
        account_id: u64,
        amount: i64,
        sybil_account_key: [u8; 32],
    },
    /// A signature-authorized ordinary order or cancellation. These events
    /// are emitted in actor acknowledgement order alongside key mutations so
    /// guest replay observes the exact key/nonce state used at admission.
    ClientActionAuthorized(ClientActionWitness),
}

/// Validity-critical signing-key record committed by `keys_digest` v2.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct KeyRecord {
    /// `0 = raw_p256`, `1 = webauthn`.
    pub auth_scheme: u8,
    /// Compressed SEC1 P-256 public key.
    #[serde(with = "pubkey_bytes")]
    pub pubkey_sec1: [u8; 33],
    /// Reserved validity-critical capability bits. All bits are authoritative
    /// today because scoped delegation is not active yet.
    pub capability_mask: u32,
}

impl KeyRecord {
    pub const FULL_CAPABILITY_MASK: u32 = u32::MAX;

    pub fn canonical_sort_key(&self) -> ([u8; 33], u8) {
        (self.pubkey_sec1, self.auth_scheme)
    }
}

/// Authorization envelope retained one-for-one with a witnessed key mutation.
/// Native and guest verification check this envelope against the running
/// active-key set and the state-bound canonical mutation bytes.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum KeyOpAuth {
    RawP256 {
        #[serde(with = "pubkey_bytes")]
        signer_pubkey: [u8; 33],
        #[serde(with = "signature_bytes")]
        signature: [u8; 64],
    },
    WebAuthn {
        #[serde(with = "pubkey_bytes")]
        signer_pubkey: [u8; 33],
        authenticator_data: Vec<u8>,
        client_data_json: Vec<u8>,
        #[serde(with = "signature_bytes")]
        signature: [u8; 64],
    },
}

/// Ordinary orders/cancels use the same bounded P256/WebAuthn envelope shape
/// as key operations, but sign their `sybil-signing` canonical action bytes.
pub type ClientActionAuth = KeyOpAuth;

mod pubkey_bytes {
    use std::fmt;

    use serde::de::{SeqAccess, Visitor};
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 33], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 33], D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_bytes(FixedBytesVisitor::<33>)
    }

    struct FixedBytesVisitor<const N: usize>;

    impl<'de, const N: usize> Visitor<'de> for FixedBytesVisitor<N> {
        type Value = [u8; N];

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "exactly {N} bytes")
        }

        fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            value
                .try_into()
                .map_err(|_| E::invalid_length(value.len(), &self))
        }

        fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            self.visit_bytes(&value)
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut out = [0u8; N];
            for (index, byte) in out.iter_mut().enumerate() {
                *byte = sequence
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(index, &self))?;
            }
            if sequence.next_element::<u8>()?.is_some() {
                return Err(serde::de::Error::invalid_length(N + 1, &self));
            }
            Ok(out)
        }
    }
}

mod signature_bytes {
    use std::fmt;

    use serde::de::{SeqAccess, Visitor};
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_bytes(FixedBytesVisitor::<64>)
    }

    struct FixedBytesVisitor<const N: usize>;

    impl<'de, const N: usize> Visitor<'de> for FixedBytesVisitor<N> {
        type Value = [u8; N];

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "exactly {N} bytes")
        }

        fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            value
                .try_into()
                .map_err(|_| E::invalid_length(value.len(), &self))
        }

        fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            self.visit_bytes(&value)
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut out = [0u8; N];
            for (index, byte) in out.iter_mut().enumerate() {
                *byte = sequence
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(index, &self))?;
            }
            if sequence.next_element::<u8>()?.is_some() {
                return Err(serde::de::Error::invalid_length(N + 1, &self));
            }
            Ok(out)
        }
    }
}

impl KeyOpAuth {
    pub fn signer_pubkey(&self) -> &[u8; 33] {
        match self {
            Self::RawP256 { signer_pubkey, .. } | Self::WebAuthn { signer_pubkey, .. } => {
                signer_pubkey
            }
        }
    }

    pub fn signer_auth_scheme(&self) -> u8 {
        match self {
            Self::RawP256 { .. } => 0,
            Self::WebAuthn { .. } => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WithdrawalRefundReasonWitness {
    L1Cancelled,
    L1Expired { observed_l1_height: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct L1DepositWitness {
    pub deposit_id: u64,
    pub chain_id: u64,
    pub vault_address: [u8; 20],
    pub token_address: [u8; 20],
    pub sender: [u8; 20],
    pub sybil_account_key: [u8; 32],
    pub amount_token_units: u64,
    pub deposit_root: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DepositAccumulatorWitness {
    /// Filled-subtree frontier at block start. Mirrors `SybilVault.filledSubtrees`.
    pub pre_frontier: DepositFrontier,
    /// Deposits before this block. Must equal `pre_state_sidecar.bridge.deposit_cursor`.
    pub pre_count: u64,
    /// Deposit leaves credited in this block only, in id order.
    pub new_deposits: Vec<L1DepositWitness>,
}

impl Default for DepositAccumulatorWitness {
    fn default() -> Self {
        Self {
            pre_frontier: [[0u8; 32]; DEPOSIT_TREE_DEPTH],
            pre_count: 0,
            new_deposits: Vec::new(),
        }
    }
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
    pub keys_digest: [u8; 32],
    /// Highest accepted ordinary order/cancel nonce for this account.
    #[serde(default)]
    pub last_trading_nonce: u64,
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
    /// Most recently committed clearing prices, indexed by outcome.
    /// Empty means the market has never cleared.
    #[serde(default)]
    pub last_clearing_prices: Vec<Nanos>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MarketGroupSnapshot {
    pub group_id: u64,
    pub name: String,
    #[serde(default)]
    pub creation_key: Option<String>,
    pub markets: Vec<MarketId>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MarketStatusSnapshot {
    Active,
    Resolved { record: ResolutionRecordSnapshot },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ResolutionRecordSnapshot {
    pub payout_nanos: Nanos,
    pub resolved_by: OracleSourceSnapshot,
    pub resolved_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OracleSourceSnapshot {
    Admin,
    DataFeed(u64),
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BridgeStateSnapshot {
    pub deposit_cursor: u64,
    pub deposit_root: [u8; 32],
    #[serde(default)]
    pub observed_l1_height: u64,
    pub next_withdrawal_id: u64,
    pub withdrawals: Vec<WithdrawalSnapshot>,
    /// Canonical opening of the single system quarantine ledger, sorted by key.
    #[serde(default)]
    pub quarantine: Vec<QuarantineEntrySnapshot>,
}

impl Default for BridgeStateSnapshot {
    fn default() -> Self {
        Self {
            deposit_cursor: 0,
            deposit_root: sybil_l1_protocol::empty_deposit_root(),
            observed_l1_height: 0,
            next_withdrawal_id: 0,
            withdrawals: Vec::new(),
            quarantine: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct QuarantineEntrySnapshot {
    pub sybil_account_key: [u8; 32],
    pub amount: i64,
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
