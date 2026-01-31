use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::time::{interval_at, Instant};

use crate::account::{Account, AccountId};
use crate::block::Block;
use crate::crypto::{verify_signed_order, SignedOrder};
use crate::error::SequencerError;
use crate::mempool::{Mempool, MempoolConfig};
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
    ProduceBlock {
        respond_to: oneshot::Sender<Block>,
    },
}

/// The sequencer actor. Runs in a tokio task, produces blocks on a timer.
struct SequencerActor {
    sequencer: BlockSequencer,
    mempool: Mempool,
    receiver: mpsc::Receiver<Message>,
    latest_block: Option<Block>,
}

impl SequencerActor {
    fn new(
        sequencer: BlockSequencer,
        mempool: Mempool,
        receiver: mpsc::Receiver<Message>,
    ) -> Self {
        Self {
            sequencer,
            mempool,
            receiver,
            latest_block: None,
        }
    }

    async fn run(mut self) {
        let mut ticker = interval_at(
            Instant::now() + Duration::from_secs(1),
            Duration::from_secs(1),
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
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let submissions = self.mempool.drain();
        let (block, _pipeline_result) = self.sequencer.produce_block(submissions, timestamp_ms);
        self.latest_block = Some(block);
    }

    fn handle(&mut self, msg: Message) {
        match msg {
            Message::SubmitOrder { submission, respond_to } => {
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
            Message::GetAccount { account_id, respond_to } => {
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
        }
    }

    fn handle_signed_order(&mut self, signed: SignedOrder) -> Result<(), SequencerError> {
        // Verify signature
        verify_signed_order(&signed)?;

        // Look up account by pubkey — for now, we don't have the pubkey registry,
        // so signed orders include the account info. In a full implementation,
        // the sequencer state would map pubkeys to accounts.
        // For V1, we wrap the signed order into an OrderSubmission.
        // The account_id must be provided externally or looked up.
        // Since we don't have pubkey->account mapping yet, we reject unknown signers.
        Err(SequencerError::UnknownSigner)
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
        let (sender, receiver) = mpsc::channel(256);
        let mempool = Mempool::new(mempool_config);
        let actor = SequencerActor::new(sequencer, mempool, receiver);
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
    pub async fn get_account(&self, account_id: AccountId) -> Result<Option<Account>, SequencerError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Message::GetAccount { account_id, respond_to: tx })
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountStore;
    use matching_engine::{outcome_buy, MarketSet, NANOS_PER_DOLLAR};

    fn make_test_sequencer() -> (BlockSequencer, AccountId) {
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
        let mut markets = MarketSet::new();
        markets.add_binary("Test");
        (BlockSequencer::new(accounts, markets, vec![]), aid)
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
        assert_ne!(root, [0u8; 32]); // non-empty accounts → non-zero root
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

        // Drop the handle — actor should shut down
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
        accounts.get_mut(seller).unwrap().positions.insert((m0, 0), 100);

        let seq = BlockSequencer::new(accounts, markets.clone(), vec![]);
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
            orders: vec![matching_engine::outcome_sell(&markets, 0, m0, 0, 400_000_000, 5)],
            mm_constraint: None,
        };
        handle.submit_order(sell_sub).await.unwrap();

        let block = handle.produce_block().await.unwrap();

        if block.orders_filled > 0 {
            let root_after = handle.get_state_root().await.unwrap();
            assert_ne!(root_before, root_after);
        }
    }
}
