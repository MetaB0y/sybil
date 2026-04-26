use axum::extract::{Path, State};
use axum::Json;

use matching_engine::mm_constraint::{MmConstraint, MmId, MmSide};
use matching_engine::MarketId;
use matching_sequencer::crypto::{PublicKey, SignedCancel, SignedOrder};
use matching_sequencer::{AccountId, OrderSubmission, PendingOrderInfo};
use p256::ecdsa::{Signature, VerifyingKey};
use p256::Sec1Point;

use crate::convert::{apply_time_in_force, order_spec_to_order, signed_order_data_to_order};
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::request::{
    CancelSignedOrderRequest, OrderSpec, SubmitOrderRequest, SubmitSignedOrderRequest,
};
use crate::types::response::{CancelOrderResponse, OrderAcceptedResponse, PendingOrderResponse};

/// Derive the MmSide from an OrderSpec for capital calculation.
fn mm_side_from_spec(spec: &OrderSpec) -> MmSide {
    match spec {
        OrderSpec::BuyYes { .. } => MmSide::BuyYes,
        OrderSpec::BuyNo { .. } => MmSide::BuyNo,
        OrderSpec::SellYes { .. } => MmSide::SellYes,
        OrderSpec::SellNo { .. } => MmSide::SellNo,
        // For complex order types, use BuyYes as a conservative default.
        // Capital = price * qty, which is the max possible cost.
        _ => MmSide::BuyYes,
    }
}

fn parse_signer_public_key(public_key_hex: &str) -> Result<PublicKey, AppError> {
    let key_bytes = hex::decode(public_key_hex)
        .map_err(|_| AppError::bad_request("Invalid hex encoding for public key"))?;
    let sec1_point = Sec1Point::from_bytes(&key_bytes)
        .map_err(|_| AppError::bad_request("Invalid P256 encoded point"))?;
    let verifying_key = VerifyingKey::from_sec1_point(&sec1_point)
        .map_err(|_| AppError::bad_request("Invalid P256 public key"))?;
    Ok(PublicKey(verifying_key))
}

fn parse_signature(signature_hex: &str) -> Result<Signature, AppError> {
    let sig_bytes = hex::decode(signature_hex)
        .map_err(|_| AppError::bad_request("Invalid hex encoding for signature"))?;
    Signature::from_slice(&sig_bytes)
        .map_err(|_| AppError::bad_request("Invalid P256 ECDSA signature"))
}

async fn next_block_height(state: &AppState) -> Result<u64, AppError> {
    let latest = state.sequencer.get_latest_block().await?;
    Ok(latest
        .map(|block| block.header.height)
        .unwrap_or(0)
        .saturating_add(1))
}

/// POST /v1/orders
#[utoipa::path(
    post,
    path = "/v1/orders",
    request_body = SubmitOrderRequest,
    responses(
        (status = 200, description = "Orders accepted", body = OrderAcceptedResponse),
        (status = 400, description = "Invalid order"),
        (status = 404, description = "Account not found")
    )
)]
pub async fn submit_orders(
    State(state): State<AppState>,
    Json(req): Json<SubmitOrderRequest>,
) -> Result<Json<OrderAcceptedResponse>, AppError> {
    // Get current markets for validation
    let markets = state.sequencer.list_markets().await?;
    let ioc_expires_at_block = next_block_height(&state).await?;

    let mut orders = Vec::with_capacity(req.orders.len());
    for spec in &req.orders {
        let mut order = order_spec_to_order(spec, &markets).map_err(AppError::bad_request)?;
        apply_time_in_force(
            &mut order,
            req.time_in_force,
            req.expires_at_block,
            Some(ioc_expires_at_block),
        )
        .map_err(AppError::bad_request)?;
        orders.push(order);
    }

    // Build MmConstraint if mm_budget_nanos is provided
    let mm_constraint = req.mm_budget_nanos.map(|budget| {
        let mut constraint = MmConstraint::new(MmId(req.account_id), budget);
        // Use temporary IDs (0, 1, 2...) matching order indices.
        // The sequencer will remap these to real order IDs.
        for (i, spec) in req.orders.iter().enumerate() {
            constraint.add_order(i as u64, mm_side_from_spec(spec));
        }
        constraint
    });

    let submission = OrderSubmission {
        account_id: AccountId(req.account_id),
        orders,
        mm_constraint,
    };

    state.sequencer.submit_order(submission).await?;

    Ok(Json(OrderAcceptedResponse { accepted: true }))
}

/// POST /v1/orders/signed
#[utoipa::path(
    post,
    path = "/v1/orders/signed",
    request_body = SubmitSignedOrderRequest,
    responses(
        (status = 200, description = "Signed order accepted", body = OrderAcceptedResponse),
        (status = 400, description = "Invalid order or signature"),
        (status = 404, description = "Unknown signer")
    )
)]
pub async fn submit_signed_order(
    State(state): State<AppState>,
    Json(req): Json<SubmitSignedOrderRequest>,
) -> Result<Json<OrderAcceptedResponse>, AppError> {
    let signer = parse_signer_public_key(&req.signer_pubkey_hex)?;
    let signature = parse_signature(&req.signature_hex)?;
    let mut order = signed_order_data_to_order(&req.order).map_err(AppError::bad_request)?;
    apply_time_in_force(&mut order, req.time_in_force, req.expires_at_block, None)
        .map_err(AppError::bad_request)?;
    let signed = SignedOrder {
        order,
        signer,
        signature,
    };

    state.sequencer.submit_signed_order(signed).await?;

    Ok(Json(OrderAcceptedResponse { accepted: true }))
}

/// POST /v1/orders/cancel/signed
#[utoipa::path(
    post,
    path = "/v1/orders/cancel/signed",
    request_body = CancelSignedOrderRequest,
    responses(
        (status = 200, description = "Signed cancel accepted", body = CancelOrderResponse),
        (status = 400, description = "Invalid signature payload"),
        (status = 403, description = "Signer or owner mismatch"),
        (status = 404, description = "Unknown signer or pending order not found")
    )
)]
pub async fn cancel_signed_order(
    State(state): State<AppState>,
    Json(req): Json<CancelSignedOrderRequest>,
) -> Result<Json<CancelOrderResponse>, AppError> {
    let signer = parse_signer_public_key(&req.signer_pubkey_hex)?;
    let signature = parse_signature(&req.signature_hex)?;
    let signed = SignedCancel {
        account_id: AccountId(req.account_id),
        order_id: req.order_id,
        signer,
        signature,
    };

    state.sequencer.cancel_signed_order(signed).await?;

    Ok(Json(CancelOrderResponse { cancelled: true }))
}

fn to_pending_response(info: &PendingOrderInfo) -> PendingOrderResponse {
    let market_id = info.market_ids.first().map(|m| m.0).unwrap_or(0);
    PendingOrderResponse {
        order_id: info.order_id,
        account_id: info.account_id.0,
        market_id,
        side: info.side.to_string(),
        limit_price_nanos: info.limit_price,
        remaining_quantity: info.remaining_qty,
        created_at_block: info.created_at_block,
        expires_at_block: info.expires_at_block,
    }
}

/// GET /v1/accounts/{id}/orders — pending orders for an account
#[utoipa::path(
    get,
    path = "/v1/accounts/{id}/orders",
    params(("id" = u64, Path, description = "Account ID")),
    responses(
        (status = 200, description = "Pending orders", body = Vec<PendingOrderResponse>),
    )
)]
pub async fn get_account_orders(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<Vec<PendingOrderResponse>>, AppError> {
    let orders = state
        .sequencer
        .get_pending_orders(Some(AccountId(id)))
        .await?;
    Ok(Json(orders.iter().map(to_pending_response).collect()))
}

/// GET /v1/markets/{id}/orderbook — all pending orders for a market (dev mode)
#[utoipa::path(
    get,
    path = "/v1/markets/{id}/orderbook",
    params(("id" = u32, Path, description = "Market ID")),
    responses(
        (status = 200, description = "Market order book", body = Vec<PendingOrderResponse>),
    )
)]
pub async fn get_market_orderbook(
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> Result<Json<Vec<PendingOrderResponse>>, AppError> {
    if !state.dev_mode {
        return Err(AppError::dev_mode_required());
    }
    let orders = state
        .sequencer
        .get_market_order_book(MarketId::new(id))
        .await?;
    Ok(Json(orders.iter().map(to_pending_response).collect()))
}

/// GET /v1/orders/pending — all pending orders (dev mode)
#[utoipa::path(
    get,
    path = "/v1/orders/pending",
    responses(
        (status = 200, description = "All pending orders", body = Vec<PendingOrderResponse>),
    )
)]
pub async fn get_all_pending_orders(
    State(state): State<AppState>,
) -> Result<Json<Vec<PendingOrderResponse>>, AppError> {
    if !state.dev_mode {
        return Err(AppError::dev_mode_required());
    }
    let orders = state.sequencer.get_pending_orders(None).await?;
    Ok(Json(orders.iter().map(to_pending_response).collect()))
}
