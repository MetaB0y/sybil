use std::sync::Arc;

use matching_engine::MarketSet;
use redb::{Database, TableDefinition};

use super::testutil::*;
use super::*;
use crate::account::AccountStore;
use crate::market_lifecycle::MarketLifecycle;
use crate::AdminOracle;

#[tokio::test]
async fn auto_resolution_records_round_trip() {
    let path = temp_db_path("auto-resolution-records");
    let store = Store::open(&path).unwrap();
    let record = AutoResolutionRecord {
        market_id: 7,
        action: AutoResolutionAction::Propose,
        payout_nanos: 1_000_000_000,
        confidence_ppm: 950_000,
        reasoning: "clear yes".to_string(),
        evidence_excerpts: vec!["evidence".to_string()],
        proposed_at_ms: 1_000,
        eta_ms: Some(90_000),
        approved_at_ms: None,
        rejected_at_ms: Some(2_000),
        rejected_payout_nanos: Some(1_000_000_000),
        rejected_reasoning_hash: Some([9; 32]),
        operator_note: Some("bad source".to_string()),
    };

    store
        .put_auto_resolution_record(record.clone())
        .await
        .unwrap();
    assert_eq!(store.auto_resolution_records().unwrap(), vec![record]);
}

#[tokio::test]
async fn witnessed_qmdb_state_root_matches_header_after_slot_reuse() {
    let path = temp_db_path("store-qmdb-root-reuse");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let mut lifecycle = MarketLifecycle::new(oracle);
    let mut markets = MarketSet::new();
    let accounts = AccountStore::new();
    let env = TestEnv::new();

    for height in 1..=3 {
        for index in 0..40 {
            let id = markets.add_binary(format!("root regression {height}-{index}"));
            lifecycle.set_market_metadata(
                id,
                MarketMetadata {
                    description: format!("description {height}-{index}"),
                    category: "regression".to_string(),
                    tags: vec![format!("height-{height}"), format!("market-{index}")],
                    resolution_criteria: format!("criteria {height}-{index}"),
                    expiry_timestamp_ms: 1_800_000_000_000 + height * 1000 + index,
                    created_at_ms: 1_700_000_000_000 + height * 1000 + index,
                    resolution_config: Some(crate::market_info::ResolutionConfig {
                        template: "admin_immediate".to_string(),
                    }),
                    committed_metadata_digest: None,
                },
            );
        }

        let (header, witness) =
            coherent_header_and_witness(height, &accounts, &markets, &lifecycle, &env.bridge_state);
        store
            .save_block_with_witness(
                env.snapshot(&accounts, &markets, &lifecycle, &header, 1, None, vec![]),
                &witness,
            )
            .await
            .unwrap();

        let qmdb_root = store.current_state_qmdb_root().await.unwrap().unwrap();
        assert_eq!(
            qmdb_root.root, header.state_root,
            "persisted typed-state qMDB root diverged from the committed block header at height {height}"
        );
    }
}

#[tokio::test]
async fn history_pruning_deletes_blocks_and_price_points_with_metadata() {
    let path = temp_db_path("store-history-retention");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("retention");
    let accounts = AccountStore::new();
    let env = TestEnv::new();

    for height in 1..=5 {
        let (header, witness) =
            coherent_header_and_witness(height, &accounts, &markets, &lifecycle, &env.bridge_state);
        let point = crate::market_info::PricePoint {
            height,
            timestamp_ms: header.timestamp_ms,
            yes_price: Nanos(500_000_000 + height),
            no_price: Nanos(500_000_000 - height),
            volume_nanos: height * 10,
        };
        let block = sample_sealed_block(&header);
        store
            .save_block_with_witness_and_history(
                env.snapshot_with_price_points(
                    &accounts,
                    &markets,
                    &lifecycle,
                    &header,
                    vec![(market_id, point)],
                ),
                &witness,
                &block,
            )
            .await
            .unwrap();
        store
            .save_da_artifact(DaArtifact::from_witness(&witness))
            .await
            .unwrap();
    }

    let report = store
        .prune_history(
            5,
            sample_header(5).timestamp_ms,
            HistoryRetentionPolicy {
                block_history_retention_blocks: 3,
                raw_price_retention_blocks: 3,
                price_candle_resolutions_secs: Vec::new(),
                price_candle_retention_secs: Vec::new(),
                prune_interval_blocks: 1,
                prune_max_rows: 10,
            },
        )
        .await
        .unwrap();

    assert_eq!(report.blocks_full_pruned, 2);
    assert_eq!(report.da_artifacts_pruned, 2);
    assert_eq!(report.price_points_pruned, 2);
    assert_eq!(report.meta.blocks_full_min_height, Some(3));
    assert_eq!(report.meta.price_points_min_height, Some(3));
    assert_eq!(report.meta.last_history_prune_height, Some(5));
    assert!(store.load_block(1).await.unwrap().is_none());
    assert!(store.load_block(2).await.unwrap().is_none());
    assert!(store.load_da_artifact(1).await.unwrap().is_none());
    assert!(store.load_da_artifact(2).await.unwrap().is_none());
    assert_eq!(
        store
            .load_da_artifact(3)
            .await
            .unwrap()
            .unwrap()
            .manifest
            .height,
        3
    );
    assert_eq!(
        store
            .load_block(3)
            .await
            .unwrap()
            .unwrap()
            .canonical
            .header
            .height,
        3
    );

    let page = store
        .load_price_history(market_id, None, None, None, 10)
        .await
        .unwrap();
    assert_eq!(page.retention_min_height, Some(3));
    let heights: Vec<_> = page.points.iter().map(|point| point.height).collect();
    assert_eq!(heights, vec![3, 4, 5]);
}

#[tokio::test]
async fn history_pruning_partial_budget_keeps_metadata_at_oldest_remaining_row() {
    let path = temp_db_path("store-history-retention-budget");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("retention budget");
    let accounts = AccountStore::new();
    let env = TestEnv::new();

    for height in 1..=5 {
        let (header, witness) =
            coherent_header_and_witness(height, &accounts, &markets, &lifecycle, &env.bridge_state);
        let point = crate::market_info::PricePoint {
            height,
            timestamp_ms: header.timestamp_ms,
            yes_price: Nanos(500_000_000),
            no_price: Nanos(500_000_000),
            volume_nanos: 1,
        };
        let block = sample_sealed_block(&header);
        store
            .save_block_with_witness_and_history(
                env.snapshot_with_price_points(
                    &accounts,
                    &markets,
                    &lifecycle,
                    &header,
                    vec![(market_id, point)],
                ),
                &witness,
                &block,
            )
            .await
            .unwrap();
    }

    let report = store
        .prune_history(
            5,
            sample_header(5).timestamp_ms,
            HistoryRetentionPolicy {
                block_history_retention_blocks: 2,
                raw_price_retention_blocks: 2,
                price_candle_resolutions_secs: Vec::new(),
                price_candle_retention_secs: Vec::new(),
                prune_interval_blocks: 1,
                prune_max_rows: 2,
            },
        )
        .await
        .unwrap();

    assert_eq!(report.blocks_full_pruned, 2);
    assert_eq!(report.price_points_pruned, 0);
    assert_eq!(
        report.meta.blocks_full_min_height,
        Some(3),
        "block floor must not jump to target 4 while block 3 remains"
    );
    assert_eq!(
        report.meta.price_points_min_height,
        Some(1),
        "price metadata must not advance when budget is exhausted first"
    );
    assert_eq!(report.meta.last_history_prune_height, Some(5));
    assert!(store.load_block(3).await.unwrap().is_some());

    let page = store
        .load_price_history(market_id, None, None, None, 10)
        .await
        .unwrap();
    let heights: Vec<_> = page.points.iter().map(|point| point.height).collect();
    assert_eq!(heights, vec![1, 2, 3, 4, 5]);
    assert_eq!(page.retention_min_height, Some(1));
}

#[tokio::test]
async fn price_candle_pruning_deletes_by_resolution_with_metadata() {
    let path = temp_db_path("store-price-candle-retention");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("candle retention");
    let accounts = AccountStore::new();
    let env = TestEnv::new();

    for (height, timestamp_ms) in [(1, 0), (2, 60_000), (3, 300_000), (4, 600_000)] {
        let mut header = sample_header(height);
        header.timestamp_ms = timestamp_ms;
        let point = crate::market_info::PricePoint {
            height,
            timestamp_ms,
            yes_price: Nanos(500_000_000 + height),
            no_price: Nanos(500_000_000 - height),
            volume_nanos: height,
        };
        store
            .save_block(env.snapshot_with_price_points(
                &accounts,
                &markets,
                &lifecycle,
                &header,
                vec![(market_id, point)],
            ))
            .await
            .unwrap();
    }

    let report = store
        .prune_history(
            4,
            600_000,
            HistoryRetentionPolicy {
                block_history_retention_blocks: 0,
                raw_price_retention_blocks: 0,
                price_candle_resolutions_secs: vec![60, 300],
                price_candle_retention_secs: vec![300, 600],
                prune_interval_blocks: 1,
                prune_max_rows: 100,
            },
        )
        .await
        .unwrap();

    assert_eq!(report.blocks_full_pruned, 0);
    assert_eq!(report.price_points_pruned, 0);
    assert_eq!(report.price_candles_pruned, 2);
    assert_eq!(
        report.meta.price_candles_min_bucket_ms.get(&60),
        Some(&300_000)
    );
    assert_eq!(report.meta.price_candles_min_bucket_ms.get(&300), Some(&0));

    let one_minute = store
        .load_price_candles(market_id, 60, None, None, None, 10)
        .await
        .unwrap();
    assert_eq!(one_minute.retention_min_bucket_ms, Some(300_000));
    assert_eq!(
        one_minute
            .candles
            .iter()
            .map(|candle| candle.bucket_start_ms)
            .collect::<Vec<_>>(),
        vec![300_000, 600_000]
    );

    let five_minute = store
        .load_price_candles(market_id, 300, None, None, None, 10)
        .await
        .unwrap();
    assert_eq!(five_minute.retention_min_bucket_ms, Some(0));
    assert_eq!(
        five_minute
            .candles
            .iter()
            .map(|candle| candle.bucket_start_ms)
            .collect::<Vec<_>>(),
        vec![0, 300_000, 600_000]
    );
}

#[tokio::test]
async fn price_candle_pruning_obeys_batch_limit_and_keeps_floor_actual() {
    let path = temp_db_path("store-price-candle-retention-budget");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("candle retention budget");
    let accounts = AccountStore::new();
    let env = TestEnv::new();

    for (height, timestamp_ms) in [(1, 0), (2, 60_000), (3, 120_000)] {
        let mut header = sample_header(height);
        header.timestamp_ms = timestamp_ms;
        let point = crate::market_info::PricePoint {
            height,
            timestamp_ms,
            yes_price: Nanos(500_000_000),
            no_price: Nanos(500_000_000),
            volume_nanos: 1,
        };
        store
            .save_block(env.snapshot_with_price_points(
                &accounts,
                &markets,
                &lifecycle,
                &header,
                vec![(market_id, point)],
            ))
            .await
            .unwrap();
    }

    let report = store
        .prune_history(
            3,
            180_000,
            HistoryRetentionPolicy {
                block_history_retention_blocks: 0,
                raw_price_retention_blocks: 0,
                price_candle_resolutions_secs: vec![60],
                price_candle_retention_secs: vec![60],
                prune_interval_blocks: 1,
                prune_max_rows: 1,
            },
        )
        .await
        .unwrap();

    assert_eq!(report.price_candles_pruned, 1);
    assert_eq!(
        report.meta.price_candles_min_bucket_ms.get(&60),
        Some(&60_000),
        "floor must remain at the oldest actual retained candle while the prune budget is exhausted"
    );

    let page = store
        .load_price_candles(market_id, 60, None, None, None, 10)
        .await
        .unwrap();
    assert_eq!(page.retention_min_bucket_ms, Some(60_000));
    assert_eq!(
        page.candles
            .iter()
            .map(|candle| candle.bucket_start_ms)
            .collect::<Vec<_>>(),
        vec![60_000, 120_000]
    );
}

#[tokio::test]
async fn price_candles_merge_committed_points_without_empty_buckets() {
    let path = temp_db_path("store-price-candles");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("candles");
    let accounts = AccountStore::new();
    let env = TestEnv::new();

    let samples = [
        (1, 1_000, 500_000_000, 500_000_000, 10),
        (2, 20_000, 700_000_000, 300_000_000, 20),
        (3, 65_000, 600_000_000, 400_000_000, 30),
    ];
    for (height, timestamp_ms, yes_price, no_price, volume_nanos) in samples {
        let mut header = sample_header(height);
        header.timestamp_ms = timestamp_ms;
        let point = crate::market_info::PricePoint {
            height,
            timestamp_ms,
            yes_price: Nanos(yes_price),
            no_price: Nanos(no_price),
            volume_nanos,
        };
        store
            .save_block(env.snapshot_with_price_points(
                &accounts,
                &markets,
                &lifecycle,
                &header,
                vec![(market_id, point)],
            ))
            .await
            .unwrap();
    }

    let page = store
        .load_price_candles(market_id, 60, Some(0), Some(180_000), None, 10)
        .await
        .unwrap();
    assert_eq!(page.resolution_secs, 60);
    assert_eq!(
        page.candles.len(),
        2,
        "no synthetic empty bucket should be stored"
    );

    let first = &page.candles[0];
    assert_eq!(first.bucket_start_ms, 0);
    assert_eq!(first.bucket_end_ms, 60_000);
    assert_eq!(first.first_height, 1);
    assert_eq!(first.last_height, 2);
    assert_eq!(first.open_yes_price, Nanos(500_000_000));
    assert_eq!(first.high_yes_price, Nanos(700_000_000));
    assert_eq!(first.low_yes_price, Nanos(500_000_000));
    assert_eq!(first.close_yes_price, Nanos(700_000_000));
    assert_eq!(first.open_no_price, Nanos(500_000_000));
    assert_eq!(first.high_no_price, Nanos(500_000_000));
    assert_eq!(first.low_no_price, Nanos(300_000_000));
    assert_eq!(first.close_no_price, Nanos(300_000_000));
    assert_eq!(first.volume_nanos, 30);
    assert_eq!(first.point_count, 2);

    let second = &page.candles[1];
    assert_eq!(second.bucket_start_ms, 60_000);
    assert_eq!(second.first_height, 3);
    assert_eq!(second.last_height, 3);
    assert_eq!(second.open_yes_price, Nanos(600_000_000));
    assert_eq!(second.close_yes_price, Nanos(600_000_000));
    assert_eq!(second.volume_nanos, 30);
    assert_eq!(second.point_count, 1);

    let newest = store
        .load_price_candles(market_id, 60, None, None, None, 1)
        .await
        .unwrap();
    assert_eq!(newest.next_before_ms, Some(60_000));
    assert_eq!(newest.candles[0].bucket_start_ms, 60_000);

    let older = store
        .load_price_candles(market_id, 60, None, None, newest.next_before_ms, 1)
        .await
        .unwrap();
    assert_eq!(older.next_before_ms, None);
    assert_eq!(older.candles[0].bucket_start_ms, 0);
}

#[tokio::test]
async fn test_store_restores_latest_committed_accounts() {
    let path = temp_db_path("store-restore");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let markets = MarketSet::new();
    let env = TestEnv::new();

    let mut accounts = AccountStore::new();
    let account_id = accounts.create_account(100);
    store
        .save_block(env.snapshot(
            &accounts,
            &markets,
            &lifecycle,
            &sample_header(1),
            1,
            None,
            vec![],
        ))
        .await
        .unwrap();

    accounts.get_mut(account_id).unwrap().balance = 200;
    store
        .save_block(env.snapshot(
            &accounts,
            &markets,
            &lifecycle,
            &sample_header(2),
            1,
            None,
            vec![],
        ))
        .await
        .unwrap();

    let restored = store.load_state().await.unwrap().unwrap();
    assert_eq!(restored.height, 2);
    assert_eq!(restored.accounts.get(account_id).unwrap().balance, 200);
}

#[tokio::test]
async fn test_store_recovery_treats_redb_fence_as_commit_point() {
    let path = temp_db_path("store-redb-fence-commit-point");
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let markets = MarketSet::new();
    let env = TestEnv::new();

    let mut accounts = AccountStore::new();
    let account_id = accounts.create_account(100);
    let (header_1, witness_1) =
        coherent_header_and_witness(1, &accounts, &markets, &lifecycle, &env.bridge_state);

    let mut accounts_after_uncommitted_qmdb = accounts.clone();
    accounts_after_uncommitted_qmdb
        .get_mut(account_id)
        .unwrap()
        .balance = 200;
    let (header_2, witness_2) = coherent_header_and_witness(
        2,
        &accounts_after_uncommitted_qmdb,
        &markets,
        &lifecycle,
        &env.bridge_state,
    );

    {
        let store = Store::open(&path).unwrap();
        store
            .save_block_with_witness(
                env.snapshot(&accounts, &markets, &lifecycle, &header_1, 1, None, vec![]),
                &witness_1,
            )
            .await
            .unwrap();

        let committed_root = store.current_state_qmdb_root().await.unwrap().unwrap();
        assert_eq!(committed_root.slot, AccountSnapshotSlot::A);
        assert_eq!(committed_root.root, header_1.state_root);

        // Simulate a crash after the inactive qMDB slot was written but
        // before redb committed the fence flip for height 2.
        store
            .account_state_store
            .persist(CommittedAccountState {
                accounts: &accounts_after_uncommitted_qmdb,
                state_sidecar: &witness_2.state_sidecar,
                height: header_2.height,
                next_account_id: accounts_after_uncommitted_qmdb.next_id(),
                slot: AccountSnapshotSlot::B,
            })
            .await
            .unwrap();

        let uncommitted_root = store.state_qmdb_root(AccountSnapshotSlot::B).await.unwrap();
        assert_eq!(uncommitted_root.root, header_2.state_root);
        let still_committed_root = store.current_state_qmdb_root().await.unwrap().unwrap();
        assert_eq!(still_committed_root.slot, AccountSnapshotSlot::A);
        assert_eq!(still_committed_root.root, header_1.state_root);
    }

    let reopened = Store::open(&path).unwrap();
    let restored = reopened.load_state().await.unwrap().unwrap();
    assert_eq!(restored.height, 1);
    assert_eq!(restored.accounts.get(account_id).unwrap().balance, 100);
    let restored_root = reopened.current_state_qmdb_root().await.unwrap().unwrap();
    assert_eq!(restored_root.slot, AccountSnapshotSlot::A);
    assert_eq!(restored_root.root, header_1.state_root);

    // Once save_block completes its redb transaction, the same qMDB slot is
    // authoritative after restart.
    reopened
        .save_block_with_witness(
            env.snapshot(
                &accounts_after_uncommitted_qmdb,
                &markets,
                &lifecycle,
                &header_2,
                1,
                None,
                vec![],
            ),
            &witness_2,
        )
        .await
        .unwrap();
    drop(reopened);

    let reopened_after_commit = Store::open(&path).unwrap();
    let restored_after_commit = reopened_after_commit.load_state().await.unwrap().unwrap();
    assert_eq!(restored_after_commit.height, 2);
    assert_eq!(
        restored_after_commit
            .accounts
            .get(account_id)
            .unwrap()
            .balance,
        200
    );
    let committed_after_flip = reopened_after_commit
        .current_state_qmdb_root()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(committed_after_flip.slot, AccountSnapshotSlot::B);
    assert_eq!(committed_after_flip.root, header_2.state_root);
}

#[tokio::test]
async fn test_store_restores_history_event_next_seq() {
    use crate::aggregates::{HistoryEvent, HistoryKind, StoredHistoryEvent};

    let path = temp_db_path("store-history-next-seq");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let markets = MarketSet::new();
    let env = TestEnv::new();

    let mut accounts = AccountStore::new();
    let account_id = accounts.create_account(100);
    let header = sample_header(1);
    let mut placed = HistoryEvent::new(
        account_id,
        HistoryKind::Placed,
        header.height,
        header.timestamp_ms,
    );
    placed.seq = 0;
    let mut filled = HistoryEvent::new(
        account_id,
        HistoryKind::Filled,
        header.height,
        header.timestamp_ms,
    );
    filled.seq = 1;
    let history_events_delta = vec![
        StoredHistoryEvent::from_event(&placed),
        StoredHistoryEvent::from_event(&filled),
    ];

    store
        .save_block(env.snapshot_with_history_events(
            &accounts,
            &markets,
            &lifecycle,
            &header,
            2,
            history_events_delta,
        ))
        .await
        .unwrap();

    let restored = store.load_state().await.unwrap().unwrap();
    assert_eq!(restored.analytics.history_event_next_seq, 2);

    // Backward compatibility for stores created before the explicit counter:
    // derive the next cursor from existing history-event keys.
    let txn = store.db.begin_write().unwrap();
    {
        let mut counters = txn.open_table(COUNTERS).unwrap();
        counters.remove(KEY_HISTORY_EVENT_NEXT_SEQ).unwrap();
    }
    txn.commit().unwrap();

    let restored = store.load_state().await.unwrap().unwrap();
    assert_eq!(restored.analytics.history_event_next_seq, 2);
}

#[tokio::test]
async fn save_block_with_witness_persists_latest_witness() {
    let path = temp_db_path("store-witness");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let markets = MarketSet::new();
    let env = TestEnv::new();
    let mut accounts = AccountStore::new();
    accounts.create_account(100);

    let (header, witness) =
        coherent_header_and_witness(1, &accounts, &markets, &lifecycle, &env.bridge_state);
    store
        .save_block_with_witness(
            env.snapshot(&accounts, &markets, &lifecycle, &header, 1, None, vec![]),
            &witness,
        )
        .await
        .unwrap();

    let latest = store
        .latest_block_witness()
        .unwrap()
        .expect("latest witness persisted");
    let by_height = store
        .block_witness(header.height)
        .unwrap()
        .expect("height witness persisted");

    assert_eq!(latest.header.height, header.height);
    assert_eq!(latest.header.state_root, header.state_root);
    assert_eq!(by_height.header.height, header.height);
}

#[tokio::test]
async fn da_artifact_from_witness_matches_commitment_chain() {
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let markets = MarketSet::new();
    let env = TestEnv::new();
    let mut accounts = AccountStore::new();
    accounts.create_account(100);
    let (header, witness) =
        coherent_header_and_witness(1, &accounts, &markets, &lifecycle, &env.bridge_state);

    let artifact = DaArtifact::from_witness(&witness);
    artifact.verify_payload_integrity().unwrap();
    assert_eq!(artifact.manifest.height, header.height);
    assert_eq!(artifact.manifest.state_root, header.state_root);
    assert_eq!(artifact.manifest.payload_len, artifact.payload.len() as u64);
    assert_eq!(
        artifact.manifest.payload_root,
        sybil_zk::da_witness_payload_root(&artifact.payload)
    );

    let provider_refs: Vec<_> = artifact
        .manifest
        .provider_refs
        .iter()
        .map(|provider_ref| provider_ref.bytes.clone())
        .collect();
    assert_eq!(
        artifact.manifest.provider_refs_hash,
        sybil_zk::da_provider_refs_hash(&provider_refs)
    );
    assert_eq!(
        artifact.manifest.da_commitment,
        sybil_zk::da_commitment_from_parts(
            artifact.manifest.height,
            artifact.manifest.state_root,
            artifact.manifest.witness_root,
            artifact.manifest.payload_root,
            artifact.manifest.payload_len,
            artifact.manifest.provider_refs_hash,
        )
    );
    let provider_ref = artifact.manifest.provider_refs.first().unwrap();
    assert_eq!(provider_ref.kind, DA_FILE_PROVIDER_REF_KIND);
    assert_eq!(provider_ref.encoding, DA_FILE_PROVIDER_REF_ENCODING);
    let expected_uri = format!(
        "sybil-file://witness/{}.witness.bin",
        hex::encode(artifact.manifest.payload_root)
    );
    assert_eq!(provider_ref.uri.as_deref(), Some(expected_uri.as_str()));
}

#[tokio::test]
async fn save_block_with_witness_prunes_historical_witnesses() {
    let path = temp_db_path("store-witness-prune");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let markets = MarketSet::new();
    let env = TestEnv::new();
    let mut accounts = AccountStore::new();
    accounts.create_account(100);

    let (header1, witness1) =
        coherent_header_and_witness(1, &accounts, &markets, &lifecycle, &env.bridge_state);
    store
        .save_block_with_witness(
            env.snapshot(&accounts, &markets, &lifecycle, &header1, 1, None, vec![]),
            &witness1,
        )
        .await
        .unwrap();

    let (header2, witness2) =
        coherent_header_and_witness(2, &accounts, &markets, &lifecycle, &env.bridge_state);
    store
        .save_block_with_witness(
            env.snapshot(&accounts, &markets, &lifecycle, &header2, 1, None, vec![]),
            &witness2,
        )
        .await
        .unwrap();

    assert!(store.block_witness(header1.height).unwrap().is_none());
    let latest = store
        .latest_block_witness()
        .unwrap()
        .expect("latest witness retained");
    assert_eq!(latest.header.height, header2.height);
}

#[tokio::test]
async fn save_block_with_witness_rejects_mismatched_header() {
    let path = temp_db_path("store-witness-mismatch");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let markets = MarketSet::new();
    let env = TestEnv::new();
    let accounts = AccountStore::new();

    let header = sample_header(1);
    let mut witness = sample_witness(&header);
    witness.header.height = 2;

    let error = store
        .save_block_with_witness(
            env.snapshot(&accounts, &markets, &lifecycle, &header, 1, None, vec![]),
            &witness,
        )
        .await
        .unwrap_err();

    assert!(matches!(error, StoreError::WitnessHeaderMismatch));
}

#[tokio::test]
async fn save_block_without_witness_clears_stale_witness_for_height() {
    let path = temp_db_path("store-witness-clear");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let markets = MarketSet::new();
    let env = TestEnv::new();
    let accounts = AccountStore::new();

    let (header, witness) =
        coherent_header_and_witness(1, &accounts, &markets, &lifecycle, &env.bridge_state);
    store
        .save_block_with_witness(
            env.snapshot(&accounts, &markets, &lifecycle, &header, 1, None, vec![]),
            &witness,
        )
        .await
        .unwrap();
    assert!(store.latest_block_witness().unwrap().is_some());

    store
        .save_block(env.snapshot(&accounts, &markets, &lifecycle, &header, 1, None, vec![]))
        .await
        .unwrap();

    assert!(store.latest_block_witness().unwrap().is_none());
}

#[tokio::test]
async fn test_store_restores_pending_bridge_wals() {
    let path = temp_db_path("store-bridge-wal");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let markets = MarketSet::new();
    let env = TestEnv::new();

    let mut accounts = AccountStore::new();
    let account_id = accounts.create_account(0);
    store
        .save_block(env.snapshot(
            &accounts,
            &markets,
            &lifecycle,
            &sample_header(1),
            1,
            None,
            vec![],
        ))
        .await
        .unwrap();

    let deposit = L1Deposit {
        deposit_id: 1,
        account_id: Some(account_id),
        chain_id: 1,
        vault_address: [0x10; 20],
        token_address: [0x20; 20],
        sender: [0x30; 20],
        sybil_account_key: crate::bridge::account_key(account_id),
        amount_token_units: 10_000,
        deposit_root: [0x44; 32],
    };
    let withdrawal = BridgeWithdrawalRequest {
        account_id,
        chain_id: 1,
        vault_address: [0x10; 20],
        recipient: [0x40; 20],
        token_address: [0x20; 20],
        amount_token_units: 4_000,
        expiry_height: 10,
    };
    store.append_pending_l1_deposit(&deposit).await.unwrap();
    store
        .append_pending_bridge_withdrawal(&withdrawal)
        .await
        .unwrap();
    let l1_input = BridgeL1Input::ObservedHeight(42);
    store
        .append_pending_bridge_l1_input(&l1_input)
        .await
        .unwrap();

    let restored = store.load_state().await.unwrap().unwrap();
    assert_eq!(restored.pending_l1_deposits, vec![deposit]);
    assert_eq!(restored.pending_bridge_withdrawals, vec![withdrawal]);
    assert_eq!(restored.pending_bridge_l1_inputs, vec![l1_input]);
}

#[tokio::test]
async fn test_store_restores_market_volumes() {
    let path = temp_db_path("store-market-volumes");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("Will it rain?");
    let env = TestEnv::new();
    let accounts = AccountStore::new();

    let volumes = HashMap::from([(market_id, 42_000_000_000u64)]);
    store
        .save_block(env.snapshot(
            &accounts,
            &markets,
            &lifecycle,
            &sample_header(1),
            1,
            Some(&volumes),
            vec![],
        ))
        .await
        .unwrap();

    let restored = store.load_state().await.unwrap().unwrap();
    assert_eq!(
        restored.analytics.market_volumes.get(&market_id),
        Some(&42_000_000_000)
    );
}

#[tokio::test]
async fn test_store_restores_resting_orders() {
    use crate::order_book::OrderBook;
    use matching_engine::{outcome_buy, MarketSet, NANOS_PER_DOLLAR};

    let path = temp_db_path("store-resting-orders");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let env = TestEnv::new();

    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("Test");

    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);

    let mut book = OrderBook::new(10);
    let order = outcome_buy(&markets, 1, market_id, 0, NANOS_PER_DOLLAR / 2, 5);
    book.accept(order, aid, accounts.get(aid).unwrap(), 1, 0)
        .unwrap();
    let expected_reserved = book.reserved_balance(aid);
    assert!(expected_reserved > 0);
    let snapshot = book.snapshot();
    assert_eq!(snapshot.len(), 1);

    store
        .save_block(env.snapshot(
            &accounts,
            &markets,
            &lifecycle,
            &sample_header(1),
            2,
            None,
            snapshot,
        ))
        .await
        .unwrap();

    let restored = store.load_state().await.unwrap().unwrap();
    assert_eq!(restored.resting_orders.len(), 1);
    assert_eq!(restored.resting_orders[0].account_id, aid);
    assert_eq!(restored.resting_orders[0].order.id, 1);
    assert_eq!(
        restored.resting_orders[0].reserved_balance,
        expected_reserved
    );
    assert_eq!(restored.resting_orders[0].created_at, 1);

    let rebuilt = OrderBook::restore(restored.resting_orders, 10);
    assert_eq!(rebuilt.reserved_balance(aid), expected_reserved);
    assert_eq!(rebuilt.len(), 1);
}

#[tokio::test]
async fn store_restore_rejects_doctored_reserved_balance_high_and_low() {
    use crate::order_book::OrderBook;
    use matching_engine::{outcome_buy, MarketSet, NANOS_PER_DOLLAR};

    for (case, delta) in [("high", 1_i64), ("low", -1_i64), ("correct", 0_i64)] {
        let path = temp_db_path(&format!("store-reserved-balance-{case}"));
        let store = Store::open(&path).unwrap();
        let lifecycle = MarketLifecycle::new(Arc::new(AdminOracle::new()));
        let env = TestEnv::new();
        let mut markets = MarketSet::new();
        let market_id = markets.add_binary("reservation restore corruption");
        let mut accounts = AccountStore::new();
        let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
        let mut book = OrderBook::new(10);
        book.accept(
            outcome_buy(&markets, 1, market_id, 0, NANOS_PER_DOLLAR / 2, 5),
            aid,
            accounts.get(aid).unwrap(),
            1,
            0,
        )
        .unwrap();
        let recomputed = book.reserved_balance(aid);
        let mut doctored = book.snapshot();
        doctored[0].reserved_balance += delta;
        let stored = recomputed + delta;

        store
            .save_block(env.snapshot(
                &accounts,
                &markets,
                &lifecycle,
                &sample_header(1),
                2,
                None,
                doctored,
            ))
            .await
            .unwrap();

        if delta == 0 {
            let restored = store.load_state().await.unwrap().unwrap();
            assert_eq!(restored.resting_orders[0].reserved_balance, recomputed);
            continue;
        }

        let error = match store.load_state().await {
            Err(error) => error,
            Ok(_) => panic!("doctored reserved_balance unexpectedly restored"),
        };
        assert!(matches!(
            error,
            StoreError::CorruptLayout(ref message)
                if message == &format!(
                    "reserved_balance mismatch for account {} order 1: stored {stored}, recomputed {recomputed}",
                    aid.0
                )
        ));
    }
}

/// A partially-filled remainder's reservation is proportionally scaled and can
/// legitimately exceed the admission formula for its remaining quantity. Both
/// the store snapshot path and the witness import path must restore such a
/// book instead of falsely flagging it as corrupt, while still rejecting an
/// under-collateralized (below remainder cost) value.
#[tokio::test]
async fn store_restore_accepts_matched_remainder_and_rejects_under_reservation() {
    use crate::order_book::OrderBook;
    use matching_engine::{outcome_buy, Fill, MarketSet, Nanos, Qty, NANOS_PER_DOLLAR};
    use std::collections::HashSet;

    let lifecycle = MarketLifecycle::new(Arc::new(AdminOracle::new()));
    let env = TestEnv::new();
    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("matched remainder restore");
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
    let mut book = OrderBook::new(10);
    // price 101 nanos x qty 10: admission reserves ceil(1010/1000) = 2; after
    // filling 1 the proportional remainder keeps ceil(2*9/10) = 2 while the
    // admission formula for the remaining 9 is ceil(909/1000) = 1.
    book.accept(
        outcome_buy(&markets, 1, market_id, 0, 101, 10),
        aid,
        accounts.get(aid).unwrap(),
        1,
        0,
    )
    .unwrap();
    book.settle(
        &[Fill {
            order_id: 1,
            fill_qty: Qty(1),
            fill_price: Nanos(101),
            account_id: aid.0,
        }],
        &HashSet::new(),
        1,
    )
    .unwrap();
    let snapshot = book.snapshot();
    assert!(snapshot[0].has_been_matched);
    assert_eq!(snapshot[0].reserved_balance, 2);

    // Correct matched remainder restores.
    let path = temp_db_path("store-matched-remainder-correct");
    let store = Store::open(&path).unwrap();
    store
        .save_block(env.snapshot(
            &accounts,
            &markets,
            &lifecycle,
            &sample_header(1),
            2,
            None,
            snapshot.clone(),
        ))
        .await
        .unwrap();
    let restored = store.load_state().await.unwrap().unwrap();
    assert_eq!(restored.resting_orders[0].reserved_balance, 2);

    // Below the remainder's worst-case cost refuses to serve.
    let mut under_reserved = snapshot;
    under_reserved[0].reserved_balance = 0;
    let path = temp_db_path("store-matched-remainder-low");
    let store = Store::open(&path).unwrap();
    store
        .save_block(env.snapshot(
            &accounts,
            &markets,
            &lifecycle,
            &sample_header(1),
            2,
            None,
            under_reserved,
        ))
        .await
        .unwrap();
    let error = match store.load_state().await {
        Err(error) => error,
        Ok(_) => panic!("under-reserved matched remainder unexpectedly restored"),
    };
    assert!(matches!(
        error,
        StoreError::CorruptLayout(ref message)
            if message == &format!(
                "reserved_balance below remainder cost for account {} order 1: stored 0, minimum 1",
                aid.0
            )
    ));
}

#[tokio::test]
async fn witness_import_rejects_doctored_reserved_balance_high_and_low_and_accepts_correct() {
    use crate::order_book::OrderBook;
    use matching_engine::{outcome_buy, Fill, MarketSet, Nanos, Qty, NANOS_PER_DOLLAR};
    use std::collections::HashSet;

    let lifecycle = MarketLifecycle::new(Arc::new(AdminOracle::new()));
    let env = TestEnv::new();
    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("witness reservation corruption");
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
    let mut book = OrderBook::new(10);
    book.accept(
        outcome_buy(&markets, 1, market_id, 0, NANOS_PER_DOLLAR / 2, 5),
        aid,
        accounts.get(aid).unwrap(),
        1,
        0,
    )
    .unwrap();
    // A second order that becomes a matched remainder whose proportional
    // reservation (2) exceeds the admission formula for its remaining
    // quantity (1) — the sidecar carries no matched provenance, so import
    // must infer it rather than falsely reject the witness.
    book.accept(
        outcome_buy(&markets, 2, market_id, 0, 101, 10),
        aid,
        accounts.get(aid).unwrap(),
        1,
        0,
    )
    .unwrap();
    book.settle(
        &[Fill {
            order_id: 2,
            fill_qty: Qty(1),
            fill_price: Nanos(101),
            account_id: aid.0,
        }],
        &HashSet::new(),
        1,
    )
    .unwrap();
    let recomputed = book.reserved_balance(aid);
    let canonical_accounts = crate::canonical_state::CanonicalState::from_accounts(&accounts);
    let sidecar = state_sidecar_snapshot_from_resting_orders(
        &env.bridge_state,
        &book.snapshot(),
        &markets,
        &[],
        &lifecycle,
        &HashMap::new(),
    );
    let state_root = sybil_verifier::block::compute_state_root_with_sidecar(
        canonical_accounts.as_snapshots(),
        &sidecar,
    );
    let header = BlockHeader {
        height: 1,
        parent_hash: [0; 32],
        state_root,
        events_root: sybil_verifier::event_commitment::empty_events_root(),
        order_count: 0,
        fill_count: 0,
        timestamp_ms: 1_000,
    };
    let mut correct = sample_witness(&header);
    correct.post_state = canonical_accounts.into_snapshots();
    correct.state_sidecar = sidecar;

    for (case, delta) in [("high", 1_i64), ("low", -1_i64)] {
        let mut doctored = correct.clone();
        doctored.state_sidecar.account_reservations[0].reserved_balance += delta;
        doctored.header.state_root = sybil_verifier::block::compute_state_root_with_sidecar(
            &doctored.post_state,
            &doctored.state_sidecar,
        );
        let stored = recomputed + delta;
        let path = temp_db_path(&format!("witness-reserved-balance-{case}"));
        let store = Store::open(&path).unwrap();

        let error = store
            .import_witness_genesis(doctored, None, None, SequencerConfig::default())
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            StoreError::WitnessImport(ref message)
                if message == &format!(
                    "reserved_balance mismatch for account {}: stored {stored}, recomputed {recomputed}",
                    aid.0
                )
        ));
    }

    let path = temp_db_path("witness-reserved-balance-correct");
    let store = Store::open(&path).unwrap();
    store
        .import_witness_genesis(correct, None, None, SequencerConfig::default())
        .await
        .unwrap();
    let restored = store.load_state().await.unwrap().unwrap();
    let restored_total: i64 = restored
        .resting_orders
        .iter()
        .map(|resting| resting.reserved_balance)
        .sum();
    assert_eq!(restored_total, recomputed);
    let remainder = restored
        .resting_orders
        .iter()
        .find(|resting| resting.order.id == 2)
        .unwrap();
    assert_eq!(remainder.reserved_balance, 2);
}

#[tokio::test]
async fn test_store_clears_resting_orders_when_snapshot_empty() {
    use crate::order_book::OrderBook;
    use matching_engine::{outcome_buy, MarketSet, NANOS_PER_DOLLAR};

    let path = temp_db_path("store-resting-orders-empty");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let env = TestEnv::new();

    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("Test");
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);

    // Block 1: save with a resting order.
    let mut book = OrderBook::new(10);
    let order = outcome_buy(&markets, 1, market_id, 0, NANOS_PER_DOLLAR / 2, 5);
    book.accept(order, aid, accounts.get(aid).unwrap(), 1, 0)
        .unwrap();
    store
        .save_block(env.snapshot(
            &accounts,
            &markets,
            &lifecycle,
            &sample_header(1),
            2,
            None,
            book.snapshot(),
        ))
        .await
        .unwrap();

    // Block 2: save with an empty snapshot (order filled/cancelled/expired).
    store
        .save_block(env.snapshot(
            &accounts,
            &markets,
            &lifecycle,
            &sample_header(2),
            2,
            None,
            vec![],
        ))
        .await
        .unwrap();

    let restored = store.load_state().await.unwrap().unwrap();
    assert!(restored.resting_orders.is_empty());
}

#[tokio::test]
async fn test_store_roundtrips_account_fill_history() {
    let path = temp_db_path("store-fill-history");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle.clone());
    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("Test");
    let mut accounts = AccountStore::new();
    let account_id = accounts.create_account(100);
    let env = TestEnv::new();

    let fill = AccountFillRecord {
        order_id: 42,
        fill_qty: 7,
        fill_price: Nanos(600_000_000),
        block_height: 1,
        timestamp_ms: 1_000,
        position_deltas: vec![(market_id, 0, 7)],
    };

    store
        .save_block(env.snapshot_with_fills(
            &accounts,
            &markets,
            &lifecycle,
            &sample_header(1),
            vec![(account_id, fill.clone())],
        ))
        .await
        .unwrap();

    let restored = store.load_state().await.unwrap().unwrap();
    assert_eq!(
        restored.analytics.account_fills,
        vec![(account_id, fill.clone())]
    );

    let seq = crate::sequencer::BlockSequencer::restore(
        restored,
        oracle,
        crate::sequencer::SequencerConfig::default(),
    );
    assert_eq!(
        seq.analytics()
            .account_fills(account_id, Some(market_id), 10, 0),
        vec![fill]
    );
}

#[tokio::test]
async fn test_store_persists_fill_recorder_snapshot_from_committed_block() {
    use crate::sequencer::{BlockSequencer, OrderSubmission, SequencerConfig};
    use matching_engine::{outcome_buy, outcome_sell, NANOS_PER_DOLLAR};

    let path = temp_db_path("store-fill-recorder-snapshot");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());

    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("Test");
    let mut accounts = AccountStore::new();
    let buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let seller = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    accounts
        .get_mut(seller)
        .unwrap()
        .positions
        .insert((market_id, 0), 10);

    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        oracle.clone(),
        SequencerConfig::default(),
    );
    seq.produce_block(
        vec![
            OrderSubmission {
                account_id: buyer,
                orders: vec![outcome_buy(&markets, 0, market_id, 0, 700_000_000, 5)],
                mm_constraint: None,
            },
            OrderSubmission {
                account_id: seller,
                orders: vec![outcome_sell(&markets, 0, market_id, 0, 300_000_000, 5)],
                mm_constraint: None,
            },
        ],
        1_000,
    );

    assert!(
        !seq.analytics().account_fills(buyer, None, 10, 0).is_empty(),
        "sanity check: block should record buyer fills before persistence"
    );
    store.save_block(seq.snapshot()).await.unwrap();

    let restored = store.load_state().await.unwrap().unwrap();
    let restored_seq = BlockSequencer::restore(restored, oracle, SequencerConfig::default());
    let fills = restored_seq
        .analytics()
        .account_fills(buyer, Some(market_id), 10, 0);
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].fill_qty, 5);
    assert_eq!(fills[0].block_height, 1);
}

#[tokio::test]
async fn import_witness_drill_restores_head_and_produces_children() {
    use crate::sequencer::{BlockSequencer, OrderSubmission, SequencerConfig};
    use matching_engine::{outcome_buy, outcome_sell, MarketId, NANOS_PER_DOLLAR};

    let source_path = temp_db_path("store-import-witness-source");
    let fresh_path = temp_db_path("store-import-witness-fresh");
    let source_store = Store::open(&source_path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let config = SequencerConfig::default();

    let mut markets = MarketSet::new();
    let active_a = markets.add_binary("Active A");
    let active_b = markets.add_binary("Active B");
    let fill_market = markets.add_binary("Fill Market");
    let resolved_market = markets.add_binary("Resolved Market");

    let mut accounts = AccountStore::new();
    let resting_buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let fill_buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let fill_seller = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let resolve_buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let resolve_seller = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let bridge_account = accounts.create_account(0);
    accounts
        .get_mut(fill_seller)
        .unwrap()
        .positions
        .insert((fill_market, 0), 10);
    accounts
        .get_mut(resolve_seller)
        .unwrap()
        .positions
        .insert((resolved_market, 0), 10);

    let mut group = matching_engine::MarketGroup::new("Active group");
    group.add_market(active_a);
    group.add_market(active_b);

    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![group],
        oracle.clone(),
        config.clone(),
    );
    let signing_key =
        <p256::ecdsa::SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
            &mut p256::elliptic_curve::rand_core::UnwrapErr(getrandom::SysRng),
        );
    seq.register_pubkey(
        fill_buyer,
        crate::crypto::PublicKey(*signing_key.verifying_key()),
    )
    .unwrap();

    let opening_deposit = next_l1_deposit_for(&seq, bridge_account, 50_000);
    seq.ingest_l1_deposit(opening_deposit).unwrap();

    let first = seq.produce_block(
        vec![
            OrderSubmission {
                account_id: resting_buyer,
                orders: vec![outcome_buy(
                    &markets,
                    0,
                    active_a,
                    0,
                    NANOS_PER_DOLLAR / 2,
                    10,
                )],
                mm_constraint: None,
            },
            OrderSubmission {
                account_id: fill_buyer,
                orders: vec![outcome_buy(&markets, 0, fill_market, 0, 700_000_000, 5)],
                mm_constraint: None,
            },
            OrderSubmission {
                account_id: fill_seller,
                orders: vec![outcome_sell(&markets, 0, fill_market, 0, 300_000_000, 5)],
                mm_constraint: None,
            },
            OrderSubmission {
                account_id: resolve_buyer,
                orders: vec![outcome_buy(&markets, 0, resolved_market, 0, 700_000_000, 5)],
                mm_constraint: None,
            },
            OrderSubmission {
                account_id: resolve_seller,
                orders: vec![outcome_sell(
                    &markets,
                    0,
                    resolved_market,
                    0,
                    300_000_000,
                    5,
                )],
                mm_constraint: None,
            },
        ],
        1_000,
    );
    assert_eq!(first.block.header.height, 1);
    assert!(!first.block.fills.is_empty());
    assert_eq!(
        seq.order_book().len(),
        1,
        "first block leaves one order resting"
    );

    let deposit = next_l1_deposit_for(&seq, bridge_account, 100_000);
    seq.ingest_l1_deposit(deposit).unwrap();
    seq.request_bridge_withdrawal(crate::bridge::BridgeWithdrawalRequest {
        account_id: bridge_account,
        chain_id: 1,
        vault_address: eth_address(0x10),
        recipient: eth_address(0x40),
        token_address: eth_address(0x20),
        amount_token_units: 10_000,
        expiry_height: 10,
    })
    .unwrap();
    seq.resolve_market(resolved_market, Nanos(NANOS_PER_DOLLAR), 2_000)
        .unwrap();

    let second = seq.produce_block(
        vec![
            OrderSubmission {
                account_id: fill_buyer,
                orders: vec![outcome_buy(&markets, 0, fill_market, 0, 700_000_000, 5)],
                mm_constraint: None,
            },
            OrderSubmission {
                account_id: fill_seller,
                orders: vec![outcome_sell(&markets, 0, fill_market, 0, 300_000_000, 5)],
                mm_constraint: None,
            },
            OrderSubmission {
                account_id: AccountId(99_999),
                orders: vec![outcome_buy(&markets, 0, active_b, 0, 400_000_000, 1)],
                mm_constraint: None,
            },
        ],
        2_000,
    );

    let witness = &second.witness;
    assert_eq!(second.block.header.height, 2);
    assert!(!witness.orders.is_empty());
    assert!(!witness.rejections.is_empty());
    assert!(!witness.system_events.is_empty());
    assert!(!witness.deposit_accumulator.new_deposits.is_empty());
    assert_ne!(
        witness.deposit_accumulator.pre_frontier,
        sybil_l1_protocol::empty_deposit_frontier()
    );
    assert!(!witness.fills.is_empty());
    assert!(!witness.clearing_prices.is_empty());
    assert!(!witness.market_groups.is_empty());
    assert!(!witness.post_state.is_empty());
    assert!(!witness.pre_state_sidecar.markets.is_empty());
    assert!(!witness.state_sidecar.markets.is_empty());
    assert!(!witness.state_sidecar.resting_orders.is_empty());
    assert!(!witness.state_sidecar.account_reservations.is_empty());
    assert!(!witness.state_sidecar.bridge.withdrawals.is_empty());
    assert!(!witness.resolved_markets.is_empty());
    assert!(witness.state_sidecar.markets.iter().any(|market| {
        market.market_id == resolved_market
            && matches!(
                market.status,
                sybil_verifier::MarketStatusSnapshot::Resolved { .. }
            )
    }));

    source_store
        .save_block_with_witness_and_history(seq.snapshot(), witness, &second.sealed_block())
        .await
        .unwrap();
    source_store
        .save_da_artifact(DaArtifact::from_witness(witness))
        .await
        .unwrap();
    let artifact = source_store
        .load_da_artifact(second.block.header.height)
        .await
        .unwrap()
        .expect("DA artifact row exists");
    artifact.verify_payload_integrity().unwrap();
    let decoded = sybil_verifier::commitments::witness_schema::decode_canonical_witness_bytes(
        &artifact.payload,
    )
    .unwrap();
    assert_eq!(
        sybil_verifier::commitments::witness_schema::canonical_witness_bytes(&decoded),
        artifact.payload
    );

    let refused = source_store
        .import_witness_genesis(
            decoded.clone(),
            Some(second.block.header.state_root),
            None,
            config.clone(),
        )
        .await;
    assert!(
        matches!(refused, Err(StoreError::WitnessImport(ref message)) if message.contains("already has committed recovery metadata")),
        "expected typed non-empty-store refusal, got {refused:?}"
    );

    let fresh_store = Store::open(&fresh_path).unwrap();
    let summary = fresh_store
        .import_witness_genesis(
            decoded.clone(),
            Some(second.block.header.state_root),
            None,
            config.clone(),
        )
        .await
        .unwrap();
    assert_eq!(summary.height, second.block.header.height);
    assert_eq!(summary.state_root, second.block.header.state_root);
    assert_eq!(
        summary.genesis_hash,
        crate::block::hash_header(&first.block.header)
    );
    assert_eq!(summary.accounts, decoded.post_state.len());
    assert_eq!(summary.markets, decoded.state_sidecar.markets.len());
    assert_eq!(
        summary.market_groups,
        decoded.state_sidecar.market_groups.len()
    );
    assert_eq!(
        summary.resting_orders,
        decoded.state_sidecar.resting_orders.len()
    );
    assert_eq!(
        summary.account_reservations,
        decoded.state_sidecar.account_reservations.len()
    );
    assert_eq!(
        summary.withdrawals,
        decoded.state_sidecar.bridge.withdrawals.len()
    );
    assert_eq!(
        summary.deposit_cursor,
        decoded.state_sidecar.bridge.deposit_cursor
    );

    let latest = fresh_store
        .latest_block_witness()
        .unwrap()
        .expect("import writes latest witness row");
    assert_eq!(
        sybil_verifier::commitments::witness_schema::canonical_witness_bytes(&latest),
        sybil_verifier::commitments::witness_schema::canonical_witness_bytes(&decoded)
    );

    let restored = fresh_store.load_state().await.unwrap().unwrap();
    assert_eq!(restored.height, second.block.header.height);
    assert_eq!(restored.last_header.as_ref(), Some(&second.block.header));
    assert_eq!(restored.accounts.iter().count(), summary.accounts);
    assert_eq!(restored.markets.len(), summary.markets);
    assert_eq!(restored.market_groups.len(), summary.market_groups);
    assert_eq!(restored.resting_orders.len(), summary.resting_orders);
    assert_eq!(restored.bridge_state.deposit_cursor, summary.deposit_cursor);
    assert_eq!(restored.bridge_state.withdrawals.len(), summary.withdrawals);
    assert_eq!(restored.genesis_hash, summary.genesis_hash);
    let imported_prices = decoded
        .state_sidecar
        .markets
        .iter()
        .filter(|market| !market.last_clearing_prices.is_empty())
        .map(|market| (market.market_id, market.last_clearing_prices.clone()))
        .collect::<HashMap<_, _>>();
    assert_eq!(restored.analytics.last_clearing_prices, imported_prices);

    let signed_seq =
        BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), config.clone());
    assert_eq!(
        signed_seq.lookup_pubkey(&crate::crypto::PublicKey(*signing_key.verifying_key())),
        Some(fill_buyer),
        "witness import must rebuild the active signing-key registry"
    );
    let signed_handle = crate::actor::SequencerHandle::spawn(signed_seq);
    let signed = crate::crypto::sign_order(
        &outcome_buy(&markets, 0, active_b, 0, 400_000_000, 1),
        1,
        summary.genesis_hash,
        &signing_key,
    );
    signed_handle.submit_signed_order(signed).await.unwrap();

    let restored = fresh_store.load_state().await.unwrap().unwrap();
    let mut restored_seq =
        BlockSequencer::restore(restored, Arc::new(AdminOracle::new()), config.clone());
    let child = restored_seq.produce_block(Vec::new(), 3_000);
    assert_eq!(child.block.header.height, second.block.header.height + 1);
    assert_eq!(
        child.block.header.parent_hash,
        crate::block::hash_header(&second.block.header)
    );
    let child_verification = sybil_verifier::verify_full(&child.witness, false);
    assert!(
        child_verification.valid,
        "violations: {:?}",
        child_verification.violations
    );
    assert_eq!(
        child.block.header.state_root,
        sybil_verifier::block::compute_state_root_with_sidecar(
            &child.witness.post_state,
            &child.witness.state_sidecar,
        )
    );

    let grandchild = restored_seq.produce_block(Vec::new(), 4_000);
    assert_eq!(
        grandchild.block.header.height,
        second.block.header.height + 2
    );
    assert_eq!(
        grandchild.block.header.parent_hash,
        crate::block::hash_header(&child.block.header)
    );
    let grandchild_verification = sybil_verifier::verify_full(&grandchild.witness, false);
    assert!(
        grandchild_verification.valid,
        "violations: {:?}",
        grandchild_verification.violations
    );
    assert_eq!(
        grandchild.block.header.state_root,
        sybil_verifier::block::compute_state_root_with_sidecar(
            &grandchild.witness.post_state,
            &grandchild.witness.state_sidecar,
        )
    );

    assert_eq!(
        summary.next_market_id,
        markets.iter().map(|market| market.id.0).max().unwrap_or(0) + 1
    );
    assert!(
        summary.next_order_id
            > witness
                .orders
                .iter()
                .map(|order| order.order.id)
                .max()
                .unwrap()
    );
    assert_eq!(summary.next_withdrawal_id, 2);
    assert_eq!(
        summary.next_account_id,
        [
            resting_buyer,
            fill_buyer,
            fill_seller,
            resolve_buyer,
            resolve_seller,
            bridge_account,
        ]
        .into_iter()
        .map(|account_id| account_id.0)
        .max()
        .unwrap()
            + 1
    );
    assert!(seq
        .markets()
        .get(MarketId(summary.next_market_id))
        .is_none());
}

#[tokio::test]
async fn test_store_account_fills_reads_full_persisted_history() {
    use crate::sequencer::{BlockSequencer, OrderSubmission, SequencerConfig};
    use matching_engine::{outcome_buy, outcome_sell, NANOS_PER_DOLLAR};

    let path = temp_db_path("store-account-fills-read");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());

    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("Test");
    let mut accounts = AccountStore::new();
    let buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let seller = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    accounts
        .get_mut(seller)
        .unwrap()
        .positions
        .insert((market_id, 0), 10);

    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        oracle.clone(),
        SequencerConfig::default(),
    );
    // Two blocks, each crossing one unit, so two distinct buyer fills persist.
    for height in 1..=2u64 {
        seq.produce_block(
            vec![
                OrderSubmission {
                    account_id: buyer,
                    orders: vec![outcome_buy(&markets, 0, market_id, 0, 700_000_000, 1)],
                    mm_constraint: None,
                },
                OrderSubmission {
                    account_id: seller,
                    orders: vec![outcome_sell(&markets, 0, market_id, 0, 300_000_000, 1)],
                    mm_constraint: None,
                },
            ],
            height * 1_000,
        );
        store.save_block(seq.snapshot()).await.unwrap();
    }

    // Reads straight from the durable store, independent of any in-memory recorder.
    let fills = store.account_fills(buyer, None, 10, 0).unwrap();
    assert_eq!(fills.len(), 2, "both persisted fills should be served");
    assert!(store.account_fills(buyer, None, 0, 0).unwrap().is_empty());
    // Newest-first: block 2 ahead of block 1.
    assert_eq!(fills[0].block_height, 2);
    assert_eq!(fills[1].block_height, 1);

    let forward = store
        .account_fills_after(buyer, None, Some(AccountFillCursor::MIN), 10)
        .unwrap();
    assert_eq!(forward.len(), 2);
    assert!(store
        .account_fills_after(buyer, None, Some(AccountFillCursor::MIN), 0)
        .unwrap()
        .is_empty());
    assert_eq!(forward[0].block_height, 1);
    assert_eq!(forward[1].block_height, 2);
    let cursor = AccountFillCursor::from_record(&forward[0]);
    let after_first = store
        .account_fills_after(buyer, None, Some(cursor), 10)
        .unwrap();
    assert_eq!(after_first.len(), 1);
    assert_eq!(after_first[0].block_height, 2);

    // Market filter keeps fills that touch the traded market...
    assert_eq!(
        store
            .account_fills(buyer, Some(market_id), 10, 0)
            .unwrap()
            .len(),
        2
    );
    // ...and drops everything for a market the account never traded.
    let untraded = markets.add_binary("Untraded");
    assert!(store
        .account_fills(buyer, Some(untraded), 10, 0)
        .unwrap()
        .is_empty());

    // offset/limit page over the newest-first sequence.
    let page = store.account_fills(buyer, None, 1, 1).unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].block_height, 1);

    // Unknown account => empty, no error.
    assert!(store
        .account_fills(AccountId(99), None, 10, 0)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_store_fill_cursor_pagination_survives_reopen() {
    use crate::sequencer::{BlockSequencer, OrderSubmission, SequencerConfig};
    use matching_engine::{outcome_buy, outcome_sell, NANOS_PER_DOLLAR};

    let path = temp_db_path("store-account-fills-cursor-reopen");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());

    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("Test");
    let mut accounts = AccountStore::new();
    let buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let seller = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    accounts
        .get_mut(seller)
        .unwrap()
        .positions
        .insert((market_id, 0), 10);

    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        oracle,
        SequencerConfig::default(),
    );
    for height in 1..=2u64 {
        let prepared = seq
            .prepare_block(
                vec![
                    OrderSubmission {
                        account_id: buyer,
                        orders: vec![outcome_buy(&markets, 0, market_id, 0, 700_000_000, 1)],
                        mm_constraint: None,
                    },
                    OrderSubmission {
                        account_id: seller,
                        orders: vec![outcome_sell(&markets, 0, market_id, 0, 300_000_000, 1)],
                        mm_constraint: None,
                    },
                ],
                height * 1_000,
            )
            .unwrap();
        store
            .save_block(prepared.next_sequencer().snapshot())
            .await
            .unwrap();
        seq.commit_prepared_block(prepared).unwrap();
    }
    drop(store);

    let reopened = Store::open(&path).unwrap();
    let fills = reopened
        .account_fills_after(buyer, None, Some(AccountFillCursor::MIN), 10)
        .unwrap();
    let heights: Vec<u64> = fills.iter().map(|fill| fill.block_height).collect();
    assert_eq!(heights, vec![1, 2]);
}

#[tokio::test]
async fn test_store_persists_fill_delta_when_hot_cap_is_zero() {
    use crate::sequencer::{BlockSequencer, OrderSubmission, SequencerConfig};
    use matching_engine::{outcome_buy, outcome_sell, NANOS_PER_DOLLAR};

    let path = temp_db_path("store-fill-cap-zero");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());

    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("Test");
    let mut accounts = AccountStore::new();
    let buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let seller = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    accounts
        .get_mut(seller)
        .unwrap()
        .positions
        .insert((market_id, 0), 10);

    let config = SequencerConfig {
        max_fill_history_per_account: 0,
        ..SequencerConfig::default()
    };
    let mut seq =
        BlockSequencer::with_default_solver(accounts, markets.clone(), vec![], oracle, config);

    let prepared = seq
        .prepare_block(
            vec![
                OrderSubmission {
                    account_id: buyer,
                    orders: vec![outcome_buy(&markets, 0, market_id, 0, 700_000_000, 1)],
                    mm_constraint: None,
                },
                OrderSubmission {
                    account_id: seller,
                    orders: vec![outcome_sell(&markets, 0, market_id, 0, 300_000_000, 1)],
                    mm_constraint: None,
                },
            ],
            1_000,
        )
        .unwrap();

    assert!(prepared
        .next_sequencer()
        .analytics()
        .account_fills_after(buyer, None, Some(AccountFillCursor::MIN), 10)
        .is_empty());

    store
        .save_block(prepared.next_sequencer().snapshot())
        .await
        .unwrap();
    seq.commit_prepared_block(prepared).unwrap();
    assert!(seq
        .analytics()
        .account_fills_after(buyer, None, Some(AccountFillCursor::MIN), 10)
        .is_empty());

    let durable = store
        .account_fills_after(buyer, None, Some(AccountFillCursor::MIN), 10)
        .unwrap();
    assert_eq!(durable.len(), 1);
    assert_eq!(durable[0].block_height, 1);
    drop(store);

    let reopened = Store::open(&path).unwrap();
    let reopened_fills = reopened
        .account_fills_after(buyer, Some(market_id), Some(AccountFillCursor::MIN), 10)
        .unwrap();
    assert_eq!(reopened_fills.len(), 1);
    assert_eq!(reopened_fills[0].fill_qty, 1);
}

#[tokio::test]
async fn test_store_reopens_after_committed_trade_and_restores_qmdb_state() {
    use crate::sequencer::{BlockSequencer, OrderSubmission, SequencerConfig};
    use matching_engine::{outcome_buy, outcome_sell, NANOS_PER_DOLLAR};

    let path = temp_db_path("store-reopen-smoke");
    let oracle = Arc::new(AdminOracle::new());

    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("Persistent restart");
    let mut accounts = AccountStore::new();
    let buyer = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    let seller = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
    accounts
        .get_mut(seller)
        .unwrap()
        .positions
        .insert((market_id, 0), 10);

    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        oracle.clone(),
        SequencerConfig::default(),
    );
    let production = seq.produce_block(
        vec![
            OrderSubmission {
                account_id: buyer,
                orders: vec![outcome_buy(&markets, 0, market_id, 0, 700_000_000, 5)],
                mm_constraint: None,
            },
            OrderSubmission {
                account_id: seller,
                orders: vec![outcome_sell(&markets, 0, market_id, 0, 300_000_000, 5)],
                mm_constraint: None,
            },
        ],
        1_000,
    );
    assert_eq!(production.block.header.height, 1);
    assert!(!production.block.fills.is_empty());

    {
        let store = Store::open(&path).unwrap();
        store
            .save_block_with_witness(seq.snapshot(), &production.witness)
            .await
            .unwrap();
    }

    let reopened = Store::open(&path).unwrap();
    let qmdb_root = reopened
        .current_state_qmdb_root()
        .await
        .unwrap()
        .expect("committed qMDB state root exists after reopen");
    let reopened_leaves = reopened.state_qmdb_leaves(qmdb_root.slot).await.unwrap();
    let expected_leaves = sybil_verifier::block::state_root_leaves(
        &production.witness.post_state,
        &production.witness.state_sidecar,
    );
    assert_eq!(reopened_leaves, expected_leaves);
    assert_eq!(
        sybil_verifier::block::state_root_from_leaves(&reopened_leaves),
        production.block.header.state_root
    );
    assert_eq!(qmdb_root.root, production.block.header.state_root);

    let buyer_key = sybil_verifier::state_schema::account_leaf_key(buyer.0);
    let buyer_proof = reopened
        .current_state_qmdb_leaf_proof(&buyer_key)
        .await
        .unwrap()
        .expect("buyer account leaf proof exists after reopen");
    assert_eq!(buyer_proof.root, production.block.header.state_root);
    assert_eq!(buyer_proof.leaf_key, buyer_key);

    let restored = reopened.load_state().await.unwrap().unwrap();
    assert_eq!(restored.height, 1);
    assert_eq!(
        restored.accounts.get(buyer).unwrap().position(market_id, 0),
        5
    );
    assert_eq!(
        restored
            .accounts
            .get(seller)
            .unwrap()
            .position(market_id, 0),
        5
    );
    assert!(
        restored
            .analytics
            .market_volumes
            .get(&market_id)
            .copied()
            .unwrap_or(0)
            > 0
    );

    let restored_seq = BlockSequencer::restore(restored, oracle, SequencerConfig::default());
    let fills = restored_seq
        .analytics()
        .account_fills(buyer, Some(market_id), 10, 0);
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].fill_qty, 5);
    assert_eq!(fills[0].block_height, 1);
}

#[test]
fn test_open_rejects_legacy_store_layout() {
    const TEST_COUNTERS: TableDefinition<&str, u64> = TableDefinition::new("counters");

    let path = temp_db_path("legacy-layout");
    let db = Database::create(&path).unwrap();
    let txn = db.begin_write().unwrap();
    let mut counters = txn.open_table(TEST_COUNTERS).unwrap();
    counters.insert(KEY_HEIGHT, 1).unwrap();
    drop(counters);
    txn.commit().unwrap();
    drop(db);

    match Store::open(&path) {
        Ok(_) => panic!("expected legacy store layout to be rejected"),
        Err(StoreError::UnsupportedLayout(_)) => {}
        Err(error) => panic!("expected unsupported layout error, got {error:?}"),
    }
}

#[tokio::test]
async fn test_store_roundtrips_admit_log_and_replays_on_restore() {
    use crate::order_book::OrderBook;
    use crate::sequencer::{BlockSequencer, SequencerConfig};
    use matching_engine::{outcome_buy, MarketSet, NANOS_PER_DOLLAR};

    let path = temp_db_path("store-admit-log");
    let store = Store::open(&path).unwrap();

    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("Test");

    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle.clone());
    let mut accounts = AccountStore::new();
    let aid = accounts.create_account(10 * NANOS_PER_DOLLAR as i64);
    let env = TestEnv::new();

    // Baseline block with no admits, so load_state has a metadata row.
    store
        .save_block(env.snapshot(
            &accounts,
            &markets,
            &lifecycle,
            &sample_header(1),
            1,
            None,
            Vec::new(),
        ))
        .await
        .unwrap();

    // Simulate a non-MM admit: build what `OrderBook::accept` would
    // produce, then append to the WAL directly.
    let mut book = OrderBook::new(10);
    let order = outcome_buy(&markets, 1, market_id, 0, NANOS_PER_DOLLAR / 2, 5);
    let accepted = book
        .accept(order, aid, accounts.get(aid).unwrap(), 1, 0)
        .unwrap();
    store
        .append_admit_log(&accepted.resting_order)
        .await
        .unwrap();

    // Load + restore: the order must live again in the book, with its
    // reservation correctly accounted for.
    let restored = store.load_state().await.unwrap().unwrap();
    assert_eq!(restored.admit_log.len(), 1);
    assert!(restored.resting_orders.is_empty());

    let seq = BlockSequencer::restore(restored, oracle, SequencerConfig::default());
    assert_eq!(
        seq.pending_orders_info(Some(aid)).len(),
        1,
        "replayed admit must be visible on the restored resting book"
    );

    // save_block clears the admit log atomically.
    store
        .save_block(env.snapshot(
            &accounts,
            &markets,
            &lifecycle,
            &sample_header(2),
            2,
            None,
            Vec::new(),
        ))
        .await
        .unwrap();

    let restored_after = store.load_state().await.unwrap().unwrap();
    assert!(restored_after.admit_log.is_empty());
}

#[tokio::test]
async fn test_store_roundtrips_data_feeds() {
    use sybil_oracle::{FeedId, FeedPubkey, ResolutionPolicy, ResolutionTemplate, TemplateId};

    let path = temp_db_path("store-data-feeds");
    let store = Store::open(&path).unwrap();

    let oracle = Arc::new(AdminOracle::new());
    let mut lifecycle = MarketLifecycle::new(oracle);
    lifecycle.register_feed(FeedPubkey(vec![1u8; 33]), "admin".into(), 100);
    lifecycle.register_feed(FeedPubkey(vec![2u8; 33]), "polymarket_mirror".into(), 200);
    lifecycle.install_template(ResolutionTemplate {
        id: TemplateId("polymarket_mirror".to_string()),
        policy: ResolutionPolicy::Immediate { feed_id: FeedId(1) },
    });

    let markets = MarketSet::new();
    let accounts = AccountStore::new();
    let env = TestEnv::new();

    store
        .save_block(env.snapshot(
            &accounts,
            &markets,
            &lifecycle,
            &sample_header(1),
            1,
            None,
            Vec::new(),
        ))
        .await
        .unwrap();

    let restored = store.load_state().await.unwrap().unwrap();
    assert_eq!(restored.data_feeds.len(), 2);
    let names: Vec<_> = restored.data_feeds.iter().map(|f| f.name.clone()).collect();
    assert!(names.contains(&"admin".to_string()));
    assert!(names.contains(&"polymarket_mirror".to_string()));
    assert_eq!(restored.resolution_templates.len(), 1);
    assert_eq!(
        restored.resolution_templates[0].id,
        TemplateId("polymarket_mirror".to_string())
    );
}

#[tokio::test]
async fn test_store_roundtrips_pending_bundles() {
    use crate::sequencer::OrderSubmission;
    use matching_engine::{
        mm_constraint::{MmConstraint, MmId, MmSide},
        outcome_buy, MarketSet, NANOS_PER_DOLLAR,
    };

    let path = temp_db_path("store-pending-bundles");
    let store = Store::open(&path).unwrap();

    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("Test");

    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let accounts = AccountStore::new();
    let env = TestEnv::new();

    // Commit a baseline block so `load_state()` has something to return.
    store
        .save_block(env.snapshot(
            &accounts,
            &markets,
            &lifecycle,
            &sample_header(1),
            1,
            None,
            Vec::new(),
        ))
        .await
        .unwrap();

    let order = outcome_buy(&markets, 7, market_id, 0, NANOS_PER_DOLLAR / 2, 3);
    let mut constraint = MmConstraint::new(MmId(1), Nanos(5 * NANOS_PER_DOLLAR));
    constraint.add_order(7, MmSide::BuyYes);
    let sub = OrderSubmission {
        account_id: AccountId(42),
        orders: vec![order],
        mm_constraint: Some(constraint),
    };

    store.append_pending_bundle(&sub).await.unwrap();
    store.append_pending_bundle(&sub).await.unwrap();

    let restored_before = store.load_state().await.unwrap().unwrap();
    assert_eq!(restored_before.pending_bundles.len(), 2);
    assert_eq!(restored_before.pending_bundles[0].account_id, AccountId(42));

    // save_block must clear the pending table atomically with the commit.
    store
        .save_block(env.snapshot(
            &accounts,
            &markets,
            &lifecycle,
            &sample_header(2),
            1,
            None,
            Vec::new(),
        ))
        .await
        .unwrap();

    let restored_after = store.load_state().await.unwrap().unwrap();
    assert!(restored_after.pending_bundles.is_empty());
}

#[test]
fn equity_and_history_rows_roundtrip() {
    use crate::account::AccountId;
    use crate::aggregates::{EquityPoint, HistoryEvent, HistoryKind, StoredHistoryEvent};

    let path = temp_db_path("equity-history-roundtrip");
    let store = Store::open(&path).unwrap();
    let aid = AccountId(7);

    let pts = vec![
        EquityPoint {
            height: 1,
            timestamp_ms: 1_000,
            portfolio_value_nanos: 100,
            deposited_nanos: 100,
        },
        EquityPoint {
            height: 2,
            timestamp_ms: 2_000,
            portfolio_value_nanos: 150,
            deposited_nanos: 100,
        },
    ];
    let mut e1 = HistoryEvent::new(aid, HistoryKind::Placed, 1, 1_000);
    e1.seq = 0;
    let mut e2 = HistoryEvent::new(aid, HistoryKind::Filled, 2, 2_000);
    e2.seq = 1;
    let events: Vec<StoredHistoryEvent> = vec![
        StoredHistoryEvent::from_event(&e1),
        StoredHistoryEvent::from_event(&e2),
    ];

    store
        .append_offblock_rows(&pts.iter().map(|p| (aid, *p)).collect::<Vec<_>>(), &events)
        .unwrap();

    // Equity: oldest-first, all points.
    let got = store.equity_series(aid, 0).unwrap();
    assert_eq!(got, pts);

    // History: newest-first, filtered + paged like AccountEventLog::query.
    let all = store.account_events(aid, 10, None, None).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].kind, HistoryKind::Filled); // newest first
    assert!(store.account_events(aid, 0, None, None).unwrap().is_empty());

    let trades = store
        .account_events(aid, 10, None, Some("trades".into()))
        .unwrap();
    assert_eq!(trades.len(), 2);

    // Cursor before (2, 1) excludes the Filled@(2,1) event.
    let page = store.account_events(aid, 10, Some((2, 1)), None).unwrap();
    assert!(page.iter().all(|e| !(e.block_height == 2 && e.seq == 1)));

    // Unknown account → empty.
    assert!(store.equity_series(AccountId(99), 0).unwrap().is_empty());
    assert!(store
        .account_events(AccountId(99), 10, None, None)
        .unwrap()
        .is_empty());
}
