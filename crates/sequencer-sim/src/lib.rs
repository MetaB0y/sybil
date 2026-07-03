//! Agent-based multi-batch simulation harness for the prediction-market sequencer.
//!
//! This crate is a **dev-only** simulation driver: it depends on
//! [`matching_sequencer`] and exercises the real [`BlockSequencer`] over many
//! batches with synthetic agents (informed / noise / market-maker). It is
//! deliberately kept out of the `matching-sequencer` library so that
//! production consumers (e.g. `sybil-api`) never compile the simulation code.
//!
//! [`BlockSequencer`]: matching_sequencer::sequencer::BlockSequencer

pub mod agent;
pub mod metrics;
pub mod scenario;
pub mod simulation;

pub use scenario::Scenario;
pub use simulation::{SimulationResult, SimulationRunner};
