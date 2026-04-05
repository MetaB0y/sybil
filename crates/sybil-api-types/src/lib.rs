//! Shared request/response types for the Sybil prediction market API.
//!
//! This crate is the single source of truth for API DTOs. Both
//! `sybil-api` (the server) and `sybil-polymarket` (a client) depend on it.
//!
//! Enable the `openapi` feature for utoipa `ToSchema` derives (used by sybil-api).

pub mod request;
pub mod response;

pub use request::*;
pub use response::*;

/// 1 dollar = 1,000,000,000 nanos.
pub const NANOS_PER_DOLLAR: u64 = 1_000_000_000;
