//! Distrustful user custody primitives. The binary is intentionally a thin
//! shell over these testable snapshot, reconstruction, proof, and ABI pieces.

pub mod abi;
pub mod api;
pub mod claim;
pub mod format;
pub mod reconstruct;
pub mod rpc;

pub use format::{CustodyManifest, CustodySnapshot, RootRecord};
