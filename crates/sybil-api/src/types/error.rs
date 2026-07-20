use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use sybil_api_types::{
    ApiErrorDetails, ApiErrorResponse, MARKET_NOT_FOUND_CODE, MARKET_NOT_TRADEABLE_CODE,
};

#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub body: Box<ApiErrorResponse>,
    pub retry_after_secs: Option<u64>,
}

impl AppError {
    fn new(status: StatusCode, error: impl Into<String>, code: &str) -> Self {
        Self::with_error_details(status, error, code, None)
    }

    fn with_error_details(
        status: StatusCode,
        error: impl Into<String>,
        code: &str,
        details: Option<ApiErrorDetails>,
    ) -> Self {
        Self {
            status,
            body: Box::new(ApiErrorResponse {
                error: error.into(),
                code: code.to_string(),
                details,
            }),
            retry_after_secs: None,
        }
    }

    pub fn bad_request(error: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, error, "BAD_REQUEST")
    }

    pub fn not_found(error: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, error, "NOT_FOUND")
    }

    pub fn market_not_found(market_id: u32) -> Self {
        Self::with_error_details(
            StatusCode::NOT_FOUND,
            format!("Market {market_id} not found"),
            MARKET_NOT_FOUND_CODE,
            Some(ApiErrorDetails {
                message: None,
                market_id: Some(market_id),
                market_status: None,
            }),
        )
    }

    pub fn market_not_tradeable(market_id: u32, market_status: impl Into<String>) -> Self {
        let market_status = market_status.into();
        Self::with_error_details(
            StatusCode::CONFLICT,
            format!("Market {market_id} is not tradeable ({market_status})"),
            MARKET_NOT_TRADEABLE_CODE,
            Some(ApiErrorDetails {
                message: None,
                market_id: Some(market_id),
                market_status: Some(market_status),
            }),
        )
    }

    pub fn gone(error: impl Into<String>) -> Self {
        Self::new(StatusCode::GONE, error, "RETENTION_GONE")
    }

    pub fn forbidden(error: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, error, "FORBIDDEN")
    }

    pub fn unauthorized(error: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, error, "UNAUTHORIZED")
    }

    pub fn internal(error: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error, "INTERNAL_ERROR")
    }

    pub fn conflict(error: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, error, "CONFLICT")
    }

    pub fn public_account_capacity_exhausted(capacity: u64) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            format!(
                "Public account capacity {capacity} is exhausted; use an existing account or contact the operator"
            ),
            "PUBLIC_ACCOUNT_CAPACITY_EXHAUSTED",
        )
    }

    pub fn replay_nonce_stale(error: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, error, "REPLAY_NONCE_STALE")
    }

    pub fn service_unavailable(error: impl Into<String>) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            error,
            "SERVICE_UNAVAILABLE",
        )
    }

    pub fn sequencer_unavailable(error: impl Into<String>) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            error,
            "SEQUENCER_UNAVAILABLE",
        )
    }

    pub fn sequencer_persistence_unavailable(error: impl Into<String>) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            error,
            "SEQUENCER_PERSISTENCE_UNAVAILABLE",
        )
    }

    pub fn bridge_unavailable() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Bridge deposits and withdrawals are unavailable because no L1 domain is configured",
            "BRIDGE_UNAVAILABLE",
        )
    }

    pub fn bridge_domain_mismatch() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "Bridge request does not match the configured chain, vault, and token",
            "BRIDGE_DOMAIN_MISMATCH",
        )
    }

    pub fn history_unavailable(error: impl Into<String>) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            error,
            "HISTORY_UNAVAILABLE",
        )
    }

    pub fn history_incomplete(error: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, error, "HISTORY_INCOMPLETE")
    }

    pub fn sequencer_integrity_halted() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Sequencer writes are unavailable after an integrity failure",
            "SEQUENCER_INTEGRITY_HALTED",
        )
    }

    pub fn mempool_full() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Mempool full",
            "MEMPOOL_FULL",
        )
    }

    pub fn rate_limited(retry_after_secs: u64) -> Self {
        let mut error = Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "Rate limited",
            "RATE_LIMITED",
        );
        error.retry_after_secs = Some(retry_after_secs);
        error
    }

    pub fn dev_mode_required() -> Self {
        Self::forbidden("This endpoint requires dev mode (--dev-mode)")
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = axum::Json(self.body);
        let mut response = (self.status, body).into_response();
        if let Some(retry_after_secs) = self.retry_after_secs
            && let Ok(value) = HeaderValue::from_str(&retry_after_secs.to_string())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        response
    }
}

impl From<matching_sequencer::SequencerError> for AppError {
    fn from(err: matching_sequencer::SequencerError) -> Self {
        match &err {
            matching_sequencer::SequencerError::Rejected(r) => {
                AppError::bad_request(format!("{}", err)).with_details(format!("{:?}", r.reason))
            }
            matching_sequencer::SequencerError::InvalidSignature => {
                AppError::bad_request("Invalid P256 signature")
            }
            matching_sequencer::SequencerError::UnknownSigner => {
                AppError::not_found("No account registered for this public key")
            }
            matching_sequencer::SequencerError::SignerAccountMismatch => {
                AppError::forbidden("Signed account does not match signer public key")
            }
            matching_sequencer::SequencerError::ReplayNonceStale { .. } => {
                AppError::replay_nonce_stale(format!("{}", err))
            }
            matching_sequencer::SequencerError::KeyOpStateStale { .. } => {
                AppError::conflict(format!("{}", err))
            }
            matching_sequencer::SequencerError::GenesisHashUnavailable => {
                AppError::service_unavailable(format!("{}", err))
            }
            matching_sequencer::SequencerError::InvalidAccountProvisioningKey { max_bytes } => {
                AppError::new(
                    StatusCode::BAD_REQUEST,
                    format!("provisioning_key must contain between 1 and {max_bytes} UTF-8 bytes"),
                    "INVALID_ACCOUNT_PROVISIONING_KEY",
                )
            }
            matching_sequencer::SequencerError::AccountProvisioningConflict => AppError::new(
                StatusCode::CONFLICT,
                "provisioning_key is already bound to different account parameters",
                "ACCOUNT_PROVISIONING_CONFLICT",
            ),
            matching_sequencer::SequencerError::PublicAccountCapacityExhausted { capacity } => {
                metrics::counter!(
                    "sybil_public_account_creation_total",
                    "result" => "capacity_exhausted"
                )
                .increment(1);
                AppError::public_account_capacity_exhausted(*capacity)
            }
            matching_sequencer::SequencerError::MempoolFull => AppError::mempool_full(),
            matching_sequencer::SequencerError::RateLimited { retry_after_secs } => {
                AppError::rate_limited(*retry_after_secs)
            }
            matching_sequencer::SequencerError::TooManyOrdersInSubmission { .. }
            | matching_sequencer::SequencerError::TooManyOpenOrders { .. }
            | matching_sequencer::SequencerError::TooManyPendingBundles { .. } => {
                AppError::rate_limited(1).with_details(format!("{}", err))
            }
            matching_sequencer::SequencerError::ActorGone => {
                AppError::sequencer_unavailable("Sequencer actor is unavailable")
            }
            matching_sequencer::SequencerError::ActorOverloaded { class } => AppError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Sequencer {class} capacity is temporarily exhausted"),
                "SEQUENCER_OVERLOADED",
            ),
            matching_sequencer::SequencerError::AccountAlreadyRegistered => {
                AppError::conflict("Public key already registered to an account")
            }
            matching_sequencer::SequencerError::FirstKeyMustBeInitial => {
                AppError::conflict(format!("{err}"))
            }
            matching_sequencer::SequencerError::SigningKeyLimit => {
                AppError::conflict(format!("{err}"))
            }
            matching_sequencer::SequencerError::KeyNotFound => {
                AppError::not_found("Signing key not found")
            }
            matching_sequencer::SequencerError::LastSigningKey => {
                AppError::conflict("Cannot revoke the account's last remaining signing key")
            }
            matching_sequencer::SequencerError::ApiKeyNotFound => {
                AppError::not_found("API key not found")
            }
            matching_sequencer::SequencerError::ApiKeyLimit { .. } => {
                AppError::conflict(format!("{err}"))
            }
            matching_sequencer::SequencerError::ApiKeyLabelTooLong { .. }
            | matching_sequencer::SequencerError::SigningKeyLabelTooLong { .. }
            | matching_sequencer::SequencerError::AccountStorageBudgetExceeded { .. } => {
                AppError::bad_request(format!("{err}"))
            }
            matching_sequencer::SequencerError::KeyOpLimit { .. } => {
                AppError::rate_limited(1).with_details(format!("{err}"))
            }
            matching_sequencer::SequencerError::ProfileInvalid(msg) => {
                AppError::bad_request(format!("Invalid profile: {msg}"))
            }
            matching_sequencer::SequencerError::MarketNotFound { market_id } => {
                AppError::market_not_found(market_id.0)
            }
            matching_sequencer::SequencerError::InvalidMarketCreationKey(message) => {
                AppError::bad_request(format!("Invalid market creation key: {message}"))
            }
            matching_sequencer::SequencerError::MarketCreationKeyConflict {
                key,
                existing_market_id,
            } => AppError::conflict(format!(
                "Market creation key {key:?} already identifies market {} with different creation fields",
                existing_market_id.0
            )),
            matching_sequencer::SequencerError::InvalidMarketGroupCreationKey(message) => {
                AppError::bad_request(format!("Invalid market group creation key: {message}"))
            }
            matching_sequencer::SequencerError::MarketGroupCreationKeyConflict {
                key,
                existing_group_id,
            } => AppError::conflict(format!(
                "Market group creation key {key:?} already identifies group {existing_group_id} with different creation fields"
            )),
            matching_sequencer::SequencerError::MarketGroupNotFound => {
                AppError::not_found("Market group not found")
            }
            matching_sequencer::SequencerError::MarketAlreadyGrouped { group_id } => {
                AppError::conflict(format!("Market already belongs to group {group_id}"))
            }
            matching_sequencer::SequencerError::BlockNotFound => {
                AppError::not_found("Block not found")
            }
            matching_sequencer::SequencerError::BlockPruned {
                requested_height,
                retention_min_height,
            } => AppError::gone(format!(
                "Block {requested_height} is older than retained history min {retention_min_height}"
            )),
            matching_sequencer::SequencerError::OrderNotFound => {
                AppError::not_found("Pending order not found")
            }
            matching_sequencer::SequencerError::OrderOwnershipMismatch => {
                AppError::forbidden("Pending order does not belong to account")
            }
            matching_sequencer::SequencerError::OracleError(msg) => {
                AppError::bad_request(format!("Oracle error: {}", msg))
            }
            matching_sequencer::SequencerError::MarketNotTradeable { market_id, status } => {
                AppError::market_not_tradeable(market_id.0, status)
            }
            matching_sequencer::SequencerError::Bridge(msg) => {
                AppError::bad_request(format!("Bridge error: {msg}"))
            }
            matching_sequencer::SequencerError::ProofUnavailable(msg) => {
                AppError::service_unavailable(format!("Proof unavailable: {msg}"))
            }
            matching_sequencer::SequencerError::IntegrityHalted => {
                AppError::sequencer_integrity_halted()
            }
            matching_sequencer::SequencerError::BlockProductionPaused => {
                AppError::conflict("Block production paused")
            }
            matching_sequencer::SequencerError::BlockInvariantFailure { height, failures } => {
                tracing::error!(
                    height = *height,
                    failures = ?failures,
                    "sequencer block invariant failure"
                );
                AppError::internal("Internal sequencer integrity failure")
            }
            matching_sequencer::SequencerError::ReservationInvariant(error) => {
                tracing::error!(%error, "sequencer reservation invariant failure");
                AppError::internal("Internal sequencer integrity failure")
            }
            matching_sequencer::SequencerError::Persistence(msg) => {
                tracing::error!(error = %msg, "sequencer persistence unavailable");
                AppError::sequencer_persistence_unavailable(
                    "Sequencer persistence is temporarily unavailable",
                )
            }
        }
    }
}

impl From<crate::history::HistoryClientError> for AppError {
    fn from(error: crate::history::HistoryClientError) -> Self {
        tracing::warn!(%error, "private history service request failed");
        AppError::history_unavailable("Historical data is temporarily unavailable")
    }
}

impl AppError {
    fn with_details(mut self, details: impl Into<String>) -> Self {
        self.body.details = Some(ApiErrorDetails {
            message: Some(details.into()),
            market_id: None,
            market_status: None,
        });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrity_halt_has_a_stable_service_unavailable_code() {
        let error = AppError::from(matching_sequencer::SequencerError::IntegrityHalted);
        assert_eq!(error.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(error.body.code, "SEQUENCER_INTEGRITY_HALTED");
        assert_eq!(
            error.body.error,
            "Sequencer writes are unavailable after an integrity failure"
        );
    }

    #[test]
    fn incomplete_history_has_a_distinct_stable_code() {
        let error = AppError::history_incomplete("window predates history");
        assert_eq!(error.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(error.body.code, "HISTORY_INCOMPLETE");
        assert_eq!(error.body.error, "window predates history");
    }

    #[test]
    fn unavailable_actor_and_persistence_are_retryable_service_failures() {
        let actor = AppError::from(matching_sequencer::SequencerError::ActorGone);
        assert_eq!(actor.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(actor.body.code, "SEQUENCER_UNAVAILABLE");

        let overload =
            AppError::from(matching_sequencer::SequencerError::ActorOverloaded { class: "write" });
        assert_eq!(overload.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(overload.body.code, "SEQUENCER_OVERLOADED");

        let persistence = AppError::from(matching_sequencer::SequencerError::Persistence(
            "disk full".into(),
        ));
        assert_eq!(persistence.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(persistence.body.code, "SEQUENCER_PERSISTENCE_UNAVAILABLE");
        assert!(!persistence.body.error.contains("disk full"));
    }

    #[test]
    fn market_not_found_preserves_machine_readable_identity() {
        let error = AppError::market_not_found(42);
        assert_eq!(error.status, StatusCode::NOT_FOUND);
        assert_eq!(error.body.code, MARKET_NOT_FOUND_CODE);
        assert_eq!(error.body.details.unwrap().market_id, Some(42));
    }

    #[test]
    fn market_not_tradeable_preserves_identity_and_status() {
        let error = AppError::market_not_tradeable(7, "resolved");
        assert_eq!(error.status, StatusCode::CONFLICT);
        assert_eq!(error.body.code, MARKET_NOT_TRADEABLE_CODE);
        let details = error.body.details.unwrap();
        assert_eq!(details.market_id, Some(7));
        assert_eq!(details.market_status.as_deref(), Some("resolved"));
    }
}
