//! Authenticated per-block event commitments.
//!
//! `events_root` is a keyless qMDB root over canonical event leaf bytes in
//! section order: system events, accepted orders, rejected orders, fills.

use std::num::{NonZeroU16, NonZeroU64, NonZeroUsize};
use std::sync::{OnceLock, mpsc};
use std::thread;

use commonware_codec::RangeCfg;
use commonware_cryptography::Sha256 as QmdbSha256;
use commonware_parallel::Sequential;
use commonware_runtime::buffer::paged::CacheRef;
use commonware_runtime::{Runner as _, deterministic};
use commonware_storage::journal::contiguous::variable::Config as VConfig;
use commonware_storage::merkle::Location;
use commonware_storage::merkle::mmr::Family as MmrFamily;
use commonware_storage::merkle::mmr::full::Config as MmrConfig;
use commonware_storage::qmdb::keyless::variable::{
    Config as KeylessVariableConfig, Db as KeylessVariableDb,
};

pub use crate::event_schema::event_leaf_values;
use crate::types::BlockWitness;

const PAGE_SIZE: u16 = 4096;
const PAGE_CACHE_PAGES: usize = 128;
const ITEMS_PER_BLOB: u64 = 1024;
const WRITE_BUFFER_BYTES: usize = 64 * 1024;
const MAX_VALUE_BYTES: usize = 1 << 20;

type EventRootDb =
    KeylessVariableDb<MmrFamily, deterministic::Context, Vec<u8>, QmdbSha256, Sequential>;

struct EventRootRequest {
    events: Vec<Vec<u8>>,
    respond_to: mpsc::SyncSender<[u8; 32]>,
}

struct EventRootWorker {
    sender: mpsc::Sender<EventRootRequest>,
}

static EVENT_ROOT_WORKER: OnceLock<EventRootWorker> = OnceLock::new();

/// Compute the authenticated event root for a block witness.
pub fn compute_events_root(witness: &BlockWitness) -> [u8; 32] {
    let events = event_leaf_values(
        &witness.system_events,
        &witness.orders,
        &witness.rejections,
        &witness.fills,
    );
    events_root_from_event_bytes(&events)
}

/// Root for a block with no events.
pub fn empty_events_root() -> [u8; 32] {
    events_root_from_event_bytes(&[])
}

pub fn events_root_from_event_bytes(events: &[Vec<u8>]) -> [u8; 32] {
    let events = events.to_vec();
    let (respond_to, response) = mpsc::sync_channel(1);
    event_root_worker()
        .sender
        .send(EventRootRequest { events, respond_to })
        .expect("event root worker should be available");
    response.recv().expect("event root worker should respond")
}

fn event_root_worker() -> &'static EventRootWorker {
    EVENT_ROOT_WORKER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<EventRootRequest>();
        thread::Builder::new()
            .name("sybil-event-root-qmdb".to_string())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    let root = event_root_from_event_bytes_inner(request.events);
                    let _ = request.respond_to.send(root);
                }
            })
            .expect("event root qmdb thread should spawn");

        EventRootWorker { sender }
    })
}

fn event_root_from_event_bytes_inner(events: Vec<Vec<u8>>) -> [u8; 32] {
    deterministic::Runner::default().start(|context| async move {
        let mut db = open_event_root_db(context)
            .await
            .expect("event root qmdb should initialize");
        let event_count = events.len() as u64;
        let mut batch = db.new_batch();
        for event in events {
            assert!(
                event.len() <= MAX_VALUE_BYTES,
                "event root value exceeds qmdb value limit"
            );
            batch = batch.append(event);
        }
        let merkleized = batch.merkleize(
            &db,
            Some(event_count.to_le_bytes().to_vec()),
            Location::new(0),
        );
        db.apply_batch(merkleized)
            .await
            .expect("event root qmdb batch should apply");
        db.root().0
    })
}

async fn open_event_root_db(context: deterministic::Context) -> Result<EventRootDb, String> {
    let page_cache = CacheRef::from_pooler(
        &context,
        NonZeroU16::new(PAGE_SIZE).unwrap(),
        NonZeroUsize::new(PAGE_CACHE_PAGES).unwrap(),
    );
    let config = KeylessVariableConfig {
        merkle: MmrConfig {
            journal_partition: "event-root-mmr-journal".to_string(),
            items_per_blob: NonZeroU64::new(ITEMS_PER_BLOB).unwrap(),
            write_buffer: NonZeroUsize::new(WRITE_BUFFER_BYTES).unwrap(),
            metadata_partition: "event-root-mmr-metadata".to_string(),
            strategy: Sequential,
            page_cache: page_cache.clone(),
        },
        log: VConfig {
            partition: "event-root-log".to_string(),
            write_buffer: NonZeroUsize::new(WRITE_BUFFER_BYTES).unwrap(),
            compression: None,
            codec_config: (RangeCfg::from(0..=MAX_VALUE_BYTES), ()),
            items_per_section: NonZeroU64::new(ITEMS_PER_BLOB).unwrap(),
            page_cache,
        },
    };

    EventRootDb::init(context, config)
        .await
        .map_err(|error| format!("failed to initialize event root qmdb: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SystemEventWitness;

    #[test]
    fn event_leaf_values_encode_deposit() {
        let system_events = vec![SystemEventWitness::Deposit {
            account_id: 7,
            amount: 50,
        }];
        let events = event_leaf_values(&system_events, &[], &[], &[]);
        let mut expected = b"sybil/event/system".to_vec();
        expected.push(1);
        expected.extend_from_slice(&7u64.to_le_bytes());
        expected.extend_from_slice(&50u64.to_le_bytes());

        assert_eq!(events, vec![expected]);
    }

    #[test]
    fn events_root_deterministic() {
        let system_events = vec![SystemEventWitness::Deposit {
            account_id: 7,
            amount: 50,
        }];
        let events = event_leaf_values(&system_events, &[], &[], &[]);

        assert_eq!(
            events_root_from_event_bytes(&events),
            events_root_from_event_bytes(&events)
        );
        assert_ne!(empty_events_root(), events_root_from_event_bytes(&events));
    }

    #[test]
    fn events_root_changes_on_event_mutation() {
        let event_a = vec![SystemEventWitness::Deposit {
            account_id: 7,
            amount: 50,
        }];
        let event_b = vec![SystemEventWitness::Deposit {
            account_id: 7,
            amount: 51,
        }];

        assert_ne!(
            events_root_from_event_bytes(&event_leaf_values(&event_a, &[], &[], &[])),
            events_root_from_event_bytes(&event_leaf_values(&event_b, &[], &[], &[]))
        );
    }

    #[test]
    fn events_root_golden_deposit() {
        let system_events = vec![SystemEventWitness::Deposit {
            account_id: 7,
            amount: 50,
        }];
        let events = event_leaf_values(&system_events, &[], &[], &[]);

        assert_eq!(
            events_root_from_event_bytes(&events),
            [
                80, 198, 242, 60, 202, 11, 62, 126, 105, 245, 100, 247, 169, 21, 83, 76, 35, 1,
                244, 153, 133, 23, 155, 42, 68, 174, 231, 138, 200, 30, 115, 90,
            ]
        );
    }
}
