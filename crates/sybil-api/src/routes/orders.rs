use axum::extract::State;
use axum::Json;

use matching_sequencer::crypto::{PublicKey, SignedOrder};
use matching_sequencer::{AccountId, OrderSubmission};
use p256::ecdsa::{Signature, VerifyingKey};
use p256::EncodedPoint;

use crate::convert::{order_spec_to_order, signed_order_data_to_order};
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::request::{SubmitOrderRequest, SubmitSignedOrderRequest};
use crate::types::response::OrderAcceptedResponse;

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

    let submission = OrderSubmission {
        account_id: AccountId(req.account_id),
        orders,
        mm_constraint: None,
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
