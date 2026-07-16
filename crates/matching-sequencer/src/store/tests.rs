use std::sync::Arc;

use matching_engine::{MarketSet, MmConstraint, MmId, NANOS_PER_DOLLAR, outcome_buy};
use redb::{Database, TableDefinition};

use super::testutil::*;
use super::*;
use crate::AdminOracle;
use crate::OrderSubmission;
use crate::account::AccountStore;
use crate::market_lifecycle::MarketLifecycle;

fn store_test_sequencer_config() -> crate::SequencerConfig {
    crate::SequencerConfig {
        // Persistence tests intentionally use tiny orders to keep fixtures
        // compact; admission-floor policy is exercised separately.
        min_resting_order_notional_nanos: 0,
        ..crate::SequencerConfig::default()
    }
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
async fn product_history_outbox_stats_are_atomic_and_backfill_on_reopen() {
    let path = temp_db_path("store-product-history-outbox-stats");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let markets = MarketSet::new();
    let accounts = AccountStore::new();
    let env = TestEnv::new();

    for height in 1..=2 {
        let (header, _) =
            coherent_header_and_witness(height, &accounts, &markets, &lifecycle, &env.bridge_state);
        store
            .save_block(env.snapshot(&accounts, &markets, &lifecycle, &header, 1, None, vec![]))
            .await
            .unwrap();
    }

    let batches = store.product_history_outbox_batches(10).unwrap();
    let payload_bytes = batches.iter().fold(0u64, |total, batch| {
        total + u64::try_from(rmp_serde::to_vec(batch).unwrap().len()).unwrap()
    });
    let initial = store.product_history_outbox_stats().unwrap();
    assert_eq!(initial.rows, 2);
    assert_eq!(initial.payload_bytes, payload_bytes);
    assert_eq!(initial.oldest_height, Some(1));
    assert_eq!(initial.newest_height, Some(2));
    assert_eq!(
        initial.oldest_committed_at_ms,
        Some(batches[0].committed_at_ms)
    );

    let (duplicate_header, _) =
        coherent_header_and_witness(2, &accounts, &markets, &lifecycle, &env.bridge_state);
    store
        .save_block(env.snapshot(
            &accounts,
            &markets,
            &lifecycle,
            &duplicate_header,
            1,
            None,
            vec![],
        ))
        .await
        .unwrap();
    assert_eq!(
        store.product_history_outbox_stats().unwrap(),
        initial,
        "an exact duplicate fenced row must not double-count payload bytes"
    );

    let mut wrong_hash = batches[0].payload_hash;
    wrong_hash[0] ^= 1;
    assert!(
        store
            .acknowledge_product_history_batch(ProductHistoryOutboxAck {
                height: batches[0].height,
                payload_hash: wrong_hash,
            })
            .is_err()
    );
    assert_eq!(store.product_history_outbox_stats().unwrap(), initial);

    assert!(
        store
            .acknowledge_product_history_batch(ProductHistoryOutboxAck {
                height: batches[0].height,
                payload_hash: batches[0].payload_hash,
            })
            .unwrap()
    );
    let remaining_payload_bytes =
        u64::try_from(rmp_serde::to_vec(&batches[1]).unwrap().len()).unwrap();
    assert_eq!(
        store.product_history_outbox_stats().unwrap(),
        ProductHistoryOutboxStats {
            rows: 1,
            payload_bytes: remaining_payload_bytes,
            oldest_height: Some(2),
            newest_height: Some(2),
            oldest_committed_at_ms: Some(batches[1].committed_at_ms),
        }
    );

    // Simulate a store written before the additive payload-byte counter. Open
    // performs one bounded migration scan, then normal reads stay O(log n).
    let txn = store.db.begin_write().unwrap();
    {
        let mut meta = txn.open_table(PRODUCT_HISTORY_OUTBOX_META).unwrap();
        meta.remove(KEY_PRODUCT_HISTORY_OUTBOX_PAYLOAD_BYTES)
            .unwrap();
        meta.remove(KEY_PRODUCT_HISTORY_OUTBOX_OLDEST_COMMITTED_AT_MS)
            .unwrap();
    }
    txn.commit().unwrap();
    drop(store);

    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.product_history_outbox_stats().unwrap().payload_bytes,
        remaining_payload_bytes
    );
    assert!(
        store
            .acknowledge_product_history_batch(ProductHistoryOutboxAck {
                height: batches[1].height,
                payload_hash: batches[1].payload_hash,
            })
            .unwrap()
    );
    assert_eq!(
        store.product_history_outbox_stats().unwrap(),
        ProductHistoryOutboxStats::default()
    );

    let (header, _) =
        coherent_header_and_witness(3, &accounts, &markets, &lifecycle, &env.bridge_state);
    store
        .save_block(env.snapshot(&accounts, &markets, &lifecycle, &header, 1, None, vec![]))
        .await
        .unwrap();
    let txn = store.db.begin_write().unwrap();
    txn.open_table(PRODUCT_HISTORY_OUTBOX_META)
        .unwrap()
        .remove(KEY_PRODUCT_HISTORY_OUTBOX_OLDEST_COMMITTED_AT_MS)
        .unwrap();
    txn.commit().unwrap();
    drop(store);
    assert!(
        matches!(Store::open(&path), Err(StoreError::CorruptLayout(message)) if message.contains("partially initialized")),
        "a populated outbox with partial stock metadata must fail closed"
    );
}

#[tokio::test]
async fn canonical_archive_pruning_deletes_replay_blocks_and_da_with_metadata() {
    let path = temp_db_path("store-canonical-archive-retention");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let markets = MarketSet::new();
    let accounts = AccountStore::new();
    let env = TestEnv::new();

    for height in 1..=5 {
        let (header, witness) =
            coherent_header_and_witness(height, &accounts, &markets, &lifecycle, &env.bridge_state);
        let block = sample_sealed_block(&header);
        store
            .save_block_with_witness_and_replay_block(
                env.snapshot(&accounts, &markets, &lifecycle, &header, 1, None, vec![]),
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
        .prune_canonical_archive(
            5,
            CanonicalArchiveRetentionPolicy {
                retention_blocks: 3,
                maintenance_interval_blocks: 1,
                max_rows_per_pass: 10,
            },
        )
        .await
        .unwrap();

    assert_eq!(report.replay_blocks_pruned, 2);
    assert_eq!(report.da_artifacts_pruned, 2);
    assert_eq!(report.meta.oldest_retained_height, Some(3));
    assert_eq!(report.meta.last_maintenance_height, Some(5));
    assert!(store.load_block(1).await.unwrap().is_none());
    assert!(store.load_block(2).await.unwrap().is_none());
    assert!(store.load_da_artifact(1).await.unwrap().is_none());
    assert!(store.load_da_artifact(2).await.unwrap().is_none());
    assert!(store.load_da_manifest(1).await.unwrap().is_none());
    assert!(store.load_da_manifest(2).await.unwrap().is_none());
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
    assert_eq!(store.load_da_manifest(3).await.unwrap().unwrap().height, 3);
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

    assert_eq!(store.product_history_outbox_len().unwrap(), 5);
}

#[tokio::test]
async fn canonical_archive_partial_budget_reports_oldest_remaining_replay_block() {
    let path = temp_db_path("store-canonical-archive-budget");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let markets = MarketSet::new();
    let accounts = AccountStore::new();
    let env = TestEnv::new();

    for height in 1..=5 {
        let (header, witness) =
            coherent_header_and_witness(height, &accounts, &markets, &lifecycle, &env.bridge_state);
        let block = sample_sealed_block(&header);
        store
            .save_block_with_witness_and_replay_block(
                env.snapshot(&accounts, &markets, &lifecycle, &header, 1, None, vec![]),
                &witness,
                &block,
            )
            .await
            .unwrap();
    }

    let report = store
        .prune_canonical_archive(
            5,
            CanonicalArchiveRetentionPolicy {
                retention_blocks: 2,
                maintenance_interval_blocks: 1,
                max_rows_per_pass: 2,
            },
        )
        .await
        .unwrap();

    assert_eq!(report.replay_blocks_pruned, 2);
    assert_eq!(
        report.meta.oldest_retained_height,
        Some(3),
        "block floor must not jump to target 4 while block 3 remains"
    );
    assert_eq!(report.meta.last_maintenance_height, Some(5));
    assert!(store.load_block(3).await.unwrap().is_some());

    assert_eq!(store.product_history_outbox_len().unwrap(), 5);
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
async fn test_store_restores_product_event_sequence() {
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
    assert_eq!(restored.analytics.next_product_event_seq, 2);

    // A missing canonical counter never triggers a projection scan. Fresh
    // genesis is required when moving a legacy store across this boundary.
    let txn = store.db.begin_write().unwrap();
    {
        let mut counters = txn.open_table(COUNTERS).unwrap();
        counters.remove(KEY_NEXT_PRODUCT_EVENT_SEQ).unwrap();
    }
    txn.commit().unwrap();

    let restored = store.load_state().await.unwrap().unwrap();
    assert_eq!(restored.analytics.next_product_event_seq, 0);
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
async fn da_manifest_cache_loads_without_reading_or_hashing_payload() {
    let path = temp_db_path("da-manifest-cache");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let markets = MarketSet::new();
    let env = TestEnv::new();
    let accounts = AccountStore::new();
    let (_, witness) =
        coherent_header_and_witness(1, &accounts, &markets, &lifecycle, &env.bridge_state);
    let mut artifact = DaArtifact::from_witness(&witness);
    let expected_manifest = artifact.manifest.clone();

    // Corrupt only the large payload after its publish-time metadata was
    // computed. A payload-backed manifest path would fail its hash check;
    // the independent manifest row remains directly readable.
    artifact.payload[0] ^= 1;
    store.save_da_artifact(artifact).await.unwrap();
    assert_eq!(
        store.load_da_manifest(1).await.unwrap(),
        Some(expected_manifest)
    );
    assert!(matches!(
        store
            .load_da_artifact(1)
            .await
            .unwrap()
            .unwrap()
            .verify_payload_integrity(),
        Err(DaArtifactIntegrityError::PayloadRootMismatch { .. })
    ));
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

    // Unlike the latest-only witness cache, portable jobs survive qMDB slot
    // rotation and remain ordered until the prover acknowledges exact bytes.
    let outbox = store.proof_job_outbox_page(None, 10).unwrap();
    assert_eq!(
        outbox.iter().map(|entry| entry.height).collect::<Vec<_>>(),
        vec![header1.height, header2.height]
    );
    assert!(outbox.iter().all(|entry| !entry.acknowledged));
    assert_eq!(
        store
            .oldest_unacknowledged_proof_job()
            .unwrap()
            .expect("oldest unacknowledged job")
            .height,
        header1.height
    );
    for entry in &outbox {
        let job: sybil_proof_protocol::StateTransitionProofJob =
            rmp_serde::from_slice(&entry.bytes).unwrap();
        assert_eq!(job.block_height, entry.height);
        sybil_proof_protocol::build_state_transition_guest_input(job).unwrap();
    }

    let mut wrong_digest = outbox[0].digest;
    wrong_digest[0] ^= 1;
    assert!(matches!(
        store
            .acknowledge_proof_job(header1.height, wrong_digest)
            .await,
        Err(StoreError::ProofJob(_))
    ));
    store
        .acknowledge_proof_job(header1.height, outbox[0].digest)
        .await
        .unwrap();
    // Exact repeated acknowledgements are idempotent.
    store
        .acknowledge_proof_job(header1.height, outbox[0].digest)
        .await
        .unwrap();

    let outbox = store.proof_job_outbox_page(None, 10).unwrap();
    assert!(outbox[0].acknowledged);
    assert!(!outbox[1].acknowledged);
    assert_eq!(
        store
            .oldest_unacknowledged_proof_job()
            .unwrap()
            .expect("second job remains unacknowledged")
            .height,
        header2.height
    );
}

#[tokio::test]
async fn acknowledged_proof_job_pruning_is_bounded_and_preserves_unacknowledged_jobs() {
    let path = temp_db_path("store-proof-job-retention");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let markets = MarketSet::new();
    let env = TestEnv::new();
    let mut accounts = AccountStore::new();
    accounts.create_account(100);

    for height in 1..=4 {
        let (header, witness) =
            coherent_header_and_witness(height, &accounts, &markets, &lifecycle, &env.bridge_state);
        store
            .save_block_with_witness(
                env.snapshot(&accounts, &markets, &lifecycle, &header, 1, None, vec![]),
                &witness,
            )
            .await
            .unwrap();
    }

    let outbox = store.proof_job_outbox_page(None, 10).unwrap();
    for height in [1, 2, 4] {
        let entry = outbox.iter().find(|entry| entry.height == height).unwrap();
        store
            .acknowledge_proof_job(height, entry.digest)
            .await
            .unwrap();
    }

    let policy = AcknowledgedProofJobRetentionPolicy {
        retention_blocks: 2,
        maintenance_interval_blocks: 1,
        max_rows_per_pass: 1,
    };
    let first = store
        .prune_acknowledged_proof_jobs(4, policy)
        .await
        .unwrap();
    assert_eq!(first.jobs_pruned, 1);
    assert_eq!(first.oldest_retained_height, Some(2));
    assert_eq!(
        store
            .proof_job_outbox_page(None, 10)
            .unwrap()
            .into_iter()
            .map(|entry| entry.height)
            .collect::<Vec<_>>(),
        vec![2, 3, 4]
    );

    let second = store
        .prune_acknowledged_proof_jobs(4, policy)
        .await
        .unwrap();
    assert_eq!(second.jobs_pruned, 1);
    assert_eq!(second.oldest_retained_height, Some(3));
    let remaining = store.proof_job_outbox_page(None, 10).unwrap();
    assert_eq!(
        remaining
            .iter()
            .map(|entry| entry.height)
            .collect::<Vec<_>>(),
        vec![3, 4]
    );
    assert!(
        !remaining[0].acknowledged,
        "old unacknowledged job must survive"
    );
    assert!(
        remaining[1].acknowledged,
        "retained safety-window job stays acked"
    );
}

#[tokio::test]
async fn acknowledged_proof_job_pruning_rotates_past_an_unacknowledged_prefix() {
    let path = temp_db_path("store-proof-job-retention-scan-cursor");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let markets = MarketSet::new();
    let env = TestEnv::new();
    let mut accounts = AccountStore::new();
    accounts.create_account(100);

    for height in 1..=8 {
        let (header, witness) =
            coherent_header_and_witness(height, &accounts, &markets, &lifecycle, &env.bridge_state);
        store
            .save_block_with_witness(
                env.snapshot(&accounts, &markets, &lifecycle, &header, 1, None, vec![]),
                &witness,
            )
            .await
            .unwrap();
    }

    let job_six = store
        .proof_job_outbox_page(None, 10)
        .unwrap()
        .into_iter()
        .find(|entry| entry.height == 6)
        .unwrap();
    store
        .acknowledge_proof_job(6, job_six.digest)
        .await
        .unwrap();

    let policy = AcknowledgedProofJobRetentionPolicy {
        retention_blocks: 1,
        maintenance_interval_blocks: 1,
        max_rows_per_pass: 2,
    };
    for _ in 0..2 {
        let report = store
            .prune_acknowledged_proof_jobs(8, policy)
            .await
            .unwrap();
        assert_eq!(report.rows_examined, 2);
        assert_eq!(report.jobs_pruned, 0);
    }

    // The scan position is durable maintenance state. Restarting must not make
    // the old unacknowledged prefix starve the acknowledged row behind it.
    drop(store);
    let store = Store::open(&path).unwrap();
    let report = store
        .prune_acknowledged_proof_jobs(8, policy)
        .await
        .unwrap();
    assert_eq!(report.rows_examined, 2);
    assert_eq!(report.jobs_pruned, 1);
    assert_eq!(
        store
            .proof_job_outbox_page(None, 10)
            .unwrap()
            .into_iter()
            .map(|entry| entry.height)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 7, 8]
    );
}

#[tokio::test]
async fn acknowledged_proof_job_pruning_fails_closed_on_digest_mismatch() {
    let path = temp_db_path("store-proof-job-retention-mismatch");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let markets = MarketSet::new();
    let env = TestEnv::new();
    let mut accounts = AccountStore::new();
    accounts.create_account(100);

    for height in 1..=2 {
        let (header, witness) =
            coherent_header_and_witness(height, &accounts, &markets, &lifecycle, &env.bridge_state);
        store
            .save_block_with_witness(
                env.snapshot(&accounts, &markets, &lifecycle, &header, 1, None, vec![]),
                &witness,
            )
            .await
            .unwrap();
    }

    let txn = store.db.begin_write().unwrap();
    {
        let wrong_digest = [9_u8; 32];
        let mut acks = txn.open_table(PROOF_JOB_ACKS).unwrap();
        acks.insert(1, wrong_digest.as_slice()).unwrap();
    }
    txn.commit().unwrap();

    let error = store
        .prune_acknowledged_proof_jobs(
            2,
            AcknowledgedProofJobRetentionPolicy {
                retention_blocks: 1,
                maintenance_interval_blocks: 1,
                max_rows_per_pass: 10,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(error, StoreError::ProofJob(_)));
    assert_eq!(
        store
            .proof_job_outbox_page(None, 10)
            .unwrap()
            .into_iter()
            .map(|entry| entry.height)
            .collect::<Vec<_>>(),
        vec![1, 2],
        "failed maintenance must leave the source outbox intact"
    );
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
    assert_eq!(restored.acknowledged_writes.len(), 3);
    assert!(matches!(
        &restored.acknowledged_writes[0].write,
        AcknowledgedWrite::L1Deposit(value) if value == &deposit
    ));
    assert!(matches!(
        &restored.acknowledged_writes[1].write,
        AcknowledgedWrite::BridgeWithdrawal(value) if value == &withdrawal
    ));
    assert!(matches!(
        &restored.acknowledged_writes[2].write,
        AcknowledgedWrite::BridgeL1Input(value) if value == &l1_input
    ));
}

#[tokio::test]
async fn acknowledged_writes_preserve_one_cross_subsystem_sequence() {
    let path = temp_db_path("store-global-ack-sequence");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("global ack order");
    let env = TestEnv::new();
    let mut accounts = AccountStore::new();
    let account_id = accounts.create_account(100 * NANOS_PER_DOLLAR as i64);
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

    store
        .append_control_plane_command(&ControlPlaneCommand::AdvanceReplayNonce {
            account_id,
            nonce: 1,
        })
        .await
        .unwrap();

    let mut book = OrderBook::new(10);
    let accepted = book
        .accept(
            outcome_buy(&markets, 1, market_id, 0, NANOS_PER_DOLLAR / 2, 1),
            account_id,
            accounts.get(account_id).unwrap(),
            1,
            0,
        )
        .unwrap();
    store
        .append_admit_log(&accepted.resting_order)
        .await
        .unwrap();

    let deferred = OrderSubmission {
        account_id,
        orders: vec![outcome_buy(
            &markets,
            2,
            market_id,
            0,
            NANOS_PER_DOLLAR / 2,
            1,
        )],
        mm_constraint: Some(MmConstraint::new(MmId(1), Nanos(NANOS_PER_DOLLAR))),
    };
    store.append_pending_bundle(&deferred).await.unwrap();

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
    store.append_pending_l1_deposit(&deposit).await.unwrap();

    let withdrawal = BridgeWithdrawalRequest {
        account_id,
        chain_id: 1,
        vault_address: [0x10; 20],
        recipient: [0x40; 20],
        token_address: [0x20; 20],
        amount_token_units: 4_000,
        expiry_height: 10,
    };
    store
        .append_pending_bridge_withdrawal(&withdrawal)
        .await
        .unwrap();
    store
        .append_pending_bridge_l1_input(&BridgeL1Input::ObservedHeight(42))
        .await
        .unwrap();

    let restored = store.load_state().await.unwrap().unwrap();
    let sequences: Vec<u64> = restored
        .acknowledged_writes
        .iter()
        .map(|entry| entry.sequence)
        .collect();
    let kinds: Vec<&str> = restored
        .acknowledged_writes
        .iter()
        .map(|entry| entry.write.kind())
        .collect();
    assert_eq!(sequences, vec![0, 1, 2, 3, 4, 5]);
    assert_eq!(
        kinds,
        vec![
            "control_plane",
            "direct_admit",
            "deferred_bundle",
            "l1_deposit",
            "bridge_withdrawal",
            "bridge_l1_input",
        ]
    );
}

#[tokio::test]
async fn acknowledged_write_requires_a_committed_replay_baseline() {
    let path = temp_db_path("store-global-ack-needs-baseline");
    let store = Store::open(&path).unwrap();
    let error = store
        .append_control_plane_command(&ControlPlaneCommand::AdvanceReplayNonce {
            account_id: AccountId(7),
            nonce: 1,
        })
        .await
        .unwrap_err();
    assert!(matches!(error, StoreError::AcknowledgedWriteBeforeSnapshot));
    assert!(store.load_state().await.unwrap().is_none());
}

#[tokio::test]
async fn acknowledged_write_floor_detects_a_missing_first_row() {
    let path = temp_db_path("store-global-ack-floor-gap");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let markets = MarketSet::new();
    let env = TestEnv::new();
    let accounts = AccountStore::new();
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
    for nonce in [1, 2] {
        store
            .append_control_plane_command(&ControlPlaneCommand::AdvanceReplayNonce {
                account_id: AccountId(7),
                nonce,
            })
            .await
            .unwrap();
    }

    let txn = store.db.begin_write().unwrap();
    {
        let mut table = txn.open_table(ACKNOWLEDGED_WRITES).unwrap();
        table.remove(0).unwrap();
    }
    txn.commit().unwrap();

    let error = match store.load_state().await {
        Ok(_) => panic!("missing first acknowledged-write row must fail closed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        StoreError::CorruptLayout(message)
            if message.contains("expected sequence 0, found 1")
    ));
}

#[tokio::test]
async fn acknowledged_write_sequence_remains_monotonic_across_block_fences() {
    let path = temp_db_path("store-global-ack-monotonic");
    let store = Store::open(&path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let lifecycle = MarketLifecycle::new(oracle);
    let markets = MarketSet::new();
    let env = TestEnv::new();
    let accounts = AccountStore::new();
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
    store
        .append_control_plane_command(&ControlPlaneCommand::AdvanceReplayNonce {
            account_id: AccountId(7),
            nonce: 1,
        })
        .await
        .unwrap();
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
    store
        .append_control_plane_command(&ControlPlaneCommand::AdvanceReplayNonce {
            account_id: AccountId(7),
            nonce: 2,
        })
        .await
        .unwrap();

    let restored = store.load_state().await.unwrap().unwrap();
    assert_eq!(restored.acknowledged_writes.len(), 1);
    assert_eq!(restored.acknowledged_writes[0].sequence, 1);
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
    use matching_engine::{MarketSet, NANOS_PER_DOLLAR, outcome_buy};

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
    use matching_engine::{MarketSet, NANOS_PER_DOLLAR, outcome_buy};

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
    use matching_engine::{Fill, MarketSet, NANOS_PER_DOLLAR, Nanos, Qty, outcome_buy};
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
    use matching_engine::{Fill, MarketSet, NANOS_PER_DOLLAR, Nanos, Qty, outcome_buy};
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
            .import_witness_genesis(doctored, None, None, store_test_sequencer_config())
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
        .import_witness_genesis(correct, None, None, store_test_sequencer_config())
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
    use matching_engine::{MarketSet, NANOS_PER_DOLLAR, outcome_buy};

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
async fn test_store_restores_fill_totals_without_hydrating_fill_history() {
    use crate::sequencer::{BlockSequencer, OrderSubmission};
    use matching_engine::{NANOS_PER_DOLLAR, outcome_buy, outcome_sell};

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
        store_test_sequencer_config(),
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
    seq.set_profile(buyer, Some("buyer".into()), None).unwrap();
    seq.set_profile(seller, Some("seller".into()), None)
        .unwrap();

    store.save_block(seq.snapshot()).await.unwrap();

    let restored = store.load_state().await.unwrap().unwrap();
    let restored_seq = BlockSequencer::restore(restored, oracle, store_test_sequencer_config());
    assert_eq!(restored_seq.analytics().total_fills(buyer), 1);
    assert_eq!(restored_seq.analytics().total_fills(seller), 1);
    let ranked: Vec<_> = restored_seq
        .leaderboard_bases()
        .into_iter()
        .map(|base| base.account_id)
        .collect();
    assert!(ranked.contains(&buyer));
    assert!(ranked.contains(&seller));
}

#[tokio::test]
async fn import_witness_drill_restores_head_and_produces_children() {
    use crate::sequencer::{BlockSequencer, OrderSubmission};
    use matching_engine::{MarketId, NANOS_PER_DOLLAR, outcome_buy, outcome_sell};

    let source_path = temp_db_path("store-import-witness-source");
    let fresh_path = temp_db_path("store-import-witness-fresh");
    let source_store = Store::open(&source_path).unwrap();
    let oracle = Arc::new(AdminOracle::new());
    let config = store_test_sequencer_config();

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
    source_store
        .save_block_with_witness_and_replay_block(
            seq.snapshot(),
            &first.witness,
            &first.sealed_block(),
        )
        .await
        .unwrap();

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
        .save_block_with_witness_and_replay_block(seq.snapshot(), witness, &second.sealed_block())
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
    assert!(
        fresh_store
            .latest_proof_job_outbox_entry()
            .unwrap()
            .is_none(),
        "the imported head is a recovery checkpoint without a local pre-state proof"
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
    fresh_store
        .save_block_with_witness(restored_seq.snapshot(), &child.witness)
        .await
        .unwrap();

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
    fresh_store
        .save_block_with_witness(restored_seq.snapshot(), &grandchild.witness)
        .await
        .unwrap();
    assert_eq!(
        fresh_store
            .proof_job_outbox_page(None, 10)
            .unwrap()
            .iter()
            .map(|entry| entry.height)
            .collect::<Vec<_>>(),
        vec![child.block.header.height, grandchild.block.header.height]
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
    assert!(
        seq.markets()
            .get(MarketId(summary.next_market_id))
            .is_none()
    );
}

#[tokio::test]
async fn test_store_reopens_after_committed_trade_and_restores_qmdb_state() {
    use crate::sequencer::{BlockSequencer, OrderSubmission};
    use matching_engine::{NANOS_PER_DOLLAR, outcome_buy, outcome_sell};

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
        store_test_sequencer_config(),
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

    let restored_seq = BlockSequencer::restore(restored, oracle, store_test_sequencer_config());
    assert_eq!(restored_seq.analytics().total_fills(buyer), 1);
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
    use crate::sequencer::BlockSequencer;
    use matching_engine::{MarketSet, NANOS_PER_DOLLAR, outcome_buy};

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
    assert!(matches!(
        restored.acknowledged_writes.as_slice(),
        [SequencedAcknowledgedWrite {
            write: AcknowledgedWrite::DirectAdmit(_),
            ..
        }]
    ));
    assert!(restored.resting_orders.is_empty());

    let seq = BlockSequencer::restore(restored, oracle, store_test_sequencer_config());
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
    assert!(restored_after.acknowledged_writes.is_empty());
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
        MarketSet, NANOS_PER_DOLLAR,
        mm_constraint::{MmConstraint, MmId, MmSide},
        outcome_buy,
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
    assert_eq!(restored_before.acknowledged_writes.len(), 2);
    assert!(
        restored_before
            .acknowledged_writes
            .iter()
            .all(|entry| matches!(
                &entry.write,
                AcknowledgedWrite::DeferredBundle(bundle) if bundle.account_id == AccountId(42)
            ))
    );

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
    assert!(restored_after.acknowledged_writes.is_empty());
}
