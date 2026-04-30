use std::collections::HashMap;
use std::num::{NonZeroU16, NonZeroU64, NonZeroUsize};
use std::path::Path;
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
use commonware_storage::qmdb::current::VariableConfig;
use commonware_storage::translator::OneCap;
use futures::StreamExt;
use tokio::sync::{mpsc as tokio_mpsc, oneshot};

use crate::account_storage::AccountSnapshotSlot;
use crate::store::StoreError;

pub const QMDB_STATE_CHUNK_SIZE: usize = 32;
const CHUNK_SIZE: usize = QMDB_STATE_CHUNK_SIZE;
const PAGE_SIZE: u16 = 4096;
const PAGE_CACHE_PAGES: usize = 128;
const ITEMS_PER_BLOB: u64 = 1024;
const WRITE_BUFFER_BYTES: usize = 64 * 1024;
const MAX_KEY_BYTES: usize = 64;
const MAX_VALUE_BYTES: usize = 1 << 20;

type StateDb = OrderedVariableDb<
    MmrFamily,
    commonware_tokio::Context,
    Vec<u8>,
    Vec<u8>,
    Sha256,
    OneCap,
    CHUNK_SIZE,
>;

pub type QmdbStateKeyValueProof =
    KeyValueProof<MmrFamily, Vec<u8>, Sha256Digest, QMDB_STATE_CHUNK_SIZE>;

pub type QmdbStateExclusionProof = ExclusionProof<
    MmrFamily,
    Vec<u8>,
    commonware_storage::qmdb::any::value::VariableEncoding<Vec<u8>>,
    Sha256Digest,
    QMDB_STATE_CHUNK_SIZE,
>;

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
}

pub struct QmdbState {
    sender: tokio_mpsc::Sender<Command>,
}

enum Command {
    ReplaceLeaves {
        leaves: Vec<(Vec<u8>, Vec<u8>)>,
        respond_to: oneshot::Sender<Result<(), StoreError>>,
    },
    Root {
        respond_to: oneshot::Sender<Result<QmdbStateRoot, StoreError>>,
    },
    Leaves {
        respond_to: oneshot::Sender<Result<Vec<(Vec<u8>, Vec<u8>)>, StoreError>>,
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
        let (sender, receiver) = tokio_mpsc::channel(8);
        let (started_tx, started_rx) = mpsc::sync_channel(1);

        thread::Builder::new()
            .name(format!("sybil-qmdb-state-{:?}", slot))
            .spawn(move || {
                let runner = commonware_tokio::Runner::new(
                    commonware_tokio::Config::default().with_storage_directory(storage_directory),
                );
                runner.start(|context| async move {
                    let opened = open_db(context).await;
                    match opened {
                        Ok(db) => {
                            let _ = started_tx.send(Ok(()));
                            run(slot, db, receiver).await;
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

    pub async fn persist(&self, leaves: Vec<(Vec<u8>, Vec<u8>)>) -> Result<(), StoreError> {
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

    pub async fn leaves(&self) -> Result<Vec<(Vec<u8>, Vec<u8>)>, StoreError> {
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
    mut db: StateDb,
    mut receiver: tokio_mpsc::Receiver<Command>,
) {
    while let Some(command) = receiver.recv().await {
        match command {
            Command::ReplaceLeaves { leaves, respond_to } => {
                let _ = respond_to.send(replace_leaves(&mut db, leaves).await);
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

async fn open_db(context: commonware_tokio::Context) -> Result<StateDb, StoreError> {
    let page_cache = CacheRef::from_pooler(
        &context,
        NonZeroU16::new(PAGE_SIZE).unwrap(),
        NonZeroUsize::new(PAGE_CACHE_PAGES).unwrap(),
    );
    let config = VariableConfig {
        merkle_config: MmrConfig {
            journal_partition: "state-mmr-journal".to_string(),
            items_per_blob: NonZeroU64::new(ITEMS_PER_BLOB).unwrap(),
            write_buffer: NonZeroUsize::new(WRITE_BUFFER_BYTES).unwrap(),
            metadata_partition: "state-mmr-metadata".to_string(),
            thread_pool: None,
            page_cache: page_cache.clone(),
        },
        journal_config: VConfig {
            partition: "state-log".to_string(),
            write_buffer: NonZeroUsize::new(WRITE_BUFFER_BYTES).unwrap(),
            compression: None,
            codec_config: (
                (RangeCfg::from(0..=MAX_KEY_BYTES), ()),
                (RangeCfg::from(0..=MAX_VALUE_BYTES), ()),
            ),
            items_per_section: NonZeroU64::new(ITEMS_PER_BLOB).unwrap(),
            page_cache,
        },
        grafted_metadata_partition: "state-grafted-mmr-metadata".to_string(),
        translator: OneCap,
    };

    StateDb::init(context, config)
        .await
        .map_err(|error| StoreError::Qmdb(format!("failed to initialize state qmdb: {error}")))
}

async fn replace_leaves(
    db: &mut StateDb,
    leaves: Vec<(Vec<u8>, Vec<u8>)>,
) -> Result<(), StoreError> {
    let current_entries = collect_entries(db).await?;
    let mut desired = HashMap::new();
    for (key, value) in leaves {
        if key.len() > MAX_KEY_BYTES {
            return Err(StoreError::Qmdb(format!(
                "state qmdb key exceeds {MAX_KEY_BYTES} bytes"
            )));
        }
        if value.len() > MAX_VALUE_BYTES {
            return Err(StoreError::Qmdb(format!(
                "state qmdb value exceeds {MAX_VALUE_BYTES} bytes"
            )));
        }
        desired.insert(key, value);
    }

    let mut batch = db.new_batch();
    let mut has_changes = false;

    for (key, value) in current_entries {
        match desired.remove(&key) {
            Some(desired_value) if desired_value != value => {
                batch = batch.write(key, Some(desired_value));
                has_changes = true;
            }
            Some(_) => {}
            None => {
                batch = batch.write(key, None);
                has_changes = true;
            }
        }
    }

    for (key, value) in desired {
        batch = batch.write(key, Some(value));
        has_changes = true;
    }

    if !has_changes {
        return Ok(());
    }

    let merkleized = batch
        .merkleize(db, None)
        .await
        .map_err(|error| StoreError::Qmdb(format!("failed to merkleize state qmdb: {error}")))?;
    db.apply_batch(merkleized)
        .await
        .map_err(|error| StoreError::Qmdb(format!("failed to apply state qmdb: {error}")))?;
    Ok(())
}

fn root(slot: AccountSnapshotSlot, db: &StateDb) -> QmdbStateRoot {
    QmdbStateRoot {
        root: db.root().0,
        slot,
    }
}

async fn collect_entries(db: &StateDb) -> Result<Vec<(Vec<u8>, Vec<u8>)>, StoreError> {
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
