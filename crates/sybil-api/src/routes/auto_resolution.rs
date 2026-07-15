//! Operator review board for automated LLM resolutions (SYB-48).
//!
//! Service-gated, mounted alongside the other admin write routes. These routes
//! are metadata only — none of them settle a market. A `propose` entry is
//! finalized by the resolver replaying its signed attestation through the
//! existing `POST /v1/markets/{id}/resolve` money path; approving/rejecting here
//! merely steers whether/when that replay happens.

use axum::Json;
use axum::extract::{Path, State};
use matching_engine::MarketId;

use crate::auto_resolution::Decision;
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::request::{AutoResolutionActionDto, SubmitAutoResolutionRequest};
use crate::types::response::{AutoResolutionEntryResponse, AutoResolutionListResponse};
use sybil_api_types::NANOS_PER_DOLLAR;

async fn rehydrate(state: &AppState) -> Result<(), AppError> {
    state.rehydrate_auto_resolutions().await?;
    Ok(())
}

async fn persist(
    state: &AppState,
    entry: &crate::auto_resolution::AutoResolutionEntry,
) -> Result<(), AppError> {
    state
        .sequencer
        .put_auto_resolution_record(entry.to_record())
        .await?;
    Ok(())
}

/// Compute the display status for an entry, folding in the live on-chain
/// resolution state so the board never shows a settled market as still pending.
async fn display_status(
    state: &AppState,
    entry: &crate::auto_resolution::AutoResolutionEntry,
) -> String {
    if let Ok(status) = state
        .sequencer
        .get_market_status(MarketId::new(entry.market_id))
        .await
        && status.as_str() == "resolved"
    {
        return "resolved".to_string();
    }
    match entry.decision {
        Some(Decision::Rejected) => "rejected".to_string(),
        Some(Decision::Approved) => "approved".to_string(),
        None => match entry.action {
            AutoResolutionActionDto::Propose => "pending".to_string(),
            AutoResolutionActionDto::Review => "needs_review".to_string(),
            AutoResolutionActionDto::Escalate => "escalated".to_string(),
        },
    }
}

async fn to_response(
    state: &AppState,
    entry: &crate::auto_resolution::AutoResolutionEntry,
) -> AutoResolutionEntryResponse {
    AutoResolutionEntryResponse {
        market_id: entry.market_id,
        status: display_status(state, entry).await,
        action: entry.action,
        payout_nanos: entry.payout_nanos,
        confidence: entry.confidence,
        reasoning: entry.reasoning.clone(),
        evidence_excerpts: entry.evidence_excerpts.clone(),
        proposed_at_ms: entry.proposed_at_ms,
        eta_ms: entry.eta_ms,
        decided_at_ms: entry.decided_at_ms,
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// POST /v1/admin/auto-resolutions — record (or refresh) an auto-resolution
/// proposal. Never settles a market.
#[utoipa::path(
    tag = "routesauto_resolution",
    post,
    path = "/v1/admin/auto-resolutions",
    request_body = SubmitAutoResolutionRequest,
    responses(
        (status = 200, description = "Proposal recorded", body = AutoResolutionEntryResponse),
        (status = 400, description = "Invalid proposal"),
        (status = 403, description = "Service token required")
    )
)]
pub async fn submit_auto_resolution(
    State(state): State<AppState>,
    Json(req): Json<SubmitAutoResolutionRequest>,
) -> Result<Json<AutoResolutionEntryResponse>, AppError> {
    rehydrate(&state).await?;
    if req.payout_nanos > NANOS_PER_DOLLAR {
        return Err(AppError::bad_request(format!(
            "Payout must be between 0 and {NANOS_PER_DOLLAR} nanos, got {}",
            req.payout_nanos
        )));
    }
    if !req.confidence.is_finite() || !(0.0..=1.0).contains(&req.confidence) {
        return Err(AppError::bad_request("confidence must be in [0, 1]"));
    }
    // A propose entry MUST carry an auto-finalize deadline; without one it would
    // never leave the queue (fail-closed on ambiguity).
    if matches!(req.action, AutoResolutionActionDto::Propose) && req.eta_ms.is_none() {
        return Err(AppError::bad_request(
            "propose entries require eta_ms (challenge-window deadline)",
        ));
    }

    let entry = state.auto_resolutions.upsert(&req, now_ms());
    persist(&state, &entry).await?;
    Ok(Json(to_response(&state, &entry).await))
}

/// GET /v1/admin/auto-resolutions — list every recorded proposal.
#[utoipa::path(
    tag = "routesauto_resolution",
    get,
    path = "/v1/admin/auto-resolutions",
    responses(
        (status = 200, description = "Pending auto-resolutions", body = AutoResolutionListResponse),
        (status = 403, description = "Service token required")
    )
)]
pub async fn list_auto_resolutions(
    State(state): State<AppState>,
) -> Result<Json<AutoResolutionListResponse>, AppError> {
    rehydrate(&state).await?;
    let mut entries = Vec::new();
    for entry in state.auto_resolutions.list() {
        entries.push(to_response(&state, &entry).await);
    }
    entries.sort_by_key(|e| e.market_id);
    Ok(Json(AutoResolutionListResponse { entries }))
}

/// POST /v1/admin/auto-resolutions/{id}/approve — approve a proposal so the
/// resolver finalizes it on its next poll (does not settle here).
#[utoipa::path(
    tag = "routesauto_resolution",
    post,
    path = "/v1/admin/auto-resolutions/{id}/approve",
    params(("id" = u32, Path, description = "Market ID")),
    responses(
        (status = 200, description = "Proposal approved", body = AutoResolutionEntryResponse),
        (status = 404, description = "No proposal for this market"),
        (status = 403, description = "Service token required")
    )
)]
pub async fn approve_auto_resolution(
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> Result<Json<AutoResolutionEntryResponse>, AppError> {
    rehydrate(&state).await?;
    let entry = state
        .auto_resolutions
        .decide(id, Decision::Approved, now_ms())
        .ok_or_else(|| AppError::not_found("no auto-resolution proposal for this market"))?;
    persist(&state, &entry).await?;
    Ok(Json(to_response(&state, &entry).await))
}

/// POST /v1/admin/auto-resolutions/{id}/reject — veto a proposal. Terminal: the
/// resolver will never finalize it.
#[utoipa::path(
    tag = "routesauto_resolution",
    post,
    path = "/v1/admin/auto-resolutions/{id}/reject",
    params(("id" = u32, Path, description = "Market ID")),
    responses(
        (status = 200, description = "Proposal rejected", body = AutoResolutionEntryResponse),
        (status = 404, description = "No proposal for this market"),
        (status = 403, description = "Service token required")
    )
)]
pub async fn reject_auto_resolution(
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> Result<Json<AutoResolutionEntryResponse>, AppError> {
    rehydrate(&state).await?;
    let entry = state
        .auto_resolutions
        .decide(id, Decision::Rejected, now_ms())
        .ok_or_else(|| AppError::not_found("no auto-resolution proposal for this market"))?;
    persist(&state, &entry).await?;
    Ok(Json(to_response(&state, &entry).await))
}
