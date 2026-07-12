use std::collections::HashMap;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde::de::DeserializeOwned;
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::error::Error;
use sybil_api_types::ws::{
    PUBLIC_BLOCK_STREAM_VERSION, PublicBlockStreamMessage, PublicBlockStreamPayload,
};
use sybil_api_types::*;

/// Result of checking the server's enclave attestation endpoint.
///
/// The only currently accepted result is explicitly untrusted. A future real
/// Nitro response must pass CBOR/COSE, certificate-chain, signature, freshness,
/// and application-policy checks before this API can return a trusted variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttestationVerification {
    /// The server exposed the development shape, not cryptographic evidence.
    StubAccepted {
        attestation: AttestationResponse,
        warning: String,
    },
}

const STUB_ATTESTATION_WARNING: &str = "development attestation stub accepted; no Nitro signature, certificate chain, or PCR was verified";

/// HTTP client for the Sybil API. This is THE shared client (SYB-171); it is
/// typed against [`sybil_api_types`] and mirrors the Python `SybilClient`.
pub struct SybilClient {
    http: Client,
    base_url: String,
    service_token: Option<String>,
}

impl SybilClient {
    /// Construct a client over a caller-provided `reqwest::Client`. Use this
    /// when you want to control the HTTP transport (TLS backend, connection
    /// pool, per-request timeouts).
    pub fn new(http: Client, base_url: String, service_token: Option<String>) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            service_token,
        }
    }

    /// Convenience constructor that builds a `reqwest::Client` with sane default
    /// timeouts. Callers with their own HTTP transport requirements should use
    /// [`SybilClient::new`] with a client they configure themselves.
    pub fn with_defaults(base_url: String, service_token: Option<String>) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build default reqwest client");
        Self::new(http, base_url, service_token)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn block_ws_url(&self, from_block: Option<u64>) -> Result<String, Error> {
        let http_url = self.url("/v2/blocks/ws");
        let mut url = if let Some(rest) = http_url.strip_prefix("http://") {
            format!("ws://{rest}")
        } else if let Some(rest) = http_url.strip_prefix("https://") {
            format!("wss://{rest}")
        } else if http_url.starts_with("ws://") || http_url.starts_with("wss://") {
            http_url
        } else {
            return Err(Error::Protocol(format!(
                "base_url must start with http:// or https:// for block stream: {}",
                self.base_url
            )));
        };

        if let Some(from) = from_block {
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str("from_block=");
            url.push_str(&from.to_string());
        }
        Ok(url)
    }

    fn with_service_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self.service_token.as_deref() {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    async fn check_response(&self, resp: reqwest::Response) -> Result<reqwest::Response, Error> {
        if resp.status().is_success() {
            Ok(resp)
        } else {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Err(Error::Api { status, body })
        }
    }

    async fn decode<T: DeserializeOwned>(&self, resp: reqwest::Response) -> Result<T, Error> {
        let resp = self.check_response(resp).await?;
        Ok(resp.json().await?)
    }

    // === Health ===

    pub async fn health(&self) -> Result<HealthResponse, Error> {
        let resp = self
            .with_service_auth(self.http.get(self.url("/v1/health")))
            .send()
            .await?;
        self.decode(resp).await
    }

    /// Fetch and classify the server's enclave attestation.
    ///
    /// Development stubs are accepted only as [`AttestationVerification::StubAccepted`]
    /// and emit a warning. Non-stub responses fail closed until real Nitro
    /// verification is implemented; they are never silently trusted.
    pub async fn verify_attestation(&self) -> Result<AttestationVerification, Error> {
        let resp = self
            .with_service_auth(self.http.get(self.url("/v1/attestation")))
            .send()
            .await?;
        let attestation: AttestationResponse = self.decode(resp).await?;
        classify_attestation(attestation)
    }

    // === Accounts ===

    pub async fn create_account(
        &self,
        initial_balance_nanos: u64,
        initial_key: RegisterKeyRequest,
    ) -> Result<AccountResponse, Error> {
        let req = CreateAccountRequest {
            initial_balance_nanos,
            initial_key: Some(initial_key),
        };
        let resp = self
            .with_service_auth(self.http.post(self.url("/v1/accounts")))
            .json(&req)
            .send()
            .await?;
        self.decode(resp).await
    }

    /// Deprecated operator-only bare account creation. This requires the
    /// service token outside dev mode; self-service callers must use
    /// [`Self::create_account`] with an initial signing key.
    pub async fn create_bare_account(
        &self,
        initial_balance_nanos: u64,
    ) -> Result<AccountResponse, Error> {
        let req = CreateAccountRequest {
            initial_balance_nanos,
            initial_key: None,
        };
        let resp = self
            .with_service_auth(self.http.post(self.url("/v1/accounts")))
            .json(&req)
            .send()
            .await?;
        self.decode(resp).await
    }

    pub async fn get_account(&self, account_id: u64) -> Result<AccountResponse, Error> {
        let resp = self
            .with_service_auth(
                self.http
                    .get(self.url(&format!("/v1/accounts/{}", account_id))),
            )
            .send()
            .await?;
        self.decode(resp).await
    }

    pub async fn fund_account(
        &self,
        account_id: u64,
        amount_nanos: u64,
    ) -> Result<AccountResponse, Error> {
        let req = FundAccountRequest { amount_nanos };
        let resp = self
            .with_service_auth(
                self.http
                    .post(self.url(&format!("/v1/accounts/{}/fund", account_id))),
            )
            .json(&req)
            .send()
            .await?;
        self.decode(resp).await
    }

    // === Bridge ===

    pub async fn bridge_status(&self) -> Result<BridgeStatusResponse, Error> {
        let resp = self
            .with_service_auth(self.http.get(self.url("/v1/bridge/status")))
            .send()
            .await?;
        self.decode(resp).await
    }

    pub async fn bridge_account_by_key(
        &self,
        sybil_account_key_hex: &str,
    ) -> Result<BridgeAccountKeyResponse, Error> {
        let resp = self
            .with_service_auth(self.http.get(self.url(&format!(
                "/v1/bridge/accounts/by-key/{}",
                sybil_account_key_hex
            ))))
            .send()
            .await?;
        self.decode(resp).await
    }

    pub async fn submit_l1_deposit(
        &self,
        req: &SubmitL1DepositRequest,
    ) -> Result<BridgeDepositResponse, Error> {
        let resp = self
            .with_service_auth(self.http.post(self.url("/v1/bridge/deposits")))
            .json(req)
            .send()
            .await?;
        self.decode(resp).await
    }

    pub async fn submit_l1_withdrawal_event(
        &self,
        req: &SubmitL1WithdrawalEventRequest,
    ) -> Result<BridgeWithdrawalL1EventResponse, Error> {
        let resp = self
            .with_service_auth(self.http.post(self.url("/v1/bridge/withdrawals/l1-events")))
            .json(req)
            .send()
            .await?;
        self.decode(resp).await
    }

    pub async fn observe_l1_height(
        &self,
        req: &ObserveL1HeightRequest,
    ) -> Result<ObserveL1HeightResponse, Error> {
        let resp = self
            .with_service_auth(self.http.post(self.url("/v1/bridge/l1-height")))
            .json(req)
            .send()
            .await?;
        self.decode(resp).await
    }

    pub async fn create_bridge_withdrawal(
        &self,
        req: &CreateBridgeWithdrawalRequest,
    ) -> Result<BridgeWithdrawalResponse, Error> {
        let resp = self
            .with_service_auth(self.http.post(self.url("/v1/bridge/withdrawals")))
            .json(req)
            .send()
            .await?;
        self.decode(resp).await
    }

    pub async fn create_signed_bridge_withdrawal(
        &self,
        req: &CreateSignedBridgeWithdrawalRequest,
    ) -> Result<BridgeWithdrawalResponse, Error> {
        let resp = self
            .with_service_auth(self.http.post(self.url("/v1/bridge/withdrawals/signed")))
            .json(req)
            .send()
            .await?;
        self.decode(resp).await
    }

    // === Markets ===

    pub async fn create_market(
        &self,
        req: &CreateMarketRequest,
    ) -> Result<CreateMarketResponse, Error> {
        let resp = self
            .with_service_auth(self.http.post(self.url("/v1/markets")))
            .json(req)
            .send()
            .await?;
        self.decode(resp).await
    }

    pub async fn list_market_summaries(&self) -> Result<Vec<MarketSummaryResponse>, Error> {
        let resp = self
            .with_service_auth(self.http.get(self.url("/v1/markets/summary")))
            .send()
            .await?;
        self.decode(resp).await
    }

    pub async fn create_market_group(
        &self,
        req: &CreateMarketGroupRequest,
    ) -> Result<MarketGroupResponse, Error> {
        let resp = self
            .with_service_auth(self.http.post(self.url("/v1/markets/groups")))
            .json(req)
            .send()
            .await?;
        self.decode(resp).await
    }

    pub async fn list_market_groups(&self) -> Result<Vec<MarketGroupResponse>, Error> {
        let resp = self
            .with_service_auth(self.http.get(self.url("/v1/markets/groups")))
            .send()
            .await?;
        self.decode(resp).await
    }

    pub async fn extend_market_group(
        &self,
        group_id: u64,
        req: &ExtendMarketGroupRequest,
    ) -> Result<MarketGroupResponse, Error> {
        let resp = self
            .with_service_auth(
                self.http
                    .post(self.url(&format!("/v1/markets/groups/{group_id}/members"))),
            )
            .json(req)
            .send()
            .await?;
        self.decode(resp).await
    }

    /// Resolve a market, returning the typed server response. Prefer the
    /// [`SybilClient::resolve_market`] / [`SybilClient::resolve_market_attested`]
    /// helpers for the common cases; this is the raw form used by callers that
    /// need the [`ResolveMarketResponse`] (e.g. `sybil-admin`).
    pub async fn resolve_market_request(
        &self,
        market_id: u32,
        req: &ResolveMarketRequest,
    ) -> Result<ResolveMarketResponse, Error> {
        let resp = self
            .with_service_auth(
                self.http
                    .post(self.url(&format!("/v1/markets/{}/resolve", market_id))),
            )
            .json(req)
            .send()
            .await?;
        self.decode(resp).await
    }

    pub async fn resolve_market(&self, market_id: u32, payout_nanos: u64) -> Result<(), Error> {
        let req = ResolveMarketRequest {
            payout_nanos,
            attestation: None,
        };
        let resp = self
            .with_service_auth(
                self.http
                    .post(self.url(&format!("/v1/markets/{}/resolve", market_id))),
            )
            .json(&req)
            .send()
            .await?;
        self.check_response(resp).await?;
        Ok(())
    }

    /// Resolve a market via a signed attestation. Does not require `--dev-mode`.
    pub async fn resolve_market_attested(
        &self,
        market_id: u32,
        payout_nanos: u64,
        attestation: SignedAttestationDto,
    ) -> Result<(), Error> {
        let req = ResolveMarketRequest {
            payout_nanos,
            attestation: Some(attestation),
        };
        let resp = self
            .with_service_auth(
                self.http
                    .post(self.url(&format!("/v1/markets/{}/resolve", market_id))),
            )
            .json(&req)
            .send()
            .await?;
        self.check_response(resp).await?;
        Ok(())
    }

    /// Fetch the current resolution state for a market. Returns `None` when
    /// the server reports 404 (e.g. market does not exist).
    pub async fn get_market_resolution(
        &self,
        market_id: u32,
    ) -> Result<Option<ResolutionResponse>, Error> {
        let resp = self
            .with_service_auth(
                self.http
                    .get(self.url(&format!("/v1/markets/{}/resolution", market_id))),
            )
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        Ok(Some(self.decode(resp).await?))
    }

    // === Automated resolution review board (SYB-48) ===

    /// Record (or refresh) an auto-resolution proposal on the review board.
    /// This never settles a market; finalization still flows through the signed
    /// `resolve_market_attested` path. Returns the stored entry so the caller
    /// learns the authoritative eta and any operator decision.
    pub async fn submit_auto_resolution(
        &self,
        req: &SubmitAutoResolutionRequest,
    ) -> Result<AutoResolutionEntryResponse, Error> {
        let resp = self
            .with_service_auth(self.http.post(self.url("/v1/admin/auto-resolutions")))
            .json(req)
            .send()
            .await?;
        self.decode(resp).await
    }

    /// List every recorded auto-resolution proposal (pending, needs-review,
    /// escalated, approved, rejected, resolved).
    pub async fn list_auto_resolutions(&self) -> Result<Vec<AutoResolutionEntryResponse>, Error> {
        let resp = self
            .with_service_auth(self.http.get(self.url("/v1/admin/auto-resolutions")))
            .send()
            .await?;
        let list: AutoResolutionListResponse = self.decode(resp).await?;
        Ok(list.entries)
    }

    // === Orders ===

    pub async fn submit_orders(&self, req: &SubmitOrderRequest) -> Result<Vec<u64>, Error> {
        let resp = self
            .with_service_auth(self.http.post(self.url("/v1/orders")))
            .json(req)
            .send()
            .await?;
        let result: OrderAcceptedResponse = self.decode(resp).await?;
        Ok(result.order_ids)
    }

    /// Submit a signed order. The caller must supply and sign a strictly
    /// increasing per-account nonce in [`SubmitSignedOrderRequest::nonce`].
    pub async fn submit_signed_order(
        &self,
        req: &SubmitSignedOrderRequest,
    ) -> Result<Vec<u64>, Error> {
        let resp = self
            .with_service_auth(self.http.post(self.url("/v1/orders/signed")))
            .json(req)
            .send()
            .await?;
        let result: OrderAcceptedResponse = self.decode(resp).await?;
        Ok(result.order_ids)
    }

    /// Cancel a resting order with a signed payload. The caller must supply
    /// and sign a strictly increasing per-account nonce.
    pub async fn cancel_signed_order(&self, req: &CancelSignedOrderRequest) -> Result<bool, Error> {
        let resp = self
            .with_service_auth(self.http.post(self.url("/v1/orders/cancel/signed")))
            .json(req)
            .send()
            .await?;
        let result: CancelOrderResponse = self.decode(resp).await?;
        Ok(result.cancelled)
    }

    /// Push mirror-derived metadata (event id/title, images, end dates,
    /// category) to sybil-api. Off-block — never enters `MarketMetadata` or
    /// the block digest. In production this is a service route protected by
    /// `SYBIL_SERVICE_TOKEN`; dev mode skips that check for local workflows.
    pub async fn set_market_metadata(
        &self,
        market_id: u32,
        req: &SetMarketMetadataRequest,
    ) -> Result<(), Error> {
        let resp = self
            .with_service_auth(
                self.http
                    .post(self.url(&format!("/v1/markets/{}/metadata", market_id))),
            )
            .json(req)
            .send()
            .await?;
        let _ = self.check_response(resp).await?;
        Ok(())
    }

    /// Push the full Polymarket event JSON to sybil-api's snapshot store.
    /// Idempotent upsert; service-authenticated in production.
    pub async fn put_event_raw(
        &self,
        event_id: &str,
        value: &serde_json::Value,
    ) -> Result<(), Error> {
        let resp = self
            .with_service_auth(
                self.http
                    .put(self.url(&format!("/v1/events/{}/raw", event_id))),
            )
            .json(value)
            .send()
            .await?;
        let _ = self.check_response(resp).await?;
        Ok(())
    }

    /// Push reference prices to sybil-api (display only, not matching logic).
    pub async fn set_reference_prices(&self, prices: &HashMap<u32, u64>) -> Result<(), Error> {
        let body = serde_json::json!({ "prices": prices });
        let resp = self
            .with_service_auth(self.http.post(self.url("/v1/markets/prices/reference")))
            .json(&body)
            .send()
            .await?;
        let _ = self.check_response(resp).await?;
        Ok(())
    }

    // === Blocks (WebSocket) ===

    /// Stream blocks via the first-party WebSocket transport.
    ///
    /// Returns an async iterator of privacy-preserving `PublicBlockResponse`.
    /// Account-attributed canonical rows are never present on this stream.
    /// The caller should handle
    /// reconnection on error; callers tracking a last seen height should use
    /// [`SybilClient::stream_blocks_from_block`] so reconnects replay missed
    /// committed blocks before switching back to live.
    pub async fn stream_blocks(
        &self,
    ) -> Result<impl futures_util::Stream<Item = Result<PublicBlockResponse, Error>> + use<>, Error>
    {
        self.stream_blocks_from_block(None).await
    }

    /// Stream blocks via WebSocket, optionally replaying from `from_block`.
    ///
    /// When `from_block` is `Some(N)`, the server sends every retained block
    /// from `N` through the current head, then follows the live stream. If `N`
    /// is below the retained `blocks_full` floor, the stream yields
    /// [`Error::RetentionGap`] so the caller can cold-resync.
    ///
    /// The caller should handle reconnection on error.
    pub async fn stream_blocks_from_block(
        &self,
        from_block: Option<u64>,
    ) -> Result<impl futures_util::Stream<Item = Result<PublicBlockResponse, Error>> + use<>, Error>
    {
        let url = self.block_ws_url(from_block)?;
        let (socket, _) = tokio_tungstenite::connect_async(&url)
            .await
            .map_err(|e| Error::WebSocket(e.to_string()))?;

        let stream = futures_util::stream::unfold(socket, |mut socket| async move {
            loop {
                let msg = match socket.next().await {
                    Some(Ok(msg)) => msg,
                    Some(Err(e)) => return Some((Err(Error::WebSocket(e.to_string())), socket)),
                    None => return None,
                };

                match msg {
                    Message::Text(text) => match decode_block_stream_message(text.as_ref()) {
                        Ok(Some(block)) => return Some((Ok(block), socket)),
                        Ok(None) => continue,
                        Err(e) => return Some((Err(e), socket)),
                    },
                    Message::Binary(bytes) => {
                        let text = match std::str::from_utf8(bytes.as_ref()) {
                            Ok(text) => text,
                            Err(e) => {
                                return Some((
                                    Err(Error::Protocol(format!(
                                        "non-UTF8 binary block stream message: {e}"
                                    ))),
                                    socket,
                                ));
                            }
                        };
                        match decode_block_stream_message(text) {
                            Ok(Some(block)) => return Some((Ok(block), socket)),
                            Ok(None) => continue,
                            Err(e) => return Some((Err(e), socket)),
                        }
                    }
                    Message::Ping(data) => {
                        if let Err(e) = socket.send(Message::Pong(data)).await {
                            return Some((Err(Error::WebSocket(e.to_string())), socket));
                        }
                    }
                    Message::Pong(_) => {}
                    Message::Close(Some(frame)) => {
                        if frame.reason.is_empty() {
                            return None;
                        }
                        return Some((
                            Err(Error::WebSocket(format!(
                                "server closed block stream ({}): {}",
                                frame.code, frame.reason
                            ))),
                            socket,
                        ));
                    }
                    Message::Close(None) => return None,
                    Message::Frame(_) => {}
                }
            }
        });

        Ok(stream)
    }
}

fn classify_attestation(
    attestation: AttestationResponse,
) -> Result<AttestationVerification, Error> {
    if !attestation.is_stub {
        return Err(Error::Attestation(
            "real enclave attestation verification is not implemented; refusing to trust non-stub response"
                .to_string(),
        ));
    }

    tracing::warn!("{STUB_ATTESTATION_WARNING}");
    Ok(AttestationVerification::StubAccepted {
        attestation,
        warning: STUB_ATTESTATION_WARNING.to_string(),
    })
}

fn decode_block_stream_message(text: &str) -> Result<Option<PublicBlockResponse>, Error> {
    let msg: PublicBlockStreamMessage = serde_json::from_str(text)?;
    if msg.v != PUBLIC_BLOCK_STREAM_VERSION {
        return Err(Error::Protocol(format!(
            "unsupported block stream version {}; expected {}",
            msg.v, PUBLIC_BLOCK_STREAM_VERSION
        )));
    }

    match msg.payload {
        PublicBlockStreamPayload::Block { data } => Ok(Some(*data)),
        PublicBlockStreamPayload::ReplayComplete { .. } => Ok(None),
        PublicBlockStreamPayload::Lagged {
            skipped,
            last_sent_height,
        } => Err(Error::BlockStreamLagged {
            skipped,
            last_sent_height,
        }),
        PublicBlockStreamPayload::RetentionGap {
            requested_height,
            retention_min_height,
            head_height,
        } => Err(Error::RetentionGap {
            requested_height,
            retention_min_height,
            head_height,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{AttestationVerification, classify_attestation};
    use crate::Error;
    use sybil_api_types::AttestationResponse;

    fn attestation(is_stub: bool) -> AttestationResponse {
        AttestationResponse {
            pcr_values: HashMap::new(),
            enclave_pubkey: String::new(),
            report_data: String::new(),
            signature: String::new(),
            is_stub,
        }
    }

    #[test]
    fn stub_attestation_is_explicitly_untrusted() {
        let result = classify_attestation(attestation(true)).expect("stub is classified");
        let AttestationVerification::StubAccepted {
            attestation,
            warning,
        } = result;
        assert!(attestation.is_stub);
        assert!(warning.contains("no Nitro signature"));
        assert!(warning.contains("no Nitro signature, certificate chain, or PCR was verified"));
    }

    #[test]
    fn non_stub_attestation_fails_closed_until_crypto_verification_exists() {
        let error = classify_attestation(attestation(false)).expect_err("must fail closed");
        assert!(
            matches!(error, Error::Attestation(message) if message.contains("refusing to trust"))
        );
    }
}
