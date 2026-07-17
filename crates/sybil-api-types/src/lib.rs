//! Shared request/response types for the Sybil prediction market API.
//!
//! This crate is the single source of truth for API DTOs. Both
//! `sybil-api` (the server) and `sybil-polymarket` (a client) depend on it.
//! Rust DTOs keep `*_nanos` fields as integers, while their JSON wire form is
//! an exact decimal string so JavaScript clients never cross the `2^53 - 1`
//! safe-integer boundary.
//!
//! Enable the `openapi` feature for utoipa `ToSchema` derives (used by sybil-api).

pub mod request;
pub mod response;
mod wire_integer;
pub mod ws;

pub use request::*;
pub use response::*;
pub use ws::*;

/// 1 dollar = 1,000,000,000 nanos.
pub const NANOS_PER_DOLLAR: u64 = 1_000_000_000;
