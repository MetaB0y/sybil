use std::collections::HashMap;
use std::time::Duration;

use reqwest::Client;
use serde::de::DeserializeOwned;
use tracing::debug;

use crate::error::Error;
use sybil_api_types::*;

/// HTTP client for the Sybil API. This is THE shared client (SYB-171); it is
/// typed against [`sybil_api_types`] and mirrors the Python `SybilClient`.
pub struct SybilClient {
    http: Client,
    base_url: String,
    service_token: Option<String>,
}

impl SybilClient {
    /// Construct a client over a caller-provided `reqwest::Client`. Use this
    /// when you want to control the transport (TLS backend, connection pool,
    /// per-request timeouts such as the long-lived SSE stream).
    pub fn new(http: Client, base_url: String, service_token: Option<String>) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            service_token,
        }
    }

    /// Convenience constructor that builds a `reqwest::Client` with sane default
    /// timeouts. Callers with their own transport requirements (e.g. the
    /// long-poll SSE stream) should use [`SybilClient::new`] with a client they
    /// configure themselves.
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

    // === Accounts ===

    pub async fn create_account(
        &self,
        initial_balance_nanos: u64,
    ) -> Result<AccountResponse, Error> {
        let req = CreateAccountRequest {
            initial_balance_nanos,
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

    // === Orders ===

    pub async fn submit_orders(&self, req: &SubmitOrderRequest) -> Result<bool, Error> {
        let resp = self
            .with_service_auth(self.http.post(self.url("/v1/orders")))
            .json(req)
            .send()
            .await?;
        let result: OrderAcceptedResponse = self.decode(resp).await?;
        Ok(result.accepted)
    }

    /// Submit a signed order. The caller must supply and sign a strictly
    /// increasing per-account nonce in [`SubmitSignedOrderRequest::nonce`].
    pub async fn submit_signed_order(&self, req: &SubmitSignedOrderRequest) -> Result<bool, Error> {
        let resp = self
            .with_service_auth(self.http.post(self.url("/v1/orders/signed")))
            .json(req)
            .send()
            .await?;
        let result: OrderAcceptedResponse = self.decode(resp).await?;
        Ok(result.accepted)
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

    // === Blocks (SSE) ===

    /// Stream blocks via SSE. Returns an async iterator of `BlockResponse`.
    /// The caller should handle reconnection on error.
    pub async fn stream_blocks(
        &self,
    ) -> Result<impl futures_util::Stream<Item = Result<BlockResponse, Error>>, Error> {
        let resp = self
            .with_service_auth(self.http.get(self.url("/v1/blocks/stream")))
            .timeout(std::time::Duration::from_secs(86400)) // SSE stream: effectively no timeout
            .send()
            .await?;
        let resp = self.check_response(resp).await?;

        let stream = futures_util::stream::unfold(resp, |mut resp| async move {
            loop {
                match resp.chunk().await {
                    Ok(Some(chunk)) => {
                        let text = String::from_utf8_lossy(&chunk);
                        for line in text.lines() {
                            if let Some(data) = line.strip_prefix("data:") {
                                let data = data.trim();
                                if data.is_empty() {
                                    continue;
                                }
                                match serde_json::from_str::<BlockResponse>(data) {
                                    Ok(block) => return Some((Ok(block), resp)),
                                    Err(e) => {
                                        debug!("skipping unparseable SSE data: {}", e);
                                    }
                                }
                            }
                        }
                    }
                    Ok(None) => return None, // Stream ended
                    Err(e) => return Some((Err(Error::Http(e)), resp)),
                }
            }
        });

        Ok(stream)
    }
}
