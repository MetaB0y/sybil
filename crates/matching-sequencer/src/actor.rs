// Exempt from the f64 ban (SYB-196): this module is off the consensus/state-root
// path. Its floats are Prometheus metric gauges/histograms; no value here is
// committed into a block's state root.
#![allow(clippy::disallowed_types)]

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use ratelimit::Ratelimiter;
use tokio::sync::broadcast;
use tokio::time::{Instant, interval_at};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use matching_engine::{MarketGroup, MarketId, MarketSet, Nanos, Order, Problem};
use sybil_oracle::{
    DataFeed, FeedId, FeedPubkey, MarketStatus, ResolutionRecord, SignedAttestation,
};

use crate::account::{Account, AccountId};
use crate::block::{BlockProduction, SealedBlock};
use crate::bridge::{BridgeWithdrawalL1Event, BridgeWithdrawalRequest, L1Deposit, WithdrawalLeaf};
use crate::crypto::{
    AccountAuthScheme, AuthenticatedApiKeyCreate, AuthenticatedApiKeyRevoke,
    AuthenticatedBridgeWithdrawal, AuthenticatedCancel, AuthenticatedKeyRegistration,
    AuthenticatedKeyRevocation, AuthenticatedOrder, AuthenticatedProfileUpdate, PublicKey,
    RegisteredPubkey, SignedApiKeyCreate, SignedApiKeyRevoke, SignedBridgeWithdrawal, SignedCancel,
    SignedKeyRegistration, SignedKeyRevocation, SignedOrder, SignedProfileUpdate,
    canonical_cancel_bytes, canonical_order_bytes, raw_client_action_authorization,
    verify_signed_api_key_create, verify_signed_api_key_revoke, verify_signed_bridge_withdrawal,
    verify_signed_cancel, verify_signed_key_registration, verify_signed_key_revocation,
    verify_signed_order, verify_signed_profile_update,
};
use crate::error::SequencerError;
use crate::market_info::{MarketMetadata, MarketSearchQuery};
use crate::portfolio::PortfolioSummary;
use crate::sequencer::{
    BlockSequencer, LeaderboardBase, OrderSubmission, PendingOrderInfo, PreparedBlock,
    SequencerConfig,
};
use crate::store::{
    AcknowledgedProofJobRetentionPolicy, CanonicalArchiveRetentionPolicy, ControlPlaneCommand,
    DaArtifact, DaArtifactLookup, DaManifestLookup,
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

use self::infra::{IndicativeSolveGate, MailboxMonitor, ScheduledTickGate, rate_limiter};
#[cfg(not(test))]
use self::messages::SequencerTestCrashpoint;
use self::messages::{SequencerActor, SequencerActorArgs, SequencerActorState};
use self::production::BlockTickOutcome;
use self::supervisor::{
    SequencerHandleInner, SequencerSupervisor, SequencerSupervisorArgs, SequencerSupervisorMsg,
};

pub use self::handle::SequencerHandle;
pub use self::messages::{
    IndicativeSnapshot, SequencerMsg, SequencerOperationalStatus, SequencerReadQuery,
};
#[cfg(test)]
pub use self::messages::{SequencerTestCrashpoint, SequencerTestTickHold};
pub use self::queries::{
    DEFAULT_PRICE_HISTORY_QUERY_POINTS, MAX_BLOCK_REPLAY_QUERY_BLOCKS,
    MAX_PRICE_HISTORY_QUERY_POINTS, MarketSearchResult, SequencerStateProof,
    SequencerStateProofKind,
};

const SEQUENCER_ACTOR_METRIC_NAME: &str = "sequencer";
