//! THE Rust HTTP client for the Sybil API.
//!
//! This crate is the single, shared client used by every in-tree Rust consumer
//! of `sybil-api` — the Polymarket mirror/market-maker (`sybil-polymarket`) and
//! the admin CLI (`sybil-admin`). It supersedes the two hand-written clients
//! that previously drifted independently (SYB-171): do not add a third.
//!
//! It is typed against [`sybil_api_types`] (the shared DTO crate) and covers the
//! union of both former surfaces: health, accounts, markets, market groups,
//! resolution, orders, off-block metadata/snapshots/reference prices, and the
//! SSE block stream. The Python SDK (`arena/`) is intentionally separate.

mod client;
mod error;

pub use client::SybilClient;
pub use error::Error;
