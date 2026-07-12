use super::*;

pub(super) struct SequencerActor;

pub(super) struct SequencerActorArgs {
    pub(super) sequencer: BlockSequencer,
    pub(super) store: Option<Arc<crate::store::Store>>,
    pub(super) block_broadcast: broadcast::Sender<SealedBlock>,
    pub(super) mailbox_monitor: MailboxMonitor,
}

pub(super) struct SequencerActorState {
    pub(super) sequencer: BlockSequencer,
    pub(super) latest_block: Option<SealedBlock>,
    pub(super) block_history: VecDeque<SealedBlock>,
    pub(super) block_broadcast: broadcast::Sender<SealedBlock>,
    pub(super) pause_count: u32,
    pub(super) halted_error: Option<SequencerError>,
    pub(super) store: Option<Arc<crate::store::Store>>,
    pub(super) global_submission_bucket: TokenBucket,
    pub(super) account_submission_buckets: HashMap<AccountId, TokenBucket>,
    pub(super) mailbox_monitor: MailboxMonitor,
    /// Per-market indicative snapshots from the C2 shadow-solver. Cache
    /// lives on the actor (not `BlockSequencer`) so pure-core stays pure.
    /// Empty until the first `IndicativeUpdate` arrives; lookup-miss
    /// returns `IndicativeSnapshot::default()` (None/None/0/0).
    pub(super) indicative_cache: HashMap<MarketId, IndicativeSnapshot>,
    pub(super) indicative_solve_gate: IndicativeSolveGate,
    #[cfg(test)]
    pub(super) next_tick_hold: Option<SequencerTestTickHold>,
}

/// Messages sent from handles to the sequencer actor.
pub enum SequencerMsg {
    Tick,
    #[cfg(test)]
    TestCrashOnNextBlock(SequencerTestCrashpoint),
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
    GetBlockPage(
        Option<u64>,
        usize,
        RpcReplyPort<Result<Vec<SealedBlock>, SequencerError>>,
    ),
    GetBlock(u64, RpcReplyPort<Result<SealedBlock, SequencerError>>),
    GetDaArtifact(u64, RpcReplyPort<Result<DaArtifactLookup, SequencerError>>),
    GetDaManifest(u64, RpcReplyPort<Result<DaManifestLookup, SequencerError>>),
    CreateMarketWithMetadata(
        String,
        MarketMetadata,
        RpcReplyPort<Result<MarketId, SequencerError>>,
    ),
    GetPriceHistory(
        MarketId,
        Option<u64>,
        Option<u64>,
        Option<u64>,
        usize,
        RpcReplyPort<Result<PriceHistoryPage, SequencerError>>,
    ),
    GetPriceCandles(
        MarketId,
        u32,
        Option<u64>,
        Option<u64>,
        Option<u64>,
        usize,
        RpcReplyPort<Result<PriceCandlePage, SequencerError>>,
    ),
    GetAccountFills(
        AccountId,
        Option<MarketId>,
        usize,
        usize,
        RpcReplyPort<Vec<AccountFillRecord>>,
    ),
    GetAccountFillsAfter(
        AccountId,
        Option<MarketId>,
        Option<AccountFillCursor>,
        usize,
        RpcReplyPort<Vec<AccountFillRecord>>,
    ),
    GetEquitySeries(
        AccountId,
        u64,
        RpcReplyPort<Vec<crate::aggregates::EquityPoint>>,
    ),
    /// Ranked leaderboard over a window (SYB-59). `since_ms == 0` is all-time
    /// (fully in-memory); a non-zero `since_ms` reads per-account windowed
    /// baselines from the durable equity store. Returns at most `limit` rows,
    /// already sorted (PnL desc, then account id asc).
    Leaderboard(u64, usize, RpcReplyPort<Vec<LeaderboardRow>>),
    GetAccountEvents(
        AccountId,
        usize,
        Option<(u64, u64)>,
        Option<String>,
        RpcReplyPort<Vec<crate::aggregates::HistoryEvent>>,
    ),
    ListAutoResolutionRecords(RpcReplyPort<Result<Vec<AutoResolutionRecord>, SequencerError>>),
    PutAutoResolutionRecord(
        AutoResolutionRecord,
        RpcReplyPort<Result<(), SequencerError>>,
    ),
    PauseBlockProduction(RpcReplyPort<()>),
    ResumeBlockProduction(RpcReplyPort<()>),
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
