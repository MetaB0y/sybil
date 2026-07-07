use super::*;

pub const DA_PAYLOAD_KIND: &str = "block_witness";
pub const DA_PAYLOAD_ENCODING: &str = "sybil-canonical-witness-v3";
pub const DA_PROVIDER_REFS_ENCODING_BYTES: &str = "bytes-v1";
pub const DA_FILE_PROVIDER_REF_KIND: &str = "file";
pub const DA_FILE_PROVIDER_REF_ENCODING: &str = "sybil-da-file-ref-v1";

const FILE_DA_PROVIDER_REF_DOMAIN: &[u8] = b"sybil/da/provider-ref/file/v1";

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct DaProviderRef {
    pub kind: String,
    pub encoding: String,
    pub bytes: Vec<u8>,
    pub uri: Option<String>,
    pub payload_root: Option<[u8; 32]>,
    pub payload_len: Option<u64>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct DaArtifactManifest {
    pub version: u8,
    pub payload_kind: String,
    pub payload_encoding: String,
    pub provider_refs_encoding: String,
    pub height: u64,
    pub block_hash: [u8; 32],
    pub state_root: [u8; 32],
    pub witness_root: [u8; 32],
    pub payload_root: [u8; 32],
    pub payload_len: u64,
    pub provider_refs_hash: [u8; 32],
    pub provider_refs: Vec<DaProviderRef>,
    pub da_commitment: [u8; 32],
    pub public_input_hash: [u8; 32],
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct DaArtifact {
    pub manifest: DaArtifactManifest,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaArtifactLookup {
    pub artifact: Option<DaArtifact>,
    pub oldest_retained_height: Option<u64>,
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum DaArtifactIntegrityError {
    #[error("DA artifact payload length mismatch at height {height}: manifest={expected}, bytes={actual}")]
    PayloadLenMismatch {
        height: u64,
        expected: u64,
        actual: u64,
    },
    #[error("DA artifact payload_root mismatch at height {height}")]
    PayloadRootMismatch {
        height: u64,
        expected: [u8; 32],
        actual: [u8; 32],
    },
}

impl DaArtifact {
    pub fn from_witness(witness: &BlockWitness) -> Self {
        let payload = sybil_zk::da_witness_payload_bytes(witness);
        let payload_root = sybil_zk::da_witness_payload_root(&payload);
        let payload_len = payload.len() as u64;
        let provider_ref = file_da_provider_ref(payload_root, payload_len);
        let provider_refs = vec![provider_ref.bytes.clone()];
        let components = sybil_zk::da_commitment_components_from_payload_and_provider_refs(
            witness,
            &payload,
            &provider_refs,
        );
        let public_inputs =
            sybil_zk::public_inputs_from_witness_and_provider_refs(witness, &provider_refs);
        let public_input_hash = sybil_zk::state_transition_public_input_hash(&public_inputs);

        Self {
            manifest: DaArtifactManifest {
                version: 1,
                payload_kind: DA_PAYLOAD_KIND.to_string(),
                payload_encoding: DA_PAYLOAD_ENCODING.to_string(),
                provider_refs_encoding: DA_PROVIDER_REFS_ENCODING_BYTES.to_string(),
                height: components.block_height,
                block_hash: sybil_zk::hash_header(&witness.header),
                state_root: components.state_root,
                witness_root: components.witness_root,
                payload_root: components.payload_root,
                payload_len: components.payload_len,
                provider_refs_hash: components.provider_refs_hash,
                provider_refs: vec![provider_ref],
                da_commitment: components.da_commitment,
                public_input_hash,
            },
            payload,
        }
    }

    pub fn verify_payload_integrity(&self) -> Result<(), DaArtifactIntegrityError> {
        let actual_len = self.payload.len() as u64;
        if actual_len != self.manifest.payload_len {
            return Err(DaArtifactIntegrityError::PayloadLenMismatch {
                height: self.manifest.height,
                expected: self.manifest.payload_len,
                actual: actual_len,
            });
        }

        let actual_root = sybil_zk::da_witness_payload_root(&self.payload);
        if actual_root != self.manifest.payload_root {
            return Err(DaArtifactIntegrityError::PayloadRootMismatch {
                height: self.manifest.height,
                expected: self.manifest.payload_root,
                actual: actual_root,
            });
        }
        Ok(())
    }
}

fn file_da_provider_ref(payload_root: [u8; 32], payload_len: u64) -> DaProviderRef {
    let uri = format!(
        "sybil-file://witness/{}.witness.bin",
        hex::encode(payload_root)
    );
    let bytes = file_da_provider_ref_bytes(&uri, payload_root, payload_len);
    DaProviderRef {
        kind: DA_FILE_PROVIDER_REF_KIND.to_string(),
        encoding: DA_FILE_PROVIDER_REF_ENCODING.to_string(),
        bytes,
        uri: Some(uri),
        payload_root: Some(payload_root),
        payload_len: Some(payload_len),
    }
}

fn file_da_provider_ref_bytes(uri: &str, payload_root: [u8; 32], payload_len: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(
        FILE_DA_PROVIDER_REF_DOMAIN.len() + 8 + uri.len() + payload_root.len() + 8,
    );
    bytes.extend_from_slice(FILE_DA_PROVIDER_REF_DOMAIN);
    bytes.extend_from_slice(&(uri.len() as u64).to_le_bytes());
    bytes.extend_from_slice(uri.as_bytes());
    bytes.extend_from_slice(&payload_root);
    bytes.extend_from_slice(&payload_len.to_le_bytes());
    bytes
}

impl Store {
    /// Persist a DA serving artifact after the block commit has succeeded.
    ///
    /// This is intentionally separate from `save_block_inner`: DA availability
    /// gaps should be observable and alertable, but they must not roll back an
    /// otherwise committed block.
    pub async fn save_da_artifact(&self, artifact: DaArtifact) -> Result<bool, StoreError> {
        self.redb_write(move |db| {
            let txn = db.begin_write()?;
            let retained_floor = {
                let meta = txn.open_table(HISTORY_META)?;
                let retained_floor = meta
                    .get(KEY_BLOCKS_FULL_MIN_HEIGHT)?
                    .map(|value| value.value());
                retained_floor
            };
            if retained_floor.is_some_and(|floor| artifact.manifest.height < floor) {
                txn.commit()?;
                return Ok(false);
            }

            let bytes = rmp_serde::to_vec(&artifact)?;
            {
                let mut table = txn.open_table(DA_ARTIFACTS)?;
                table.insert(artifact.manifest.height, bytes.as_slice())?;
            }
            txn.commit()?;
            Ok(true)
        })
        .await
    }

    /// Load a retained DA artifact by block height.
    pub async fn load_da_artifact(&self, height: u64) -> Result<Option<DaArtifact>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(DA_ARTIFACTS)?;
        table
            .get(height)?
            .map(|value| rmp_serde::from_slice(value.value()))
            .transpose()
            .map_err(StoreError::from)
    }
}
