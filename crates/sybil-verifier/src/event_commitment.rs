//! Authenticated per-block event commitments.
//!
//! `events_root` is a keyless qMDB root over canonical event leaf bytes in
//! section order: system events, accepted orders, rejected orders, fills.

use std::num::{NonZeroU16, NonZeroU64, NonZeroUsize};
use std::sync::{mpsc, OnceLock};
use std::thread;

use commonware_codec::RangeCfg;
use commonware_cryptography::Sha256 as QmdbSha256;
use commonware_runtime::buffer::paged::CacheRef;
use commonware_runtime::{deterministic, Runner as _};
use commonware_storage::journal::contiguous::variable::Config as VConfig;
use commonware_storage::merkle::mmr::journaled::Config as MmrConfig;
use commonware_storage::merkle::mmr::Family as MmrFamily;
use commonware_storage::qmdb::keyless::variable::{
    Config as KeylessVariableConfig, Db as KeylessVariableDb,
};
use matching_engine::Fill;

use crate::canonical::append_order;
use crate::types::{
    BlockWitness, RejectionReason, SystemEventWitness, WitnessOrder, WitnessRejection,
};

const PAGE_SIZE: u16 = 4096;
const PAGE_CACHE_PAGES: usize = 128;
const ITEMS_PER_BLOB: u64 = 1024;
const WRITE_BUFFER_BYTES: usize = 64 * 1024;
const MAX_VALUE_BYTES: usize = 1 << 20;

type EventRootDb = KeylessVariableDb<MmrFamily, deterministic::Context, Vec<u8>, QmdbSha256>;

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

/// Return canonical event leaf bytes in the section order committed by `events_root`.
pub fn event_leaf_values(
    system_events: &[SystemEventWitness],
    orders: &[WitnessOrder],
    rejections: &[WitnessRejection],
    fills: &[Fill],
) -> Vec<Vec<u8>> {
    let mut events =
        Vec::with_capacity(system_events.len() + orders.len() + rejections.len() + fills.len());
    events.extend(system_events.iter().map(system_event_leaf_value));
    events.extend(orders.iter().map(order_accepted_leaf_value));
    events.extend(rejections.iter().map(order_rejected_leaf_value));
    events.extend(fills.iter().map(fill_leaf_value));
    events
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
        let merkleized = batch.merkleize(&db, Some(event_count.to_le_bytes().to_vec()));
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
            thread_pool: None,
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

fn system_event_leaf_value(event: &SystemEventWitness) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(b"sybil/event/system");
    match event {
        SystemEventWitness::CreateAccount {
            account_id,
            initial_balance,
        } => {
            value.push(0);
            value.extend_from_slice(&account_id.to_le_bytes());
            value.extend_from_slice(&initial_balance.to_le_bytes());
        }
        SystemEventWitness::Deposit { account_id, amount } => {
            value.push(1);
            value.extend_from_slice(&account_id.to_le_bytes());
            value.extend_from_slice(&amount.to_le_bytes());
        }
        SystemEventWitness::L1Deposit {
            account_id,
            amount,
            deposit_id,
            deposit_root,
            sybil_account_key,
        } => {
            value.push(2);
            value.extend_from_slice(&account_id.to_le_bytes());
            value.extend_from_slice(&amount.to_le_bytes());
            value.extend_from_slice(&deposit_id.to_le_bytes());
            value.extend_from_slice(deposit_root);
            value.extend_from_slice(sybil_account_key);
        }
        SystemEventWitness::WithdrawalCreated {
            account_id,
            amount,
            withdrawal_id,
            recipient,
            token,
            amount_token_units,
            expiry_height,
            nullifier,
        } => {
            value.push(3);
            value.extend_from_slice(&account_id.to_le_bytes());
            value.extend_from_slice(&amount.to_le_bytes());
            value.extend_from_slice(&withdrawal_id.to_le_bytes());
            value.extend_from_slice(recipient);
            value.extend_from_slice(token);
            value.extend_from_slice(&amount_token_units.to_le_bytes());
            value.extend_from_slice(&expiry_height.to_le_bytes());
            value.extend_from_slice(nullifier);
        }
        SystemEventWitness::MarketResolved {
            market_id,
            payout_nanos,
            affected_accounts,
        } => {
            value.push(4);
            value.extend_from_slice(&market_id.0.to_le_bytes());
            value.extend_from_slice(&payout_nanos.to_le_bytes());
            let mut affected_accounts = affected_accounts.clone();
            affected_accounts.sort_unstable();
            value.extend_from_slice(&(affected_accounts.len() as u64).to_le_bytes());
            for account_id in affected_accounts {
                value.extend_from_slice(&account_id.to_le_bytes());
            }
        }
    }
    value
}

fn order_accepted_leaf_value(event: &WitnessOrder) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(b"sybil/event/order-accepted");
    value.extend_from_slice(&event.account_id.to_le_bytes());
    value.push(u8::from(event.is_mm));
    append_order(&mut value, &event.order);
    value
}

fn order_rejected_leaf_value(event: &WitnessRejection) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(b"sybil/event/order-rejected");
    value.extend_from_slice(&event.account_id.to_le_bytes());
    append_order(&mut value, &event.order);
    append_rejection_reason(&mut value, &event.reason);
    value
}

fn fill_leaf_value(fill: &Fill) -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(b"sybil/event/fill");
    value.extend_from_slice(&fill.order_id.to_le_bytes());
    value.extend_from_slice(&fill.fill_qty.to_le_bytes());
    value.extend_from_slice(&fill.fill_price.to_le_bytes());
    value.extend_from_slice(&fill.account_id.to_le_bytes());
    value
}

fn append_rejection_reason(value: &mut Vec<u8>, reason: &RejectionReason) {
    match reason {
        RejectionReason::InsufficientBalance {
            required,
            available,
        } => {
            value.push(0);
            value.extend_from_slice(&required.to_le_bytes());
            value.extend_from_slice(&available.to_le_bytes());
        }
        RejectionReason::InsufficientPosition {
            market,
            outcome,
            required,
            available,
        } => {
            value.push(1);
            value.extend_from_slice(&market.0.to_le_bytes());
            value.push(*outcome);
            value.extend_from_slice(&required.to_le_bytes());
            value.extend_from_slice(&available.to_le_bytes());
        }
        RejectionReason::AccountNotFound => value.push(2),
        RejectionReason::CompleteSetFormation => value.push(3),
        RejectionReason::Expired {
            current_block,
            expires_at_block,
        } => {
            value.push(4);
            value.extend_from_slice(&current_block.to_le_bytes());
            value.extend_from_slice(&expires_at_block.to_le_bytes());
        }
    }
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
                192, 49, 15, 127, 205, 199, 131, 164, 175, 240, 21, 115, 173, 61, 247, 113, 35,
                129, 44, 150, 211, 36, 13, 167, 222, 164, 46, 216, 180, 50, 124, 160,
            ]
        );
    }
}
