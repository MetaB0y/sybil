use super::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ControlPlaneCommand {
    CreateAccount {
        initial_balance: i64,
    },
    CreateAccountAt {
        initial_balance: i64,
        timestamp_ms: u64,
    },
    FundAccount {
        account_id: AccountId,
        amount: i64,
        timestamp_ms: u64,
    },
    RegisterPubkey {
        account_id: AccountId,
        compressed_pubkey: Vec<u8>,
        #[serde(default)]
        auth_scheme: crate::crypto::AccountAuthScheme,
    },
    AdvanceReplayNonce {
        account_id: AccountId,
        nonce: u64,
    },
    CreateMarket {
        name: String,
    },
    CreateMarketWithMetadata {
        name: String,
        metadata: MarketMetadata,
    },
    CreateMarketGroup {
        name: String,
        market_ids: Vec<MarketId>,
    },
    CancelPendingOrder {
        account_id: AccountId,
        order_id: u64,
        timestamp_ms: u64,
    },
    ResolveMarket {
        market_id: MarketId,
        payout_nanos: Nanos,
        timestamp_ms: u64,
    },
    ResolveMarketAttested {
        market_id: MarketId,
        signed: SignedAttestation,
        timestamp_ms: u64,
    },
    RegisterFeed {
        pubkey: FeedPubkey,
        name: String,
        timestamp_ms: u64,
    },
    InstallTemplate {
        template: ResolutionTemplate,
    },
    ExtendMarketGroup {
        group_id: u64,
        market_id: MarketId,
    },
    /// Register a signing key carrying SYB-60 management metadata.
    RegisterPubkeyWithMeta {
        account_id: AccountId,
        compressed_pubkey: Vec<u8>,
        #[serde(default)]
        auth_scheme: crate::crypto::AccountAuthScheme,
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        scope: crate::crypto::KeyScope,
        #[serde(default)]
        created_at_ms: u64,
    },
    RegisterPubkeyAuthorized {
        account_id: AccountId,
        compressed_pubkey: Vec<u8>,
        auth_scheme: crate::crypto::AccountAuthScheme,
        label: Option<String>,
        scope: crate::crypto::KeyScope,
        created_at_ms: u64,
        authorization: sybil_verifier::KeyOpAuth,
    },
    /// Revoke a registered signing key (SYB-60).
    RevokeSigningKey {
        account_id: AccountId,
        compressed_pubkey: Vec<u8>,
        authorization: sybil_verifier::KeyOpAuth,
    },
    /// Set/clear an account's opt-in profile (SYB-60).
    SetProfile {
        account_id: AccountId,
        #[serde(default)]
        display_name: Option<String>,
        #[serde(default)]
        avatar_seed: Option<String>,
    },
    /// Create a read-scoped bearer API key from its blake3 hash (SYB-60).
    CreateApiKey {
        account_id: AccountId,
        token_hash: [u8; 32],
        #[serde(default)]
        label: Option<String>,
        created_at_ms: u64,
    },
    /// Revoke a read-scoped bearer API key by id (SYB-60).
    RevokeApiKey {
        account_id: AccountId,
        api_key_id: u64,
        revoked_at_ms: u64,
    },
}

impl Store {
    /// Append one pending bundle submission to the durable recovery log.
    ///
    /// Called by the actor on every admit that routes to the in-memory
    /// pending queue (MM-constrained, multi-order, or multi-market orders).
    /// The row is cleared atomically inside `save_block` when the bundle is
    /// consumed into a committed block. The next-seq is derived from the
    /// current table max so restart-then-admit doesn't collide with the
    /// replayed rows that are still in memory.
    pub async fn append_pending_bundle(
        &self,
        submission: &crate::sequencer::OrderSubmission,
    ) -> Result<(), StoreError> {
        let bytes = rmp_serde::to_vec(submission)?;
        self.redb_write(move |db| append_msgpack_row_bytes(&db, PENDING_BUNDLES, bytes))
            .await
    }

    /// Append one `RestingOrder` to the admit-log WAL.
    ///
    /// Called by the actor right after `try_admit_direct` inserts a non-MM
    /// admit into the live resting book; the 200 OK only returns once this
    /// row is committed to redb. Rows are cleared atomically by `save_block`
    /// once the admit is rolled into the next `RESTING_ORDERS` snapshot.
    pub async fn append_admit_log(&self, resting: &RestingOrder) -> Result<(), StoreError> {
        let bytes = rmp_serde::to_vec(resting)?;
        self.redb_write(move |db| append_msgpack_row_bytes(&db, ADMIT_LOG, bytes))
            .await
    }

    pub async fn append_control_plane_command(
        &self,
        command: &ControlPlaneCommand,
    ) -> Result<(), StoreError> {
        let bytes = rmp_serde::to_vec(command)?;
        self.redb_write(move |db| append_msgpack_row_bytes(&db, CONTROL_PLANE_LOG, bytes))
            .await
    }

    pub async fn append_pending_l1_deposit(&self, deposit: &L1Deposit) -> Result<(), StoreError> {
        let bytes = rmp_serde::to_vec(deposit)?;
        self.redb_write(move |db| append_msgpack_row_bytes(&db, PENDING_L1_DEPOSITS, bytes))
            .await
    }

    pub async fn append_pending_bridge_withdrawal(
        &self,
        request: &BridgeWithdrawalRequest,
    ) -> Result<(), StoreError> {
        let bytes = rmp_serde::to_vec(request)?;
        self.redb_write(move |db| append_msgpack_row_bytes(&db, PENDING_BRIDGE_WITHDRAWALS, bytes))
            .await
    }

    pub async fn append_pending_bridge_l1_input(
        &self,
        input: &crate::bridge::BridgeL1Input,
    ) -> Result<(), StoreError> {
        let bytes = rmp_serde::to_vec(input)?;
        self.redb_write(move |db| append_msgpack_row_bytes(&db, PENDING_BRIDGE_L1_INPUTS, bytes))
            .await
    }
}

fn append_msgpack_row_bytes(
    db: &Database,
    table: TableDefinition<u64, &[u8]>,
    bytes: Vec<u8>,
) -> Result<(), StoreError> {
    let txn = db.begin_write()?;
    let next_seq = {
        let table = txn.open_table(table)?;
        let last_key = table
            .iter()?
            .next_back()
            .transpose()?
            .map(|(k, _)| k.value());
        last_key.map(|k| k + 1).unwrap_or(0)
    };
    {
        let mut table = txn.open_table(table)?;
        table.insert(next_seq, bytes.as_slice())?;
    }
    txn.commit()?;
    Ok(())
}
