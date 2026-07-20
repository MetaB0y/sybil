use std::collections::HashMap;
use std::num::{NonZeroU16, NonZeroU64, NonZeroUsize};
use std::path::Path;
use std::sync::mpsc;
use std::thread;

use commonware_codec::RangeCfg;
use commonware_cryptography::Sha256;
use commonware_parallel::Sequential;
use commonware_runtime::buffer::paged::CacheRef;
use commonware_runtime::{Runner as _, tokio as commonware_tokio};
use commonware_storage::journal::contiguous::variable::Config as VConfig;
use commonware_storage::merkle::mmr::Family as MmrFamily;
use commonware_storage::merkle::mmr::full::Config as MmrConfig;
use commonware_storage::qmdb::current::VariableConfig;
use commonware_storage::qmdb::current::ordered::variable::Db as OrderedVariableDb;
use commonware_storage::translator::OneCap;
use futures::StreamExt;
use tokio::sync::{mpsc as tokio_mpsc, oneshot};

use crate::account::{Account, AccountId, AccountStore};
use crate::account_storage::AccountSnapshotSlot;
use crate::store::StoreError;

const CHUNK_SIZE: usize = 32;
const PAGE_SIZE: u16 = 4096;
const PAGE_CACHE_PAGES: usize = 128;
const ITEMS_PER_BLOB: u64 = 1024;
const WRITE_BUFFER_BYTES: usize = 64 * 1024;
const MAX_KEY_BYTES: usize = 64;
const MAX_VALUE_BYTES: usize = 1 << 20;

const ACCOUNT_KEY_PREFIX: u8 = b'a';
const HEIGHT_KEY: &[u8] = b"meta:height";
const NEXT_ACCOUNT_ID_KEY: &[u8] = b"meta:next_account_id";

type AccountDb = OrderedVariableDb<
    MmrFamily,
    commonware_tokio::Context,
    Vec<u8>,
    Vec<u8>,
    Sha256,
    OneCap,
    CHUNK_SIZE,
    Sequential,
>;

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
        snapshot: PersistedAccountSnapshot,
        respond_to: oneshot::Sender<Result<(), StoreError>>,
    },
    LoadSnapshot {
        slot: AccountSnapshotSlot,
        respond_to: oneshot::Sender<Result<LoadedAccountSnapshot, StoreError>>,
    },
}

struct PersistedAccountSnapshot {
    accounts: Vec<Account>,
    height: u64,
    next_account_id: u64,
}

impl QmdbAccounts {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(path)?;
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
        height: u64,
        next_account_id: u64,
    ) -> Result<(), StoreError> {
        let snapshot = PersistedAccountSnapshot {
            accounts: accounts
                .iter()
                .map(|(_, account)| account.clone())
                .collect(),
            height,
            next_account_id,
        };
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(Command::ReplaceSnapshot {
                slot,
                snapshot,
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
}

async fn run(mut db: AccountDb, mut receiver: tokio_mpsc::Receiver<Command>) {
    while let Some(command) = receiver.recv().await {
        match command {
            Command::ReplaceSnapshot {
                slot,
                snapshot,
                respond_to,
            } => {
                let _ = respond_to.send(replace_snapshot(&mut db, slot, snapshot).await);
            }
            Command::LoadSnapshot { slot, respond_to } => {
                let _ = respond_to.send(load_snapshot(&db, slot).await);
            }
        }
    }
}

// `NonZero*::new(CONST).unwrap()` on compile-time non-zero page/blob/buffer
// constants is infallible; no fallible runtime input reaches these.
#[allow(
    clippy::unwrap_used,
    reason = "NonZero constructors receive compile-time positive constants"
)]
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
            strategy: Sequential,
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
    db.commit()
        .await
        .map_err(|error| StoreError::Qmdb(format!("failed to commit qmdb batch: {error}")))?;
    // This database is a fenced current-state snapshot, not a historical
    // operation archive. Once the new snapshot is durable, discard operations
    // below qMDB's safe boundary. The active A/B keyspace and root are
    // unchanged, while old journal sections can be reclaimed.
    let prune_to = db.sync_boundary();
    db.prune(prune_to)
        .await
        .map_err(|error| StoreError::Qmdb(format!("failed to prune qmdb history: {error}")))?;
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(prefix: &str) -> PathBuf {
        let unique = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("sybil-{prefix}-{}-{unique}", std::process::id()))
    }

    fn data_blob_count(path: &Path) -> usize {
        fs::read_dir(path.join("accounts-log_data"))
            .expect("account qMDB data directory should exist")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .count()
    }

    #[tokio::test]
    async fn account_snapshot_journal_stays_bounded_and_both_slots_recover() {
        let path = temp_dir("qmdb-account-pruning");
        let qmdb = QmdbAccounts::open(&path).unwrap();
        let mut accounts = AccountStore::new();
        for _ in 0..38 {
            accounts.create_account(1_000_000_000);
        }

        for height in 1..=400 {
            for (_, account) in accounts.iter_mut() {
                account.balance = account.balance.saturating_add(1);
                account.events_digest[0] = height as u8;
            }
            qmdb.persist(
                if height % 2 == 0 {
                    AccountSnapshotSlot::A
                } else {
                    AccountSnapshotSlot::B
                },
                &accounts,
                height,
                accounts.next_id(),
            )
            .await
            .unwrap();
        }

        // Without pruning, this workload creates roughly fifteen 1,024-item
        // journal sections. qMDB retains only the small tail required by its
        // current-state grafting boundary.
        assert!(
            data_blob_count(&path) <= 3,
            "account snapshot operation journal retained too many data sections"
        );

        for (slot, height) in [(AccountSnapshotSlot::A, 400), (AccountSnapshotSlot::B, 399)] {
            let loaded = qmdb.load(slot).await.unwrap();
            assert_eq!(loaded.height, Some(height));
            assert_eq!(loaded.next_account_id, Some(accounts.next_id()));
            assert_eq!(loaded.accounts.len(), accounts.iter().count());
        }
    }
}
