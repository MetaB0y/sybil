//! In-memory review board for automated LLM resolutions (SYB-48).
//!
//! This store is deliberately NOT a settlement path. It records the proposals
//! that the auto-resolution resolver (sybil-polymarket) produces so operators
//! can inspect and gate them. The actual money path is unchanged: a `propose`
//! entry only ever settles when the resolver replays its own signed attestation
//! through `POST /v1/markets/{id}/resolve`, which runs every existing oracle
//! guard. Operators steer that by approving (finalize sooner) or rejecting
//! (never finalize) an entry here; the resolver polls those decisions.
//!
//! A `reject` is a durable, terminal veto: once set it survives resolver
//! restarts and is never silently overwritten, so an operator's "no" cannot be
//! undone by the resolver re-proposing the same market.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sybil_api_types::request::{AutoResolutionActionDto, SubmitAutoResolutionRequest};

/// Operator decision recorded against a proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Operator approved: finalize as soon as the resolver next polls.
    Approved,
    /// Operator rejected: terminal veto, never finalizes.
    Rejected,
}

/// A single recorded proposal plus any operator decision.
#[derive(Debug, Clone)]
pub struct AutoResolutionEntry {
    pub market_id: u32,
    pub action: AutoResolutionActionDto,
    pub payout_nanos: u64,
    pub confidence: f64,
    pub reasoning: String,
    pub evidence_excerpts: Vec<String>,
    pub proposed_at_ms: u64,
    pub eta_ms: Option<u64>,
    pub decision: Option<Decision>,
    pub decided_at_ms: Option<u64>,
}

impl AutoResolutionEntry {
    /// Whether this entry is in a terminal state that the resolver must not
    /// overwrite with a fresh proposal (an operator veto). `Approved` is NOT
    /// terminal here: it still needs the resolver to replay the attestation.
    fn is_vetoed(&self) -> bool {
        self.decision == Some(Decision::Rejected)
    }
}

/// Thread-safe review board keyed by market id.
#[derive(Clone, Default)]
pub struct AutoResolutionStore {
    inner: Arc<Mutex<HashMap<u32, AutoResolutionEntry>>>,
}

impl AutoResolutionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record or refresh a proposal. Preserves `proposed_at_ms`/`eta_ms` and any
    /// existing operator decision when the market already has an entry, and
    /// refuses to disturb a rejected (vetoed) entry. Returns the resulting
    /// stored entry so the resolver learns the authoritative eta/decision.
    pub fn upsert(&self, req: &SubmitAutoResolutionRequest, now_ms: u64) -> AutoResolutionEntry {
        let mut guard = self.inner.lock().expect("auto-resolution store poisoned");
        match guard.get_mut(&req.market_id) {
            Some(existing) if existing.is_vetoed() => existing.clone(),
            Some(existing) => {
                existing.action = req.action;
                existing.payout_nanos = req.payout_nanos;
                existing.confidence = req.confidence;
                existing.reasoning = req.reasoning.clone();
                existing.evidence_excerpts = req.evidence_excerpts.clone();
                // Keep the original window: re-proposing must not extend or
                // shorten an in-flight challenge window.
                if existing.eta_ms.is_none() {
                    existing.eta_ms = req.eta_ms;
                }
                existing.clone()
            }
            None => {
                let entry = AutoResolutionEntry {
                    market_id: req.market_id,
                    action: req.action,
                    payout_nanos: req.payout_nanos,
                    confidence: req.confidence,
                    reasoning: req.reasoning.clone(),
                    evidence_excerpts: req.evidence_excerpts.clone(),
                    proposed_at_ms: now_ms,
                    eta_ms: req.eta_ms,
                    decision: None,
                    decided_at_ms: None,
                };
                guard.insert(req.market_id, entry.clone());
                entry
            }
        }
    }

    /// Record an operator decision. Returns the updated entry, or `None` if the
    /// market has no proposal on the board.
    pub fn decide(
        &self,
        market_id: u32,
        decision: Decision,
        now_ms: u64,
    ) -> Option<AutoResolutionEntry> {
        let mut guard = self.inner.lock().expect("auto-resolution store poisoned");
        let entry = guard.get_mut(&market_id)?;
        entry.decision = Some(decision);
        entry.decided_at_ms = Some(now_ms);
        Some(entry.clone())
    }

    /// Snapshot every recorded proposal.
    pub fn list(&self) -> Vec<AutoResolutionEntry> {
        let guard = self.inner.lock().expect("auto-resolution store poisoned");
        guard.values().cloned().collect()
    }

    pub fn get(&self, market_id: u32) -> Option<AutoResolutionEntry> {
        let guard = self.inner.lock().expect("auto-resolution store poisoned");
        guard.get(&market_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn propose_req(id: u32, eta: Option<u64>, confidence: f64) -> SubmitAutoResolutionRequest {
        SubmitAutoResolutionRequest {
            market_id: id,
            action: AutoResolutionActionDto::Propose,
            payout_nanos: 1_000_000_000,
            confidence,
            reasoning: "clear yes".into(),
            evidence_excerpts: vec!["evidence".into()],
            eta_ms: eta,
        }
    }

    fn upsert_propose(
        store: &AutoResolutionStore,
        id: u32,
        eta: Option<u64>,
    ) -> AutoResolutionEntry {
        store.upsert(&propose_req(id, eta, 0.95), 1_000)
    }

    #[test]
    fn upsert_preserves_window_and_proposed_at() {
        let store = AutoResolutionStore::new();
        let first = upsert_propose(&store, 7, Some(90_000));
        assert_eq!(first.eta_ms, Some(90_000));
        assert_eq!(first.proposed_at_ms, 1_000);

        // Re-propose with a different eta later: original window is kept.
        let again = store.upsert(&propose_req(7, Some(500_000), 0.99), 5_000);
        assert_eq!(again.eta_ms, Some(90_000));
        assert_eq!(again.proposed_at_ms, 1_000);
        assert_eq!(again.confidence, 0.99);
    }

    #[test]
    fn reject_is_a_durable_veto() {
        let store = AutoResolutionStore::new();
        upsert_propose(&store, 7, Some(90_000));
        let rejected = store.decide(7, Decision::Rejected, 2_000).unwrap();
        assert_eq!(rejected.decision, Some(Decision::Rejected));

        // A subsequent re-proposal must not clear the veto.
        let after = upsert_propose(&store, 7, Some(90_000));
        assert_eq!(after.decision, Some(Decision::Rejected));
    }

    #[test]
    fn approve_is_recorded_but_not_terminal_for_overwrite() {
        let store = AutoResolutionStore::new();
        upsert_propose(&store, 7, Some(90_000));
        let approved = store.decide(7, Decision::Approved, 2_000).unwrap();
        assert_eq!(approved.decision, Some(Decision::Approved));
        assert_eq!(approved.decided_at_ms, Some(2_000));
    }

    #[test]
    fn decide_missing_market_is_none() {
        let store = AutoResolutionStore::new();
        assert!(store.decide(42, Decision::Approved, 1).is_none());
    }
}
