use axum::extract::State;
use axum::Json;

use matching_engine::mm_constraint::{MmConstraint, MmId, MmSide};
use matching_sequencer::crypto::{PublicKey, SignedOrder};
use matching_sequencer::{AccountId, OrderSubmission};
use p256::ecdsa::{Signature, VerifyingKey};
use p256::EncodedPoint;

use crate::convert::{order_spec_to_order, signed_order_data_to_order};
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::request::{OrderSpec, SubmitOrderRequest, SubmitSignedOrderRequest};
use crate::types::response::OrderAcceptedResponse;

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

    let mut orders = Vec::with_capacity(req.orders.len());
    for spec in &req.orders {
        let order = order_spec_to_order(spec, &markets).map_err(AppError::bad_request)?;
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
    // Parse public key
    let key_bytes = hex::decode(&req.signer_pubkey_hex)
        .map_err(|_| AppError::bad_request("Invalid hex encoding for public key"))?;
    let encoded_point = EncodedPoint::from_bytes(&key_bytes)
        .map_err(|_| AppError::bad_request("Invalid P256 encoded point"))?;
    let verifying_key = VerifyingKey::from_encoded_point(&encoded_point)
        .map_err(|_| AppError::bad_request("Invalid P256 public key"))?;

    // Parse signature
    let sig_bytes = hex::decode(&req.signature_hex)
        .map_err(|_| AppError::bad_request("Invalid hex encoding for signature"))?;
    let signature = Signature::from_slice(&sig_bytes)
        .map_err(|_| AppError::bad_request("Invalid P256 ECDSA signature"))?;

    // Build the order
    let order = signed_order_data_to_order(&req.order).map_err(AppError::bad_request)?;

    let signed = SignedOrder {
        order,
        signer: PublicKey(verifying_key),
        signature,
    };

    state.sequencer.submit_signed_order(signed).await?;

    Ok(Json(OrderAcceptedResponse { accepted: true }))
}
