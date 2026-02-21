use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::{interval_at, Instant};

use matching_engine::{MarketGroup, MarketId, MarketSet, Nanos};
use sybil_oracle::{MarketStatus, ResolutionRecord};

use crate::account::{Account, AccountId};
use crate::block::Block;
use crate::crypto::{verify_signed_order, PublicKey, SignedOrder};
use crate::error::SequencerError;
use crate::market_info::{AccountFillRecord, MarketMetadata, MarketSearchQuery, PricePoint};
use crate::mempool::{Mempool, MempoolConfig};
use crate::portfolio::{self, PortfolioSummary};
use crate::sequencer::{BlockSequencer, OrderSubmission};

/// Messages sent from handles to the actor.
pub enum Message {
    SubmitOrder {
        submission: OrderSubmission,
        respond_to: oneshot::Sender<Result<(), SequencerError>>,
    },
    SubmitSignedOrder {
        signed: SignedOrder,
        respond_to: oneshot::Sender<Result<(), SequencerError>>,
    },
    GetLatestBlock {
        respond_to: oneshot::Sender<Option<Block>>,
    },
    GetAccount {
        account_id: AccountId,
        respond_to: oneshot::Sender<Option<Account>>,
    },
    GetStateRoot {
        respond_to: oneshot::Sender<[u8; 32]>,
    },
    /// Force-produce a block immediately (for testing).
    ProduceBlock { respond_to: oneshot::Sender<Block> },
    // --- New messages for API support ---
    CreateAccount {
        initial_balance: i64,
        respond_to: oneshot::Sender<Account>,
    },
    FundAccount {
        account_id: AccountId,
        amount: i64,
        respond_to: oneshot::Sender<Result<Account, SequencerError>>,
    },
    RegisterPubkey {
        account_id: AccountId,
        pubkey: PublicKey,
        respond_to: oneshot::Sender<Result<(), SequencerError>>,
    },
    ListMarkets {
        respond_to: oneshot::Sender<MarketSet>,
    },
    CreateMarket {
        name: String,
        respond_to: oneshot::Sender<MarketId>,
    },
    CreateMarketGroup {
        name: String,
        market_ids: Vec<MarketId>,
        respond_to: oneshot::Sender<MarketGroup>,
    },
    ListMarketGroups {
        respond_to: oneshot::Sender<Vec<MarketGroup>>,
    },
    ResolveMarket {
        market_id: MarketId,
        payout_nanos: Nanos,
        respond_to: oneshot::Sender<Result<ResolutionRecord, SequencerError>>,
    },
    GetMarketStatus {
        market_id: MarketId,
        respond_to: oneshot::Sender<MarketStatus>,
    },
    GetAllMarketStatuses {
        respond_to: oneshot::Sender<HashMap<MarketId, MarketStatus>>,
    },
    GetBlock {
        height: u64,
        respond_to: oneshot::Sender<Result<Block, SequencerError>>,
    },
    SubscribeBlocks {
        respond_to: oneshot::Sender<broadcast::Receiver<Block>>,
    },
    GetMarketPrices {
        respond_to: oneshot::Sender<HashMap<MarketId, Vec<Nanos>>>,
    },
    // --- New messages for enriched API ---
    GetPortfolio {
        account_id: AccountId,
        respond_to: oneshot::Sender<Result<PortfolioSummary, SequencerError>>,
    },
    CreateMarketWithMetadata {
        name: String,
        metadata: MarketMetadata,
        respond_to: oneshot::Sender<MarketId>,
    },
    GetMarketMetadata {
        market_id: MarketId,
        respond_to: oneshot::Sender<Option<MarketMetadata>>,
    },
    GetPriceHistory {
        market_id: MarketId,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
        respond_to: oneshot::Sender<Vec<PricePoint>>,
    },
    GetAccountFills {
        account_id: AccountId,
        market_id: Option<MarketId>,
        limit: usize,
        offset: usize,
        respond_to: oneshot::Sender<Vec<AccountFillRecord>>,
    },
    SearchMarkets {
        query: MarketSearchQuery,
        respond_to: oneshot::Sender<Vec<MarketSearchResult>>,
    },
    PauseBlockProduction {
        respond_to: oneshot::Sender<()>,
    },
    ResumeBlockProduction {
        respond_to: oneshot::Sender<()>,
    },
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

/// The sequencer actor. Runs in a tokio task, produces blocks on a timer.
struct SequencerActor {
    sequencer: BlockSequencer,
    mempool: Mempool,
    receiver: mpsc::Receiver<Message>,
    latest_block: Option<Block>,
    /// P256 public key to account mapping.
    pubkey_registry: HashMap<PublicKey, AccountId>,
    /// Recent block history (ring buffer, last N blocks).
    block_history: Vec<Block>,
    /// Broadcast channel for new blocks (SSE).
    block_broadcast: broadcast::Sender<Block>,
    /// Last known clearing prices across all markets.
    last_prices: HashMap<MarketId, Vec<Nanos>>,
    /// Interval between block production ticks.
    block_interval: Duration,
    /// Whether block production is paused (for simulation freeze during LLM calls).
    paused: bool,
}

impl SequencerActor {
    fn new(
        sequencer: BlockSequencer,
        mempool: Mempool,
        receiver: mpsc::Receiver<Message>,
        block_interval: Duration,
    ) -> Self {
        let (block_broadcast, _) = broadcast::channel(64);
        Self {
            sequencer,
            mempool,
            receiver,
            latest_block: None,
            pubkey_registry: HashMap::new(),
            block_history: Vec::new(),
            block_broadcast,
            last_prices: HashMap::new(),
            block_interval,
            paused: false,
        }
    }

    async fn run(mut self) {
        let mut ticker = interval_at(
            Instant::now() + self.block_interval,
            self.block_interval,
        );

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    self.on_tick();
                }
                msg = self.receiver.recv() => {
                    match msg {
                        Some(msg) => self.handle(msg),
                        None => break, // all handles dropped
                    }
                }
            }
        }
    }

    fn on_tick(&mut self) {
        if self.paused {
            return;
        }
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let submissions = self.mempool.drain();
        let bp = self.sequencer.produce_block(submissions, timestamp_ms);
        let block = bp.block;

        // Update last known prices from this block
        for (market_id, prices) in &block.clearing_prices {
            self.last_prices.insert(*market_id, prices.clone());
        }

        // Store in history (ring buffer)
        if self.block_history.len() >= BLOCK_HISTORY_CAPACITY {
            self.block_history.remove(0);
        }
        self.block_history.push(block.clone());

        // Broadcast to SSE subscribers (ignore if no receivers)
        let _ = self.block_broadcast.send(block.clone());

        self.latest_block = Some(block);
    }

    fn handle(&mut self, msg: Message) {
        match msg {
            Message::SubmitOrder {
                submission,
                respond_to,
            } => {
                let result = self.mempool.submit(submission);
                let _ = respond_to.send(result);
            }
            Message::SubmitSignedOrder { signed, respond_to } => {
                let result = self.handle_signed_order(signed);
                let _ = respond_to.send(result);
            }
            Message::GetLatestBlock { respond_to } => {
                let _ = respond_to.send(self.latest_block.clone());
            }
            Message::GetAccount {
                account_id,
                respond_to,
            } => {
                let account = self.sequencer.accounts.get(account_id).cloned();
                let _ = respond_to.send(account);
            }
            Message::GetStateRoot { respond_to } => {
                let root = crate::block::compute_state_root(&self.sequencer.accounts);
                let _ = respond_to.send(root);
            }
            Message::ProduceBlock { respond_to } => {
                self.on_tick();
                let _ = respond_to.send(self.latest_block.clone().unwrap());
            }
            Message::CreateAccount {
                initial_balance,
                respond_to,
            } => {
                let account_id = self.sequencer.accounts.create_account(initial_balance);
                let account = self.sequencer.accounts.get(account_id).cloned().unwrap();
                let _ = respond_to.send(account);
            }
            Message::FundAccount {
                account_id,
                amount,
                respond_to,
            } => {
                let result = match self.sequencer.accounts.get_mut(account_id) {
                    Some(account) => {
                        account.balance += amount;
                        account.total_deposited += amount;
                        Ok(account.clone())
                    }
                    None => Err(SequencerError::Rejected(crate::error::Rejection {
                        order_id: 0,
                        account_id,
                        reason: crate::error::RejectionReason::AccountNotFound,
                    })),
                };
                let _ = respond_to.send(result);
            }
            Message::RegisterPubkey {
                account_id,
                pubkey,
                respond_to,
            } => {
                let result = self.handle_register_pubkey(account_id, pubkey);
                let _ = respond_to.send(result);
            }
            Message::ListMarkets { respond_to } => {
                let _ = respond_to.send(self.sequencer.markets().clone());
            }
            Message::CreateMarket { name, respond_to } => {
                let market_id = self.sequencer.markets_mut().add_binary(name);
                let _ = respond_to.send(market_id);
            }
            Message::CreateMarketGroup {
                name,
                market_ids,
                respond_to,
            } => {
                let mut group = MarketGroup::new(&name);
                for mid in &market_ids {
                    group.add_market(*mid);
                }
                self.sequencer.market_groups_mut().push(group.clone());
                let _ = respond_to.send(group);
            }
            Message::ListMarketGroups { respond_to } => {
                let _ = respond_to.send(self.sequencer.market_groups().to_vec());
            }
            Message::ResolveMarket {
                market_id,
                payout_nanos,
                respond_to,
            } => {
                let timestamp_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let result = self
                    .sequencer
                    .resolve_market(market_id, payout_nanos, timestamp_ms);
                let _ = respond_to.send(result);
            }
            Message::GetMarketStatus {
                market_id,
                respond_to,
            } => {
                let status = self.sequencer.market_status(market_id);
                let _ = respond_to.send(status);
            }
            Message::GetAllMarketStatuses { respond_to } => {
                let statuses = self.sequencer.market_statuses().clone();
                let _ = respond_to.send(statuses);
            }
            Message::GetBlock { height, respond_to } => {
                let block = self
                    .block_history
                    .iter()
                    .find(|b| b.header.height == height)
                    .cloned();
                let result = block.ok_or(SequencerError::BlockNotFound);
                let _ = respond_to.send(result);
            }
            Message::SubscribeBlocks { respond_to } => {
                let rx = self.block_broadcast.subscribe();
                let _ = respond_to.send(rx);
            }
            Message::GetMarketPrices { respond_to } => {
                let _ = respond_to.send(self.last_prices.clone());
            }
            Message::GetPortfolio {
                account_id,
                respond_to,
            } => {
                let result = match self.sequencer.accounts.get(account_id) {
                    Some(account) => Ok(portfolio::compute_portfolio(account, &self.last_prices)),
                    None => Err(SequencerError::Rejected(crate::error::Rejection {
                        order_id: 0,
                        account_id,
                        reason: crate::error::RejectionReason::AccountNotFound,
                    })),
                };
                let _ = respond_to.send(result);
            }
            Message::CreateMarketWithMetadata {
                name,
                metadata,
                respond_to,
            } => {
                let market_id = self.sequencer.markets_mut().add_binary(name);
                self.sequencer.set_market_metadata(market_id, metadata);
                let _ = respond_to.send(market_id);
            }
            Message::GetMarketMetadata {
                market_id,
                respond_to,
            } => {
                let metadata = self.sequencer.market_metadata(market_id).cloned();
                let _ = respond_to.send(metadata);
            }
            Message::GetPriceHistory {
                market_id,
                from_ms,
                to_ms,
                respond_to,
            } => {
                let history = self.sequencer.price_history(market_id, from_ms, to_ms);
                let _ = respond_to.send(history);
            }
            Message::GetAccountFills {
                account_id,
                market_id,
                limit,
                offset,
                respond_to,
            } => {
                let fills = self
                    .sequencer
                    .account_fills(account_id, market_id, limit, offset);
                let _ = respond_to.send(fills);
            }
            Message::SearchMarkets { query, respond_to } => {
                let results = self.handle_search_markets(query);
                let _ = respond_to.send(results);
            }
            Message::PauseBlockProduction { respond_to } => {
                self.paused = true;
                let _ = respond_to.send(());
            }
            Message::ResumeBlockProduction { respond_to } => {
                self.paused = false;
                let _ = respond_to.send(());
            }
        }
    }

    fn handle_signed_order(&mut self, signed: SignedOrder) -> Result<(), SequencerError> {
        // Verify signature
        verify_signed_order(&signed)?;

        // Look up account by pubkey
        let account_id = self
            .pubkey_registry
            .get(&signed.signer)
            .copied()
            .ok_or(SequencerError::UnknownSigner)?;

        // Create an OrderSubmission and route to mempool
        let submission = OrderSubmission {
            account_id,
            orders: vec![signed.order],
            mm_constraint: None,
        };

        self.mempool.submit(submission)
    }

    fn handle_search_markets(&self, query: MarketSearchQuery) -> Vec<MarketSearchResult> {
        let markets = self.sequencer.markets();
        let mut results: Vec<MarketSearchResult> = Vec::new();

        for market in markets.iter() {
            let mid = market.id;
            let metadata = self.sequencer.market_metadata(mid);
            let status = self.sequencer.market_status(mid);

            // Filter by status
            if let Some(ref status_filter) = query.status {
                if status.as_str() != status_filter.as_str() {
                    continue;
                }
            }

            // Filter by text (name + description)
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

            // Filter by tags
            if let Some(ref filter_tags) = query.tags {
                let has_match = metadata
                    .as_ref()
                    .map(|m| filter_tags.iter().any(|t| m.tags.contains(t)))
                    .unwrap_or(false);
                if !has_match {
                    continue;
                }
            }

            // Filter by category
            if let Some(ref cat) = query.category {
                let matches = metadata
                    .as_ref()
                    .map(|m| &m.category == cat)
                    .unwrap_or(false);
                if !matches {
                    continue;
                }
            }

            let market_prices = self.last_prices.get(&mid);
            let yes_price = market_prices.and_then(|p| p.first().copied());
            let no_price = market_prices.and_then(|p| p.get(1).copied());
            let volume = self.sequencer.market_volume(mid);

            // Filter by price range
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

            // Filter by volume
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

        // Sort
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

        // Paginate
        let offset = query.offset.unwrap_or(0);
        let limit = query.limit.unwrap_or(100);
        results.into_iter().skip(offset).take(limit).collect()
    }

    fn handle_register_pubkey(
        &mut self,
        account_id: AccountId,
        pubkey: PublicKey,
    ) -> Result<(), SequencerError> {
        // Check that the account exists
        if self.sequencer.accounts.get(account_id).is_none() {
            return Err(SequencerError::Rejected(crate::error::Rejection {
                order_id: 0,
                account_id,
                reason: crate::error::RejectionReason::AccountNotFound,
            }));
        }

        // Check that the pubkey is not already registered
        if self.pubkey_registry.contains_key(&pubkey) {
            return Err(SequencerError::AccountAlreadyRegistered);
        }

        self.pubkey_registry.insert(pubkey, account_id);
        Ok(())
    }
}

/// Cloneable handle to the sequencer actor.
#[derive(Clone)]
pub struct SequencerHandle {
    sender: mpsc::Sender<Message>,
}

impl SequencerHandle {
    /// Spawn a new sequencer actor and return a handle.
    pub fn spawn(sequencer: BlockSequencer, mempool_config: MempoolConfig) -> Self {
        Self::spawn_with_interval(sequencer, mempool_config, Duration::from_secs(1))
    }

    /// Spawn with a custom block production interval.
    pub fn spawn_with_interval(
        sequencer: BlockSequencer,
        mempool_config: MempoolConfig,
        block_interval: Duration,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(256);
        let mempool = Mempool::new(mempool_config);
        let actor = SequencerActor::new(sequencer, mempool, receiver, block_interval);
        tokio::spawn(actor.run());
        Self { sender }
    }

    /// Submit an unsigned order submission.
    pub async fn submit_order(&self, submission: OrderSubmission) -> Result<(), SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::SubmitOrder {
                submission,
                respond_to: tx,
            })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)?
    }

    /// Submit a P256-signed order.
    pub async fn submit_signed_order(&self, signed: SignedOrder) -> Result<(), SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::SubmitSignedOrder {
                signed,
                respond_to: tx,
            })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)?
    }

    /// Get the latest produced block.
    pub async fn get_latest_block(&self) -> Result<Option<Block>, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::GetLatestBlock { respond_to: tx })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }

    /// Get an account by ID.
    pub async fn get_account(
        &self,
        account_id: AccountId,
    ) -> Result<Option<Account>, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::GetAccount {
                account_id,
                respond_to: tx,
            })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }

    /// Get the current state root.
    pub async fn get_state_root(&self) -> Result<[u8; 32], SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::GetStateRoot { respond_to: tx })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }

    /// Force-produce a block immediately (for testing).
    pub async fn produce_block(&self) -> Result<Block, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::ProduceBlock { respond_to: tx })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }

    /// Create a new account with the given initial balance (in nanos).
    pub async fn create_account(&self, initial_balance: i64) -> Result<Account, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::CreateAccount {
                initial_balance,
                respond_to: tx,
            })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }

    /// Fund an existing account by the given amount (in nanos).
    pub async fn fund_account(
        &self,
        account_id: AccountId,
        amount: i64,
    ) -> Result<Account, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::FundAccount {
                account_id,
                amount,
                respond_to: tx,
            })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)?
    }

    /// Register a P256 public key for an account.
    pub async fn register_pubkey(
        &self,
        account_id: AccountId,
        pubkey: PublicKey,
    ) -> Result<(), SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::RegisterPubkey {
                account_id,
                pubkey,
                respond_to: tx,
            })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)?
    }

    /// List all markets.
    pub async fn list_markets(&self) -> Result<MarketSet, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::ListMarkets { respond_to: tx })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }

    /// Create a new binary market.
    pub async fn create_market(&self, name: String) -> Result<MarketId, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::CreateMarket {
                name,
                respond_to: tx,
            })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }

    /// Create a market group (mutually exclusive markets).
    pub async fn create_market_group(
        &self,
        name: String,
        market_ids: Vec<MarketId>,
    ) -> Result<MarketGroup, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::CreateMarketGroup {
                name,
                market_ids,
                respond_to: tx,
            })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }

    /// List all market groups.
    pub async fn list_market_groups(&self) -> Result<Vec<MarketGroup>, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::ListMarketGroups { respond_to: tx })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }

    /// Resolve a market with the given YES payout (0 to NANOS_PER_DOLLAR).
    pub async fn resolve_market(
        &self,
        market_id: MarketId,
        payout_nanos: Nanos,
    ) -> Result<ResolutionRecord, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::ResolveMarket {
                market_id,
                payout_nanos,
                respond_to: tx,
            })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)?
    }

    /// Get the oracle-tracked status for a market.
    pub async fn get_market_status(
        &self,
        market_id: MarketId,
    ) -> Result<MarketStatus, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::GetMarketStatus {
                market_id,
                respond_to: tx,
            })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }

    /// Get all explicitly tracked market statuses.
    pub async fn get_all_market_statuses(
        &self,
    ) -> Result<HashMap<MarketId, MarketStatus>, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::GetAllMarketStatuses { respond_to: tx })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }

    /// Get a block by height.
    pub async fn get_block(&self, height: u64) -> Result<Block, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::GetBlock {
                height,
                respond_to: tx,
            })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)?
    }

    /// Subscribe to new block events (for SSE streaming).
    pub async fn subscribe_blocks(&self) -> Result<broadcast::Receiver<Block>, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::SubscribeBlocks { respond_to: tx })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }

    /// Get last known clearing prices for all markets.
    pub async fn get_market_prices(&self) -> Result<HashMap<MarketId, Vec<Nanos>>, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::GetMarketPrices { respond_to: tx })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }

    /// Get portfolio summary for an account.
    pub async fn get_portfolio(
        &self,
        account_id: AccountId,
    ) -> Result<PortfolioSummary, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::GetPortfolio {
                account_id,
                respond_to: tx,
            })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)?
    }

    /// Create a market with metadata.
    pub async fn create_market_with_metadata(
        &self,
        name: String,
        metadata: MarketMetadata,
    ) -> Result<MarketId, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::CreateMarketWithMetadata {
                name,
                metadata,
                respond_to: tx,
            })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }

    /// Get metadata for a market.
    pub async fn get_market_metadata(
        &self,
        market_id: MarketId,
    ) -> Result<Option<MarketMetadata>, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::GetMarketMetadata {
                market_id,
                respond_to: tx,
            })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }

    /// Get price history for a market.
    pub async fn get_price_history(
        &self,
        market_id: MarketId,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
    ) -> Result<Vec<PricePoint>, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::GetPriceHistory {
                market_id,
                from_ms,
                to_ms,
                respond_to: tx,
            })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }

    /// Get fill history for an account.
    pub async fn get_account_fills(
        &self,
        account_id: AccountId,
        market_id: Option<MarketId>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AccountFillRecord>, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::GetAccountFills {
                account_id,
                market_id,
                limit,
                offset,
                respond_to: tx,
            })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }

    /// Search markets by various criteria.
    pub async fn search_markets(
        &self,
        query: MarketSearchQuery,
    ) -> Result<Vec<MarketSearchResult>, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::SearchMarkets {
                query,
                respond_to: tx,
            })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }

    /// Pause block production (for simulation freeze during LLM calls).
    pub async fn pause_block_production(&self) -> Result<(), SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::PauseBlockProduction { respond_to: tx })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }

    /// Resume block production after a pause.
    pub async fn resume_block_production(&self) -> Result<(), SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::ResumeBlockProduction { respond_to: tx })
            .await
            .map_err(|_| SequencerError::ActorGone)?;
        rx.await.map_err(|_| SequencerError::ActorGone)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountStore;
    use matching_engine::{outcome_buy, MarketSet, NANOS_PER_DOLLAR};
    use std::sync::Arc;
    use sybil_oracle::AdminOracle;

    fn make_test_sequencer() -> (BlockSequencer, AccountId) {
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        markets.add_binary("Test");
        let oracle = Arc::new(AdminOracle::new());
        (BlockSequencer::new(accounts, markets, vec![], oracle), aid)
    }

    #[tokio::test]
    async fn test_spawn_and_produce_block() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

        let block = handle.produce_block().await.unwrap();
        assert_eq!(block.header.height, 1);
    }

    #[tokio::test]
    async fn test_submit_and_produce() {
        let (seq, aid) = make_test_sequencer();
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");

        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

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
        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

        let root = handle.get_state_root().await.unwrap();
        assert_ne!(root, [0u8; 32]); // non-empty accounts -> non-zero root
    }

    #[tokio::test]
    async fn test_get_account() {
        let (seq, aid) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

        let account = handle.get_account(aid).await.unwrap();
        assert!(account.is_some());
        assert_eq!(account.unwrap().balance, 100 * NANOS_PER_DOLLAR as i64);
    }

    #[tokio::test]
    async fn test_get_latest_block_none_initially() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

        let block = handle.get_latest_block().await.unwrap();
        assert!(block.is_none());
    }

    #[tokio::test]
    async fn test_get_latest_block_after_produce() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

        handle.produce_block().await.unwrap();

        let block = handle.get_latest_block().await.unwrap();
        assert!(block.is_some());
        assert_eq!(block.unwrap().header.height, 1);
    }

    #[tokio::test]
    async fn test_graceful_shutdown() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

        // Produce a block to verify actor is running
        handle.produce_block().await.unwrap();

        // Drop the handle -- actor should shut down
        drop(handle);

        // Give actor time to notice
        tokio::time::sleep(Duration::from_millis(50)).await;
        // No panic or hang = success
    }

    #[tokio::test]
    async fn test_block_chain_via_actor() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

        let block1 = handle.produce_block().await.unwrap();
        assert_eq!(block1.header.height, 1);
        assert_eq!(block1.header.parent_hash, [0u8; 32]); // genesis

        let block2 = handle.produce_block().await.unwrap();
        assert_eq!(block2.header.height, 2);
        // Parent hash should be hash of block1's header
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
        // Give seller a position
        accounts
            .get_mut(seller)
            .unwrap()
            .positions
            .insert((m0, 0), 100);

        let seq = BlockSequencer::new(
            accounts,
            markets.clone(),
            vec![],
            Arc::new(AdminOracle::new()),
        );
        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

        let root_before = handle.get_state_root().await.unwrap();

        // Submit matching buy and sell
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

    // --- Tests for new actor messages ---

    #[tokio::test]
    async fn test_create_account() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

        let account = handle
            .create_account(50 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();
        assert_eq!(account.balance, 50 * NANOS_PER_DOLLAR as i64);

        // Verify we can retrieve it
        let fetched = handle.get_account(account.id).await.unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().balance, 50 * NANOS_PER_DOLLAR as i64);
    }

    #[tokio::test]
    async fn test_fund_account() {
        let (seq, aid) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

        let account = handle
            .fund_account(aid, 25 * NANOS_PER_DOLLAR as i64)
            .await
            .unwrap();
        assert_eq!(account.balance, 125 * NANOS_PER_DOLLAR as i64);
    }

    #[tokio::test]
    async fn test_fund_nonexistent_account() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

        let result = handle.fund_account(AccountId(999), 100).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_register_pubkey_and_signed_order() {
        let (seq, aid) = make_test_sequencer();
        let mut ms = MarketSet::new();
        let m0 = ms.add_binary("Test");
        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

        // Generate a P256 key
        let signing_key = p256::ecdsa::SigningKey::random(
            &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
        );
        let pubkey = PublicKey(signing_key.verifying_key().clone());

        // Register the key
        handle.register_pubkey(aid, pubkey).await.unwrap();

        // Sign and submit an order
        let order = outcome_buy(&ms, 0, m0, 0, 500_000_000, 1);
        let signed = crate::crypto::sign_order(&order, &signing_key);
        handle.submit_signed_order(signed).await.unwrap();

        // Produce block and verify the order was included
        let block = handle.produce_block().await.unwrap();
        assert!(block.header.order_count >= 1);
    }

    #[tokio::test]
    async fn test_register_pubkey_duplicate() {
        let (seq, aid) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

        let signing_key = p256::ecdsa::SigningKey::random(
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
        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

        let markets = handle.list_markets().await.unwrap();
        assert_eq!(markets.len(), 1); // "Test" from make_test_sequencer

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
        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

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
        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

        let m0 = MarketId::new(0);
        let record = handle.resolve_market(m0, NANOS_PER_DOLLAR).await.unwrap();
        assert_eq!(record.payout_nanos, NANOS_PER_DOLLAR);
        assert_eq!(record.market_id, m0);
    }

    #[tokio::test]
    async fn test_resolve_nonexistent_market() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

        let result = handle
            .resolve_market(MarketId::new(999), NANOS_PER_DOLLAR)
            .await;
        assert!(matches!(result, Err(SequencerError::MarketNotFound)));
    }

    #[tokio::test]
    async fn test_get_block_by_height() {
        let (seq, _) = make_test_sequencer();
        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

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
        let handle = SequencerHandle::spawn(seq, MempoolConfig::default());

        let mut rx = handle.subscribe_blocks().await.unwrap();

        // Produce a block -- subscriber should receive it
        handle.produce_block().await.unwrap();

        let block = rx.recv().await.unwrap();
        assert_eq!(block.header.height, 1);
    }
}
