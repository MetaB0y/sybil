use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Polymarket API error: {0}")]
    PolymarketApi(String),

    #[error("Sybil API error (HTTP {status}): {body}")]
    SybilApi { status: u16, body: String },

    #[error("WebSocket error: {0}")]
    WebSocket(String),

    #[error("SSE stream error: {0}")]
    Sse(String),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Mapping error: {0}")]
    Mapping(String),

    #[error("Native market catalog error: {0}")]
    NativeCatalog(String),

    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Channel send error: {0}")]
    Channel(String),
}

impl From<sybil_client::Error> for Error {
    fn from(err: sybil_client::Error) -> Self {
        match err {
            // Preserve the raw status + body so downstream parsing (e.g. the MM's
            // poisoned-market-from-400-body detection) keeps working unchanged.
            sybil_client::Error::Api { status, body } => Self::SybilApi { status, body },
            sybil_client::Error::Http(err) => Self::Http(err),
            sybil_client::Error::Json(err) => Self::Json(err),
            sybil_client::Error::WebSocket(message)
            | sybil_client::Error::Protocol(message) => Self::WebSocket(message),
            sybil_client::Error::BlockStreamLagged {
                skipped,
                last_sent_height,
            } => Self::WebSocket(format!(
                "block stream lagged: skipped {skipped}, last_sent_height={last_sent_height:?}"
            )),
            sybil_client::Error::RetentionGap {
                requested_height,
                retention_min_height,
                head_height,
            } => Self::WebSocket(format!(
                "block stream retention gap: requested {requested_height}, retention_min_height={retention_min_height}, head_height={head_height}"
            )),
        }
    }
}

impl Error {
    pub fn is_expected_websocket_disconnect(&self) -> bool {
        match self {
            Self::WebSocket(message) => {
                let message = message.to_ascii_lowercase();
                message.contains("without closing handshake")
                    || message.contains("connection reset")
                    || message.contains("broken pipe")
            }
            _ => false,
        }
    }
}
