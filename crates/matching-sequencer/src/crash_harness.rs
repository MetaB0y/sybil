// Crash-injection test harness compiled into the lib for integration tests.
// Its `.unwrap()`s assert setup invariants of a test scenario (keys, oracle
// wiring, deterministic RNG); a panic here is the intended test-failure signal.
#![allow(clippy::unwrap_used)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use matching_engine::{MarketId, MarketSet, NANOS_PER_DOLLAR, outcome_buy};
use p256::ecdsa::SigningKey;
use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;
use sybil_oracle::AdminOracle;

use crate::account::{AccountId, AccountStore};
use crate::actor::{SequencerHandle, SequencerTestCrashpoint};
use crate::block::compute_complete_state_root;
use crate::bridge::{BridgeWithdrawalRequest, L1Deposit, account_key};
use crate::crypto::{PublicKey, sign_cancel};
use crate::market_info::MarketMetadata;
use crate::order_book::reservation_snapshots_from_resting_orders;
use crate::sequencer::{BlockSequencer, OrderSubmission, SequencerConfig};
use crate::store::{RestoredState, Store, StoreFaultPoint};

const DEFAULT_SEEDS: &[u64] = &[0x5eed_0158, 0x5eed_0159, 0x5eed_015a];
const DEFAULT_ITERATIONS: usize = 9;
const INITIAL_ACCOUNTS: usize = 3;
const INITIAL_BALANCE: i64 = 1_000 * NANOS_PER_DOLLAR as i64;

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug)]
enum RecoveryBoundary {
    CrashAfterAdmissionLogged,
    CrashBeforePrepare,
    CrashAfterPrepareBeforePersist,
    CrashAfterPersistBeforeCommit,
    CrashAfterCommit,
    StoreFaultBeforeQmdbPersist,
    StoreFaultAfterQmdbBeforeRedbFence,
    StoreFaultBeforeRedbFenceCommit,
    StoreFaultAfterRedbFenceCommit,
}

impl RecoveryBoundary {
    const ALL: [Self; 9] = [
        Self::CrashAfterAdmissionLogged,
        Self::CrashBeforePrepare,
        Self::CrashAfterPrepareBeforePersist,
        Self::CrashAfterPersistBeforeCommit,
        Self::CrashAfterCommit,
        Self::StoreFaultBeforeQmdbPersist,
        Self::StoreFaultAfterQmdbBeforeRedbFence,
        Self::StoreFaultBeforeRedbFenceCommit,
        Self::StoreFaultAfterRedbFenceCommit,
    ];

    fn actor_crashpoint(self) -> Option<SequencerTestCrashpoint> {
        match self {
            Self::CrashBeforePrepare => Some(SequencerTestCrashpoint::BeforePrepare),
            Self::CrashAfterPrepareBeforePersist => {
                Some(SequencerTestCrashpoint::AfterPrepareBeforePersist)
            }
            Self::CrashAfterPersistBeforeCommit => {
                Some(SequencerTestCrashpoint::AfterPersistBeforeCommit)
            }
            Self::CrashAfterCommit => Some(SequencerTestCrashpoint::AfterCommit),
            _ => None,
        }
    }

    fn store_fault(self) -> Option<StoreFaultPoint> {
        match self {
            Self::StoreFaultBeforeQmdbPersist => Some(StoreFaultPoint::BeforeQmdbPersist),
            Self::StoreFaultAfterQmdbBeforeRedbFence => {
                Some(StoreFaultPoint::AfterQmdbPersistBeforeRedbFence)
            }
            Self::StoreFaultBeforeRedbFenceCommit => Some(StoreFaultPoint::BeforeRedbFenceCommit),
            Self::StoreFaultAfterRedbFenceCommit => Some(StoreFaultPoint::AfterRedbFenceCommit),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum OperationKind {
    CreateAccount,
    FundAccount,
    CreateMarket,
    DirectOrder,
    DeferredBundle,
    Cancel,
    Resolve,
    L1Deposit,
    Withdrawal,
}

impl OperationKind {
    const ALL: [Self; 9] = [
        Self::CreateAccount,
        Self::FundAccount,
        Self::CreateMarket,
        Self::DirectOrder,
        Self::DeferredBundle,
        Self::Cancel,
        Self::Resolve,
        Self::L1Deposit,
        Self::Withdrawal,
    ];
}

struct CrashProfile {
    seeds: Vec<u64>,
    iterations: usize,
}

impl CrashProfile {
    fn from_env() -> Self {
        let seeds = std::env::var("SYBIL_CRASH_SEEDS")
            .ok()
            .map(|raw| {
                raw.split([',', ' ', ';'])
                    .filter(|part| !part.is_empty())
                    .map(parse_seed)
                    .collect::<Vec<_>>()
            })
            .filter(|seeds| !seeds.is_empty())
            .unwrap_or_else(|| DEFAULT_SEEDS.to_vec());

        let iterations = std::env::var("SYBIL_CRASH_ITERS")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .filter(|iters| *iters > 0)
            .unwrap_or(DEFAULT_ITERATIONS);

        Self { seeds, iterations }
    }
}

fn parse_seed(raw: &str) -> u64 {
    let trimmed = raw.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16).expect("SYBIL_CRASH_SEEDS contains an invalid hex seed")
    } else {
        trimmed
            .parse()
            .expect("SYBIL_CRASH_SEEDS contains an invalid decimal seed")
    }
}

struct TempStoreDir {
    root: PathBuf,
}

impl TempStoreDir {
    fn new(seed: u64) -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sybil-crash-fi-{}-{seed:x}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("tmp")).expect("crash harness temp dir can be created");
        Self { root }
    }

    fn store_path(&self) -> PathBuf {
        self.root.join("sequencer.redb")
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for TempStoreDir {
    fn drop(&mut self) {
        if std::thread::panicking() || std::env::var_os("SYBIL_CRASH_KEEP_TMP").is_some() {
            eprintln!(
                "keeping crash harness temp store dir: {}",
                self.root.display()
            );
            return;
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[derive(Clone, Debug)]
struct ActiveOrder {
    account_id: AccountId,
    market_id: MarketId,
    placed_height: u64,
}

#[derive(Default)]
struct AckLedger {
    accounts: HashSet<AccountId>,
    created_accounts: HashSet<AccountId>,
    markets: HashSet<MarketId>,
    resolved_markets: HashSet<MarketId>,
    active_orders: HashMap<u64, ActiveOrder>,
    placed_order_ids: HashSet<u64>,
    canceled_order_ids: HashSet<u64>,
    withdrawals: HashSet<u64>,
    fund_amounts: HashMap<AccountId, Vec<i64>>,
    deferred_by_height: HashMap<u64, usize>,
}

struct Harness {
    seed: u64,
    store_dir: TempStoreDir,
    store: Arc<Store>,
    handle: SequencerHandle,
    config: SequencerConfig,
    rng: ChaCha8Rng,
    accounts: Vec<AccountId>,
    keys: HashMap<AccountId, SigningKey>,
    unresolved_markets: Vec<MarketId>,
    ledger: AckLedger,
    op_index: u64,
}

impl Harness {
    async fn new(seed: u64) -> Self {
        let store_dir = TempStoreDir::new(seed);
        let store = Arc::new(Store::open(&store_dir.store_path()).unwrap());
        let config = SequencerConfig {
            block_interval: std::time::Duration::from_secs(60 * 60),
            max_open_orders_per_account: 10_000,
            max_pending_bundles: 10_000,
            max_pending_bundles_per_account: 10_000,
            // The crash harness fuzzes acknowledged-write ordering with tiny
            // orders; order economics are outside its target invariant.
            min_resting_order_notional_nanos: 0,
            ..SequencerConfig::default()
        };

        let mut accounts = AccountStore::new();
        let mut account_ids = Vec::new();
        let mut keys = HashMap::new();
        for _ in 0..INITIAL_ACCOUNTS {
            let account_id = accounts.create_account(INITIAL_BALANCE);
            let key = signing_key_for(seed, account_id);
            account_ids.push(account_id);
            keys.insert(account_id, key);
        }

        let mut markets = matching_engine::MarketSet::new();
        let m0 = markets.add_binary("crash-fi market 0");
        let m1 = markets.add_binary("crash-fi market 1");

        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets,
            vec![],
            Arc::new(AdminOracle::new()),
            config.clone(),
        );
        for (&account_id, key) in &keys {
            seq.register_pubkey(account_id, PublicKey(*key.verifying_key()))
                .unwrap();
        }
        let genesis = seq.try_produce_block(Vec::new(), 1).unwrap();
        let sealed = genesis.sealed_block();
        store
            .save_block_with_witness_and_replay_block(seq.snapshot(), &genesis.witness, &sealed)
            .await
            .unwrap();

        let handle = SequencerHandle::spawn_with_store_arc_for_test(seq, store.clone());
        let mut ledger = AckLedger::default();
        ledger.accounts.extend(account_ids.iter().copied());
        ledger.markets.insert(m0);
        ledger.markets.insert(m1);

        Self {
            seed,
            store_dir,
            store,
            handle,
            config,
            rng: ChaCha8Rng::seed_from_u64(seed),
            accounts: account_ids,
            keys,
            unresolved_markets: vec![m0, m1],
            ledger,
            op_index: 0,
        }
    }

    async fn run(&mut self, iterations: usize) {
        for iteration in 0..iterations {
            let boundary = RecoveryBoundary::ALL[iteration % RecoveryBoundary::ALL.len()];
            let op = OperationKind::ALL[iteration % OperationKind::ALL.len()];
            self.run_operation(op).await;

            if !matches!(boundary, RecoveryBoundary::CrashAfterAdmissionLogged) {
                let random_ops = self.rng.random_range(1..=2);
                for _ in 0..random_ops {
                    self.run_random_operation().await;
                }
            }

            self.inject_boundary(boundary).await;
            self.assert_recovered_invariants(iteration, boundary).await;
        }
    }

    async fn run_random_operation(&mut self) {
        let op = match self.rng.random_range(0..100) {
            0..=12 => OperationKind::CreateAccount,
            13..=26 => OperationKind::FundAccount,
            27..=38 => OperationKind::CreateMarket,
            39..=59 => OperationKind::DirectOrder,
            60..=69 => OperationKind::DeferredBundle,
            70..=77 => OperationKind::Cancel,
            78..=85 => OperationKind::Resolve,
            86..=93 => OperationKind::L1Deposit,
            _ => OperationKind::Withdrawal,
        };
        self.run_operation(op).await;
    }

    async fn run_operation(&mut self, op: OperationKind) {
        self.op_index = self.op_index.saturating_add(1);
        match op {
            OperationKind::CreateAccount => self.create_account().await,
            OperationKind::FundAccount => self.fund_account().await,
            OperationKind::CreateMarket => self.create_market().await,
            OperationKind::DirectOrder => {
                let _ = self.submit_direct_order().await;
            }
            OperationKind::DeferredBundle => self.submit_deferred_bundle().await,
            OperationKind::Cancel => self.cancel_pending_order().await,
            OperationKind::Resolve => self.resolve_market().await,
            OperationKind::L1Deposit => self.submit_l1_deposit().await,
            OperationKind::Withdrawal => self.create_withdrawal().await,
        }
    }

    async fn inject_boundary(&mut self, boundary: RecoveryBoundary) {
        match boundary {
            RecoveryBoundary::CrashAfterAdmissionLogged => {
                let order_id = self
                    .force_direct_order()
                    .await
                    .expect("crash-after-admission needs a durable direct admit");
                assert!(
                    self.ledger.placed_order_ids.contains(&order_id),
                    "seed={:#x} boundary={boundary:?} direct admit was not tracked",
                    self.seed
                );
                self.handle.crash_actor_for_test().await.unwrap();
            }
            _ if boundary.actor_crashpoint().is_some() => {
                self.handle
                    .produce_block_and_crash_for_test(boundary.actor_crashpoint().unwrap())
                    .await
                    .unwrap();
            }
            _ if boundary.store_fault().is_some() => {
                self.store
                    .inject_next_save_block_fault(boundary.store_fault().unwrap());
                let _ = self.handle.produce_block().await;
                self.handle.crash_actor_for_test().await.unwrap();
            }
            _ => unreachable!("all boundaries handled"),
        }
    }

    async fn assert_recovered_invariants(&self, iteration: usize, boundary: RecoveryBoundary) {
        let context = format!(
            "seed={:#x} iteration={iteration} boundary={boundary:?} temp_store={}",
            self.seed,
            self.store_dir.path().display()
        );
        let restored = self
            .store
            .load_state()
            .await
            .unwrap_or_else(|error| panic!("{context}: load_state failed: {error}"))
            .unwrap_or_else(|| panic!("{context}: store unexpectedly empty after recovery"));
        let header = restored
            .last_header
            .as_ref()
            .unwrap_or_else(|| panic!("{context}: restored sequencer has no committed header"));
        let qmdb_root = self
            .store
            .current_state_qmdb_root()
            .await
            .unwrap_or_else(|error| panic!("{context}: qMDB root read failed: {error}"))
            .unwrap_or_else(|| panic!("{context}: missing fenced qMDB root"));
        assert_eq!(
            qmdb_root.root, header.state_root,
            "{context}: fenced qMDB root differs from committed header"
        );
        let has_uncommitted_wal = has_uncommitted_wal(&restored);
        let seq =
            BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), self.config.clone());
        let live_recomputed = compute_complete_state_root(
            &seq.accounts,
            seq.bridge_state(),
            seq.order_book(),
            seq.markets(),
            seq.market_groups(),
            seq.market_lifecycle(),
            seq.analytics().last_clearing_prices(),
        );
        if !has_uncommitted_wal {
            assert_eq!(
                live_recomputed,
                seq.last_header()
                    .expect("restored sequencer should retain committed header")
                    .state_root,
                "{context}: WAL-free live root differs from committed header"
            );
        }
        let actor_root = self
            .handle
            .get_state_root()
            .await
            .unwrap_or_else(|error| panic!("{context}: actor state-root read failed: {error}"));
        assert_eq!(
            actor_root, live_recomputed,
            "{context}: restarted actor root differs from restored store root"
        );

        assert_no_negative_non_mint_balances(&seq, &context);
        assert_no_duplicate_order_ids(&seq, &context);
        assert_reservations_rederive(&seq, &context);
        self.assert_acknowledged_writes_visible(&seq, &context);
        self.assert_history_exactly_once(&seq, &context);
    }

    async fn create_account(&mut self) {
        let balance = 500 * NANOS_PER_DOLLAR as i64 + self.op_index as i64;
        if let Ok(account) = self.handle.create_account(balance).await {
            let key = signing_key_for(self.seed, account.id);
            if self
                .handle
                .register_pubkey(account.id, PublicKey(*key.verifying_key()))
                .await
                .is_ok()
            {
                self.keys.insert(account.id, key);
            }
            self.accounts.push(account.id);
            self.ledger.accounts.insert(account.id);
            self.ledger.created_accounts.insert(account.id);
        }
    }

    async fn fund_account(&mut self) {
        let account_id = self.random_account();
        let amount = 10 * NANOS_PER_DOLLAR as i64 + self.op_index as i64;
        if self.handle.fund_account(account_id, amount).await.is_ok() {
            self.ledger
                .fund_amounts
                .entry(account_id)
                .or_default()
                .push(amount);
        }
    }

    async fn create_market(&mut self) {
        let metadata = if self.rng.random_bool(0.4) {
            Some(MarketMetadata {
                description: format!("crash harness metadata {}", self.op_index),
                category: "crash-fi".to_string(),
                tags: vec!["persistence".to_string()],
                resolution_criteria: "test harness admin resolution".to_string(),
                expiry_timestamp_ms: 1_900_000_000_000 + self.op_index,
                created_at_ms: 1_800_000_000_000 + self.op_index,
                resolution_config: None,
                committed_metadata_digest: None,
            })
        } else {
            None
        };

        let name = format!("crash-fi market {}", self.op_index);
        let result = match metadata {
            Some(metadata) => {
                self.handle
                    .create_market_with_metadata(name, metadata)
                    .await
            }
            None => self.handle.create_market(name).await,
        };
        if let Ok(market_id) = result {
            self.unresolved_markets.push(market_id);
            self.ledger.markets.insert(market_id);
        }
    }

    async fn ensure_unresolved_market(&mut self) -> MarketId {
        if self.unresolved_markets.is_empty() {
            self.create_market().await;
        }
        let idx = self.rng.random_range(0..self.unresolved_markets.len());
        self.unresolved_markets[idx]
    }

    async fn force_direct_order(&mut self) -> Option<u64> {
        for _ in 0..4 {
            if let Some(order_id) = self.submit_direct_order().await {
                return Some(order_id);
            }
            self.fund_account().await;
            self.create_market().await;
        }
        None
    }

    async fn submit_direct_order(&mut self) -> Option<u64> {
        let account_id = self.random_account();
        let market_id = self.ensure_unresolved_market().await;
        let markets = self.handle.list_markets().await.ok()?;
        let before = self.pending_order_ids(account_id).await;
        let outcome = self.rng.random_range(0..=1);
        let price = self.rng.random_range(200_000_000..=850_000_000);
        let qty = self.rng.random_range(1..=25);
        let order = outcome_buy(&markets, 0, market_id, outcome, price, qty);
        let height = self.current_height().await;
        let submission = OrderSubmission {
            account_id,
            orders: vec![order],
            mm_constraint: None,
        };
        if self.handle.submit_order(submission).await.is_err() {
            return None;
        }
        let after = self.pending_order_ids(account_id).await;
        let order_id = after.difference(&before).copied().max()?;
        self.ledger.placed_order_ids.insert(order_id);
        self.ledger.active_orders.insert(
            order_id,
            ActiveOrder {
                account_id,
                market_id,
                placed_height: height,
            },
        );
        Some(order_id)
    }

    async fn submit_deferred_bundle(&mut self) {
        let account_id = self.random_account();
        let market_id = self.ensure_unresolved_market().await;
        let Ok(markets) = self.handle.list_markets().await else {
            return;
        };
        let price_a = self.rng.random_range(150_000_000..=700_000_000);
        let price_b = self.rng.random_range(150_000_000..=700_000_000);
        let orders = vec![
            outcome_buy(&markets, 0, market_id, 0, price_a, 1),
            outcome_buy(&markets, 0, market_id, 0, price_b, 1),
        ];
        let height = self.current_height().await;
        let submission = OrderSubmission {
            account_id,
            orders,
            mm_constraint: None,
        };
        if self.handle.submit_order(submission).await.is_ok() {
            *self.ledger.deferred_by_height.entry(height).or_insert(0) += 1;
        }
    }

    async fn cancel_pending_order(&mut self) {
        let mut candidates = Vec::new();
        for &account_id in &self.accounts {
            if let Ok(pending) = self.handle.get_pending_orders(Some(account_id)).await {
                for order in pending {
                    candidates.push((account_id, order.order_id));
                }
            }
        }
        if candidates.is_empty() {
            let _ = self.submit_direct_order().await;
            for &account_id in &self.accounts {
                if let Ok(pending) = self.handle.get_pending_orders(Some(account_id)).await {
                    for order in pending {
                        candidates.push((account_id, order.order_id));
                    }
                }
            }
        }
        if candidates.is_empty() {
            return;
        }

        let (account_id, order_id) = candidates[self.rng.random_range(0..candidates.len())];
        let Some(key) = self.keys.get(&account_id) else {
            return;
        };
        let Ok(Some(genesis_hash)) = self.handle.get_genesis_hash().await else {
            return;
        };
        let signed = sign_cancel(
            account_id,
            order_id,
            self.op_index.saturating_add(1),
            genesis_hash,
            key,
        );
        if self.handle.cancel_signed_order(signed).await.is_ok() {
            self.ledger.active_orders.remove(&order_id);
            self.ledger.canceled_order_ids.insert(order_id);
        }
    }

    async fn resolve_market(&mut self) {
        if self.unresolved_markets.is_empty() {
            self.create_market().await;
        }
        if self.unresolved_markets.is_empty() {
            return;
        }
        let idx = self.rng.random_range(0..self.unresolved_markets.len());
        let market_id = self.unresolved_markets.swap_remove(idx);
        let payout = match self.rng.random_range(0..3) {
            0 => 0,
            1 => NANOS_PER_DOLLAR / 2,
            _ => NANOS_PER_DOLLAR,
        };
        if self
            .handle
            .resolve_market(market_id, matching_engine::Nanos(payout))
            .await
            .is_ok()
        {
            self.ledger.resolved_markets.insert(market_id);
        } else {
            self.unresolved_markets.push(market_id);
        }
    }

    async fn submit_l1_deposit(&mut self) {
        let account_id = self.random_account();
        let Ok(bridge_state) = self.handle.get_bridge_state().await else {
            return;
        };
        let deposit_id = bridge_state.deposit_cursor.saturating_add(1);
        let mut deposit = L1Deposit {
            deposit_id,
            account_id: Some(account_id),
            chain_id: 1,
            vault_address: eth_address(self.seed, self.op_index, 1),
            token_address: eth_address(self.seed, self.op_index, 2),
            sender: eth_address(self.seed, self.op_index, 3),
            sybil_account_key: account_key(account_id),
            amount_token_units: 1_000 + self.op_index,
            deposit_root: [0u8; 32],
        };
        let mut frontier = bridge_state.deposit_frontier;
        let Some(deposit_root) = crate::bridge::append_deposit_frontier(
            &mut frontier,
            bridge_state.deposit_cursor,
            &deposit,
        ) else {
            return;
        };
        deposit.deposit_root = deposit_root;
        let _ = self.handle.submit_l1_deposit(deposit).await;
    }

    async fn create_withdrawal(&mut self) {
        let account_id = self.random_account();
        let height = self.current_height().await;
        let request = BridgeWithdrawalRequest {
            account_id,
            chain_id: 1,
            vault_address: eth_address(self.seed, self.op_index, 11),
            recipient: eth_address(self.seed, self.op_index, 12),
            token_address: eth_address(self.seed, self.op_index, 13),
            amount_token_units: 100 + (self.op_index % 100),
            expiry_height: height.saturating_add(10),
        };
        if let Ok(withdrawal) = self.handle.create_bridge_withdrawal(request).await {
            self.ledger.withdrawals.insert(withdrawal.withdrawal_id);
        }
    }

    async fn pending_order_ids(&self, account_id: AccountId) -> HashSet<u64> {
        self.handle
            .get_pending_orders(Some(account_id))
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|order| order.order_id)
            .collect()
    }

    async fn current_height(&self) -> u64 {
        self.handle
            .get_committed_height()
            .await
            .unwrap_or(None)
            .unwrap_or(0)
    }

    fn random_account(&mut self) -> AccountId {
        let idx = self.rng.random_range(0..self.accounts.len());
        self.accounts[idx]
    }

    fn assert_acknowledged_writes_visible(&self, seq: &BlockSequencer, context: &str) {
        for account_id in &self.ledger.accounts {
            assert!(
                seq.accounts.get(*account_id).is_some(),
                "{context}: acknowledged account {account_id:?} missing after recovery"
            );
        }
        for market_id in &self.ledger.markets {
            assert!(
                seq.markets().get(*market_id).is_some(),
                "{context}: acknowledged market {market_id:?} missing after recovery"
            );
        }
        for market_id in &self.ledger.resolved_markets {
            assert!(
                matches!(
                    seq.market_status(*market_id),
                    sybil_oracle::MarketStatus::Resolved { .. }
                ),
                "{context}: acknowledged resolution for market {market_id:?} missing after recovery"
            );
        }
        for withdrawal_id in &self.ledger.withdrawals {
            assert!(
                seq.bridge_withdrawal(*withdrawal_id).is_some(),
                "{context}: acknowledged withdrawal {withdrawal_id} missing after recovery"
            );
        }

        let restored_height = seq.height();
        let pending: HashSet<u64> = seq
            .pending_orders_info(None)
            .into_iter()
            .map(|order| order.order_id)
            .collect();
        for (order_id, order) in &self.ledger.active_orders {
            if order.placed_height == restored_height
                && !self.ledger.resolved_markets.contains(&order.market_id)
            {
                assert!(
                    pending.contains(order_id),
                    "{context}: acknowledged direct order {order_id} for account {:?} missing before commit",
                    order.account_id
                );
            }
        }
        for order_id in &self.ledger.canceled_order_ids {
            assert!(
                !pending.contains(order_id),
                "{context}: acknowledged cancel for order {order_id} was lost"
            );
        }

        let expected_deferred = self
            .ledger
            .deferred_by_height
            .get(&restored_height)
            .copied()
            .unwrap_or(0);
        assert!(
            seq.pending_bundles_for_test().len() >= expected_deferred,
            "{context}: acknowledged deferred bundle WAL rows missing after recovery"
        );
    }

    fn assert_history_exactly_once(&self, seq: &BlockSequencer, context: &str) {
        for account_id in &self.ledger.accounts {
            let history = self.account_history(seq, *account_id, context);
            let mut seen = HashSet::new();
            for event in &history {
                let key = (
                    event.account_id.0,
                    event.block_height,
                    event.seq,
                    event.kind as u8,
                    event.order_id,
                    event.amount_nanos,
                );
                assert!(
                    seen.insert(key),
                    "{context}: duplicate account history event for account {account_id:?}: {event:?}"
                );
            }

            if self.ledger.created_accounts.contains(account_id) {
                let created = history
                    .iter()
                    .filter(|event| {
                        event.kind == crate::aggregates::HistoryKind::Created
                            && event.account_id == *account_id
                    })
                    .count();
                assert_eq!(
                    created, 1,
                    "{context}: acknowledged account creation history for {account_id:?} was not exactly once"
                );
            }

            if let Some(amounts) = self.ledger.fund_amounts.get(account_id) {
                for amount in amounts {
                    let count = history
                        .iter()
                        .filter(|event| {
                            event.kind == crate::aggregates::HistoryKind::Deposit
                                && event.amount_nanos == Some(*amount)
                        })
                        .count();
                    assert_eq!(
                        count, 1,
                        "{context}: acknowledged fund history amount {amount} for {account_id:?} was not exactly once"
                    );
                }
            }
        }

        for order_id in &self.ledger.canceled_order_ids {
            let count = self
                .ledger
                .accounts
                .iter()
                .flat_map(|account_id| self.account_history(seq, *account_id, context))
                .filter(|event| {
                    event.kind == crate::aggregates::HistoryKind::Cancelled
                        && event.order_id == Some(*order_id)
                })
                .count();
            assert_eq!(
                count, 1,
                "{context}: acknowledged cancel history for order {order_id} was not exactly once"
            );
        }
    }

    fn account_history(
        &self,
        seq: &BlockSequencer,
        account_id: AccountId,
        context: &str,
    ) -> Vec<crate::aggregates::HistoryEvent> {
        let mut events: Vec<_> = self
            .store
            .product_history_outbox_batches(10_000)
            .unwrap_or_else(|error| {
                panic!("{context}: product-history outbox read failed: {error}")
            })
            .into_iter()
            .flat_map(|batch| batch.events)
            .filter(|event| event.account_id == account_id.0)
            .map(history_event_from_fact)
            .collect();
        events.extend(
            seq.analytics()
                .pending_account_history(account_id, None, None),
        );
        events.sort_by_key(|event| std::cmp::Reverse((event.block_height, event.seq)));
        events.dedup_by_key(|event| (event.account_id.0, event.block_height, event.seq));
        events
    }
}

fn history_event_from_fact(
    fact: sybil_history_types::AccountEventFact,
) -> crate::aggregates::HistoryEvent {
    use crate::aggregates::{HistoryEvent, HistoryKind};
    use sybil_history_types::AccountEventKind;

    let kind = match fact.kind {
        AccountEventKind::Created => HistoryKind::Created,
        AccountEventKind::Placed => HistoryKind::Placed,
        AccountEventKind::PartialFill => HistoryKind::PartialFill,
        AccountEventKind::Filled => HistoryKind::Filled,
        AccountEventKind::Cancelled => HistoryKind::Cancelled,
        AccountEventKind::Expired => HistoryKind::Expired,
        AccountEventKind::Deposit => HistoryKind::Deposit,
        AccountEventKind::Withdrawal => HistoryKind::Withdrawal,
        AccountEventKind::Resolved => HistoryKind::Resolved,
        AccountEventKind::Rejected => HistoryKind::Rejected,
    };
    let mut event = HistoryEvent::new(
        AccountId(fact.account_id),
        kind,
        fact.block_height,
        fact.timestamp_ms,
    );
    event.seq = fact.seq;
    event.market_id = fact.market_id.map(MarketId::new);
    event.order_id = fact.order_id;
    event.qty = fact.qty;
    event.price_nanos = fact.price_nanos;
    event.amount_nanos = fact.amount_nanos;
    event.realized_pnl_nanos = fact.realized_pnl_nanos;
    event.required_nanos = fact.required_nanos;
    event.available_nanos = fact.available_nanos;
    event
}

fn assert_no_negative_non_mint_balances(seq: &BlockSequencer, context: &str) {
    for (account_id, account) in seq.accounts.iter() {
        if *account_id != AccountId::MINT {
            assert!(
                account.balance >= 0,
                "{context}: non-MINT account {account_id:?} has negative balance {}",
                account.balance
            );
        }
    }
}

fn assert_no_duplicate_order_ids(seq: &BlockSequencer, context: &str) {
    let mut seen = HashSet::new();
    for resting in seq.order_book().snapshot() {
        assert!(
            seen.insert(resting.order.id),
            "{context}: duplicate resting order id {} after recovery",
            resting.order.id
        );
    }
    for bundle in seq.pending_bundles_for_test() {
        for order in &bundle.orders {
            if order.id != 0 {
                assert!(
                    seen.insert(order.id),
                    "{context}: duplicate pending-bundle order id {} after recovery",
                    order.id
                );
            }
        }
    }
}

fn assert_reservations_rederive(seq: &BlockSequencer, context: &str) {
    let aggregate = seq.order_book().state_root_reservations();
    let rederived = reservation_snapshots_from_resting_orders(&seq.order_book().snapshot());
    assert_eq!(
        aggregate, rederived,
        "{context}: aggregate reservations differ from per-order re-derivation"
    );
}

fn has_uncommitted_wal(restored: &RestoredState) -> bool {
    !restored.pending_bundles.is_empty()
        || !restored.admit_log.is_empty()
        || !restored.control_plane_log.is_empty()
        || !restored.pending_l1_deposits.is_empty()
        || !restored.pending_bridge_withdrawals.is_empty()
        || !restored.pending_bridge_l1_inputs.is_empty()
}

fn signing_key_for(seed: u64, account_id: AccountId) -> SigningKey {
    for nonce in 0u64.. {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sybil/crash-harness/signing-key");
        hasher.update(&seed.to_le_bytes());
        hasher.update(&account_id.0.to_le_bytes());
        hasher.update(&nonce.to_le_bytes());
        let bytes = *hasher.finalize().as_bytes();
        if let Ok(key) = SigningKey::from_bytes((&bytes).into()) {
            return key;
        }
    }
    unreachable!("eventually produces a valid P-256 scalar")
}

fn bytes32(seed: u64, op_index: u64, domain: u8) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sybil/crash-harness/bytes32");
    hasher.update(&seed.to_le_bytes());
    hasher.update(&op_index.to_le_bytes());
    hasher.update(&[domain]);
    *hasher.finalize().as_bytes()
}

fn eth_address(seed: u64, op_index: u64, domain: u8) -> [u8; 20] {
    let bytes = bytes32(seed, op_index, domain);
    let mut out = [0u8; 20];
    out.copy_from_slice(&bytes[..20]);
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn randomized_persistence_crashpoints_default_profile() {
    let profile = CrashProfile::from_env();
    for seed in profile.seeds {
        let mut harness = Harness::new(seed).await;
        harness.run(profile.iterations).await;
    }
}

#[tokio::test]
async fn bridge_state_size_is_bounded_across_deposits_and_root_survives_restart() {
    const DEPOSIT_COUNT: u64 = 64;

    let store_dir = TempStoreDir::new(0x266b);
    let store = Store::open(&store_dir.store_path()).unwrap();
    let mut accounts = AccountStore::new();
    let account_id = accounts.create_account(0);
    let config = SequencerConfig::default();
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        MarketSet::new(),
        Vec::new(),
        Arc::new(AdminOracle::new()),
        config.clone(),
    );
    let serialized_size_bound = rmp_serde::to_vec(&crate::bridge::BridgeState {
        deposit_cursor: u64::MAX,
        deposit_root: [u8::MAX; 32],
        deposit_frontier: [[u8::MAX; 32]; sybil_l1_protocol::DEPOSIT_TREE_DEPTH],
        observed_l1_height: u64::MAX,
        next_withdrawal_id: u64::MAX,
        withdrawals: Default::default(),
        quarantine: Default::default(),
    })
    .unwrap()
    .len();

    for deposit_id in 1..=DEPOSIT_COUNT {
        let mut deposit = L1Deposit {
            deposit_id,
            account_id: Some(account_id),
            chain_id: 1,
            vault_address: eth_address(0x266b, deposit_id, 1),
            token_address: eth_address(0x266b, deposit_id, 2),
            sender: eth_address(0x266b, deposit_id, 3),
            sybil_account_key: account_key(account_id),
            amount_token_units: 1,
            deposit_root: [0; 32],
        };
        let mut frontier = seq.bridge_state().deposit_frontier;
        deposit.deposit_root = crate::bridge::append_deposit_frontier(
            &mut frontier,
            seq.bridge_state().deposit_cursor,
            &deposit,
        )
        .unwrap();
        seq.ingest_l1_deposit(deposit).unwrap();
        assert!(
            rmp_serde::to_vec(seq.bridge_state()).unwrap().len() <= serialized_size_bound,
            "bridge state exceeded its fixed-field serialization bound"
        );
    }

    let production = seq.try_produce_block(Vec::new(), 1_000).unwrap();
    let committed_root = production.block.header.state_root;
    store
        .save_block_with_witness(seq.snapshot(), &production.witness)
        .await
        .unwrap();
    drop(store);

    let reopened = Store::open(&store_dir.store_path()).unwrap();
    let restored = reopened.load_state().await.unwrap().unwrap();
    assert_eq!(restored.bridge_state.deposit_cursor, DEPOSIT_COUNT);
    assert!(
        rmp_serde::to_vec(&restored.bridge_state).unwrap().len() <= serialized_size_bound,
        "restored bridge state exceeded its fixed-field serialization bound"
    );
    let restored_seq = BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), config);
    let restarted_root = compute_complete_state_root(
        &restored_seq.accounts,
        restored_seq.bridge_state(),
        restored_seq.order_book(),
        restored_seq.markets(),
        restored_seq.market_groups(),
        restored_seq.market_lifecycle(),
        restored_seq.analytics().last_clearing_prices(),
    );
    assert_eq!(restarted_root, committed_root);
}
