use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub body: ErrorBody,
    pub retry_after_secs: Option<u64>,
}

impl AppError {
    pub fn bad_request(error: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            body: ErrorBody {
                error: error.into(),
                code: "BAD_REQUEST".to_string(),
                details: None,
            },
            retry_after_secs: None,
        }
    }

    pub fn not_found(error: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            body: ErrorBody {
                error: error.into(),
                code: "NOT_FOUND".to_string(),
                details: None,
            },
            retry_after_secs: None,
        }
    }

    pub fn gone(error: impl Into<String>) -> Self {
        Self {
            status: StatusCode::GONE,
            body: ErrorBody {
                error: error.into(),
                code: "RETENTION_GONE".to_string(),
                details: None,
            },
            retry_after_secs: None,
        }
    }

    pub fn forbidden(error: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            body: ErrorBody {
                error: error.into(),
                code: "FORBIDDEN".to_string(),
                details: None,
            },
            retry_after_secs: None,
        }
    }

    pub fn unauthorized(error: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            body: ErrorBody {
                error: error.into(),
                code: "UNAUTHORIZED".to_string(),
                details: None,
            },
            retry_after_secs: None,
        }
    }

    pub fn internal(error: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: ErrorBody {
                error: error.into(),
                code: "INTERNAL_ERROR".to_string(),
                details: None,
            },
            retry_after_secs: None,
        }
    }

    pub fn conflict(error: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            body: ErrorBody {
                error: error.into(),
                code: "CONFLICT".to_string(),
                details: None,
            },
            retry_after_secs: None,
        }
    }

    pub fn replay_nonce_stale(error: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            body: ErrorBody {
                error: error.into(),
                code: "REPLAY_NONCE_STALE".to_string(),
                details: None,
            },
            retry_after_secs: None,
        }
    }

    pub fn service_unavailable(error: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            body: ErrorBody {
                error: error.into(),
                code: "SERVICE_UNAVAILABLE".to_string(),
                details: None,
            },
            retry_after_secs: None,
        }
    }

    pub fn mempool_full() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            body: ErrorBody {
                error: "Mempool full".to_string(),
                code: "MEMPOOL_FULL".to_string(),
                details: None,
            },
            retry_after_secs: None,
        }
    }

    pub fn rate_limited(retry_after_secs: u64) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            body: ErrorBody {
                error: "Rate limited".to_string(),
                code: "RATE_LIMITED".to_string(),
                details: None,
            },
            retry_after_secs: Some(retry_after_secs),
        }
    }

    pub fn dev_mode_required() -> Self {
        Self::forbidden("This endpoint requires dev mode (--dev-mode)")
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = axum::Json(self.body);
        let mut response = (self.status, body).into_response();
        if let Some(retry_after_secs) = self.retry_after_secs {
            if let Ok(value) = HeaderValue::from_str(&retry_after_secs.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
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
            matching_sequencer::SequencerError::GenesisHashUnavailable => {
                AppError::service_unavailable(format!("{}", err))
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
                AppError::internal("Sequencer actor shut down")
            }
            matching_sequencer::SequencerError::AccountAlreadyRegistered => {
                AppError::conflict("Public key already registered to an account")
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
            matching_sequencer::SequencerError::ProfileInvalid(ref msg) => {
                AppError::bad_request(format!("Invalid profile: {msg}"))
            }
            matching_sequencer::SequencerError::MarketNotFound => {
                AppError::not_found("Market not found")
            }
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
            matching_sequencer::SequencerError::OracleError(ref msg) => {
                AppError::bad_request(format!("Oracle error: {}", msg))
            }
            matching_sequencer::SequencerError::InvalidMarketState(ref msg) => {
                AppError::conflict(format!("Invalid market state: {msg}"))
            }
            matching_sequencer::SequencerError::Bridge(ref msg) => {
                AppError::bad_request(format!("Bridge error: {msg}"))
            }
            matching_sequencer::SequencerError::ProofUnavailable(ref msg) => {
                AppError::service_unavailable(format!("Proof unavailable: {msg}"))
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
            matching_sequencer::SequencerError::Persistence(ref msg) => {
                AppError::internal(format!("Persistence error: {msg}"))
            }
        }
    }
}

impl AppError {
    fn with_details(mut self, details: impl Into<String>) -> Self {
        self.body.details = Some(details.into());
        self
    }
}
