// Exempt from the f64 ban (SYB-196): this module is off the consensus/state-root
// path. Its floats are the token-bucket admission rate limiter and Prometheus
// metric gauges/histograms — both explicitly exempt (admission heuristic +
// observability). No value here is committed into a block's state root.
#![allow(clippy::disallowed_types)]

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use tokio::sync::broadcast;
use tokio::time::{Instant, interval_at};

use matching_engine::{MarketGroup, MarketId, MarketSet, Nanos, Order, Problem};
use sybil_oracle::{
    DataFeed, FeedId, FeedPubkey, MarketStatus, Oracle, ResolutionRecord, SignedAttestation,
};

use crate::account::{Account, AccountId};
use crate::block::{BlockProduction, SealedBlock};
use crate::bridge::{
    BridgeState, BridgeWithdrawalL1Event, BridgeWithdrawalRequest, L1Deposit, WithdrawalLeaf,
};
use crate::crypto::{
    AccountAuthScheme, AuthenticatedApiKeyCreate, AuthenticatedApiKeyRevoke,
    AuthenticatedBridgeWithdrawal, AuthenticatedCancel, AuthenticatedKeyRegistration,
    AuthenticatedKeyRevocation, AuthenticatedOrder, AuthenticatedProfileUpdate, PublicKey,
    RegisteredPubkey, SignedApiKeyCreate, SignedApiKeyRevoke, SignedBridgeWithdrawal, SignedCancel,
    SignedKeyRegistration, SignedKeyRevocation, SignedOrder, SignedProfileUpdate,
    verify_signed_api_key_create, verify_signed_api_key_revoke, verify_signed_bridge_withdrawal,
    verify_signed_cancel, verify_signed_key_registration, verify_signed_key_revocation,
    verify_signed_order, verify_signed_profile_update,
};
use crate::error::SequencerError;
use crate::market_info::{
    AccountFillCursor, AccountFillRecord, MarketMetadata, MarketSearchQuery, PriceCandle,
    PriceCandlePage, PriceHistoryPage, PricePoint,
};
use crate::portfolio::PortfolioSummary;
use crate::sequencer::{
    BlockSequencer, LeaderboardRow, OrderSubmission, PendingOrderInfo, PreparedBlock,
    SequencerConfig,
};
use crate::store::{
    AutoResolutionRecord, ControlPlaneCommand, DaArtifact, DaArtifactLookup, DaManifestLookup,
    HistoryRetentionPolicy,
};
use crate::{
    AccountSnapshotSlot, QMDB_STATE_MAX_KEY_BYTES, QmdbStateExclusionProofParts,
    QmdbStateKeyValueProofParts,
};

mod handle;
mod handlers;
mod infra;
mod messages;
mod production;
mod queries;
mod supervisor;

use self::infra::{IndicativeSolveGate, MailboxMonitor, TokenBucket};
#[cfg(not(test))]
use self::messages::SequencerTestCrashpoint;
use self::messages::{SequencerActor, SequencerActorArgs, SequencerActorState};
use self::production::BlockTickOutcome;
use self::queries::{limit_price_point_page, price_candle_page_from_points};
use self::supervisor::{
    SequencerHandleInner, SequencerSupervisor, SequencerSupervisorArgs, SequencerSupervisorMsg,
};

pub use self::handle::SequencerHandle;
pub use self::messages::{IndicativeSnapshot, SequencerMsg, SequencerReadQuery};
#[cfg(test)]
pub use self::messages::{SequencerTestCrashpoint, SequencerTestTickHold};
pub use self::queries::{
    DEFAULT_PRICE_HISTORY_QUERY_POINTS, MAX_BLOCK_HISTORY_QUERY_BLOCKS,
    MAX_PRICE_HISTORY_QUERY_POINTS, MarketSearchResult, SequencerStateProof,
    SequencerStateProofKind,
};

const SEQUENCER_ACTOR_METRIC_NAME: &str = "sequencer";
