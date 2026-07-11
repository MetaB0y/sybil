//! Automated LLM resolution actor (SYB-48).
//!
//! Polls native markets whose `resolution_source` is `api_poll` and whose end
//! time has passed, fetches the configured endpoint, asks an LLM to judge the
//! outcome against the market's FULL resolution criteria, and — depending on the
//! model's confidence — routes the result through a confidence policy:
//!
//! - `>= confidence_propose` (default 0.9): sign a resolution attestation and
//!   hold it in a resolver-side pending queue for a challenge window (default
//!   24h). Post the proposal to sybil-api's review board so operators can see
//!   it and, if needed, reject it (a durable veto) or approve it (finalize
//!   early). When the window elapses with no veto, the resolver replays the
//!   signed attestation through the EXISTING `resolve_market_attested` money
//!   path — nothing here bypasses the oracle guards.
//! - `>= confidence_review` (default 0.7): queue for human review only. No
//!   attestation, nothing auto-finalizes.
//! - below that, or any parse/fetch failure: escalate (fail-closed).
//!
//! `manual` sources are never touched — they are operator workflows.
//!
//! Disabled by default; enable explicitly in deployment.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use sybil_api_types::SignedAttestationDto;
use sybil_api_types::request::{AutoResolutionActionDto, SubmitAutoResolutionRequest};
use sybil_client::SybilClient;

use crate::error::Error;
use crate::llm::{LlmClient, LlmRequest, LlmVerdict};
use crate::mapping::MappingStore;
use crate::native::{NativeMarketCatalog, NativeMarketSpec, ResolutionSourceConfig};
use crate::signer::ResolutionSigner;

/// Max bytes of fetched source content forwarded to the model. Bounds prompt
/// size (and cost) for pathologically large endpoints.
const MAX_SOURCE_CONTENT_BYTES: usize = 100_000;

/// Tunables for the auto-resolution actor. All windows are in the units named.
#[derive(Debug, Clone)]
pub struct AutoResolveConfig {
    /// Master switch. Default OFF — must be explicitly enabled in deployment.
    pub enabled: bool,
    /// Seconds between poll ticks.
    pub poll_interval_secs: u64,
    /// Confidence at/above which a signed proposal is queued (propose path).
    pub confidence_propose: f64,
    /// Confidence at/above which a market is queued for review (below propose).
    pub confidence_review: f64,
    /// Challenge window before a proposed resolution auto-finalizes. Milliseconds.
    pub challenge_window_ms: u64,
    /// Minimum seconds between fetches of the same endpoint (per-source limit).
    pub source_min_interval_secs: u64,
    /// Per-fetch HTTP timeout. Seconds.
    pub fetch_timeout_secs: u64,
    /// OpenRouter model id.
    pub model: String,
}

impl Default for AutoResolveConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            poll_interval_secs: 300,
            confidence_propose: 0.9,
            confidence_review: 0.7,
            challenge_window_ms: 24 * 60 * 60 * 1000,
            source_min_interval_secs: 300,
            fetch_timeout_secs: 30,
            model: "deepseek/deepseek-v4-flash".to_string(),
        }
    }
}

/// Confidence-policy classification of a parsed verdict (pure).
#[derive(Debug, Clone, PartialEq)]
pub enum Classification {
    /// High confidence → sign + propose with a challenge window.
    Propose,
    /// Mid confidence → review queue only.
    Review,
    /// Low confidence → escalate (fail-closed).
    Escalate,
}

impl AutoResolveConfig {
    /// Apply the confidence policy to a verdict. Pure and total.
    pub fn classify(&self, confidence: f64) -> Classification {
        self.classify_with_veto(confidence, false)
    }

    /// Apply the confidence policy, downgrading otherwise-proposable verdicts
    /// for vetoed markets to review-only. A durable market-level veto never
    /// creates a fresh signed pending item.
    pub fn classify_with_veto(&self, confidence: f64, vetoed: bool) -> Classification {
        if !confidence.is_finite() {
            return Classification::Escalate;
        }
        if confidence >= self.confidence_propose {
            if vetoed {
                Classification::Review
            } else {
                Classification::Propose
            }
        } else if confidence >= self.confidence_review {
            Classification::Review
        } else {
            Classification::Escalate
        }
    }
}

/// What to do with a pending (signed) proposal on a given tick. Pure.
#[derive(Debug, Clone, PartialEq)]
pub enum DueDecision {
    /// Replay the attestation through the resolve money path now.
    Finalize,
    /// Give up on this item (operator veto, or already resolved elsewhere).
    Drop,
    /// Keep waiting for the challenge window / operator action.
    Wait,
}

/// Whether a resolution source is eligible for automated polling. `manual`
/// sources are operator workflows and are never touched. Pure.
pub fn is_pollable(source: &ResolutionSourceConfig) -> bool {
    matches!(source, ResolutionSourceConfig::ApiPoll { .. })
}

/// Decide the fate of a pending proposal given the market's current review-board
/// status and the clock. Pure — the heart of the challenge-window mechanics.
///
/// - operator `rejected` (or an already `resolved` market) → Drop
/// - operator `approved` → Finalize immediately (early)
/// - otherwise finalize once the challenge window (`eta_ms`) has elapsed
pub fn due_decision(eta_ms: u64, entry_status: Option<&str>, now_ms: u64) -> DueDecision {
    match entry_status {
        Some("rejected") | Some("resolved") => DueDecision::Drop,
        Some("approved") => DueDecision::Finalize,
        _ if now_ms >= eta_ms => DueDecision::Finalize,
        _ => DueDecision::Wait,
    }
}

/// A signed proposal awaiting finalization, held resolver-side.
#[derive(Debug, Clone)]
struct PendingItem {
    sybil_market_id: u32,
    payout_nanos: u64,
    attestation: SignedAttestationDto,
    eta_ms: u64,
}

/// The auto-resolution actor.
pub struct AutoResolveActor {
    config: AutoResolveConfig,
    catalog: NativeMarketCatalog,
    mapping: Arc<RwLock<MappingStore>>,
    sybil: SybilClient,
    llm: Arc<dyn LlmClient>,
    signer: ResolutionSigner,
    http: reqwest::Client,
    /// Signed proposals awaiting finalization, keyed by sybil market id.
    pending: HashMap<u32, PendingItem>,
    /// Last successful fetch per endpoint (ms) for per-source rate limiting.
    last_fetch_ms: HashMap<String, u64>,
    /// Markets already submitted to the review board this process lifetime, so
    /// review/escalate markets are not re-evaluated every tick.
    submitted: HashMap<u32, ()>,
}

impl AutoResolveActor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: AutoResolveConfig,
        catalog: NativeMarketCatalog,
        mapping: Arc<RwLock<MappingStore>>,
        sybil: SybilClient,
        llm: Arc<dyn LlmClient>,
        signer: ResolutionSigner,
        http: reqwest::Client,
    ) -> Self {
        Self {
            config,
            catalog,
            mapping,
            sybil,
            llm,
            signer,
            http,
            pending: HashMap::new(),
            last_fetch_ms: HashMap::new(),
            submitted: HashMap::new(),
        }
    }

    pub async fn run(mut self, cancel: CancellationToken) {
        info!(
            poll_interval_secs = self.config.poll_interval_secs,
            confidence_propose = self.config.confidence_propose,
            confidence_review = self.config.confidence_review,
            challenge_window_ms = self.config.challenge_window_ms,
            model = %self.config.model,
            pubkey = self.signer.pubkey_hex(),
            "AutoResolveActor started; register this pubkey as a resolution feed on sybil-api"
        );

        let interval = Duration::from_secs(self.config.poll_interval_secs.max(1));
        loop {
            if let Err(e) = self.tick().await {
                warn!(error = %e, "auto-resolution tick failed");
            }
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("AutoResolveActor shutting down");
                    return;
                }
                _ = tokio::time::sleep(interval) => {}
            }
        }
    }

    async fn tick(&mut self) -> Result<(), Error> {
        let now_ms = now_ms();

        // One read of the review board per tick: authoritative operator
        // decisions + challenge-window etas that survive resolver restarts.
        let board: HashMap<u32, (String, Option<u64>)> = self
            .sybil
            .list_auto_resolutions()
            .await
            .unwrap_or_else(|e| {
                warn!(error = %e, "could not read auto-resolution board; continuing best-effort");
                Vec::new()
            })
            .into_iter()
            .map(|e| (e.market_id, (e.status, e.eta_ms)))
            .collect();

        let specs = self.catalog.enabled_market_specs();
        for spec in &specs {
            if let Err(e) = self.process_spec(spec, &board, now_ms).await {
                warn!(market_key = %spec.market_key, error = %e, "auto-resolution: market failed");
            }
        }
        Ok(())
    }

    async fn process_spec(
        &mut self,
        spec: &NativeMarketSpec,
        board: &HashMap<u32, (String, Option<u64>)>,
        now_ms: u64,
    ) -> Result<(), Error> {
        // Manual sources are operator workflows: never touched.
        if !is_pollable(spec.resolution_source()) {
            return Ok(());
        }
        let (endpoint, method) = match spec.resolution_source() {
            ResolutionSourceConfig::Manual { .. } => return Ok(()),
            ResolutionSourceConfig::ApiPoll {
                endpoint, method, ..
            } => (endpoint.clone(), method.clone()),
        };

        // Only act after the market's stated end time.
        if now_ms < spec.end_time_ms {
            return Ok(());
        }

        // Map to the live sybil market id; unmapped markets are not created yet.
        let sybil_market_id = {
            let mapping = self.mapping.read().await;
            match mapping.native_market_id(&spec.market_key) {
                Some(id) => id,
                None => return Ok(()),
            }
        };

        // Already settled on-chain? Nothing to do; clear any local pending item.
        if let Ok(Some(res)) = self.sybil.get_market_resolution(sybil_market_id).await
            && res.status == "resolved"
        {
            self.pending.remove(&sybil_market_id);
            return Ok(());
        }

        let entry = board.get(&sybil_market_id);
        let entry_status = entry.map(|(s, _)| s.as_str());
        let vetoed = entry_status == Some("rejected");

        // Durable operator veto: any held attestation is dead. We still allow
        // a fresh evidence pass below, but route it as review-only.
        if vetoed {
            self.pending.remove(&sybil_market_id);
        }

        // Finalization path: a signed proposal we are already holding.
        if let Some(item) = self.pending.get(&sybil_market_id).cloned() {
            match due_decision(item.eta_ms, entry_status, now_ms) {
                DueDecision::Finalize => {
                    self.sybil
                        .resolve_market_attested(
                            item.sybil_market_id,
                            item.payout_nanos,
                            item.attestation.clone(),
                        )
                        .await?;
                    self.pending.remove(&sybil_market_id);
                    info!(
                        sybil_market_id,
                        payout_nanos = item.payout_nanos,
                        "auto-resolution finalized via signed attestation"
                    );
                }
                DueDecision::Drop => {
                    self.pending.remove(&sybil_market_id);
                }
                DueDecision::Wait => {}
            }
            return Ok(());
        }

        // Already submitted a terminal-ish review/escalate this lifetime, and the
        // board still shows it — don't re-run the LLM every tick.
        let already_reviewed = matches!(entry_status, Some("needs_review") | Some("escalated"))
            || self.submitted.contains_key(&sybil_market_id);
        // A pending/approved board entry with no local item means we restarted
        // and lost the signed attestation: re-evaluate to re-sign.
        let needs_resign = matches!(entry_status, Some("pending") | Some("approved"));
        if already_reviewed && !needs_resign {
            return Ok(());
        }

        // Per-source rate limit.
        if let Some(&last) = self.last_fetch_ms.get(&endpoint)
            && now_ms.saturating_sub(last) < self.config.source_min_interval_secs * 1000
        {
            debug!(sybil_market_id, %endpoint, "rate-limited; skipping this tick");
            return Ok(());
        }

        // Fetch → evaluate. Any failure fails closed to an escalation.
        let content = match self.fetch_source(&endpoint, method.as_deref()).await {
            Ok(content) => {
                self.last_fetch_ms.insert(endpoint.clone(), now_ms);
                content
            }
            Err(e) => {
                warn!(sybil_market_id, %endpoint, error = %e, "fetch failed; escalating");
                self.submit_escalation(sybil_market_id, format!("source fetch failed: {e}"))
                    .await?;
                return Ok(());
            }
        };

        let req = LlmRequest {
            question: spec.resolution_question(),
            resolution_criteria: spec.resolution_criteria().to_string(),
            source_content: content,
        };

        let verdict = match self.llm.evaluate(&req).await {
            Ok(v) => v,
            Err(e) => {
                warn!(sybil_market_id, error = %e, "LLM verdict unusable; escalating (fail-closed)");
                self.submit_escalation(sybil_market_id, format!("llm/parse failure: {e}"))
                    .await?;
                return Ok(());
            }
        };

        self.route_verdict(
            sybil_market_id,
            &verdict,
            entry.and_then(|(_, eta)| *eta),
            now_ms,
            vetoed,
        )
        .await
    }

    /// Apply the confidence policy to a verdict and act on it.
    async fn route_verdict(
        &mut self,
        sybil_market_id: u32,
        verdict: &LlmVerdict,
        existing_eta_ms: Option<u64>,
        now_ms: u64,
        vetoed: bool,
    ) -> Result<(), Error> {
        match self.config.classify_with_veto(verdict.confidence, vetoed) {
            Classification::Propose => {
                // Preserve any in-flight challenge window; else open a fresh one.
                let eta_ms = existing_eta_ms.unwrap_or(now_ms + self.config.challenge_window_ms);
                let attestation =
                    self.signer
                        .sign_attestation(sybil_market_id, verdict.payout_nanos, now_ms);

                let stored = self
                    .sybil
                    .submit_auto_resolution(&self.submission(
                        sybil_market_id,
                        AutoResolutionActionDto::Propose,
                        verdict,
                        Some(eta_ms),
                    ))
                    .await?;

                // Trust the board's eta (it wins on restart / re-propose).
                let eta_ms = stored.eta_ms.unwrap_or(eta_ms);
                self.pending.insert(
                    sybil_market_id,
                    PendingItem {
                        sybil_market_id,
                        payout_nanos: verdict.payout_nanos,
                        attestation,
                        eta_ms,
                    },
                );
                self.submitted.insert(sybil_market_id, ());
                info!(
                    sybil_market_id,
                    payout_nanos = verdict.payout_nanos,
                    confidence = verdict.confidence,
                    eta_ms,
                    "auto-resolution proposed; holding through challenge window"
                );
            }
            Classification::Review => {
                self.sybil
                    .submit_auto_resolution(&self.submission(
                        sybil_market_id,
                        AutoResolutionActionDto::Review,
                        verdict,
                        None,
                    ))
                    .await?;
                self.submitted.insert(sybil_market_id, ());
                info!(
                    sybil_market_id,
                    confidence = verdict.confidence,
                    "auto-resolution queued for human review"
                );
            }
            Classification::Escalate => {
                self.sybil
                    .submit_auto_resolution(&self.submission(
                        sybil_market_id,
                        AutoResolutionActionDto::Escalate,
                        verdict,
                        None,
                    ))
                    .await?;
                self.submitted.insert(sybil_market_id, ());
                info!(
                    sybil_market_id,
                    confidence = verdict.confidence,
                    "auto-resolution escalated (low confidence)"
                );
            }
        }
        Ok(())
    }

    fn submission(
        &self,
        market_id: u32,
        action: AutoResolutionActionDto,
        verdict: &LlmVerdict,
        eta_ms: Option<u64>,
    ) -> SubmitAutoResolutionRequest {
        SubmitAutoResolutionRequest {
            market_id,
            action,
            payout_nanos: verdict.payout_nanos,
            confidence: verdict.confidence,
            reasoning: verdict.reasoning.clone(),
            evidence_excerpts: verdict.evidence_excerpts.clone(),
            eta_ms,
        }
    }

    async fn submit_escalation(&self, market_id: u32, reason: String) -> Result<(), Error> {
        self.sybil
            .submit_auto_resolution(&SubmitAutoResolutionRequest {
                market_id,
                action: AutoResolutionActionDto::Escalate,
                // No trustworthy outcome; record a neutral placeholder payout.
                payout_nanos: 0,
                confidence: 0.0,
                reasoning: reason,
                evidence_excerpts: Vec::new(),
                eta_ms: None,
            })
            .await?;
        Ok(())
    }

    async fn fetch_source(&self, endpoint: &str, method: Option<&str>) -> Result<String, Error> {
        let timeout = Duration::from_secs(self.config.fetch_timeout_secs.max(1));
        let builder = match method.map(|m| m.trim().to_ascii_uppercase()).as_deref() {
            Some("POST") => self.http.post(endpoint),
            _ => self.http.get(endpoint),
        };
        let resp = builder.timeout(timeout).send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::SybilApi { status, body });
        }
        let mut text = resp.text().await?;
        if text.len() > MAX_SOURCE_CONTENT_BYTES {
            text.truncate(MAX_SOURCE_CONTENT_BYTES);
        }
        Ok(text)
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> AutoResolveConfig {
        AutoResolveConfig::default()
    }

    #[test]
    fn classify_follows_confidence_policy() {
        let c = cfg();
        assert_eq!(c.classify(0.95), Classification::Propose);
        assert_eq!(c.classify(0.90), Classification::Propose);
        assert_eq!(c.classify(0.80), Classification::Review);
        assert_eq!(c.classify(0.70), Classification::Review);
        assert_eq!(c.classify(0.69), Classification::Escalate);
        assert_eq!(c.classify(0.0), Classification::Escalate);
        assert_eq!(c.classify(f64::NAN), Classification::Escalate);
    }

    #[test]
    fn window_finalizes_after_elapse() {
        // No operator action, window not elapsed → Wait; then elapsed → Finalize.
        assert_eq!(due_decision(1_000, None, 999), DueDecision::Wait);
        assert_eq!(
            due_decision(1_000, Some("pending"), 1_000),
            DueDecision::Finalize
        );
    }

    #[test]
    fn operator_reject_cancels_window() {
        // Even before the window elapses, a reject drops the pending item.
        assert_eq!(due_decision(10_000, Some("rejected"), 1), DueDecision::Drop);
    }

    #[test]
    fn operator_approve_finalizes_early() {
        assert_eq!(
            due_decision(10_000, Some("approved"), 1),
            DueDecision::Finalize
        );
    }

    #[test]
    fn resolved_elsewhere_drops() {
        assert_eq!(due_decision(1, Some("resolved"), 10), DueDecision::Drop);
    }

    #[test]
    fn manual_sources_are_not_pollable() {
        assert!(!is_pollable(&ResolutionSourceConfig::Manual {
            instructions: "read the page".into(),
        }));
        assert!(is_pollable(&ResolutionSourceConfig::ApiPoll {
            endpoint: "https://example.com/api".into(),
            method: None,
            notes: "poll it".into(),
        }));
    }

    // --- Fixture tests driving the deterministic MockLlm (no network). ---

    use crate::llm::{LlmClient, LlmRequest, MockLlm};
    use sybil_api_types::NANOS_PER_DOLLAR;

    fn sample_req() -> LlmRequest {
        LlmRequest {
            question: "Will X happen?".into(),
            resolution_criteria: "Resolve YES if X happened.".into(),
            source_content: "X happened.".into(),
        }
    }

    #[tokio::test]
    async fn high_confidence_yes_flows_to_propose() {
        let mock = MockLlm::verdict(1.0, 0.95);
        let verdict = mock.evaluate(&sample_req()).await.unwrap();
        assert_eq!(verdict.payout_nanos, NANOS_PER_DOLLAR);
        assert_eq!(cfg().classify(verdict.confidence), Classification::Propose);
    }

    #[tokio::test]
    async fn vetoed_high_confidence_repoll_flows_to_review_queue() {
        let mock = MockLlm::verdict(1.0, 0.95);
        let verdict = mock.evaluate(&sample_req()).await.unwrap();
        assert_eq!(
            cfg().classify_with_veto(verdict.confidence, true),
            Classification::Review
        );
    }

    #[tokio::test]
    async fn mid_confidence_flows_to_review_queue() {
        let mock = MockLlm::verdict(1.0, 0.8);
        let verdict = mock.evaluate(&sample_req()).await.unwrap();
        assert_eq!(cfg().classify(verdict.confidence), Classification::Review);
    }

    #[tokio::test]
    async fn garbled_json_fails_closed_to_escalate() {
        // The model returns prose, not strict JSON: evaluate() errors, which the
        // actor turns into an escalation (never a resolution).
        let mock = MockLlm::raw("I think the answer is probably yes.");
        assert!(mock.evaluate(&sample_req()).await.is_err());
    }
}
