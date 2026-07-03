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
}
