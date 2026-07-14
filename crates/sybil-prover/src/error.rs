use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ProverCliError {
    #[error("open {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("read MessagePack proof job from {path}: {source}")]
    DecodeJob {
        path: PathBuf,
        #[source]
        source: rmp_serde::decode::Error,
    },
    #[error("read MessagePack guest input from {path}: {source}")]
    DecodeGuestInput {
        path: PathBuf,
        #[source]
        source: rmp_serde::decode::Error,
    },
    #[error("encode MessagePack artifact for {path}: {source}")]
    Encode {
        path: PathBuf,
        #[source]
        source: rmp_serde::encode::Error,
    },
    #[error("write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("create directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("list directory {path}: {source}")]
    ListDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("read OpenVM EVM proof JSON from {path}: {source}")]
    DecodeOpenVmEvmProof {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("read JSON artifact from {path}: {source}")]
    DecodeJson {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("decode hex field {field}: {source}")]
    DecodeHex {
        field: &'static str,
        #[source]
        source: hex::FromHexError,
    },
    #[error("field {field} must be 32 bytes, got {actual}")]
    InvalidBytes32Field { field: &'static str, actual: usize },
    #[error("proof file is empty: {path}")]
    EmptyProof { path: PathBuf },
    #[error("--from is required when --rpc-request is set")]
    MissingRpcRequestFrom,
    #[error("encode JSON artifact for {path}: {source}")]
    EncodeJson {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("bind proof API listener at {addr}: {source}")]
    Bind {
        addr: String,
        #[source]
        source: std::io::Error,
    },
    #[error("serve proof API: {source}")]
    Serve {
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    ProofJob(#[from] crate::ProofJobError),
    #[error(transparent)]
    Daemon(#[from] crate::daemon::DaemonError),
    #[cfg(feature = "sequencer-store")]
    #[error(transparent)]
    Witgen(#[from] crate::witgen_cli::WitgenCliError),
    #[error("verify prepared guest input: {0}")]
    ZkTransition(#[from] sybil_zk::ZkTransitionError),
}
