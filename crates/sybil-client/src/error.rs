/// Errors surfaced by [`crate::SybilClient`].
///
/// The client keeps the raw wire surface intact: HTTP-level failures the server
/// reports (non-2xx) are returned as [`Error::Api`] carrying the exact status
/// code and response body, so callers can parse structured error bodies (e.g.
/// the polymarket MM's poisoned-market detection) without the client second
/// guessing them. Transport/decode failures are [`Error::Http`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The server responded with a non-success status. `body` is the raw,
    /// undecoded response body so callers can inspect structured error payloads.
    #[error("Sybil API error (HTTP {status}): {body}")]
    Api { status: u16, body: String },

    /// A transport-level or response-decoding failure from `reqwest`.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// A WebSocket transport failure while connecting to or reading the block
    /// stream.
    #[error("WebSocket error: {0}")]
    WebSocket(String),

    /// A JSON decoding failure for the WebSocket block stream envelope.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// The WebSocket stream sent an unsupported or malformed protocol message.
    #[error("block stream protocol error: {0}")]
    Protocol(String),

    /// The attestation response cannot be assigned the requested trust level.
    #[error("attestation verification error: {0}")]
    Attestation(String),

    /// The server-side broadcast buffer overflowed. Reconnect with
    /// `last_sent_height + 1` when present, or cold-resync if the client never
    /// received a block on this connection.
    #[error("block stream lagged: skipped {skipped} blocks, last_sent_height={last_sent_height:?}")]
    BlockStreamLagged {
        skipped: u64,
        last_sent_height: Option<u64>,
    },

    /// Requested replay starts before the retained `blocks_full` floor.
    #[error(
        "block stream retention gap: requested {requested_height}, retention_min_height={retention_min_height}, head_height={head_height}"
    )]
    RetentionGap {
        requested_height: u64,
        retention_min_height: u64,
        head_height: u64,
    },
}
