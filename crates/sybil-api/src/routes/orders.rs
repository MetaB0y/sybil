use axum::Json;
use axum::extract::{Path, State};
use axum::http::HeaderMap;

use matching_engine::MarketId;
use matching_engine::Nanos;
use matching_engine::mm_constraint::{MmConstraint, MmId, MmSide};
use matching_sequencer::crypto::{
    AccountAuthScheme, AuthenticatedCancel, AuthenticatedMmBundle, AuthenticatedMmBundleCancel,
    AuthenticatedMmBundleReplace, AuthenticatedOrder, PublicKey, SignedCancel, SignedMmBundle,
    SignedMmBundleCancel, SignedMmBundleReplace, SignedOrder, canonical_cancel_bytes,
    canonical_mm_bundle_bytes, canonical_mm_bundle_cancel_bytes, canonical_mm_bundle_replace_bytes,
    canonical_order_bytes,
};
use matching_sequencer::{AccountId, OrderSubmission, PendingOrderInfo};
use p256::Sec1Point;
use p256::ecdsa::{Signature, VerifyingKey};

use crate::convert::{apply_time_in_force, order_spec_to_order, signed_order_data_to_order};
use crate::state::AppState;
use crate::types::error::AppError;
use crate::types::request::{
    AuthScheme, CancelSignedMmBundleRequest, CancelSignedOrderRequest, OrderSpec,
    ReplaceSignedMmBundleRequest, SubmitOrderRequest, SubmitSignedMmBundleRequest,
    SubmitSignedOrderRequest, TimeInForce,
};
use crate::types::response::{
    ApiErrorResponse, CancelOrderResponse, OrderAcceptedResponse, PendingOrderResponse,
};
use crate::webauthn;

/// Derive the MmSide from an OrderSpec for capital calculation.
fn mm_side_from_spec(spec: &OrderSpec) -> MmSide {
    match spec {
        OrderSpec::BuyYes { .. } => MmSide::BuyYes,
        OrderSpec::BuyNo { .. } => MmSide::BuyNo,
        OrderSpec::SellYes { .. } => MmSide::SellYes,
        OrderSpec::SellNo { .. } => MmSide::SellNo,
    }
}

fn parse_signer_public_key(public_key_hex: &str) -> Result<PublicKey, AppError> {
    let key_bytes = hex::decode(
        public_key_hex
            .trim_start_matches("0x")
            .trim_start_matches("0X"),
    )
    .map_err(|_| AppError::bad_request("Invalid hex encoding for public key"))?;
    let sec1_point = Sec1Point::from_bytes(&key_bytes)
        .map_err(|_| AppError::bad_request("Invalid P256 encoded point"))?;
    let verifying_key = VerifyingKey::from_sec1_point(&sec1_point)
        .map_err(|_| AppError::bad_request("Invalid P256 public key"))?;
    Ok(PublicKey(verifying_key))
}

fn parse_signature(signature_hex: &str) -> Result<Signature, AppError> {
    let sig_bytes = hex::decode(
        signature_hex
            .trim_start_matches("0x")
            .trim_start_matches("0X"),
    )
    .map_err(|_| AppError::bad_request("Invalid hex encoding for signature"))?;
    Signature::from_slice(&sig_bytes)
        .map_err(|_| AppError::bad_request("Invalid P256 ECDSA signature"))
}

fn parse_required_signature(signature_hex: Option<&str>) -> Result<Signature, AppError> {
    parse_signature(signature_hex.ok_or_else(|| {
        AppError::bad_request("signature_hex is required for raw_p256 signed requests")
    })?)
}

fn parse_bundle_id(bundle_id_hex: &str) -> Result<[u8; 32], AppError> {
    let bytes = hex::decode(
        bundle_id_hex
            .trim_start_matches("0x")
            .trim_start_matches("0X"),
    )
    .map_err(|_| AppError::bad_request("Invalid hex encoding for bundle_id_hex"))?;
    bytes
        .try_into()
        .map_err(|_| AppError::bad_request("bundle_id_hex must encode exactly 32 bytes"))
}

fn sequencer_auth_scheme(scheme: AuthScheme) -> AccountAuthScheme {
    match scheme {
        AuthScheme::RawP256 => AccountAuthScheme::RawP256,
        AuthScheme::WebAuthn => AccountAuthScheme::WebAuthn,
    }
}

async fn ensure_registered_scheme(
    state: &AppState,
    signer: &PublicKey,
    expected: AccountAuthScheme,
) -> Result<(), AppError> {
    let registered = state
        .sequencer
        .lookup_registered_pubkey(signer.clone())
        .await?
        .ok_or_else(|| AppError::not_found("No account registered for this public key"))?;
    if registered.auth_scheme != expected {
        return Err(AppError::forbidden(
            "Signer key is not registered for this auth scheme",
        ));
    }
    Ok(())
}

/// POST /v1/orders
#[utoipa::path(
    tag = "routesorders",
    post,
    path = "/v1/orders",
    request_body = SubmitOrderRequest,
    responses(
        (status = 200, description = "Orders accepted", body = OrderAcceptedResponse),
        (status = 400, description = "Invalid order", body = ApiErrorResponse),
        (status = 404, description = "Account or market not found", body = ApiErrorResponse),
        (status = 409, description = "Market is not tradeable", body = ApiErrorResponse)
    )
)]
pub async fn submit_orders(
    State(state): State<AppState>,
    Json(req): Json<SubmitOrderRequest>,
) -> Result<Json<OrderAcceptedResponse>, AppError> {
    // Get current markets for validation
    let markets = state.sequencer.list_markets().await?;
    let is_ioc = req.time_in_force == TimeInForce::Ioc;
    if is_ioc && req.expires_at_block.is_some() {
        return Err(AppError::bad_request(
            "expires_at_block is not valid for IOC orders",
        ));
    }

    let mut orders = Vec::with_capacity(req.orders.len());
    for spec in &req.orders {
        let mut order = order_spec_to_order(spec, &markets).map_err(|error| match error {
            crate::convert::OrderSpecConversionError::MarketNotFound(market_id) => {
                AppError::market_not_found(market_id.0)
            }
            crate::convert::OrderSpecConversionError::Invalid(error) => {
                AppError::bad_request(error)
            }
        })?;
        if !is_ioc {
            apply_time_in_force(&mut order, req.time_in_force, req.expires_at_block, None)
                .map_err(AppError::bad_request)?;
        }
        orders.push(order);
    }

    // Build MmConstraint if mm_budget_nanos is provided
    let mm_constraint = req.mm_budget_nanos.map(|budget| {
        let mut constraint = MmConstraint::new(MmId(req.account_id), Nanos(budget));
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

    let order_ids = if is_ioc {
        state.sequencer.submit_ioc_order(submission).await?
    } else {
        state.sequencer.submit_order(submission).await?
    };

    Ok(Json(OrderAcceptedResponse {
        accepted: true,
        order_ids,
    }))
}

/// POST /v1/orders/signed
#[utoipa::path(
    tag = "routesorders",
    post,
    path = "/v1/orders/signed",
    request_body = SubmitSignedOrderRequest,
    responses(
        (status = 200, description = "Signed order accepted", body = OrderAcceptedResponse),
        (status = 400, description = "Invalid order or signature"),
        (status = 409, description = "Replay nonce is stale or duplicate"),
        (status = 404, description = "Unknown signer")
    )
)]
pub async fn submit_signed_order(
    State(state): State<AppState>,
    Json(req): Json<SubmitSignedOrderRequest>,
) -> Result<Json<OrderAcceptedResponse>, AppError> {
    let signer = parse_signer_public_key(&req.signer_pubkey_hex)?;
    let mut order = signed_order_data_to_order(&req.order).map_err(AppError::bad_request)?;
    apply_time_in_force(&mut order, req.time_in_force, req.expires_at_block, None)
        .map_err(AppError::bad_request)?;
    let order_ids = match req.auth_scheme {
        AuthScheme::RawP256 => {
            let signature = parse_required_signature(req.signature_hex.as_deref())?;
            let signed = SignedOrder {
                order,
                nonce: req.nonce,
                signer,
                signature,
            };
            state.sequencer.submit_signed_order(signed).await?
        }
        AuthScheme::WebAuthn => {
            ensure_registered_scheme(&state, &signer, sequencer_auth_scheme(req.auth_scheme))
                .await?;
            let assertion = req.webauthn_assertion.as_ref().ok_or_else(|| {
                AppError::bad_request("webauthn_assertion is required for webauthn signed requests")
            })?;
            let genesis_hash = state
                .sequencer
                .get_genesis_hash()
                .await?
                .ok_or(matching_sequencer::SequencerError::GenesisHashUnavailable)?;
            let canonical = canonical_order_bytes(&order, req.nonce, genesis_hash);
            webauthn::verify_assertion(&state.webauthn, &signer.0, &canonical, assertion).map_err(
                |err| AppError::bad_request(format!("Invalid WebAuthn assertion: {err}")),
            )?;
            let authorization = webauthn::client_action_authorization(&signer.0, assertion)
                .map_err(|err| {
                    AppError::bad_request(format!("Invalid WebAuthn assertion envelope: {err}"))
                })?;
            state
                .sequencer
                .submit_authenticated_order(AuthenticatedOrder {
                    order,
                    nonce: req.nonce,
                    authorization,
                })
                .await?
        }
    };

    Ok(Json(OrderAcceptedResponse {
        accepted: true,
        order_ids,
    }))
}

/// POST /v1/orders/mm-bundles/signed
#[utoipa::path(
    tag = "routesorders",
    post,
    path = "/v1/orders/mm-bundles/signed",
    request_body = SubmitSignedMmBundleRequest,
    responses(
        (status = 200, description = "Signed atomic MM bundle accepted", body = OrderAcceptedResponse),
        (status = 400, description = "Invalid bundle or signature", body = ApiErrorResponse),
        (status = 403, description = "Signer or account mismatch", body = ApiErrorResponse),
        (status = 404, description = "Unknown signer, account, or market", body = ApiErrorResponse),
        (status = 409, description = "Stale nonce or target block", body = ApiErrorResponse)
    )
)]
pub async fn submit_signed_mm_bundle(
    State(state): State<AppState>,
    Json(req): Json<SubmitSignedMmBundleRequest>,
) -> Result<Json<OrderAcceptedResponse>, AppError> {
    if req.orders.is_empty() {
        return Err(AppError::bad_request("MM bundle orders must not be empty"));
    }
    if req.orders.len() > state.max_orders_per_submission {
        return Err(
            matching_sequencer::SequencerError::TooManyOrdersInSubmission {
                count: req.orders.len(),
                limit: state.max_orders_per_submission,
            }
            .into(),
        );
    }
    let signer = parse_signer_public_key(&req.signer_pubkey_hex)?;
    let bundle_id = parse_bundle_id(&req.bundle_id_hex)?;
    let markets = state.sequencer.list_markets().await?;
    let mut orders = Vec::with_capacity(req.orders.len());
    let mut order_sides = Vec::with_capacity(req.orders.len());
    for spec in &req.orders {
        let mut order = order_spec_to_order(spec, &markets).map_err(|error| match error {
            crate::convert::OrderSpecConversionError::MarketNotFound(market_id) => {
                AppError::market_not_found(market_id.0)
            }
            crate::convert::OrderSpecConversionError::Invalid(error) => {
                AppError::bad_request(error)
            }
        })?;
        order.expires_at_block = Some(req.expires_at_block);
        orders.push(order);
        order_sides.push(mm_side_from_spec(spec));
    }
    let max_capital = Nanos(req.mm_budget_nanos);
    let order_ids = match req.auth_scheme {
        AuthScheme::RawP256 => {
            let signature = parse_required_signature(req.signature_hex.as_deref())?;
            state
                .sequencer
                .submit_signed_mm_bundle(SignedMmBundle {
                    account_id: AccountId(req.account_id),
                    bundle_id,
                    revision: req.revision,
                    orders,
                    order_sides,
                    max_capital,
                    nonce: req.nonce,
                    signer,
                    signature,
                })
                .await?
        }
        AuthScheme::WebAuthn => {
            ensure_registered_scheme(&state, &signer, sequencer_auth_scheme(req.auth_scheme))
                .await?;
            let assertion = req.webauthn_assertion.as_ref().ok_or_else(|| {
                AppError::bad_request("webauthn_assertion is required for webauthn signed requests")
            })?;
            let genesis_hash = state
                .sequencer
                .get_genesis_hash()
                .await?
                .ok_or(matching_sequencer::SequencerError::GenesisHashUnavailable)?;
            let canonical = canonical_mm_bundle_bytes(
                AccountId(req.account_id),
                bundle_id,
                req.revision,
                &orders,
                &order_sides,
                max_capital,
                req.nonce,
                genesis_hash,
            )?;
            webauthn::verify_assertion(&state.webauthn, &signer.0, &canonical, assertion).map_err(
                |err| AppError::bad_request(format!("Invalid WebAuthn assertion: {err}")),
            )?;
            let authorization = webauthn::client_action_authorization(&signer.0, assertion)
                .map_err(|err| {
                    AppError::bad_request(format!("Invalid WebAuthn assertion envelope: {err}"))
                })?;
            state
                .sequencer
                .submit_authenticated_mm_bundle(AuthenticatedMmBundle {
                    account_id: AccountId(req.account_id),
                    bundle_id,
                    revision: req.revision,
                    orders,
                    order_sides,
                    max_capital,
                    nonce: req.nonce,
                    authorization,
                })
                .await?
        }
    };

    Ok(Json(OrderAcceptedResponse {
        accepted: true,
        order_ids,
    }))
}

/// POST /v1/orders/mm-bundles/replace/signed
#[utoipa::path(
    tag = "routesorders",
    post,
    path = "/v1/orders/mm-bundles/replace/signed",
    request_body = ReplaceSignedMmBundleRequest,
    responses(
        (status = 200, description = "Signed atomic MM bundle replaced", body = OrderAcceptedResponse),
        (status = 400, description = "Invalid replacement or signature", body = ApiErrorResponse),
        (status = 403, description = "Signer or account mismatch", body = ApiErrorResponse),
        (status = 404, description = "Unknown signer, account, or market", body = ApiErrorResponse),
        (status = 409, description = "Bundle is absent, stale, or already advanced", body = ApiErrorResponse)
    )
)]
pub async fn replace_signed_mm_bundle(
    State(state): State<AppState>,
    Json(req): Json<ReplaceSignedMmBundleRequest>,
) -> Result<Json<OrderAcceptedResponse>, AppError> {
    if req.orders.is_empty() {
        return Err(AppError::bad_request(
            "MM bundle replacement orders must not be empty",
        ));
    }
    if req.orders.len() > state.max_orders_per_submission {
        return Err(
            matching_sequencer::SequencerError::TooManyOrdersInSubmission {
                count: req.orders.len(),
                limit: state.max_orders_per_submission,
            }
            .into(),
        );
    }
    let signer = parse_signer_public_key(&req.signer_pubkey_hex)?;
    let bundle_id = parse_bundle_id(&req.bundle_id_hex)?;
    let markets = state.sequencer.list_markets().await?;
    let mut orders = Vec::with_capacity(req.orders.len());
    let mut order_sides = Vec::with_capacity(req.orders.len());
    for spec in &req.orders {
        let mut order = order_spec_to_order(spec, &markets).map_err(|error| match error {
            crate::convert::OrderSpecConversionError::MarketNotFound(market_id) => {
                AppError::market_not_found(market_id.0)
            }
            crate::convert::OrderSpecConversionError::Invalid(error) => {
                AppError::bad_request(error)
            }
        })?;
        order.expires_at_block = Some(req.expires_at_block);
        orders.push(order);
        order_sides.push(mm_side_from_spec(spec));
    }
    let max_capital = Nanos(req.mm_budget_nanos);
    let order_ids = match req.auth_scheme {
        AuthScheme::RawP256 => {
            let signature = parse_required_signature(req.signature_hex.as_deref())?;
            state
                .sequencer
                .replace_signed_mm_bundle(SignedMmBundleReplace {
                    account_id: AccountId(req.account_id),
                    bundle_id,
                    expected_revision: req.expected_revision,
                    new_revision: req.new_revision,
                    orders,
                    order_sides,
                    max_capital,
                    nonce: req.nonce,
                    signer,
                    signature,
                })
                .await?
        }
        AuthScheme::WebAuthn => {
            ensure_registered_scheme(&state, &signer, sequencer_auth_scheme(req.auth_scheme))
                .await?;
            let assertion = req.webauthn_assertion.as_ref().ok_or_else(|| {
                AppError::bad_request("webauthn_assertion is required for webauthn signed requests")
            })?;
            let genesis_hash = state
                .sequencer
                .get_genesis_hash()
                .await?
                .ok_or(matching_sequencer::SequencerError::GenesisHashUnavailable)?;
            let canonical = canonical_mm_bundle_replace_bytes(
                AccountId(req.account_id),
                bundle_id,
                req.expected_revision,
                req.new_revision,
                &orders,
                &order_sides,
                max_capital,
                req.nonce,
                genesis_hash,
            )?;
            webauthn::verify_assertion(&state.webauthn, &signer.0, &canonical, assertion).map_err(
                |err| AppError::bad_request(format!("Invalid WebAuthn assertion: {err}")),
            )?;
            let authorization = webauthn::client_action_authorization(&signer.0, assertion)
                .map_err(|err| {
                    AppError::bad_request(format!("Invalid WebAuthn assertion envelope: {err}"))
                })?;
            state
                .sequencer
                .replace_authenticated_mm_bundle(AuthenticatedMmBundleReplace {
                    account_id: AccountId(req.account_id),
                    bundle_id,
                    expected_revision: req.expected_revision,
                    new_revision: req.new_revision,
                    orders,
                    order_sides,
                    max_capital,
                    nonce: req.nonce,
                    authorization,
                })
                .await?
        }
    };

    Ok(Json(OrderAcceptedResponse {
        accepted: true,
        order_ids,
    }))
}

/// POST /v1/orders/mm-bundles/cancel/signed
#[utoipa::path(
    tag = "routesorders",
    post,
    path = "/v1/orders/mm-bundles/cancel/signed",
    request_body = CancelSignedMmBundleRequest,
    responses(
        (status = 200, description = "Signed atomic MM bundle cancelled", body = CancelOrderResponse),
        (status = 400, description = "Invalid cancellation or signature", body = ApiErrorResponse),
        (status = 403, description = "Signer or account mismatch", body = ApiErrorResponse),
        (status = 404, description = "Unknown signer or account", body = ApiErrorResponse),
        (status = 409, description = "Bundle is absent, stale, or already advanced", body = ApiErrorResponse)
    )
)]
pub async fn cancel_signed_mm_bundle(
    State(state): State<AppState>,
    Json(req): Json<CancelSignedMmBundleRequest>,
) -> Result<Json<CancelOrderResponse>, AppError> {
    let signer = parse_signer_public_key(&req.signer_pubkey_hex)?;
    let bundle_id = parse_bundle_id(&req.bundle_id_hex)?;
    match req.auth_scheme {
        AuthScheme::RawP256 => {
            let signature = parse_required_signature(req.signature_hex.as_deref())?;
            state
                .sequencer
                .cancel_signed_mm_bundle(SignedMmBundleCancel {
                    account_id: AccountId(req.account_id),
                    bundle_id,
                    expected_revision: req.expected_revision,
                    nonce: req.nonce,
                    signer,
                    signature,
                })
                .await?;
        }
        AuthScheme::WebAuthn => {
            ensure_registered_scheme(&state, &signer, sequencer_auth_scheme(req.auth_scheme))
                .await?;
            let assertion = req.webauthn_assertion.as_ref().ok_or_else(|| {
                AppError::bad_request("webauthn_assertion is required for webauthn signed requests")
            })?;
            let genesis_hash = state
                .sequencer
                .get_genesis_hash()
                .await?
                .ok_or(matching_sequencer::SequencerError::GenesisHashUnavailable)?;
            let canonical = canonical_mm_bundle_cancel_bytes(
                AccountId(req.account_id),
                bundle_id,
                req.expected_revision,
                req.nonce,
                genesis_hash,
            );
            webauthn::verify_assertion(&state.webauthn, &signer.0, &canonical, assertion).map_err(
                |err| AppError::bad_request(format!("Invalid WebAuthn assertion: {err}")),
            )?;
            let authorization = webauthn::client_action_authorization(&signer.0, assertion)
                .map_err(|err| {
                    AppError::bad_request(format!("Invalid WebAuthn assertion envelope: {err}"))
                })?;
            state
                .sequencer
                .cancel_authenticated_mm_bundle(AuthenticatedMmBundleCancel {
                    account_id: AccountId(req.account_id),
                    bundle_id,
                    expected_revision: req.expected_revision,
                    nonce: req.nonce,
                    authorization,
                })
                .await?;
        }
    }

    Ok(Json(CancelOrderResponse { cancelled: true }))
}

/// POST /v1/orders/cancel/signed
#[utoipa::path(
    tag = "routesorders",
    post,
    path = "/v1/orders/cancel/signed",
    request_body = CancelSignedOrderRequest,
    responses(
        (status = 200, description = "Signed cancel accepted", body = CancelOrderResponse),
        (status = 400, description = "Invalid signature payload"),
        (status = 403, description = "Signer or owner mismatch"),
        (status = 409, description = "Replay nonce is stale or duplicate"),
        (status = 404, description = "Unknown signer or pending order not found")
    )
)]
pub async fn cancel_signed_order(
    State(state): State<AppState>,
    Json(req): Json<CancelSignedOrderRequest>,
) -> Result<Json<CancelOrderResponse>, AppError> {
    let signer = parse_signer_public_key(&req.signer_pubkey_hex)?;
    match req.auth_scheme {
        AuthScheme::RawP256 => {
            let signature = parse_required_signature(req.signature_hex.as_deref())?;
            let signed = SignedCancel {
                account_id: AccountId(req.account_id),
                order_id: req.order_id,
                nonce: req.nonce,
                signer,
                signature,
            };
            state.sequencer.cancel_signed_order(signed).await?;
        }
        AuthScheme::WebAuthn => {
            ensure_registered_scheme(&state, &signer, sequencer_auth_scheme(req.auth_scheme))
                .await?;
            let assertion = req.webauthn_assertion.as_ref().ok_or_else(|| {
                AppError::bad_request("webauthn_assertion is required for webauthn signed requests")
            })?;
            let genesis_hash = state
                .sequencer
                .get_genesis_hash()
                .await?
                .ok_or(matching_sequencer::SequencerError::GenesisHashUnavailable)?;
            let canonical = canonical_cancel_bytes(
                AccountId(req.account_id),
                req.order_id,
                req.nonce,
                genesis_hash,
            );
            webauthn::verify_assertion(&state.webauthn, &signer.0, &canonical, assertion).map_err(
                |err| AppError::bad_request(format!("Invalid WebAuthn assertion: {err}")),
            )?;
            let authorization = webauthn::client_action_authorization(&signer.0, assertion)
                .map_err(|err| {
                    AppError::bad_request(format!("Invalid WebAuthn assertion envelope: {err}"))
                })?;
            state
                .sequencer
                .cancel_authenticated_order(AuthenticatedCancel {
                    account_id: AccountId(req.account_id),
                    order_id: req.order_id,
                    nonce: req.nonce,
                    authorization,
                })
                .await?;
        }
    }

    Ok(Json(CancelOrderResponse { cancelled: true }))
}

fn to_pending_response(info: &PendingOrderInfo) -> PendingOrderResponse {
    let market_id = info.market_ids.first().map(|m| m.0).unwrap_or(0);
    PendingOrderResponse {
        order_id: info.order_id,
        account_id: info.account_id.0,
        market_id,
        side: info.side.to_string(),
        limit_price_nanos: info.limit_price.0,
        remaining_quantity: info.remaining_qty,
        created_at_block: info.created_at_block,
        expires_at_block: info.expires_at_block,
        original_quantity: info.original_quantity,
        created_at_ms: info.created_at_ms,
    }
}

/// GET /v1/accounts/{id}/orders — pending orders for an account
#[utoipa::path(
    tag = "routesorders",
    get,
    path = "/v1/accounts/{id}/orders",
    params(("id" = u64, Path, description = "Account ID")),
    responses(
        (status = 200, description = "Pending orders", body = Vec<PendingOrderResponse>),
        (status = 401, description = "Missing/invalid bearer token"),
        (status = 403, description = "Token belongs to a different account"),
    ),
    security(("bearer_read" = []))
)]
pub async fn get_account_orders(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    headers: HeaderMap,
) -> Result<Json<Vec<PendingOrderResponse>>, AppError> {
    crate::routes::accounts::authorize_account_read(&state, &headers, AccountId(id)).await?;
    let orders = state
        .sequencer
        .get_pending_orders(Some(AccountId(id)))
        .await?;
    Ok(Json(orders.iter().map(to_pending_response).collect()))
}

/// GET /v1/markets/{id}/orderbook — all pending orders for a market (dev mode)
#[utoipa::path(
    tag = "routesorders",
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
    tag = "routesorders",
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

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::SigningKey;
    use p256::ecdsa::signature::Signer;

    /// A `0x`-prefixed pubkey must parse identically to the bare form, matching
    /// the sibling bridge/proofs/accounts endpoints (client-footgun fix).
    #[test]
    fn parse_signer_public_key_accepts_0x_prefix() {
        let key = SigningKey::from_slice(&[7u8; 32]).expect("fixed scalar");
        let pubkey_hex = hex::encode(key.verifying_key().to_sec1_point(false).as_bytes());

        let bare = parse_signer_public_key(&pubkey_hex).expect("bare pubkey parses");
        let prefixed =
            parse_signer_public_key(&format!("0x{pubkey_hex}")).expect("0x pubkey parses");
        let upper = parse_signer_public_key(&format!("0X{pubkey_hex}")).expect("0X pubkey parses");

        assert_eq!(bare.0, prefixed.0);
        assert_eq!(bare.0, upper.0);
    }

    /// Likewise for signatures — a `0x`-prefixed signature must decode to the
    /// same signature bytes as the bare form.
    #[test]
    fn parse_signature_accepts_0x_prefix() {
        let key = SigningKey::from_slice(&[9u8; 32]).expect("fixed scalar");
        let signature: Signature = key.sign(b"canonical-order-bytes");
        let sig_hex = hex::encode(signature.to_bytes());

        let bare = parse_signature(&sig_hex).expect("bare signature parses");
        let prefixed = parse_signature(&format!("0x{sig_hex}")).expect("0x signature parses");

        assert_eq!(bare, prefixed);
    }
}
