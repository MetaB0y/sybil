use super::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InitialAccountKeyCommand {
    pub compressed_pubkey: Vec<u8>,
    #[serde(default)]
    pub auth_scheme: crate::crypto::AccountAuthScheme,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub scope: crate::crypto::KeyScope,
    #[serde(default)]
    pub created_at_ms: u64,
}

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
    /// Public onboarding: allocate the account and install its first signing
    /// key as one durable command. Kept at the enum tail so legacy MessagePack
    /// variant indexes remain replay-compatible.
    CreateAccountWithInitialKey {
        initial_balance: i64,
        timestamp_ms: u64,
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
    /// Atomic public allocation with a durable counter independent from the
    /// service account id space.
    CreatePublicAccountWithInitialKey {
        expected_public_index: u64,
        initial_balance: i64,
        timestamp_ms: u64,
        initial_key: InitialAccountKeyCommand,
    },
    /// Genesis-bound retry-safe service allocation. The raw caller key is
    /// retained only in the short acknowledged-write suffix so replay can
    /// independently derive and validate the durable receipt identity.
    ProvisionServiceAccount {
        provisioning_key: String,
        expected_account_id: AccountId,
        initial_balance: i64,
        timestamp_ms: u64,
        initial_key: Option<InitialAccountKeyCommand>,
    },
}

impl Store {
    /// Append one pending bundle submission to the durable recovery log.
    ///
    /// Called by the actor on every admit that routes to the in-memory
    /// pending queue (MM-constrained, multi-order, or multi-market orders).
    /// The row is cleared atomically inside `save_block` when the bundle is
    /// consumed into a committed block. Sequence allocation and row insertion
    /// share one redb transaction, so restart-then-admit cannot collide with
    /// replayed rows that are still pending after the committed floor.
    pub async fn append_pending_bundle(
        &self,
        submission: &crate::sequencer::OrderSubmission,
    ) -> Result<(), StoreError> {
        self.append_acknowledged_write(AcknowledgedWrite::DeferredBundle(submission.clone()))
            .await
            .map(|_| ())
    }

    /// Append one `RestingOrder` to the admit-log WAL.
    ///
    /// Called by the actor right after `try_admit_direct` inserts a non-MM
    /// admit into the live resting book; the 200 OK only returns once this
    /// row is committed to redb. Rows are cleared atomically by `save_block`
    /// once the admit is rolled into the next `RESTING_ORDERS` snapshot.
    pub async fn append_admit_log(&self, resting: &RestingOrder) -> Result<(), StoreError> {
        self.append_acknowledged_write(AcknowledgedWrite::DirectAdmit(resting.clone()))
            .await
            .map(|_| ())
    }

    pub async fn append_authenticated_direct_admit(
        &self,
        resting: &RestingOrder,
        nonce: u64,
        authorization: &sybil_verifier::ClientActionAuth,
    ) -> Result<(), StoreError> {
        self.append_acknowledged_write(AcknowledgedWrite::AuthenticatedDirectAdmit {
            resting: resting.clone(),
            nonce,
            authorization: authorization.clone(),
        })
        .await
        .map(|_| ())
    }

    pub async fn append_authenticated_deferred_bundle(
        &self,
        submission: &crate::sequencer::OrderSubmission,
        nonce: u64,
        authorization: &sybil_verifier::ClientActionAuth,
    ) -> Result<(), StoreError> {
        self.append_acknowledged_write(AcknowledgedWrite::AuthenticatedDeferredBundle {
            submission: submission.clone(),
            nonce,
            authorization: authorization.clone(),
        })
        .await
        .map(|_| ())
    }

    pub async fn append_authenticated_cancel(
        &self,
        account_id: AccountId,
        order_id: u64,
        nonce: u64,
        authorization: &sybil_verifier::ClientActionAuth,
        timestamp_ms: u64,
    ) -> Result<(), StoreError> {
        self.append_acknowledged_write(AcknowledgedWrite::AuthenticatedCancel {
            account_id,
            order_id,
            nonce,
            authorization: authorization.clone(),
            timestamp_ms,
        })
        .await
        .map(|_| ())
    }

    pub async fn append_control_plane_command(
        &self,
        command: &ControlPlaneCommand,
    ) -> Result<(), StoreError> {
        self.append_acknowledged_write(AcknowledgedWrite::ControlPlane(command.clone()))
            .await
            .map(|_| ())
    }

    pub async fn append_pending_l1_deposit(&self, deposit: &L1Deposit) -> Result<(), StoreError> {
        self.append_acknowledged_write(AcknowledgedWrite::L1Deposit(deposit.clone()))
            .await
            .map(|_| ())
    }

    pub async fn append_pending_bridge_withdrawal(
        &self,
        request: &BridgeWithdrawalRequest,
    ) -> Result<(), StoreError> {
        self.append_acknowledged_write(AcknowledgedWrite::BridgeWithdrawal(request.clone()))
            .await
            .map(|_| ())
    }

    pub async fn append_pending_bridge_l1_input(
        &self,
        input: &crate::bridge::BridgeL1Input,
    ) -> Result<(), StoreError> {
        self.append_acknowledged_write(AcknowledgedWrite::BridgeL1Input(input.clone()))
            .await
            .map(|_| ())
    }

    // Prometheus gauges use f64 at the metrics-library boundary; protocol
    // state and sequence allocation remain integer-only.
    #[allow(
        clippy::disallowed_types,
        reason = "integer sequence values are converted only at the Prometheus boundary"
    )]
    async fn append_acknowledged_write(&self, write: AcknowledgedWrite) -> Result<u64, StoreError> {
        let kind = write.kind();
        let sequence = self
            .redb_write(move |db| append_acknowledged_write_row(&db, write))
            .await?;
        metrics::counter!("sybil_acknowledged_writes_appended_total", "kind" => kind).increment(1);
        metrics::gauge!("sybil_acknowledged_write_next_sequence")
            .set(sequence.saturating_add(1) as f64);
        Ok(sequence)
    }
}

pub const ACKNOWLEDGED_WRITE_ENVELOPE_VERSION: u8 = 1;

/// One actor-ordered mutation accepted after the committed block fence.
///
/// Variants are intentionally append-only. The outer envelope carries an
/// explicit format version and repeats the redb key sequence so moving bytes
/// between keys is detected during restore.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum AcknowledgedWrite {
    DirectAdmit(RestingOrder),
    DeferredBundle(crate::sequencer::OrderSubmission),
    ControlPlane(ControlPlaneCommand),
    L1Deposit(L1Deposit),
    BridgeWithdrawal(BridgeWithdrawalRequest),
    BridgeL1Input(crate::bridge::BridgeL1Input),
    AuthenticatedDirectAdmit {
        resting: RestingOrder,
        nonce: u64,
        authorization: sybil_verifier::ClientActionAuth,
    },
    AuthenticatedDeferredBundle {
        submission: crate::sequencer::OrderSubmission,
        nonce: u64,
        authorization: sybil_verifier::ClientActionAuth,
    },
    AuthenticatedCancel {
        account_id: AccountId,
        order_id: u64,
        nonce: u64,
        authorization: sybil_verifier::ClientActionAuth,
        timestamp_ms: u64,
    },
}

impl AcknowledgedWrite {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::DirectAdmit(_) => "direct_admit",
            Self::DeferredBundle(_) => "deferred_bundle",
            Self::AuthenticatedDirectAdmit { .. } => "authenticated_direct_admit",
            Self::AuthenticatedDeferredBundle { .. } => "authenticated_deferred_bundle",
            Self::AuthenticatedCancel { .. } => "authenticated_cancel",
            Self::ControlPlane(_) => "control_plane",
            Self::L1Deposit(_) => "l1_deposit",
            Self::BridgeWithdrawal(_) => "bridge_withdrawal",
            Self::BridgeL1Input(_) => "bridge_l1_input",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SequencedAcknowledgedWrite {
    pub sequence: u64,
    pub write: AcknowledgedWrite,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct AcknowledgedWriteEnvelope {
    pub version: u8,
    pub sequence: u64,
    pub write: AcknowledgedWrite,
}

fn append_acknowledged_write_row(
    db: &Database,
    write: AcknowledgedWrite,
) -> Result<u64, StoreError> {
    let txn = db.begin_write()?;
    let sequence = {
        let mut counters = txn.open_table(COUNTERS)?;
        if counters.get(KEY_HEIGHT)?.is_none() {
            return Err(StoreError::AcknowledgedWriteBeforeSnapshot);
        }
        let floor = counters
            .get(KEY_ACKNOWLEDGED_WRITE_FLOOR)?
            .ok_or_else(|| {
                StoreError::CorruptLayout("missing acknowledged-write floor".to_string())
            })?
            .value();
        let next = counters
            .get(KEY_NEXT_ACKNOWLEDGED_WRITE_SEQ)?
            .ok_or_else(|| {
                StoreError::CorruptLayout("missing acknowledged-write next sequence".to_string())
            })?
            .value();
        if next < floor {
            return Err(StoreError::CorruptLayout(format!(
                "acknowledged-write next sequence {next} is below floor {floor}"
            )));
        }
        let following = next.checked_add(1).ok_or_else(|| {
            StoreError::CorruptLayout("acknowledged-write sequence exhausted".to_string())
        })?;
        counters.insert(KEY_NEXT_ACKNOWLEDGED_WRITE_SEQ, following)?;
        next
    };
    let envelope = AcknowledgedWriteEnvelope {
        version: ACKNOWLEDGED_WRITE_ENVELOPE_VERSION,
        sequence,
        write,
    };
    let bytes = rmp_serde::to_vec_named(&envelope)?;
    {
        let mut table = txn.open_table(ACKNOWLEDGED_WRITES)?;
        if table.get(sequence)?.is_some() {
            return Err(StoreError::CorruptLayout(format!(
                "acknowledged-write sequence {sequence} already exists"
            )));
        }
        table.insert(sequence, bytes.as_slice())?;
    }
    txn.commit()?;
    Ok(sequence)
}
