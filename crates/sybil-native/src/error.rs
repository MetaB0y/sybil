#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("native market catalog error: {0}")]
    NativeCatalog(String),
    #[error("native deployment error: {0}")]
    Deployment(String),
    #[error("Sybil API error: {0}")]
    Sybil(#[from] sybil_client::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
