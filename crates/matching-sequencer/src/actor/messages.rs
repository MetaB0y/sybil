use super::*;

pub(super) struct SequencerActor;

pub(super) struct SequencerActorArgs {
    pub(super) sequencer: BlockSequencer,
    pub(super) store: Option<Arc<crate::store::Store>>,
    pub(super) block_broadcast: broadcast::Sender<SealedBlock>,
    pub(super) recent_blocks: Arc<RwLock<VecDeque<SealedBlock>>>,
    pub(super) mailbox_monitor: MailboxMonitor,
}

pub(super) struct SequencerActorState {
    pub(super) sequencer: BlockSequencer,
    pub(super) latest_block: Option<SealedBlock>,
    pub(super) recent_blocks: Arc<RwLock<VecDeque<SealedBlock>>>,
    pub(super) block_broadcast: broadcast::Sender<SealedBlock>,
    pub(super) pause_count: u32,
    pub(super) halted_error: Option<SequencerError>,
    pub(super) store: Option<Arc<crate::store::Store>>,
    pub(super) global_submission_limiter: Ratelimiter,
    pub(super) account_submission_limiters: HashMap<AccountId, Ratelimiter>,
    pub(super) mailbox_monitor: MailboxMonitor,
    /// Per-market indicative snapshots from the C2 shadow-solver. Cache
    /// lives on the actor (not `BlockSequencer`) so pure-core stays pure.
    /// Empty until the first `IndicativeUpdate` arrives; lookup-miss
    /// returns `IndicativeSnapshot::default()` (None/None/0/0).
    pub(super) indicative_cache: HashMap<MarketId, IndicativeSnapshot>,
    pub(super) indicative_solve_gate: IndicativeSolveGate,
    /// Bounds timer-driven block production to one queued tick. Manual block
    /// production remains an independent RPC.
    pub(super) scheduled_tick_gate: ScheduledTickGate,
    /// Owns timer, DA, retention, and indicative-solve futures that are not
    /// actors themselves. Ractor owns the sequencer lifecycle; this tracker
    /// makes those Tokio children part of its clean-stop boundary.
    pub(super) background_tasks: TaskTracker,
    pub(super) background_cancel: CancellationToken,
    #[cfg(test)]
    pub(super) next_tick_hold: Option<SequencerTestTickHold>,
}

/// Messages sent from handles to the sequencer actor.
pub enum SequencerMsg {
    Tick,
    #[cfg(test)]
    TestCrashOnNextBlock(SequencerTestCrashpoint),
    #[cfg(test)]
    TestEnterIntegrityHalt(SequencerError, RpcReplyPort<()>),
    #[cfg(test)]
    TestHoldNextTick(SequencerTestTickHold, RpcReplyPort<()>),
    SubmitOrder(
        OrderSubmission,
        RpcReplyPort<Result<Vec<u64>, SequencerError>>,
    ),
    SubmitIocOrder(
        OrderSubmission,
        RpcReplyPort<Result<Vec<u64>, SequencerError>>,
    ),
    SubmitSignedOrder(SignedOrder, RpcReplyPort<Result<Vec<u64>, SequencerError>>),
    SubmitAuthenticatedOrder(
        AuthenticatedOrder,
        RpcReplyPort<Result<Vec<u64>, SequencerError>>,
    ),
    CancelSignedOrder(SignedCancel, RpcReplyPort<Result<(), SequencerError>>),
    CancelAuthenticatedOrder(
        AuthenticatedCancel,
        RpcReplyPort<Result<(), SequencerError>>,
    ),
    GetStateProof(
        Vec<u8>,
        RpcReplyPort<Result<SequencerStateProof, SequencerError>>,
    ),
    ProduceBlock(RpcReplyPort<Result<SealedBlock, SequencerError>>),
    CreateAccount(i64, RpcReplyPort<Result<Account, SequencerError>>),
    CreateAccountWithInitialKey(
        i64,
        PublicKey,
        RegisteredPubkey,
        RpcReplyPort<Result<Account, SequencerError>>,
    ),
    CreatePublicAccountWithInitialKey(
        u64,
        i64,
        PublicKey,
        RegisteredPubkey,
        RpcReplyPort<Result<Account, SequencerError>>,
    ),
    ProvisionServiceAccount(
        String,
        i64,
        Option<(PublicKey, RegisteredPubkey)>,
        RpcReplyPort<Result<ServiceAccountProvisioningResult, SequencerError>>,
    ),
    FundAccount(
        AccountId,
        i64,
        RpcReplyPort<Result<Account, SequencerError>>,
    ),
    SubmitL1Deposit(
        L1Deposit,
        RpcReplyPort<Result<crate::bridge::DepositDisposition, SequencerError>>,
    ),
    CreateBridgeWithdrawal(
        BridgeWithdrawalRequest,
        RpcReplyPort<Result<WithdrawalLeaf, SequencerError>>,
    ),
    CreateSignedBridgeWithdrawal(
        SignedBridgeWithdrawal,
        RpcReplyPort<Result<WithdrawalLeaf, SequencerError>>,
    ),
    CreateAuthenticatedBridgeWithdrawal(
        AuthenticatedBridgeWithdrawal,
        RpcReplyPort<Result<WithdrawalLeaf, SequencerError>>,
    ),
    ApplyBridgeWithdrawalL1Event(
        BridgeWithdrawalL1Event,
        RpcReplyPort<Result<Option<WithdrawalLeaf>, SequencerError>>,
    ),
    ObserveBridgeL1Height(
        u64,
        RpcReplyPort<Result<Vec<WithdrawalLeaf>, SequencerError>>,
    ),
    RegisterPubkey(
        AccountId,
        PublicKey,
        AccountAuthScheme,
        RpcReplyPort<Result<(), SequencerError>>,
    ),
    RegisterPubkeyWithMeta(
        AccountId,
        PublicKey,
        RegisteredPubkey,
        RpcReplyPort<Result<(), SequencerError>>,
    ),
    RegisterKeySigned(
        SignedKeyRegistration,
        RpcReplyPort<Result<(), SequencerError>>,
    ),
    RegisterKeyAuthenticated(
        AuthenticatedKeyRegistration,
        RpcReplyPort<Result<(), SequencerError>>,
    ),
    SetProfileSigned(
        SignedProfileUpdate,
        RpcReplyPort<Result<Account, SequencerError>>,
    ),
    SetProfileAuthenticated(
        AuthenticatedProfileUpdate,
        RpcReplyPort<Result<Account, SequencerError>>,
    ),
    RevokeSigningKeySigned(
        SignedKeyRevocation,
        RpcReplyPort<Result<(), SequencerError>>,
    ),
    RevokeSigningKeyAuthenticated(
        AuthenticatedKeyRevocation,
        RpcReplyPort<Result<(), SequencerError>>,
    ),
    CreateApiKeySigned(
        SignedApiKeyCreate,
        RpcReplyPort<Result<u64, SequencerError>>,
    ),
    CreateApiKeyAuthenticated(
        AuthenticatedApiKeyCreate,
        RpcReplyPort<Result<u64, SequencerError>>,
    ),
    RevokeApiKeySigned(SignedApiKeyRevoke, RpcReplyPort<Result<(), SequencerError>>),
    RevokeApiKeyAuthenticated(
        AuthenticatedApiKeyRevoke,
        RpcReplyPort<Result<(), SequencerError>>,
    ),
    CreateMarket(String, RpcReplyPort<Result<MarketId, SequencerError>>),
    CreateMarketGroup(
        String,
        Vec<MarketId>,
        RpcReplyPort<Result<(u64, MarketGroup), SequencerError>>,
    ),
    ExtendMarketGroup(
        u64,
        MarketId,
        RpcReplyPort<Result<(MarketGroup, bool), SequencerError>>,
    ),
    ResolveMarket(
        MarketId,
        Nanos,
        RpcReplyPort<Result<ResolutionRecord, SequencerError>>,
    ),
    ResolveMarketAttested(
        MarketId,
        SignedAttestation,
        RpcReplyPort<Result<ResolutionRecord, SequencerError>>,
    ),
    RegisterFeed(
        FeedPubkey,
        String,
        RpcReplyPort<Result<FeedId, SequencerError>>,
    ),
    InstallTemplate(
        sybil_oracle::ResolutionTemplate,
        RpcReplyPort<Result<(), SequencerError>>,
    ),
    GetDaArtifact(u64, RpcReplyPort<Result<DaArtifactLookup, SequencerError>>),
    GetDaManifest(u64, RpcReplyPort<Result<DaManifestLookup, SequencerError>>),
    CreateMarketWithMetadata(
        String,
        MarketMetadata,
        RpcReplyPort<Result<MarketId, SequencerError>>,
    ),
    PauseBlockProduction(RpcReplyPort<Result<(), SequencerError>>),
    ResumeBlockProduction(RpcReplyPort<Result<(), SequencerError>>),
    Query(SequencerReadQuery),
    /// Periodic indicative-solve tick (C2). Fires from a dedicated timer
    /// task at ~750ms cadence, decoupled from block production. The arm
    /// kicks off a `spawn_blocking` shadow-solve over the resting book and
    /// self-sends an `IndicativeUpdate` once the solver returns.
    IndicativeTick,
    /// Result of one shadow-solve: a per-market cache of indicative prices
    /// + per-market notional volume + computed_at_ms.
    IndicativeUpdate(HashMap<MarketId, IndicativeSnapshot>),
    /// Shadow-solve failed before producing a cache update.
    IndicativeSolveFailed {
        solver: String,
        error: String,
    },
}

/// One atomic view of the sequencer's chain identity and write availability.
///
/// Health callers must not assemble these fields through separate mailbox
/// reads: an invariant failure could occur between them and expose a healthy
/// chain snapshot after writes have already failed closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SequencerOperationalStatus {
    pub committed_height: Option<u64>,
    pub genesis_hash: Option<[u8; 32]>,
    pub integrity_halted: bool,
}

pub struct SequencerReadQuery {
    run: Box<dyn FnOnce(&mut SequencerActorState) + Send + 'static>,
}

impl SequencerReadQuery {
    pub(super) fn new<T>(
        reply: RpcReplyPort<T>,
        query: impl FnOnce(&SequencerActorState) -> T + Send + 'static,
    ) -> Self
    where
        T: Send + 'static,
    {
        Self {
            run: Box::new(move |state| {
                let _ = reply.send(query(state));
            }),
        }
    }

    pub(super) fn execute(self, state: &mut SequencerActorState) {
        (self.run)(state);
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SequencerTestCrashpoint {
    BeforePrepare,
    AfterPrepareBeforePersist,
    AfterPersistBeforeCommit,
    AfterCommit,
}

#[cfg(test)]
pub struct SequencerTestTickHold {
    pub started: tokio::sync::oneshot::Sender<()>,
    pub release: tokio::sync::oneshot::Receiver<()>,
}

#[cfg(not(test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SequencerTestCrashpoint {}

/// Shadow-solve snapshot for one market (C2). Off-block — produced by the
/// indicative scheduler, cached on `SequencerActorState`, served by
/// `GET /v1/markets/{id}/open-batch`. Pure-core (`BlockSequencer`) never
/// sees this.
#[derive(Clone, Debug, Default)]
pub struct IndicativeSnapshot {
    /// YES clearing price the solver discovered, or the last-clearing
    /// fallback when the book has orders but no matchable cross. `None`
    /// when the book is empty for this market or the solver is infeasible.
    pub yes_price_nanos: Option<u64>,
    /// NO clearing price; same semantics as `yes_price_nanos`.
    pub no_price_nanos: Option<u64>,
    /// Sum of `fill_price * fill_qty` over fills the shadow-solve produced
    /// that touched this market. `0` for fallback (no cross) or empty book.
    pub volume_nanos: u64,
    /// Wall-clock timestamp (ms since UNIX epoch) when this snapshot was
    /// computed. FE renders staleness from it.
    pub computed_at_ms: u64,
}
