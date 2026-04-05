use reqwest::Client;
use tracing::debug;

use super::types::*;
use crate::error::Error;

/// HTTP client for the Sybil API. Mirrors the Python `SybilClient`.
pub struct SybilClient {
    http: Client,
    base_url: String,
}

impl SybilClient {
    pub fn new(http: Client, base_url: String) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn check_response(&self, resp: reqwest::Response) -> Result<reqwest::Response, Error> {
        if resp.status().is_success() {
            Ok(resp)
        } else {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Err(Error::SybilApi { status, body })
        }
    }

    // === Health ===

    pub async fn health(&self) -> Result<HealthResponse, Error> {
        let resp = self.http.get(self.url("/v1/health")).send().await?;
        let resp = self.check_response(resp).await?;
        Ok(resp.json().await?)
    }

    // === Accounts ===

    pub async fn create_account(&self, initial_balance_nanos: u64) -> Result<AccountResponse, Error> {
        let req = CreateAccountRequest {
            initial_balance_nanos,
        };
        let resp = self
            .http
            .post(self.url("/v1/accounts"))
            .json(&req)
            .send()
            .await?;
        let resp = self.check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn fund_account(
        &self,
        account_id: u64,
        amount_nanos: u64,
    ) -> Result<AccountResponse, Error> {
        let req = FundAccountRequest { amount_nanos };
        let resp = self
            .http
            .post(self.url(&format!("/v1/accounts/{}/fund", account_id)))
            .json(&req)
            .send()
            .await?;
        let resp = self.check_response(resp).await?;
        Ok(resp.json().await?)
    }

    // === Markets ===

    pub async fn create_market(
        &self,
        req: &CreateMarketRequest,
    ) -> Result<CreateMarketResponse, Error> {
        let resp = self
            .http
            .post(self.url("/v1/markets"))
            .json(req)
            .send()
            .await?;
        let resp = self.check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn create_market_group(
        &self,
        req: &CreateMarketGroupRequest,
    ) -> Result<MarketGroupResponse, Error> {
        let resp = self
            .http
            .post(self.url("/v1/markets/groups"))
            .json(req)
            .send()
            .await?;
        let resp = self.check_response(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn resolve_market(
        &self,
        market_id: u32,
        payout_nanos: u64,
    ) -> Result<(), Error> {
        let req = ResolveMarketRequest { payout_nanos };
        let resp = self
            .http
            .post(self.url(&format!("/v1/markets/{}/resolve", market_id)))
            .json(&req)
            .send()
            .await?;
        self.check_response(resp).await?;
        Ok(())
    }

    // === Orders ===

    pub async fn submit_orders(&self, req: &SubmitOrderRequest) -> Result<bool, Error> {
        let resp = self
            .http
            .post(self.url("/v1/orders"))
            .json(req)
            .send()
            .await?;
        let resp = self.check_response(resp).await?;
        let result: OrderAcceptedResponse = resp.json().await?;
        Ok(result.accepted)
    }

    // === Blocks (SSE) ===

    /// Stream blocks via SSE. Returns an async iterator of `BlockResponse`.
    /// The caller should handle reconnection on error.
    pub async fn stream_blocks(
        &self,
    ) -> Result<impl futures_util::Stream<Item = Result<BlockResponse, Error>>, Error> {
        let resp = self
            .http
            .get(self.url("/v1/blocks/stream"))
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
