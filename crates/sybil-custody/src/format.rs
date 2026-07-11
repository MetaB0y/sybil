use serde::{Deserialize, Serialize};
use sybil_api_types::{DaManifestResponse, StateProofResponse};
use sybil_verifier::{AccountReservationSnapshot, AccountSnapshot, KeyRecord, MarketSnapshot};

pub const CUSTODY_SNAPSHOT_VERSION: u8 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustodySnapshot {
    pub version: u8,
    pub account_id: u64,
    pub block_height: u64,
    pub state_root: String,
    pub genesis_hash: String,
    pub account: AccountSnapshot,
    pub account_proof: StateProofResponse,
    pub reservation: Option<AccountReservationSnapshot>,
    pub reservation_proof: StateProofResponse,
    pub markets: Vec<MarketSnapshot>,
    pub market_proofs: Vec<StateProofResponse>,
    pub active_keys: Vec<KeyRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustodyManifest {
    pub version: u8,
    pub manifest: DaManifestResponse,
    /// Present when `snapshot` was given an Ethereum RPC and settlement
    /// address. A later offline reconstruction can reuse this authenticated
    /// record, but callers should refresh from L1 whenever possible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_record: Option<RootRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootRecord {
    pub height: u64,
    pub state_root: [u8; 32],
    pub previous_state_root: [u8; 32],
    pub block_hash: [u8; 32],
    pub events_root: [u8; 32],
    pub witness_root: [u8; 32],
    pub da_commitment: [u8; 32],
    pub deposit_root: [u8; 32],
    pub deposit_count: u64,
    pub verified_at: u64,
    pub verifier_version: u32,
}
