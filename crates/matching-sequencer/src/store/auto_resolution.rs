use super::*;

/// Durable operator decision state for an automated resolution proposal.
///
/// This is off-block metadata used by sybil-api/sybil-polymarket to preserve
/// operator vetoes and approvals across process restarts. It intentionally does
/// not enter the state root or verifier witness.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct AutoResolutionRecord {
    pub market_id: u32,
    pub action: AutoResolutionAction,
    pub payout_nanos: u64,
    pub confidence_ppm: u32,
    pub reasoning: String,
    #[serde(default)]
    pub evidence_excerpts: Vec<String>,
    pub proposed_at_ms: u64,
    #[serde(default)]
    pub eta_ms: Option<u64>,
    #[serde(default)]
    pub approved_at_ms: Option<u64>,
    #[serde(default)]
    pub rejected_at_ms: Option<u64>,
    #[serde(default)]
    pub rejected_payout_nanos: Option<u64>,
    #[serde(default)]
    pub rejected_reasoning_hash: Option<[u8; 32]>,
    #[serde(default)]
    pub operator_note: Option<String>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum AutoResolutionAction {
    Propose,
    Review,
    Escalate,
}

impl Store {
    /// Persist or replace one auto-resolution review-board record.
    pub async fn put_auto_resolution_record(
        &self,
        record: AutoResolutionRecord,
    ) -> Result<(), StoreError> {
        let bytes = rmp_serde::to_vec(&record)?;
        self.redb_write(move |db| {
            let txn = db.begin_write()?;
            {
                let mut table = txn.open_table(AUTO_RESOLUTION_RECORDS)?;
                table.insert(record.market_id, bytes.as_slice())?;
            }
            txn.commit()?;
            Ok(())
        })
        .await
    }

    /// Load every durable auto-resolution review-board record.
    pub fn auto_resolution_records(&self) -> Result<Vec<AutoResolutionRecord>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(AUTO_RESOLUTION_RECORDS)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            let (_, value) = entry?;
            out.push(rmp_serde::from_slice(value.value())?);
        }
        Ok(out)
    }
}
