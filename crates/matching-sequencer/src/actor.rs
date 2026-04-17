use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, SupervisionEvent};
use tokio::sync::broadcast;
use tokio::time::{interval_at, Instant};

use matching_engine::{MarketGroup, MarketId, MarketSet, Nanos};
use sybil_oracle::{MarketStatus, Oracle, ResolutionRecord};

use crate::account::{Account, AccountId};
use crate::block::{Block, BlockProduction};
use crate::crypto::{
    verify_signed_cancel, verify_signed_order, PublicKey, SignedCancel, SignedOrder,
};
use crate::error::SequencerError;
use crate::market_info::{AccountFillRecord, MarketMetadata, MarketSearchQuery, PricePoint};
use crate::mempool::Mempool;
use crate::portfolio::{self, PortfolioSummary};
use crate::sequencer::{BlockSequencer, OrderSubmission, PendingOrderInfo, PreparedBlock, SequencerConfig};

/// Messages sent from handles to the sequencer actor.
pub enum SequencerMsg {
    Tick,
    SubmitOrder(OrderSubmission, RpcReplyPort<Result<(), SequencerError>>),
    SubmitSignedOrder(SignedOrder, RpcReplyPort<Result<(), SequencerError>>),
    CancelSignedOrder(SignedCancel, RpcReplyPort<Result<(), SequencerError>>),
    GetLatestBlock(RpcReplyPort<Option<Block>>),
    GetAccount(AccountId, RpcReplyPort<Option<Account>>),
    GetStateRoot(RpcReplyPort<[u8; 32]>),
    ProduceBlock(RpcReplyPort<Result<Block, SequencerError>>),
    CreateAccount(i64, RpcReplyPort<Account>),
    FundAccount(
        AccountId,
        i64,
        RpcReplyPort<Result<Account, SequencerError>>,
    ),
    RegisterPubkey(
        AccountId,
        PublicKey,
        RpcReplyPort<Result<(), SequencerError>>,
    ),
    ListMarkets(RpcReplyPort<MarketSet>),
    CreateMarket(String, RpcReplyPort<MarketId>),
    CreateMarketGroup(String, Vec<MarketId>, RpcReplyPort<MarketGroup>),
    ListMarketGroups(RpcReplyPort<Vec<MarketGroup>>),
    ResolveMarket(
        MarketId,
        Nanos,
        RpcReplyPort<Result<ResolutionRecord, SequencerError>>,
    ),
    GetMarketStatus(MarketId, RpcReplyPort<MarketStatus>),
    GetAllMarketStatuses(RpcReplyPort<HashMap<MarketId, MarketStatus>>),
    GetBlock(u64, RpcReplyPort<Result<Block, SequencerError>>),
    GetMarketPrices(RpcReplyPort<HashMap<MarketId, Vec<Nanos>>>),
    GetMarketVolume(MarketId, RpcReplyPort<u64>),
    GetAllMarketVolumes(RpcReplyPort<HashMap<MarketId, u64>>),
    GetAllMarketMetadata(RpcReplyPort<HashMap<MarketId, MarketMetadata>>),
    GetPortfolio(
        AccountId,
        RpcReplyPort<Result<PortfolioSummary, SequencerError>>,
    ),
    CreateMarketWithMetadata(String, MarketMetadata, RpcReplyPort<MarketId>),
    GetMarketMetadata(MarketId, RpcReplyPort<Option<MarketMetadata>>),
    GetPriceHistory(
        MarketId,
        Option<u64>,
        Option<u64>,
        RpcReplyPort<Vec<PricePoint>>,
    ),
    GetAccountFills(
        AccountId,
        Option<MarketId>,
        usize,
        usize,
        RpcReplyPort<Vec<AccountFillRecord>>,
    ),
    SearchMarkets(MarketSearchQuery, RpcReplyPort<Vec<MarketSearchResult>>),
    GetPendingOrders(Option<AccountId>, RpcReplyPort<Vec<PendingOrderInfo>>),
    GetMarketOrderBook(MarketId, RpcReplyPort<Vec<PendingOrderInfo>>),
    PauseBlockProduction(RpcReplyPort<()>),
    ResumeBlockProduction(RpcReplyPort<()>),
}

/// A market search result enriched with metadata, prices, and volume.
#[derive(Clone, Debug)]
pub struct MarketSearchResult {
    pub market_id: MarketId,
    pub name: String,
    pub metadata: Option<MarketMetadata>,
    pub yes_price_nanos: Option<Nanos>,
    pub no_price_nanos: Option<Nanos>,
    pub volume_nanos: u64,
    pub status: MarketStatus,
}

const BLOCK_HISTORY_CAPACITY: usize = 100;

struct SequencerActor;

struct SequencerActorArgs {
    sequencer: BlockSequencer,
    store: Option<Arc<crate::store::Store>>,
    block_broadcast: broadcast::Sender<Block>,
}

struct SequencerActorState {
    sequencer: BlockSequencer,
    mempool: Mempool,
    latest_block: Option<Block>,
    block_history: VecDeque<Block>,
    block_broadcast: broadcast::Sender<Block>,
    pause_count: u32,
    store: Option<Arc<crate::store::Store>>,
}

impl SequencerActorState {
    #[tracing::instrument(
        skip_all,
        fields(height = tracing::field::Empty, mempool_size = tracing::field::Empty)
    )]
    async fn on_tick(&mut self) {
        if self.pause_count > 0 {
            return;
        }
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mempool_size = self.mempool.len();
        tracing::Span::current().record("mempool_size", mempool_size);
        let submissions = self.mempool.drain();

        let prepared = self
            .sequencer
            .prepare_block(submissions.clone(), timestamp_ms);
        tracing::Span::current().record("height", prepared.production().block.header.height);

        if let Err(error) = self.persist_block(&prepared).await {
            metrics::counter!("sybil_persistence_failures").increment(1);
            tracing::error!(error = %error, "prepared block discarded before commit");
            for submission in submissions {
                if let Err(requeue_error) = self.mempool.submit(submission) {
                    tracing::error!(error = %requeue_error, "failed to requeue submission after persistence failure");
                }
            }
            return;
        }

        let bp = self.sequencer.commit_prepared_block(prepared);
        self.record_metrics(&bp, mempool_size);
        self.push_to_history(bp.block.clone());
        let _ = self.block_broadcast.send(bp.block.clone());
        self.latest_block = Some(bp.block);
    }

    async fn persist_block(&self, prepared: &PreparedBlock) -> Result<(), SequencerError> {
        if let Some(ref store) = self.store {
            store
                .save_block(
                    &prepared.next_sequencer().accounts,
                    prepared.next_sequencer().markets(),
                    prepared.next_sequencer().market_groups(),
                    &prepared.next_sequencer().lifecycle,
                    &prepared.production().block.header,
                    prepared.next_sequencer().next_order_id(),
                    prepared.next_sequencer().pubkey_registry(),
                    prepared.next_sequencer().last_clearing_prices(),
                    prepared.next_sequencer().market_volumes(),
                )
                .await
                .map_err(|error| SequencerError::Persistence(error.to_string()))?;
        }
        Ok(())
    }

    fn record_metrics(&self, bp: &BlockProduction, mempool_size: usize) {
        metrics::counter!("sybil_blocks_produced").increment(1);
        metrics::gauge!("sybil_block_height").set(bp.block.header.height as f64);
        metrics::histogram!("sybil_orders_per_block").record(bp.block.header.order_count as f64);
        metrics::histogram!("sybil_batch_orders_per_block")
            .record(bp.block.header.order_count as f64);
        metrics::histogram!("sybil_fresh_submissions_per_block")
            .record(bp.flow_metrics.fresh_submissions as f64);
        metrics::histogram!("sybil_fresh_orders_per_block")
            .record(bp.flow_metrics.fresh_orders_received as f64);
        metrics::histogram!("sybil_carried_resting_orders_per_block")
            .record(bp.flow_metrics.carried_resting_orders as f64);
        metrics::histogram!("sybil_fresh_accepted_orders_per_block")
            .record(bp.flow_metrics.fresh_orders_accepted as f64);
        metrics::histogram!("sybil_rejections_per_block")
            .record(bp.flow_metrics.rejected_orders as f64);
        metrics::histogram!("sybil_fills_per_block").record(bp.block.header.fill_count as f64);
        metrics::gauge!("sybil_welfare_nanos").set(bp.block.total_welfare as f64);
        metrics::gauge!("sybil_volume_nanos").set(bp.block.total_volume as f64);
        metrics::gauge!("sybil_mempool_size").set(mempool_size as f64);
        metrics::gauge!("sybil_pending_orders").set(bp.flow_metrics.pending_orders_after as f64);
        metrics::histogram!("sybil_solve_time_seconds").record(bp.pipeline.total_time_secs);

        self.record_per_market_metrics(bp);
    }

    // Cardinality note: bounded by active markets this block (those with clearing
    // prices). Fine for MVP scale (tens of markets). Revisit top-N bucketing if
    // we ever exceed ~1000 concurrently active markets.
    fn record_per_market_metrics(&self, bp: &BlockProduction) {
        let order_to_market: HashMap<u64, MarketId> = bp
            .witness
            .orders
            .iter()
            .filter_map(|wo| wo.order.active_markets().next().map(|m| (wo.order.id, m)))
            .collect();

        let mut fills_per_market: HashMap<MarketId, u64> = HashMap::new();
        for fill in &bp.block.fills {
            if fill.fill_qty == 0 {
                continue;
            }
            if let Some(&market_id) = order_to_market.get(&fill.order_id) {
                *fills_per_market.entry(market_id).or_default() += 1;
            }
        }
        for (market_id, count) in fills_per_market {
            metrics::counter!(
                "sybil_market_fills_total",
                "market_id" => market_id.0.to_string()
            )
            .increment(count);
        }

        let market_volumes = self.sequencer.market_volumes();
        for (market_id, prices) in &bp.block.clearing_prices {
            for (outcome, &price) in prices.iter().enumerate() {
                metrics::gauge!(
                    "sybil_market_clearing_price_nanos",
                    "market_id" => market_id.0.to_string(),
                    "outcome" => outcome.to_string()
                )
                .set(price as f64);
            }
            if let Some(&volume) = market_volumes.get(market_id) {
                metrics::gauge!(
                    "sybil_market_volume_nanos",
                    "market_id" => market_id.0.to_string()
                )
                .set(volume as f64);
            }
        }
    }

    fn record_submission_metrics(
        &self,
        source: &'static str,
        order_count: usize,
        result: &Result<(), SequencerError>,
    ) {
        let outcome = if result.is_ok() {
            "accepted"
        } else {
            "rejected"
        };
        metrics::counter!("sybil_order_submissions_total", "source" => source, "result" => outcome)
            .increment(1);
        metrics::counter!("sybil_orders_received_total", "source" => source, "result" => outcome)
            .increment(order_count as u64);
    }

    fn record_cancel_metrics(&self, source: &'static str, result: &Result<(), SequencerError>) {
        let outcome = if result.is_ok() {
            "accepted"
        } else {
            "rejected"
        };
        metrics::counter!("sybil_order_cancels_total", "source" => source, "result" => outcome)
            .increment(1);
    }

    fn push_to_history(&mut self, block: Block) {
        if self.block_history.len() >= BLOCK_HISTORY_CAPACITY {
            self.block_history.pop_front();
        }
        self.block_history.push_back(block);
    }

    fn handle_signed_order(&mut self, signed: SignedOrder) -> Result<(), SequencerError> {
        verify_signed_order(&signed)?;

        let account_id = self
            .sequencer
            .lookup_pubkey(&signed.signer)
            .ok_or(SequencerError::UnknownSigner)?;

        let submission = OrderSubmission {
            account_id,
            orders: vec![signed.order],
            mm_constraint: None,
        };

        self.mempool.submit(submission)
    }

    fn handle_signed_cancel(&mut self, signed: SignedCancel) -> Result<(), SequencerError> {
        verify_signed_cancel(&signed)?;

        let account_id = self
            .sequencer
            .lookup_pubkey(&signed.signer)
            .ok_or(SequencerError::UnknownSigner)?;

        if account_id != signed.account_id {
            return Err(SequencerError::SignerAccountMismatch);
        }

        self.sequencer
            .cancel_pending_order(signed.account_id, signed.order_id)
    }

    fn handle_search_markets(&self, query: MarketSearchQuery) -> Vec<MarketSearchResult> {
        let markets = self.sequencer.markets();
        let mut results: Vec<MarketSearchResult> = Vec::new();

        for market in markets.iter() {
            let mid = market.id;
            let metadata = self.sequencer.market_metadata(mid);
            let status = self.sequencer.market_status(mid);

            if let Some(ref status_filter) = query.status {
                if status.as_str() != status_filter.as_str() {
                    continue;
                }
            }

            if let Some(ref text) = query.text {
                let text_lower = text.to_lowercase();
                let name_matches = market.name.to_lowercase().contains(&text_lower);
                let desc_matches = metadata
                    .as_ref()
                    .map(|m| m.description.to_lowercase().contains(&text_lower))
                    .unwrap_or(false);
                if !name_matches && !desc_matches {
                    continue;
                }
            }

            if let Some(ref filter_tags) = query.tags {
                let has_match = metadata
                    .as_ref()
                    .map(|m| filter_tags.iter().any(|t| m.tags.contains(t)))
                    .unwrap_or(false);
                if !has_match {
                    continue;
                }
            }

            if let Some(ref cat) = query.category {
                let matches = metadata
                    .as_ref()
                    .map(|m| &m.category == cat)
                    .unwrap_or(false);
                if !matches {
                    continue;
                }
            }

            let market_prices = self.sequencer.last_clearing_prices().get(&mid);
            let yes_price = market_prices.and_then(|p| p.first().copied());
            let no_price = market_prices.and_then(|p| p.get(1).copied());
            let volume = self.sequencer.market_volume(mid);

            if let Some(min_p) = query.min_yes_price {
                if yes_price.unwrap_or(0) < min_p {
                    continue;
                }
            }
            if let Some(max_p) = query.max_yes_price {
                if yes_price.unwrap_or(0) > max_p {
                    continue;
                }
            }

            if let Some(min_vol) = query.min_volume {
                if volume < min_vol {
                    continue;
                }
            }

            results.push(MarketSearchResult {
                market_id: mid,
                name: market.name.clone(),
                metadata: metadata.cloned(),
                yes_price_nanos: yes_price,
                no_price_nanos: no_price,
                volume_nanos: volume,
                status,
            });
        }

        if let Some(ref sort_field) = query.sort_by {
            match sort_field {
                crate::market_info::MarketSortField::Volume => {
                    results.sort_by(|a, b| b.volume_nanos.cmp(&a.volume_nanos));
                }
                crate::market_info::MarketSortField::CreatedAt => {
                    results.sort_by(|a, b| {
                        let a_ts = a.metadata.as_ref().map(|m| m.created_at_ms).unwrap_or(0);
                        let b_ts = b.metadata.as_ref().map(|m| m.created_at_ms).unwrap_or(0);
                        b_ts.cmp(&a_ts)
                    });
                }
                crate::market_info::MarketSortField::Name => {
                    results.sort_by(|a, b| a.name.cmp(&b.name));
                }
                crate::market_info::MarketSortField::Price => {
                    results.sort_by(|a, b| {
                        b.yes_price_nanos
                            .unwrap_or(0)
                            .cmp(&a.yes_price_nanos.unwrap_or(0))
                    });
                }
            }
        }

        let offset = query.offset.unwrap_or(0);
        let limit = query.limit.unwrap_or(100);
        results.into_iter().skip(offset).take(limit).collect()
    }

    fn handle_register_pubkey(
        &mut self,
        account_id: AccountId,
        pubkey: PublicKey,
    ) -> Result<(), SequencerError> {
        self.sequencer.register_pubkey(account_id, pubkey)
    }
}

#[ractor::async_trait]
impl Actor for SequencerActor {
    type Msg = SequencerMsg;
    type State = SequencerActorState;
    type Arguments = SequencerActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let mempool = Mempool::new(args.sequencer.config.mempool.clone());
        Ok(SequencerActorState {
            sequencer: args.sequencer,
            mempool,
            latest_block: None,
            block_history: VecDeque::new(),
            block_broadcast: args.block_broadcast,
            pause_count: 0,
            store: args.store,
        })
    }

    async fn post_start(
        &self,
        myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        let actor = myself.clone();
        let block_interval = state.sequencer.config.block_interval;
        tokio::spawn(async move {
            let mut ticker = interval_at(Instant::now() + block_interval, block_interval);
            loop {
                ticker.tick().await;
                if actor.send_message(SequencerMsg::Tick).is_err() {
                    break;
                }
            }
        });
        Ok(())
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            SequencerMsg::Tick => {
                state.on_tick().await;
            }
            SequencerMsg::SubmitOrder(submission, reply) => {
                let order_count = submission.orders.len();
                let result = state.mempool.submit(submission);
                state.record_submission_metrics("unsigned", order_count, &result);
                let _ = reply.send(result);
            }
            SequencerMsg::SubmitSignedOrder(signed, reply) => {
                let result = state.handle_signed_order(signed);
                state.record_submission_metrics("signed", 1, &result);
                let _ = reply.send(result);
            }
            SequencerMsg::CancelSignedOrder(signed, reply) => {
                let result = state.handle_signed_cancel(signed);
                state.record_cancel_metrics("signed", &result);
                let _ = reply.send(result);
            }
            SequencerMsg::GetLatestBlock(reply) => {
                let _ = reply.send(state.latest_block.clone());
            }
            SequencerMsg::GetAccount(account_id, reply) => {
                let _ = reply.send(state.sequencer.accounts.get(account_id).cloned());
            }
            SequencerMsg::GetStateRoot(reply) => {
                let root = crate::block::compute_state_root(&state.sequencer.accounts);
                let _ = reply.send(root);
            }
            SequencerMsg::ProduceBlock(reply) => {
                state.on_tick().await;
                let result = state
                    .latest_block
                    .clone()
                    .ok_or_else(|| SequencerError::Persistence("no block committed".to_string()));
                let _ = reply.send(result);
            }
            SequencerMsg::CreateAccount(initial_balance, reply) => {
                let account_id = state.sequencer.create_account(initial_balance);
                let account = state
                    .sequencer
                    .accounts
                    .get(account_id)
                    .cloned()
                    .expect("created account should exist");
                let _ = reply.send(account);
            }
            SequencerMsg::FundAccount(account_id, amount, reply) => {
                let _ = reply.send(state.sequencer.fund_account(account_id, amount));
            }
            SequencerMsg::RegisterPubkey(account_id, pubkey, reply) => {
                let _ = reply.send(state.handle_register_pubkey(account_id, pubkey));
            }
            SequencerMsg::ListMarkets(reply) => {
                let _ = reply.send(state.sequencer.markets().clone());
            }
            SequencerMsg::CreateMarket(name, reply) => {
                let _ = reply.send(state.sequencer.markets_mut().add_binary(name));
            }
            SequencerMsg::CreateMarketGroup(name, market_ids, reply) => {
                let mut group = MarketGroup::new(&name);
                for mid in &market_ids {
                    group.add_market(*mid);
                }
                state.sequencer.market_groups_mut().push(group.clone());
                let _ = reply.send(group);
            }
            SequencerMsg::ListMarketGroups(reply) => {
                let _ = reply.send(state.sequencer.market_groups().to_vec());
            }
            SequencerMsg::ResolveMarket(market_id, payout_nanos, reply) => {
                let timestamp_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let _ = reply.send(state.sequencer.resolve_market(
                    market_id,
                    payout_nanos,
                    timestamp_ms,
                ));
            }
            SequencerMsg::GetMarketStatus(market_id, reply) => {
                let _ = reply.send(state.sequencer.market_status(market_id));
            }
            SequencerMsg::GetAllMarketStatuses(reply) => {
                let _ = reply.send(state.sequencer.market_statuses().clone());
            }
            SequencerMsg::GetBlock(height, reply) => {
                let block = state
                    .block_history
                    .iter()
                    .find(|b| b.header.height == height)
                    .cloned();
                let _ = reply.send(block.ok_or(SequencerError::BlockNotFound));
            }
            SequencerMsg::GetMarketPrices(reply) => {
                let _ = reply.send(state.sequencer.last_clearing_prices().clone());
            }
            SequencerMsg::GetMarketVolume(market_id, reply) => {
                let _ = reply.send(state.sequencer.market_volume(market_id));
            }
            SequencerMsg::GetAllMarketVolumes(reply) => {
                let _ = reply.send(state.sequencer.market_volumes().clone());
            }
            SequencerMsg::GetAllMarketMetadata(reply) => {
                let _ = reply.send(state.sequencer.market_metadata_all().clone());
            }
            SequencerMsg::GetPortfolio(account_id, reply) => {
                let result = match state.sequencer.accounts.get(account_id) {
                    Some(account) => Ok(portfolio::compute_portfolio(
                        account,
                        state.sequencer.last_clearing_prices(),
                    )),
                    None => Err(SequencerError::Rejected(crate::error::Rejection {
                        order_id: 0,
                        account_id,
                        reason: crate::error::RejectionReason::AccountNotFound,
                    })),
                };
                let _ = reply.send(result);
            }
            SequencerMsg::CreateMarketWithMetadata(name, metadata, reply) => {
                let market_id = state.sequencer.markets_mut().add_binary(name);
                state.sequencer.set_market_metadata(market_id, metadata);
                let _ = reply.send(market_id);
            }
            SequencerMsg::GetMarketMetadata(market_id, reply) => {
                let _ = reply.send(state.sequencer.market_metadata(market_id).cloned());
            }
            SequencerMsg::GetPriceHistory(market_id, from_ms, to_ms, reply) => {
                let _ = reply.send(state.sequencer.price_history(market_id, from_ms, to_ms));
            }
            SequencerMsg::GetAccountFills(account_id, market_id, limit, offset, reply) => {
                let _ = reply.send(
                    state
                        .sequencer
                        .account_fills(account_id, market_id, limit, offset),
                );
            }
            SequencerMsg::SearchMarkets(query, reply) => {
                let _ = reply.send(state.handle_search_markets(query));
            }
            SequencerMsg::GetPendingOrders(account_id, reply) => {
                let _ = reply.send(state.sequencer.pending_orders_info(account_id));
            }
            SequencerMsg::GetMarketOrderBook(market_id, reply) => {
                let _ = reply.send(state.sequencer.market_orderbook(market_id));
            }
            SequencerMsg::PauseBlockProduction(reply) => {
                state.pause_count = state.pause_count.saturating_add(1);
                let _ = reply.send(());
            }
            SequencerMsg::ResumeBlockProduction(reply) => {
                state.pause_count = state.pause_count.saturating_sub(1);
                let _ = reply.send(());
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
struct SequencerHandleInner {
    actor: Arc<RwLock<Option<ActorRef<SequencerMsg>>>>,
    block_broadcast: broadcast::Sender<Block>,
}

struct SequencerSupervisor;

struct SequencerSupervisorArgs {
    config: SequencerConfig,
    store: Option<Arc<crate::store::Store>>,
    oracle: Arc<dyn Oracle>,
    handle: SequencerHandleInner,
}

struct SequencerSupervisorState {
    current_actor: Option<ActorRef<SequencerMsg>>,
    config: SequencerConfig,
    store: Option<Arc<crate::store::Store>>,
    oracle: Arc<dyn Oracle>,
    handle: SequencerHandleInner,
}

enum SequencerSupervisorMsg {
    AdoptChild(ActorRef<SequencerMsg>),
}

impl SequencerSupervisorState {
    fn publish_actor(&self, actor: Option<ActorRef<SequencerMsg>>) {
        *self
            .handle
            .actor
            .write()
            .expect("sequencer actor ref lock poisoned") = actor;
    }

    async fn spawn_child(
        &mut self,
        myself: ActorRef<SequencerSupervisorMsg>,
        sequencer: BlockSequencer,
    ) -> Result<(), ActorProcessingErr> {
        let args = SequencerActorArgs {
            sequencer,
            store: self.store.clone(),
            block_broadcast: self.handle.block_broadcast.clone(),
        };
        let (child, _) =
            <SequencerActor as Actor>::spawn_linked(None, SequencerActor, args, myself.get_cell())
                .await
                .map_err(|error| ActorProcessingErr::from(error.to_string()))?;
        self.current_actor = Some(child.clone());
        self.publish_actor(Some(child));
        Ok(())
    }

    async fn restart_from_store(&mut self, myself: ActorRef<SequencerSupervisorMsg>) {
        self.current_actor = None;
        self.publish_actor(None);

        let Some(store) = self.store.clone() else {
            tracing::error!(
                "sequencer actor exited without a persistent store; restart unavailable"
            );
            return;
        };

        let restored = match store.load_state().await {
            Ok(state) => state,
            Err(error) => {
                tracing::error!(error = %error, "failed to load sequencer snapshot for restart");
                return;
            }
        };

        let Some(state) = restored else {
            tracing::error!("no persisted sequencer snapshot available for restart");
            return;
        };

        let sequencer = BlockSequencer::restore(
            state.accounts,
            state.markets,
            state.market_groups,
            self.oracle.clone(),
            state.height,
            state.last_header,
            state.next_order_id,
            state.pubkey_registry,
            state.market_statuses,
            state.market_metadata,
            state.last_clearing_prices,
            state.market_volumes,
            self.config.clone(),
        );

        match self.spawn_child(myself, sequencer).await {
            Ok(()) => tracing::warn!("sequencer actor restarted from persistent snapshot"),
            Err(error) => {
                tracing::error!(error = %error, "failed to restart sequencer actor from snapshot");
            }
        }
    }
}

#[ractor::async_trait]
impl Actor for SequencerSupervisor {
    type Msg = SequencerSupervisorMsg;
    type State = SequencerSupervisorState;
    type Arguments = SequencerSupervisorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(SequencerSupervisorState {
            current_actor: None,
            config: args.config,
            store: args.store,
            oracle: args.oracle,
            handle: args.handle,
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            SequencerSupervisorMsg::AdoptChild(actor) => {
                state.current_actor = Some(actor.clone());
                state.publish_actor(Some(actor));
            }
        }
        Ok(())
    }

    async fn handle_supervisor_evt(
        &self,
        myself: ActorRef<Self::Msg>,
        message: SupervisionEvent,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        let Some(current_actor) = state.current_actor.as_ref() else {
            return Ok(());
        };

        match message {
            SupervisionEvent::ActorStarted(actor) if actor.get_id() == current_actor.get_id() => {
                tracing::info!("sequencer actor started under supervisor");
            }
            SupervisionEvent::ActorFailed(actor, error)
                if actor.get_id() == current_actor.get_id() =>
            {
                tracing::error!(error = %error, "sequencer actor failed; attempting restart");
                state.restart_from_store(myself).await;
            }
            SupervisionEvent::ActorTerminated(actor, _, reason)
                if actor.get_id() == current_actor.get_id() =>
            {
                if let Some(reason) = reason.as_deref() {
                    tracing::warn!(reason, "sequencer actor terminated; attempting restart");
                } else {
                    tracing::warn!("sequencer actor terminated; attempting restart");
                }
                state.restart_from_store(myself).await;
            }
            _ => {}
        }
        Ok(())
    }
}

/// Cloneable handle to the sequencer actor.
#[derive(Clone)]
pub struct SequencerHandle {
    inner: SequencerHandleInner,
}

impl SequencerHandle {
    async fn actor_ref(&self) -> Result<ActorRef<SequencerMsg>, SequencerError> {
        self.inner
            .actor
            .read()
            .expect("sequencer actor ref lock poisoned")
            .clone()
            .ok_or(SequencerError::ActorGone)
    }

    async fn rpc<T>(
        &self,
        build_message: impl FnOnce(RpcReplyPort<T>) -> SequencerMsg,
    ) -> Result<T, SequencerError>
    where
        T: Send + 'static,
    {
        let actor = self.actor_ref().await?;
        match actor.call(build_message, None).await {
            Ok(ractor::rpc::CallResult::Success(value)) => Ok(value),
            _ => Err(SequencerError::ActorGone),
        }
    }

    /// Spawn with default config (1-second block interval, default mempool).
    /// Prefer [`spawn_with_store`] for production use.
    pub fn spawn(sequencer: BlockSequencer) -> Self {
        Self::spawn_with_store(sequencer, None)
    }

    pub fn spawn_with_store(
        sequencer: BlockSequencer,
        store: Option<crate::store::Store>,
    ) -> Self {
        let oracle = sequencer.oracle();
        let config = sequencer.config.clone();
        let store = store.map(Arc::new);
        let (block_broadcast, _) = broadcast::channel(64);
        let inner = SequencerHandleInner {
            actor: Arc::new(RwLock::new(None)),
            block_broadcast: block_broadcast.clone(),
        };
        let supervisor_args = SequencerSupervisorArgs {
            config,
            store: store.clone(),
            oracle,
            handle: inner.clone(),
        };
        let (supervisor, _) =
            ractor::ActorRuntime::spawn_instant(None, SequencerSupervisor, supervisor_args)
                .expect("failed to spawn sequencer supervisor");
        let actor_args = SequencerActorArgs {
            sequencer,
            store,
            block_broadcast,
        };
        let (child, _) = ractor::ActorRuntime::spawn_linked_instant(
            None,
            SequencerActor,
            actor_args,
            supervisor.get_cell(),
        )
        .expect("failed to spawn sequencer actor");
        *inner
            .actor
            .write()
            .expect("sequencer actor ref lock poisoned") = Some(child.clone());
        supervisor
            .send_message(SequencerSupervisorMsg::AdoptChild(child))
            .expect("failed to hand child actor to supervisor");
        Self { inner }
    }

    pub async fn submit_order(&self, submission: OrderSubmission) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::SubmitOrder(submission, reply))
            .await?
    }

    pub async fn submit_signed_order(&self, signed: SignedOrder) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::SubmitSignedOrder(signed, reply))
            .await?
    }

    pub async fn cancel_signed_order(&self, signed: SignedCancel) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::CancelSignedOrder(signed, reply))
            .await?
    }

    pub async fn get_latest_block(&self) -> Result<Option<Block>, SequencerError> {
        self.rpc(SequencerMsg::GetLatestBlock).await
    }

    pub async fn get_account(
        &self,
        account_id: AccountId,
    ) -> Result<Option<Account>, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetAccount(account_id, reply))
            .await
    }

    pub async fn get_state_root(&self) -> Result<[u8; 32], SequencerError> {
        self.rpc(SequencerMsg::GetStateRoot).await
    }

    pub async fn produce_block(&self) -> Result<Block, SequencerError> {
        self.rpc(SequencerMsg::ProduceBlock).await?
    }

    pub async fn create_account(&self, initial_balance: i64) -> Result<Account, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateAccount(initial_balance, reply))
            .await
    }

    pub async fn fund_account(
        &self,
        account_id: AccountId,
        amount: i64,
    ) -> Result<Account, SequencerError> {
        self.rpc(|reply| SequencerMsg::FundAccount(account_id, amount, reply))
            .await?
    }

    pub async fn register_pubkey(
        &self,
        account_id: AccountId,
        pubkey: PublicKey,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::RegisterPubkey(account_id, pubkey, reply))
            .await?
    }

    #[tracing::instrument(skip_all)]
    pub async fn list_markets(&self) -> Result<MarketSet, SequencerError> {
        self.rpc(SequencerMsg::ListMarkets).await
    }

    pub async fn create_market(&self, name: String) -> Result<MarketId, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateMarket(name, reply))
            .await
    }

    pub async fn create_market_group(
        &self,
        name: String,
        market_ids: Vec<MarketId>,
    ) -> Result<MarketGroup, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateMarketGroup(name, market_ids, reply))
            .await
    }

    pub async fn list_market_groups(&self) -> Result<Vec<MarketGroup>, SequencerError> {
        self.rpc(SequencerMsg::ListMarketGroups).await
    }

    pub async fn resolve_market(
        &self,
        market_id: MarketId,
        payout_nanos: Nanos,
    ) -> Result<ResolutionRecord, SequencerError> {
        self.rpc(|reply| SequencerMsg::ResolveMarket(market_id, payout_nanos, reply))
            .await?
    }

    pub async fn get_market_status(
        &self,
        market_id: MarketId,
    ) -> Result<MarketStatus, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetMarketStatus(market_id, reply))
            .await
    }

    #[tracing::instrument(skip_all)]
    pub async fn get_all_market_statuses(
        &self,
    ) -> Result<HashMap<MarketId, MarketStatus>, SequencerError> {
        self.rpc(SequencerMsg::GetAllMarketStatuses).await
    }

    pub async fn get_block(&self, height: u64) -> Result<Block, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetBlock(height, reply))
            .await?
    }

    pub async fn subscribe_blocks(&self) -> Result<broadcast::Receiver<Block>, SequencerError> {
        Ok(self.inner.block_broadcast.subscribe())
    }

    #[tracing::instrument(skip_all)]
    pub async fn get_market_prices(&self) -> Result<HashMap<MarketId, Vec<Nanos>>, SequencerError> {
        self.rpc(SequencerMsg::GetMarketPrices).await
    }

    pub async fn get_portfolio(
        &self,
        account_id: AccountId,
    ) -> Result<PortfolioSummary, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetPortfolio(account_id, reply))
            .await?
    }

    pub async fn create_market_with_metadata(
        &self,
        name: String,
        metadata: MarketMetadata,
    ) -> Result<MarketId, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateMarketWithMetadata(name, metadata, reply))
            .await
    }

    pub async fn get_market_metadata(
        &self,
        market_id: MarketId,
    ) -> Result<Option<MarketMetadata>, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetMarketMetadata(market_id, reply))
            .await
    }

    pub async fn get_price_history(
        &self,
        market_id: MarketId,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
    ) -> Result<Vec<PricePoint>, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetPriceHistory(market_id, from_ms, to_ms, reply))
            .await
    }

    pub async fn get_account_fills(
        &self,
        account_id: AccountId,
        market_id: Option<MarketId>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AccountFillRecord>, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetAccountFills(account_id, market_id, limit, offset, reply))
            .await
    }

    pub async fn get_pending_orders(
        &self,
        account_id: Option<AccountId>,
    ) -> Result<Vec<PendingOrderInfo>, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetPendingOrders(account_id, reply))
            .await
    }

    pub async fn get_market_order_book(
        &self,
        market_id: MarketId,
    ) -> Result<Vec<PendingOrderInfo>, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetMarketOrderBook(market_id, reply))
            .await
    }

    pub async fn search_markets(
        &self,
        query: MarketSearchQuery,
    ) -> Result<Vec<MarketSearchResult>, SequencerError> {
        self.rpc(|reply| SequencerMsg::SearchMarkets(query, reply))
            .await
    }

    pub async fn get_market_volume(&self, market_id: MarketId) -> Result<u64, SequencerError> {
        self.rpc(|reply| SequencerMsg::GetMarketVolume(market_id, reply))
            .await
    }

    #[tracing::instrument(skip_all)]
    pub async fn get_all_market_volumes(&self) -> Result<HashMap<MarketId, u64>, SequencerError> {
        self.rpc(SequencerMsg::GetAllMarketVolumes).await
    }

    #[tracing::instrument(skip_all)]
    pub async fn get_all_market_metadata(
        &self,
    ) -> Result<HashMap<MarketId, MarketMetadata>, SequencerError> {
        self.rpc(SequencerMsg::GetAllMarketMetadata).await
    }

    pub async fn pause_block_production(&self) -> Result<(), SequencerError> {
        self.rpc(SequencerMsg::PauseBlockProduction).await
    }

    pub async fn resume_block_production(&self) -> Result<(), SequencerError> {
        self.rpc(SequencerMsg::ResumeBlockProduction).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountStore;
    use crate::sequencer::SequencerConfig;
    use crate::system_event::SystemEvent;
    use matching_engine::{outcome_buy, MarketSet, NANOS_PER_DOLLAR};
    use std::sync::Arc;
    use std::time::Duration;
    use sybil_oracle::AdminOracle;

    fn make_test_sequencer() -> (BlockSequencer, AccountId) {
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        markets.add_binary("Test");
        let oracle = Arc::new(AdminOracle::new());
        (
            BlockSequencer::with_default_solver(
                accounts,
                markets,
                vec![],
                oracle,
                SequencerConfig::default(),
            ),
            aid,
        )
    }

    #[tokio::test]
    async fn test_spawn_and_produce_block() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let block = handle.produce_block().await.unwrap();
        assert_eq!(block.header.height, 1);
    }

    #[tokio::test]
    async fn test_submit_and_produce() {
        let (seq, aid) = make_test_sequencer();
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");

        let handle = SequencerHandle::spawn(seq);

        let sub = OrderSubmission {
            account_id: aid,
            orders: vec![outcome_buy(&ms, 0, m0, 0, 500_000_000, 1)],
            mm_constraint: None,
        };

        handle.submit_order(sub).await.unwrap();

        let block = handle.produce_block().await.unwrap();
        assert_eq!(block.header.height, 1);
        assert!(block.header.order_count >= 1);
    }

    #[tokio::test]
    async fn test_get_state_root() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let root = handle.get_state_root().await.unwrap();
        assert_ne!(root, [0u8; 32]); // non-empty accounts -> non-zero root
    }

    #[tokio::test]
    async fn test_get_account() {
        let (seq, aid) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let account = handle.get_account(aid).await.unwrap();
        assert!(account.is_some());
        assert_eq!(account.unwrap().balance, 100 * NANOS_PER_DOLLAR as i64);
    }

    #[tokio::test]
    async fn test_get_latest_block_none_initially() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let block = handle.get_latest_block().await.unwrap();
        assert!(block.is_none());
    }

    #[tokio::test]
    async fn test_get_latest_block_after_produce() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        handle.produce_block().await.unwrap();

        let block = handle.get_latest_block().await.unwrap();
        assert!(block.is_some());
        assert_eq!(block.unwrap().header.height, 1);
    }

    #[tokio::test]
    async fn test_graceful_shutdown() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        handle.produce_block().await.unwrap();
        drop(handle);

        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn test_block_chain_via_actor() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let block1 = handle.produce_block().await.unwrap();
        assert_eq!(block1.header.height, 1);
        assert_eq!(block1.header.parent_hash, [0u8; 32]); // genesis

        let block2 = handle.produce_block().await.unwrap();
        assert_eq!(block2.header.height, 2);
        let expected = crate::block::hash_header(&block1.header);
        assert_eq!(block2.header.parent_hash, expected);
    }

    #[tokio::test]
    async fn test_state_root_changes_after_fill() {
        let mut accounts = AccountStore::new();
        let buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let seller = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");
        accounts
            .get_mut(seller)
            .unwrap()
            .positions
            .insert((m0, 0), 100);

        let seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            Arc::new(AdminOracle::new()),
            SequencerConfig::default(),
        );
        let handle = SequencerHandle::spawn(seq);

        let root_before = handle.get_state_root().await.unwrap();

        let buy_sub = OrderSubmission {
            account_id: buyer,
            orders: vec![outcome_buy(&markets, 0, m0, 0, 600_000_000, 5)],
            mm_constraint: None,
        };
        handle.submit_order(buy_sub).await.unwrap();

        let sell_sub = OrderSubmission {
            account_id: seller,
            orders: vec![matching_engine::outcome_sell(
                &markets,
                0,
                m0,
                0,
                400_000_000,
                5,
            )],
            mm_constraint: None,
        };
        handle.submit_order(sell_sub).await.unwrap();

        let block = handle.produce_block().await.unwrap();

        if block.orders_filled > 0 {
            let root_after = handle.get_state_root().await.unwrap();
            assert_ne!(root_before, root_after);
        }
    }

    #[tokio::test]
    async fn test_create_account() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let account = handle
            .create_account(50 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();
        assert_eq!(account.balance, 50 * NANOS_PER_DOLLAR as i64);

        let fetched = handle.get_account(account.id).await.unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().balance, 50 * NANOS_PER_DOLLAR as i64);
    }

    #[tokio::test]
    async fn test_fund_account() {
        let (seq, aid) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let account = handle
            .fund_account(aid, 25 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();
        assert_eq!(account.balance, 125 * NANOS_PER_DOLLAR as i64);
    }

    #[tokio::test]
    async fn test_system_events_emitted_for_create_and_fund() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let account = handle
            .create_account(50 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();
        handle
            .fund_account(account.id, 25 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();

        let block = handle.produce_block().await.unwrap();
        assert_eq!(block.system_events.len(), 2);

        match &block.system_events[0] {
            SystemEvent::CreateAccount {
                account_id,
                initial_balance,
            } => {
                assert_eq!(*account_id, account.id);
                assert_eq!(*initial_balance, 50 * NANOS_PER_DOLLAR as i64);
            }
            other => panic!("expected CreateAccount event, got {:?}", other),
        }

        match &block.system_events[1] {
            SystemEvent::Deposit { account_id, amount } => {
                assert_eq!(*account_id, account.id);
                assert_eq!(*amount, 25 * NANOS_PER_DOLLAR as i64);
            }
            other => panic!("expected Deposit event, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_fund_nonexistent_account() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let result = handle.fund_account(AccountId(999), 100).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_register_pubkey_and_signed_order() {
        let (seq, aid) = make_test_sequencer();
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(signing_key.verifying_key().clone());

        handle.register_pubkey(aid, pubkey).await.unwrap();

        let order = outcome_buy(&ms, 0, m0, 0, 500_000_000, 1);
        let signed = crate::crypto::sign_order(&order, &signing_key);
        handle.submit_signed_order(signed).await.unwrap();

        let block = handle.produce_block().await.unwrap();
        assert!(block.header.order_count >= 1);
    }

    #[tokio::test]
    async fn test_cancel_signed_order_by_owner() {
        let (seq, aid) = make_test_sequencer();
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(signing_key.verifying_key().clone());
        handle.register_pubkey(aid, pubkey).await.unwrap();

        handle
            .submit_order(OrderSubmission {
                account_id: aid,
                orders: vec![outcome_buy(&ms, 1, m0, 0, 500_000_000, 1)],
                mm_constraint: None,
            })
            .await
            .unwrap();
        handle.produce_block().await.unwrap();

        let pending = handle.get_pending_orders(Some(aid)).await.unwrap();
        assert_eq!(pending.len(), 1);

        let cancel = crate::crypto::sign_cancel(aid, pending[0].order_id, &signing_key);
        handle.cancel_signed_order(cancel).await.unwrap();

        let pending = handle.get_pending_orders(Some(aid)).await.unwrap();
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn test_cancel_signed_order_rejects_wrong_account_claim() {
        let (seq, aid) = make_test_sequencer();
        let other = AccountId(aid.0 + 1);
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq);

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(signing_key.verifying_key().clone());
        handle.register_pubkey(aid, pubkey).await.unwrap();

        handle
            .submit_order(OrderSubmission {
                account_id: aid,
                orders: vec![outcome_buy(&ms, 1, m0, 0, 500_000_000, 1)],
                mm_constraint: None,
            })
            .await
            .unwrap();
        handle.produce_block().await.unwrap();

        let pending = handle.get_pending_orders(Some(aid)).await.unwrap();
        let cancel = crate::crypto::sign_cancel(other, pending[0].order_id, &signing_key);
        let error = handle.cancel_signed_order(cancel).await.unwrap_err();
        assert!(matches!(error, SequencerError::SignerAccountMismatch));
    }

    #[tokio::test]
    async fn test_register_pubkey_duplicate() {
        let (seq, aid) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let signing_key =
            <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
            );
        let pubkey = PublicKey(signing_key.verifying_key().clone());

        handle.register_pubkey(aid, pubkey.clone()).await.unwrap();
        let result = handle.register_pubkey(aid, pubkey).await;
        assert!(matches!(
            result,
            Err(SequencerError::AccountAlreadyRegistered)
        ));
    }

    #[tokio::test]
    async fn test_list_and_create_markets() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let markets = handle.list_markets().await.unwrap();
        assert_eq!(markets.len(), 1);

        let new_id = handle
            .create_market("New Market".to_string())
            .await
            .unwrap();
        let markets = handle.list_markets().await.unwrap();
        assert_eq!(markets.len(), 2);
        assert!(markets.get(new_id).is_some());
    }

    #[tokio::test]
    async fn test_create_and_list_market_groups() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let m1 = handle.create_market("A wins".to_string()).await.unwrap();
        let m2 = handle.create_market("B wins".to_string()).await.unwrap();

        let group = handle
            .create_market_group("Election".to_string(), vec![m1, m2])
            .await
            .unwrap();
        assert_eq!(group.name, "Election");
        assert_eq!(group.markets.len(), 2);

        let groups = handle.list_market_groups().await.unwrap();
        assert_eq!(groups.len(), 1);
    }

    #[tokio::test]
    async fn test_resolve_market() {
        let (seq, _aid) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let m0 = MarketId::new(0);
        let record = handle.resolve_market(m0, NANOS_PER_DOLLAR).await.unwrap();
        assert_eq!(record.payout_nanos, NANOS_PER_DOLLAR);
        assert_eq!(record.market_id, m0);
    }

    #[tokio::test]
    async fn test_system_event_emitted_for_market_resolution() {
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        let m0 = markets.add_binary("Test");
        accounts.get_mut(aid).unwrap().positions.insert((m0, 0), 10);

        let seq = BlockSequencer::with_default_solver(
            accounts,
            markets,
            vec![],
            Arc::new(AdminOracle::new()),
            SequencerConfig::default(),
        );
        let handle = SequencerHandle::spawn(seq);

        handle.resolve_market(m0, NANOS_PER_DOLLAR).await.unwrap();
        let block = handle.produce_block().await.unwrap();

        assert_eq!(block.system_events.len(), 1);
        match &block.system_events[0] {
            SystemEvent::MarketResolved {
                market_id,
                payout_nanos,
                affected_accounts,
            } => {
                assert_eq!(*market_id, m0);
                assert_eq!(*payout_nanos, NANOS_PER_DOLLAR);
                assert_eq!(affected_accounts, &vec![aid]);
            }
            other => panic!("expected MarketResolved event, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_resolve_nonexistent_market() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let result = handle
            .resolve_market(MarketId::new(999), NANOS_PER_DOLLAR)
            .await;
        assert!(matches!(result, Err(SequencerError::MarketNotFound)));
    }

    #[tokio::test]
    async fn test_get_block_by_height() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        handle.produce_block().await.unwrap();
        handle.produce_block().await.unwrap();

        let block = handle.get_block(1).await.unwrap();
        assert_eq!(block.header.height, 1);

        let block = handle.get_block(2).await.unwrap();
        assert_eq!(block.header.height, 2);

        let result = handle.get_block(99).await;
        assert!(matches!(result, Err(SequencerError::BlockNotFound)));
    }

    #[tokio::test]
    async fn test_subscribe_blocks() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq);

        let mut rx = handle.subscribe_blocks().await.unwrap();

        handle.produce_block().await.unwrap();

        let block = rx.recv().await.unwrap();
        assert_eq!(block.header.height, 1);
    }
}
