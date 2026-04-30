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

use crate::account::{Account, AccountId, AccountStore};
use crate::account_storage::AccountSnapshotSlot;
use crate::canonical_state::snapshot_account;
use crate::store::StoreError;

pub const QMDB_ACCOUNT_CHUNK_SIZE: usize = 32;
const CHUNK_SIZE: usize = QMDB_ACCOUNT_CHUNK_SIZE;
const PAGE_SIZE: u16 = 4096;
const PAGE_CACHE_PAGES: usize = 128;
const ITEMS_PER_BLOB: u64 = 1024;
const WRITE_BUFFER_BYTES: usize = 64 * 1024;
const MAX_KEY_BYTES: usize = 64;
const MAX_VALUE_BYTES: usize = 1 << 20;

const ACCOUNT_KEY_PREFIX: u8 = b'a';
const HEIGHT_KEY: &[u8] = b"meta:height";
const NEXT_ACCOUNT_ID_KEY: &[u8] = b"meta:next_account_id";
const STATE_V2_KEY_PREFIX: &[u8] = b"v2:";

type AccountDb = OrderedVariableDb<
    MmrFamily,
    commonware_tokio::Context,
    Vec<u8>,
    Vec<u8>,
    Sha256,
    OneCap,
    CHUNK_SIZE,
>;

pub type QmdbAccountKeyValueProof =
    KeyValueProof<MmrFamily, Vec<u8>, Sha256Digest, QMDB_ACCOUNT_CHUNK_SIZE>;

pub type QmdbAccountExclusionProof = ExclusionProof<
    MmrFamily,
    Vec<u8>,
    commonware_storage::qmdb::any::value::VariableEncoding<Vec<u8>>,
    Sha256Digest,
    QMDB_ACCOUNT_CHUNK_SIZE,
>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QmdbAccountRootScope {
    /// The root covers every active key in the account qMDB, including both
    /// snapshot slots, slot metadata, legacy account rows, and typed `v2:`
    /// leaves. redb remains the authority for which slot is committed.
    AccountDbAllSlots,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QmdbAccountRoot {
    pub root: [u8; 32],
    pub scope: QmdbAccountRootScope,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QmdbTypedLeafProof {
    pub root: [u8; 32],
    pub root_scope: QmdbAccountRootScope,
    pub slot: AccountSnapshotSlot,
    /// Unprefixed verifier/state-root leaf key.
    pub leaf_key: Vec<u8>,
    /// Actual qMDB key: `s{slot}:v2:{leaf_key}`.
    pub encoded_key: Vec<u8>,
    pub leaf_value: Vec<u8>,
    pub proof: QmdbAccountKeyValueProof,
}

impl QmdbTypedLeafProof {
    pub fn verify(&self) -> bool {
        let mut hasher = Sha256::new();
        AccountDb::verify_key_value_proof(
            &mut hasher,
            self.encoded_key.clone(),
            self.leaf_value.clone(),
            &self.proof,
            &Sha256Digest::from(self.root),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QmdbTypedLeafExclusionProof {
    pub root: [u8; 32],
    pub root_scope: QmdbAccountRootScope,
    pub slot: AccountSnapshotSlot,
    /// Unprefixed verifier/state-root leaf key.
    pub leaf_key: Vec<u8>,
    /// Actual qMDB key: `s{slot}:v2:{leaf_key}`.
    pub encoded_key: Vec<u8>,
    pub proof: QmdbAccountExclusionProof,
}

impl QmdbTypedLeafExclusionProof {
    pub fn verify(&self) -> bool {
        let mut hasher = Sha256::new();
        AccountDb::verify_exclusion_proof(
            &mut hasher,
            &self.encoded_key,
            &self.proof,
            &Sha256Digest::from(self.root),
        )
    }
}

pub struct LoadedAccountSnapshot {
    pub accounts: HashMap<AccountId, Account>,
    pub height: Option<u64>,
    pub next_account_id: Option<u64>,
}

pub struct QmdbAccounts {
    sender: tokio_mpsc::Sender<Command>,
}

enum Command {
    ReplaceSnapshot {
        slot: AccountSnapshotSlot,
        snapshot: Box<PersistedAccountSnapshot>,
        respond_to: oneshot::Sender<Result<(), StoreError>>,
    },
    LoadSnapshot {
        slot: AccountSnapshotSlot,
        respond_to: oneshot::Sender<Result<LoadedAccountSnapshot, StoreError>>,
    },
    AccountRoot {
        respond_to: oneshot::Sender<Result<QmdbAccountRoot, StoreError>>,
    },
    TypedLeaves {
        slot: AccountSnapshotSlot,
        respond_to: oneshot::Sender<Result<Vec<(Vec<u8>, Vec<u8>)>, StoreError>>,
    },
    TypedLeafProof {
        slot: AccountSnapshotSlot,
        leaf_key: Vec<u8>,
        respond_to: oneshot::Sender<Result<Option<QmdbTypedLeafProof>, StoreError>>,
    },
    TypedLeafExclusionProof {
        slot: AccountSnapshotSlot,
        leaf_key: Vec<u8>,
        respond_to: oneshot::Sender<Result<Option<QmdbTypedLeafExclusionProof>, StoreError>>,
    },
}

struct PersistedAccountSnapshot {
    accounts: Vec<Account>,
    state_sidecar: sybil_verifier::StateSidecarSnapshot,
    height: u64,
    next_account_id: u64,
}

impl QmdbAccounts {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let storage_directory = path.to_path_buf();
        let (sender, receiver) = tokio_mpsc::channel(8);
        let (started_tx, started_rx) = mpsc::sync_channel(1);

        thread::Builder::new()
            .name("sybil-qmdb-accounts".to_string())
            .spawn(move || {
                let runner = commonware_tokio::Runner::new(
                    commonware_tokio::Config::default().with_storage_directory(storage_directory),
                );
                runner.start(|context| async move {
                    let opened = open_db(context).await;
                    match opened {
                        Ok(db) => {
                            let _ = started_tx.send(Ok(()));
                            run(db, receiver).await;
                        }
                        Err(error) => {
                            let _ = started_tx.send(Err(error));
                        }
                    }
                });
            })
            .map_err(|error| StoreError::Qmdb(format!("failed to start qmdb thread: {error}")))?;

        started_rx
            .recv()
            .map_err(|error| StoreError::Qmdb(format!("qmdb startup channel failed: {error}")))??;

        Ok(Self { sender })
    }

    pub async fn persist(
        &self,
        slot: AccountSnapshotSlot,
        accounts: &AccountStore,
        state_sidecar: &sybil_verifier::StateSidecarSnapshot,
        height: u64,
        next_account_id: u64,
    ) -> Result<(), StoreError> {
        let snapshot = PersistedAccountSnapshot {
            accounts: accounts
                .iter()
                .map(|(_, account)| account.clone())
                .collect(),
            state_sidecar: state_sidecar.clone(),
            height,
            next_account_id,
        };
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(Command::ReplaceSnapshot {
                slot,
                snapshot: Box::new(snapshot),
                respond_to,
            })
            .await
            .map_err(|_| StoreError::Qmdb("qmdb account service unavailable".to_string()))?;
        response
            .await
            .map_err(|_| StoreError::Qmdb("qmdb account response channel dropped".to_string()))?
    }

    pub async fn load(
        &self,
        slot: AccountSnapshotSlot,
    ) -> Result<LoadedAccountSnapshot, StoreError> {
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(Command::LoadSnapshot { slot, respond_to })
            .await
            .map_err(|_| StoreError::Qmdb("qmdb account service unavailable".to_string()))?;
        response
            .await
            .map_err(|_| StoreError::Qmdb("qmdb account response channel dropped".to_string()))?
    }

    pub async fn account_root(&self) -> Result<QmdbAccountRoot, StoreError> {
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(Command::AccountRoot { respond_to })
            .await
            .map_err(|_| StoreError::Qmdb("qmdb account service unavailable".to_string()))?;
        response
            .await
            .map_err(|_| StoreError::Qmdb("qmdb account response channel dropped".to_string()))?
    }

    pub async fn typed_leaves(
        &self,
        slot: AccountSnapshotSlot,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, StoreError> {
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(Command::TypedLeaves { slot, respond_to })
            .await
            .map_err(|_| StoreError::Qmdb("qmdb account service unavailable".to_string()))?;
        response
            .await
            .map_err(|_| StoreError::Qmdb("qmdb account response channel dropped".to_string()))?
    }

    pub async fn typed_leaf_proof(
        &self,
        slot: AccountSnapshotSlot,
        leaf_key: &[u8],
    ) -> Result<Option<QmdbTypedLeafProof>, StoreError> {
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(Command::TypedLeafProof {
                slot,
                leaf_key: leaf_key.to_vec(),
                respond_to,
            })
            .await
            .map_err(|_| StoreError::Qmdb("qmdb account service unavailable".to_string()))?;
        response
            .await
            .map_err(|_| StoreError::Qmdb("qmdb account response channel dropped".to_string()))?
    }

    pub async fn typed_leaf_exclusion_proof(
        &self,
        slot: AccountSnapshotSlot,
        leaf_key: &[u8],
    ) -> Result<Option<QmdbTypedLeafExclusionProof>, StoreError> {
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(Command::TypedLeafExclusionProof {
                slot,
                leaf_key: leaf_key.to_vec(),
                respond_to,
            })
            .await
            .map_err(|_| StoreError::Qmdb("qmdb account service unavailable".to_string()))?;
        response
            .await
            .map_err(|_| StoreError::Qmdb("qmdb account response channel dropped".to_string()))?
    }
}

async fn run(mut db: AccountDb, mut receiver: tokio_mpsc::Receiver<Command>) {
    while let Some(command) = receiver.recv().await {
        match command {
            Command::ReplaceSnapshot {
                slot,
                snapshot,
                respond_to,
            } => {
                let _ = respond_to.send(replace_snapshot(&mut db, slot, *snapshot).await);
            }
            Command::LoadSnapshot { slot, respond_to } => {
                let _ = respond_to.send(load_snapshot(&db, slot).await);
            }
            Command::AccountRoot { respond_to } => {
                let _ = respond_to.send(Ok(account_root(&db)));
            }
            Command::TypedLeaves { slot, respond_to } => {
                let _ = respond_to.send(collect_typed_leaves(&db, slot).await);
            }
            Command::TypedLeafProof {
                slot,
                leaf_key,
                respond_to,
            } => {
                let _ = respond_to.send(typed_leaf_proof(&db, slot, leaf_key).await);
            }
            Command::TypedLeafExclusionProof {
                slot,
                leaf_key,
                respond_to,
            } => {
                let _ = respond_to.send(typed_leaf_exclusion_proof(&db, slot, leaf_key).await);
            }
        }
    }
}

async fn open_db(context: commonware_tokio::Context) -> Result<AccountDb, StoreError> {
    let page_cache = CacheRef::from_pooler(
        &context,
        NonZeroU16::new(PAGE_SIZE).unwrap(),
        NonZeroUsize::new(PAGE_CACHE_PAGES).unwrap(),
    );
    let config = VariableConfig {
        merkle_config: MmrConfig {
            journal_partition: "accounts-mmr-journal".to_string(),
            items_per_blob: NonZeroU64::new(ITEMS_PER_BLOB).unwrap(),
            write_buffer: NonZeroUsize::new(WRITE_BUFFER_BYTES).unwrap(),
            metadata_partition: "accounts-mmr-metadata".to_string(),
            thread_pool: None,
            page_cache: page_cache.clone(),
        },
        journal_config: VConfig {
            partition: "accounts-log".to_string(),
            write_buffer: NonZeroUsize::new(WRITE_BUFFER_BYTES).unwrap(),
            compression: None,
            codec_config: (
                (RangeCfg::from(0..=MAX_KEY_BYTES), ()),
                (RangeCfg::from(0..=MAX_VALUE_BYTES), ()),
            ),
            items_per_section: NonZeroU64::new(ITEMS_PER_BLOB).unwrap(),
            page_cache,
        },
        grafted_metadata_partition: "accounts-grafted-mmr-metadata".to_string(),
        translator: OneCap,
    };

    AccountDb::init(context, config)
        .await
        .map_err(|error| StoreError::Qmdb(format!("failed to initialize qmdb: {error}")))
}

async fn replace_snapshot(
    db: &mut AccountDb,
    slot: AccountSnapshotSlot,
    snapshot: PersistedAccountSnapshot,
) -> Result<(), StoreError> {
    let current_entries = collect_entries(db, slot).await?;
    let mut desired = HashMap::new();

    let account_snapshots: Vec<_> = snapshot.accounts.iter().map(snapshot_account).collect();
    for (leaf_key, leaf_value) in
        sybil_verifier::block::state_root_v2_leaves(&account_snapshots, &snapshot.state_sidecar)
    {
        desired.insert(encode_state_v2_key(slot, &leaf_key), leaf_value);
    }

    for account in snapshot.accounts {
        desired.insert(
            encode_account_key(slot, account.id),
            rmp_serde::to_vec(&account)?,
        );
    }
    desired.insert(
        encode_height_key(slot),
        snapshot.height.to_le_bytes().to_vec(),
    );
    desired.insert(
        encode_next_account_id_key(slot),
        snapshot.next_account_id.to_le_bytes().to_vec(),
    );

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
        .map_err(|error| StoreError::Qmdb(format!("failed to merkleize qmdb batch: {error}")))?;
    db.apply_batch(merkleized)
        .await
        .map_err(|error| StoreError::Qmdb(format!("failed to apply qmdb batch: {error}")))?;
    Ok(())
}

async fn load_snapshot(
    db: &AccountDb,
    slot: AccountSnapshotSlot,
) -> Result<LoadedAccountSnapshot, StoreError> {
    let mut accounts = HashMap::new();
    let mut height = None;
    let mut next_account_id = None;

    for (key, value) in collect_entries(db, slot).await? {
        if key == encode_height_key(slot) {
            height = Some(decode_u64(&value, "height")?);
            continue;
        }
        if key == encode_next_account_id_key(slot) {
            next_account_id = Some(decode_u64(&value, "next_account_id")?);
            continue;
        }

        let Some(account_id) = decode_account_key(slot, &key) else {
            continue;
        };
        let account: Account = rmp_serde::from_slice(&value)?;
        if account.id != account_id {
            return Err(StoreError::Qmdb(format!(
                "account key {:?} did not match serialized id {}",
                key, account.id.0
            )));
        }
        accounts.insert(account_id, account);
    }

    Ok(LoadedAccountSnapshot {
        accounts,
        height,
        next_account_id,
    })
}

fn account_root(db: &AccountDb) -> QmdbAccountRoot {
    QmdbAccountRoot {
        root: db.root().0,
        scope: QmdbAccountRootScope::AccountDbAllSlots,
    }
}

async fn collect_typed_leaves(
    db: &AccountDb,
    slot: AccountSnapshotSlot,
) -> Result<Vec<(Vec<u8>, Vec<u8>)>, StoreError> {
    let prefix = encode_state_v2_key(slot, b"");
    let stream = db.stream_range(prefix.clone()).await.map_err(|error| {
        StoreError::Qmdb(format!("failed to stream qmdb typed leaves: {error}"))
    })?;
    futures::pin_mut!(stream);

    let mut leaves = Vec::new();
    while let Some(item) = stream.next().await {
        let (key, value) = item.map_err(|error| {
            StoreError::Qmdb(format!("failed to read qmdb typed leaf: {error}"))
        })?;
        if !key.starts_with(&prefix) {
            break;
        }
        leaves.push((key[prefix.len()..].to_vec(), value));
    }
    Ok(leaves)
}

async fn typed_leaf_proof(
    db: &AccountDb,
    slot: AccountSnapshotSlot,
    leaf_key: Vec<u8>,
) -> Result<Option<QmdbTypedLeafProof>, StoreError> {
    let encoded_key = encode_state_v2_key(slot, &leaf_key);
    let Some(leaf_value) = db
        .get(&encoded_key)
        .await
        .map_err(|error| StoreError::Qmdb(format!("failed to read qmdb typed leaf: {error}")))?
    else {
        return Ok(None);
    };

    let mut hasher = Sha256::new();
    let proof = db
        .key_value_proof(&mut hasher, encoded_key.clone())
        .await
        .map_err(|error| StoreError::Qmdb(format!("failed to prove qmdb typed leaf: {error}")))?;
    let root = account_root(db);
    Ok(Some(QmdbTypedLeafProof {
        root: root.root,
        root_scope: root.scope,
        slot,
        leaf_key,
        encoded_key,
        leaf_value,
        proof,
    }))
}

async fn typed_leaf_exclusion_proof(
    db: &AccountDb,
    slot: AccountSnapshotSlot,
    leaf_key: Vec<u8>,
) -> Result<Option<QmdbTypedLeafExclusionProof>, StoreError> {
    let encoded_key = encode_state_v2_key(slot, &leaf_key);
    let mut hasher = Sha256::new();
    let proof = match db.exclusion_proof(&mut hasher, &encoded_key).await {
        Ok(proof) => proof,
        Err(commonware_storage::qmdb::Error::KeyExists) => return Ok(None),
        Err(error) => {
            return Err(StoreError::Qmdb(format!(
                "failed to prove qmdb typed leaf exclusion: {error}"
            )));
        }
    };
    let root = account_root(db);
    Ok(Some(QmdbTypedLeafExclusionProof {
        root: root.root,
        root_scope: root.scope,
        slot,
        leaf_key,
        encoded_key,
        proof,
    }))
}

async fn collect_entries(
    db: &AccountDb,
    slot: AccountSnapshotSlot,
) -> Result<Vec<(Vec<u8>, Vec<u8>)>, StoreError> {
    let prefix = slot_prefix(slot);
    let stream = db
        .stream_range(prefix.clone())
        .await
        .map_err(|error| StoreError::Qmdb(format!("failed to stream qmdb entries: {error}")))?;
    futures::pin_mut!(stream);

    let mut entries = Vec::new();
    while let Some(item) = stream.next().await {
        let (key, value) =
            item.map_err(|error| StoreError::Qmdb(format!("failed to read qmdb entry: {error}")))?;
        if !key.starts_with(&prefix) {
            break;
        }
        entries.push((key, value));
    }
    Ok(entries)
}

fn slot_prefix(slot: AccountSnapshotSlot) -> Vec<u8> {
    vec![b's', slot.encode() as u8, b':']
}

fn encode_account_key(slot: AccountSnapshotSlot, account_id: AccountId) -> Vec<u8> {
    let mut key = Vec::with_capacity(12);
    key.extend_from_slice(&slot_prefix(slot));
    key.push(ACCOUNT_KEY_PREFIX);
    key.extend_from_slice(&account_id.0.to_be_bytes());
    key
}

fn encode_height_key(slot: AccountSnapshotSlot) -> Vec<u8> {
    let mut key = slot_prefix(slot);
    key.extend_from_slice(HEIGHT_KEY);
    key
}

fn encode_next_account_id_key(slot: AccountSnapshotSlot) -> Vec<u8> {
    let mut key = slot_prefix(slot);
    key.extend_from_slice(NEXT_ACCOUNT_ID_KEY);
    key
}

fn encode_state_v2_key(slot: AccountSnapshotSlot, leaf_key: &[u8]) -> Vec<u8> {
    let prefix = slot_prefix(slot);
    let mut key = Vec::with_capacity(prefix.len() + STATE_V2_KEY_PREFIX.len() + leaf_key.len());
    key.extend_from_slice(&prefix);
    key.extend_from_slice(STATE_V2_KEY_PREFIX);
    key.extend_from_slice(leaf_key);
    key
}

fn decode_account_key(slot: AccountSnapshotSlot, key: &[u8]) -> Option<AccountId> {
    let prefix = slot_prefix(slot);
    if key.len() != prefix.len() + 9
        || !key.starts_with(&prefix)
        || key[prefix.len()] != ACCOUNT_KEY_PREFIX
    {
        return None;
    }
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&key[prefix.len() + 1..]);
    Some(AccountId(u64::from_be_bytes(raw)))
}

fn decode_u64(value: &[u8], label: &str) -> Result<u64, StoreError> {
    if value.len() != 8 {
        return Err(StoreError::Qmdb(format!(
            "invalid {label} length in qmdb snapshot: expected 8 bytes, got {}",
            value.len()
        )));
    }
    let mut raw = [0u8; 8];
    raw.copy_from_slice(value);
    Ok(u64::from_le_bytes(raw))
}
