use std::fs;
use std::io::ErrorKind;
use std::num::{NonZeroU16, NonZeroU64, NonZeroUsize};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use commonware_codec::RangeCfg;
use commonware_cryptography::sha256::Digest as Sha256Digest;
use commonware_cryptography::Hasher as _;
use commonware_cryptography::Sha256;
use commonware_runtime::buffer::paged::CacheRef;
use commonware_runtime::{tokio as commonware_tokio, Runner as _};
use commonware_storage::journal::contiguous::variable::Config as VConfig;
use commonware_storage::merkle::mmr::journaled::Config as MmrConfig;
use commonware_storage::merkle::mmr::Family as MmrFamily;
use commonware_storage::qmdb::current::ordered::variable::{
    Db as OrderedVariableDb, KeyValueProof,
};
use commonware_storage::qmdb::current::ordered::ExclusionProof;
use commonware_storage::qmdb::current::proof::{OperationProof, RangeProof};
use commonware_storage::qmdb::current::VariableConfig;
use commonware_storage::translator::OneCap;
use futures::StreamExt;
use tokio::sync::{mpsc as tokio_mpsc, oneshot};

use crate::account_storage::AccountSnapshotSlot;
use crate::store::StoreError;

pub const QMDB_STATE_CHUNK_SIZE: usize = 32;
pub const QMDB_STATE_MAX_KEY_BYTES: usize = 64;
const CHUNK_SIZE: usize = QMDB_STATE_CHUNK_SIZE;
const PAGE_SIZE: u16 = 4096;
const PAGE_CACHE_PAGES: usize = 128;
const ITEMS_PER_BLOB: u64 = 1024;
const WRITE_BUFFER_BYTES: usize = 64 * 1024;
const MAX_VALUE_BYTES: usize = 1 << 20;
const GENERATION_FILE: &str = "state-generation";

type StateDb = OrderedVariableDb<
    MmrFamily,
    commonware_tokio::Context,
    Vec<u8>,
    Vec<u8>,
    Sha256,
    OneCap,
    CHUNK_SIZE,
>;
type StateLeafEntries = Vec<(Vec<u8>, Vec<u8>)>;

pub type QmdbStateKeyValueProof =
    KeyValueProof<MmrFamily, Vec<u8>, Sha256Digest, QMDB_STATE_CHUNK_SIZE>;

pub type QmdbStateExclusionProof = ExclusionProof<
    MmrFamily,
    Vec<u8>,
    commonware_storage::qmdb::any::value::VariableEncoding<Vec<u8>>,
    Sha256Digest,
    QMDB_STATE_CHUNK_SIZE,
>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QmdbStateRangeProofParts {
    pub leaves: u64,
    pub digests: Vec<[u8; 32]>,
    pub pre_prefix_acc: Option<[u8; 32]>,
    pub unfolded_prefix_peaks: Vec<[u8; 32]>,
    pub partial_chunk_digest: Option<[u8; 32]>,
    pub ops_root: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QmdbStateOperationProofParts {
    pub location: u64,
    pub activity_chunk: [u8; QMDB_STATE_CHUNK_SIZE],
    pub range: QmdbStateRangeProofParts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QmdbStateKeyValueProofParts {
    pub operation: QmdbStateOperationProofParts,
    pub next_key: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QmdbStateExclusionProofParts {
    KeyValue {
        operation: QmdbStateOperationProofParts,
        span_key: Vec<u8>,
        span_value: Vec<u8>,
        span_next_key: Vec<u8>,
    },
    Commit {
        operation: QmdbStateOperationProofParts,
        metadata: Option<Vec<u8>>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QmdbStateRoot {
    pub root: [u8; 32],
    pub slot: AccountSnapshotSlot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QmdbStateLeafProof {
    pub root: [u8; 32],
    pub slot: AccountSnapshotSlot,
    pub leaf_key: Vec<u8>,
    pub leaf_value: Vec<u8>,
    pub proof: QmdbStateKeyValueProof,
}

impl QmdbStateLeafProof {
    pub fn verify(&self) -> bool {
        let mut hasher = Sha256::new();
        StateDb::verify_key_value_proof(
            &mut hasher,
            self.leaf_key.clone(),
            self.leaf_value.clone(),
            &self.proof,
            &Sha256Digest::from(self.root),
        )
    }

    pub fn proof_parts(&self) -> QmdbStateKeyValueProofParts {
        QmdbStateKeyValueProofParts {
            operation: operation_proof_parts(&self.proof.proof),
            next_key: self.proof.next_key.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QmdbStateLeafExclusionProof {
    pub root: [u8; 32],
    pub slot: AccountSnapshotSlot,
    pub leaf_key: Vec<u8>,
    pub proof: QmdbStateExclusionProof,
}

impl QmdbStateLeafExclusionProof {
    pub fn verify(&self) -> bool {
        let mut hasher = Sha256::new();
        StateDb::verify_exclusion_proof(
            &mut hasher,
            &self.leaf_key,
            &self.proof,
            &Sha256Digest::from(self.root),
        )
    }

    pub fn proof_parts(&self) -> QmdbStateExclusionProofParts {
        match &self.proof {
            ExclusionProof::KeyValue(operation, update) => QmdbStateExclusionProofParts::KeyValue {
                operation: operation_proof_parts(operation),
                span_key: update.key.clone(),
                span_value: update.value.clone(),
                span_next_key: update.next_key.clone(),
            },
            ExclusionProof::Commit(operation, metadata) => QmdbStateExclusionProofParts::Commit {
                operation: operation_proof_parts(operation),
                metadata: metadata.clone(),
            },
        }
    }
}

pub struct QmdbState {
    sender: tokio_mpsc::Sender<Command>,
}

enum Command {
    ReplaceLeaves {
        leaves: StateLeafEntries,
        respond_to: oneshot::Sender<Result<(), StoreError>>,
    },
    Root {
        respond_to: oneshot::Sender<Result<QmdbStateRoot, StoreError>>,
    },
    Leaves {
        respond_to: oneshot::Sender<Result<StateLeafEntries, StoreError>>,
    },
    LeafProof {
        leaf_key: Vec<u8>,
        respond_to: oneshot::Sender<Result<Option<QmdbStateLeafProof>, StoreError>>,
    },
    LeafExclusionProof {
        leaf_key: Vec<u8>,
        respond_to: oneshot::Sender<Result<Option<QmdbStateLeafExclusionProof>, StoreError>>,
    },
}

impl QmdbState {
    pub fn open(path: &Path, slot: AccountSnapshotSlot) -> Result<Self, StoreError> {
        std::fs::create_dir_all(path)?;
        let storage_directory = path.to_path_buf();
        let generation_file = storage_directory.join(GENERATION_FILE);
        let (sender, receiver) = tokio_mpsc::channel(8);
        let (started_tx, started_rx) = mpsc::sync_channel(1);

        thread::Builder::new()
            .name(format!("sybil-qmdb-state-{:?}", slot))
            .spawn(move || {
                let generation = read_generation_file(&generation_file);
                let runner = commonware_tokio::Runner::new(
                    commonware_tokio::Config::default().with_storage_directory(storage_directory),
                );
                runner.start(|context| async move {
                    let opened = match generation {
                        Ok(generation) => open_db(context.clone(), generation)
                            .await
                            .map(|db| (db, generation)),
                        Err(error) => Err(error),
                    };
                    match opened {
                        Ok((db, generation)) => {
                            let _ = started_tx.send(Ok(()));
                            run(slot, context, db, generation, generation_file, receiver).await;
                        }
                        Err(error) => {
                            let _ = started_tx.send(Err(error));
                        }
                    }
                });
            })
            .map_err(|error| StoreError::Qmdb(format!("failed to start state qmdb: {error}")))?;

        started_rx.recv().map_err(|error| {
            StoreError::Qmdb(format!("state qmdb startup channel failed: {error}"))
        })??;

        Ok(Self { sender })
    }

    pub async fn persist(&self, leaves: StateLeafEntries) -> Result<(), StoreError> {
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(Command::ReplaceLeaves { leaves, respond_to })
            .await
            .map_err(|_| StoreError::Qmdb("state qmdb service unavailable".to_string()))?;
        response
            .await
            .map_err(|_| StoreError::Qmdb("state qmdb response channel dropped".to_string()))?
    }

    pub async fn root(&self) -> Result<QmdbStateRoot, StoreError> {
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(Command::Root { respond_to })
            .await
            .map_err(|_| StoreError::Qmdb("state qmdb service unavailable".to_string()))?;
        response
            .await
            .map_err(|_| StoreError::Qmdb("state qmdb response channel dropped".to_string()))?
    }

    pub async fn leaves(&self) -> Result<StateLeafEntries, StoreError> {
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(Command::Leaves { respond_to })
            .await
            .map_err(|_| StoreError::Qmdb("state qmdb service unavailable".to_string()))?;
        response
            .await
            .map_err(|_| StoreError::Qmdb("state qmdb response channel dropped".to_string()))?
    }

    pub async fn leaf_proof(
        &self,
        leaf_key: &[u8],
    ) -> Result<Option<QmdbStateLeafProof>, StoreError> {
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(Command::LeafProof {
                leaf_key: leaf_key.to_vec(),
                respond_to,
            })
            .await
            .map_err(|_| StoreError::Qmdb("state qmdb service unavailable".to_string()))?;
        response
            .await
            .map_err(|_| StoreError::Qmdb("state qmdb response channel dropped".to_string()))?
    }

    pub async fn leaf_exclusion_proof(
        &self,
        leaf_key: &[u8],
    ) -> Result<Option<QmdbStateLeafExclusionProof>, StoreError> {
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(Command::LeafExclusionProof {
                leaf_key: leaf_key.to_vec(),
                respond_to,
            })
            .await
            .map_err(|_| StoreError::Qmdb("state qmdb service unavailable".to_string()))?;
        response
            .await
            .map_err(|_| StoreError::Qmdb("state qmdb response channel dropped".to_string()))?
    }
}

async fn run(
    slot: AccountSnapshotSlot,
    context: commonware_tokio::Context,
    mut db: StateDb,
    mut generation: u64,
    generation_file: PathBuf,
    mut receiver: tokio_mpsc::Receiver<Command>,
) {
    while let Some(command) = receiver.recv().await {
        match command {
            Command::ReplaceLeaves { leaves, respond_to } => {
                let _ = respond_to.send(
                    replace_leaves(&context, &mut db, &mut generation, &generation_file, leaves)
                        .await,
                );
            }
            Command::Root { respond_to } => {
                let _ = respond_to.send(Ok(root(slot, &db)));
            }
            Command::Leaves { respond_to } => {
                let _ = respond_to.send(collect_entries(&db).await);
            }
            Command::LeafProof {
                leaf_key,
                respond_to,
            } => {
                let _ = respond_to.send(leaf_proof(slot, &db, leaf_key).await);
            }
            Command::LeafExclusionProof {
                leaf_key,
                respond_to,
            } => {
                let _ = respond_to.send(leaf_exclusion_proof(slot, &db, leaf_key).await);
            }
        }
    }
}

async fn open_db(
    context: commonware_tokio::Context,
    generation: u64,
) -> Result<StateDb, StoreError> {
    let page_cache = CacheRef::from_pooler(
        &context,
        NonZeroU16::new(PAGE_SIZE).unwrap(),
        NonZeroUsize::new(PAGE_CACHE_PAGES).unwrap(),
    );
    let config = VariableConfig {
        merkle_config: MmrConfig {
            journal_partition: partition("state-mmr-journal", generation),
            items_per_blob: NonZeroU64::new(ITEMS_PER_BLOB).unwrap(),
            write_buffer: NonZeroUsize::new(WRITE_BUFFER_BYTES).unwrap(),
            metadata_partition: partition("state-mmr-metadata", generation),
            thread_pool: None,
            page_cache: page_cache.clone(),
        },
        journal_config: VConfig {
            partition: partition("state-log", generation),
            write_buffer: NonZeroUsize::new(WRITE_BUFFER_BYTES).unwrap(),
            compression: None,
            codec_config: (
                (RangeCfg::from(0..=QMDB_STATE_MAX_KEY_BYTES), ()),
                (RangeCfg::from(0..=MAX_VALUE_BYTES), ()),
            ),
            items_per_section: NonZeroU64::new(ITEMS_PER_BLOB).unwrap(),
            page_cache,
        },
        grafted_metadata_partition: partition("state-grafted-mmr-metadata", generation),
        translator: OneCap,
    };

    StateDb::init(context, config)
        .await
        .map_err(|error| StoreError::Qmdb(format!("failed to initialize state qmdb: {error}")))
}

fn partition(prefix: &str, generation: u64) -> String {
    format!("{prefix}-{generation}")
}

fn read_generation_file(path: &Path) -> Result<u64, StoreError> {
    match fs::read_to_string(path) {
        Ok(contents) => contents.trim().parse::<u64>().map_err(|error| {
            StoreError::Qmdb(format!("invalid state qMDB generation file: {error}"))
        }),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(0),
        Err(error) => Err(StoreError::Qmdb(format!(
            "failed to read state qMDB generation file: {error}"
        ))),
    }
}

fn write_generation_file(path: &Path, generation: u64) -> Result<(), StoreError> {
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, generation.to_string()).map_err(|error| {
        StoreError::Qmdb(format!(
            "failed to write state qMDB generation file: {error}"
        ))
    })?;
    fs::rename(&tmp_path, path).map_err(|error| {
        StoreError::Qmdb(format!(
            "failed to install state qMDB generation file: {error}"
        ))
    })
}

fn cleanup_generation(storage_directory: &Path, generation: u64) {
    let partitions = [
        format!("state-mmr-journal-{generation}-blobs"),
        format!("state-mmr-journal-{generation}-metadata"),
        format!("state-mmr-metadata-{generation}"),
        format!("state-log-{generation}_data"),
        format!("state-log-{generation}_offsets-blobs"),
        format!("state-log-{generation}_offsets-metadata"),
        format!("state-grafted-mmr-metadata-{generation}"),
    ];
    for partition in partitions {
        let path = storage_directory.join(partition);
        if let Err(error) = fs::remove_dir_all(&path) {
            if error.kind() != ErrorKind::NotFound {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

async fn replace_leaves(
    context: &commonware_tokio::Context,
    db: &mut StateDb,
    generation: &mut u64,
    generation_file: &Path,
    mut leaves: StateLeafEntries,
) -> Result<(), StoreError> {
    for (key, value) in &leaves {
        if key.len() > QMDB_STATE_MAX_KEY_BYTES {
            return Err(StoreError::Qmdb(format!(
                "state qmdb key exceeds {QMDB_STATE_MAX_KEY_BYTES} bytes"
            )));
        }
        if value.len() > MAX_VALUE_BYTES {
            return Err(StoreError::Qmdb(format!(
                "state qmdb value exceeds {MAX_VALUE_BYTES} bytes"
            )));
        }
    }
    leaves.sort_by(|(left, _), (right, _)| left.cmp(right));

    for pair in leaves.windows(2) {
        if pair[0].0 == pair[1].0 {
            return Err(StoreError::Qmdb(format!(
                "duplicate state qMDB leaf key: {:?}",
                pair[0].0
            )));
        }
    }

    let previous_generation = *generation;
    let next_generation = generation.saturating_add(1);
    let mut next_db = open_db(context.clone(), next_generation).await?;

    if !leaves.is_empty() {
        let mut batch = next_db.new_batch();
        for (key, value) in leaves {
            batch = batch.write(key, Some(value));
        }

        let merkleized = batch.merkleize(&next_db, None).await.map_err(|error| {
            StoreError::Qmdb(format!("failed to merkleize state qmdb: {error}"))
        })?;
        next_db
            .apply_batch(merkleized)
            .await
            .map_err(|error| StoreError::Qmdb(format!("failed to apply state qmdb: {error}")))?;
    }
    next_db
        .commit()
        .await
        .map_err(|error| StoreError::Qmdb(format!("failed to commit state qmdb: {error}")))?;
    write_generation_file(generation_file, next_generation)?;
    *db = next_db;
    *generation = next_generation;
    if let Some(storage_directory) = generation_file.parent() {
        cleanup_generation(storage_directory, previous_generation);
    }
    Ok(())
}

fn root(slot: AccountSnapshotSlot, db: &StateDb) -> QmdbStateRoot {
    QmdbStateRoot {
        root: db.root().0,
        slot,
    }
}

fn digest_bytes(digest: Sha256Digest) -> [u8; 32] {
    digest.0
}

fn range_proof_parts(proof: &RangeProof<MmrFamily, Sha256Digest>) -> QmdbStateRangeProofParts {
    QmdbStateRangeProofParts {
        leaves: u64::from(proof.proof.leaves),
        digests: proof
            .proof
            .digests
            .iter()
            .copied()
            .map(digest_bytes)
            .collect(),
        pre_prefix_acc: proof.pre_prefix_acc.map(digest_bytes),
        unfolded_prefix_peaks: proof
            .unfolded_prefix_peaks
            .iter()
            .copied()
            .map(digest_bytes)
            .collect(),
        partial_chunk_digest: proof.partial_chunk_digest.map(digest_bytes),
        ops_root: digest_bytes(proof.ops_root),
    }
}

fn operation_proof_parts(
    proof: &OperationProof<MmrFamily, Sha256Digest, QMDB_STATE_CHUNK_SIZE>,
) -> QmdbStateOperationProofParts {
    QmdbStateOperationProofParts {
        location: u64::from(proof.loc),
        activity_chunk: proof.chunk,
        range: range_proof_parts(&proof.range_proof),
    }
}

async fn collect_entries(db: &StateDb) -> Result<StateLeafEntries, StoreError> {
    let stream = db
        .stream_range(Vec::new())
        .await
        .map_err(|error| StoreError::Qmdb(format!("failed to stream state qmdb: {error}")))?;
    futures::pin_mut!(stream);

    let mut entries = Vec::new();
    while let Some(item) = stream.next().await {
        entries.push(
            item.map_err(|error| StoreError::Qmdb(format!("failed to read state qmdb: {error}")))?,
        );
    }
    Ok(entries)
}

async fn leaf_proof(
    slot: AccountSnapshotSlot,
    db: &StateDb,
    leaf_key: Vec<u8>,
) -> Result<Option<QmdbStateLeafProof>, StoreError> {
    let Some(leaf_value) = db
        .get(&leaf_key)
        .await
        .map_err(|error| StoreError::Qmdb(format!("failed to read state qmdb leaf: {error}")))?
    else {
        return Ok(None);
    };

    let mut hasher = Sha256::new();
    let proof = db
        .key_value_proof(&mut hasher, leaf_key.clone())
        .await
        .map_err(|error| StoreError::Qmdb(format!("failed to prove state qmdb leaf: {error}")))?;
    Ok(Some(QmdbStateLeafProof {
        root: root(slot, db).root,
        slot,
        leaf_key,
        leaf_value,
        proof,
    }))
}

async fn leaf_exclusion_proof(
    slot: AccountSnapshotSlot,
    db: &StateDb,
    leaf_key: Vec<u8>,
) -> Result<Option<QmdbStateLeafExclusionProof>, StoreError> {
    let mut hasher = Sha256::new();
    let proof = match db.exclusion_proof(&mut hasher, &leaf_key).await {
        Ok(proof) => proof,
        Err(commonware_storage::qmdb::Error::KeyExists) => return Ok(None),
        Err(error) => {
            return Err(StoreError::Qmdb(format!(
                "failed to prove state qmdb exclusion: {error}"
            )));
        }
    };
    Ok(Some(QmdbStateLeafExclusionProof {
        root: root(slot, db).root,
        slot,
        leaf_key,
        proof,
    }))
}
