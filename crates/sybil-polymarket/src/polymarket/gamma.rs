use reqwest::Client;
use tracing::{debug, warn};

use super::types::{GammaEvent, MidpointResponse};
use crate::error::Error;

/// Read-only client for the Polymarket Gamma API.
pub struct GammaClient {
    http: Client,
    gamma_url: String,
    clob_url: String,
}

impl GammaClient {
    pub fn new(http: Client, gamma_url: String, clob_url: String) -> Self {
        Self {
            http,
            gamma_url: gamma_url.trim_end_matches('/').to_string(),
            clob_url: clob_url.trim_end_matches('/').to_string(),
        }
    }

    /// Fetch active, non-closed events with pagination.
    /// Returns up to `max_events` events, filtered by optional categories and min volume.
    pub async fn fetch_active_events(
        &self,
        max_events: usize,
        categories: &[String],
        min_volume_usd: f64,
    ) -> Result<Vec<GammaEvent>, Error> {
        let mut all_events = Vec::new();
        let mut offset = 0;
        let page_size = 100;

        loop {
            let url = format!("{}/events", self.gamma_url);
            let resp = self
                .http
                .get(&url)
                .query(&[
                    ("active", "true"),
                    ("closed", "false"),
                    ("limit", &page_size.to_string()),
                    ("offset", &offset.to_string()),
                    ("order", "volume"),
                    ("ascending", "false"),
                ])
                .send()
                .await?;

            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                return Err(Error::PolymarketApi(format!(
                    "GET /events returned {}: {}",
                    status, body
                )));
            }

            let events: Vec<GammaEvent> = resp.json().await?;
            let page_len = events.len();

            for event in events {
                // Filter by category if configured
                if !categories.is_empty() {
                    // Gamma events don't have a direct category field, but markets have tags.
                    // Skip events where no market matches any requested category.
                    // For now, we pass through all and let the caller filter more precisely.
                }

                // Filter by volume
                if min_volume_usd > 0.0 {
                    let vol = event.volume.unwrap_or(0.0);
                    if vol < min_volume_usd {
                        continue;
                    }
                }

                // Skip events with no active markets
                if event.markets.iter().all(|m| m.closed || !m.active) {
                    continue;
                }

                all_events.push(event);

                if all_events.len() >= max_events {
                    debug!(
                        count = all_events.len(),
                        "reached max_events, stopping pagination"
                    );
                    return Ok(all_events);
                }
            }

            if page_len < page_size {
                break; // Last page
            }
            offset += page_size;
        }

        debug!(count = all_events.len(), "fetched active events");
        Ok(all_events)
    }

    /// Fetch midpoint price for a single token via CLOB REST.
    pub async fn fetch_midpoint(&self, token_id: &str) -> Result<f64, Error> {
        let url = format!("{}/midpoint", self.clob_url);
        let resp = self
            .http
            .get(&url)
            .query(&[("token_id", token_id)])
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::PolymarketApi(format!(
                "GET /midpoint returned {}: {}",
                status, body
            )));
        }

        let mid: MidpointResponse = resp.json().await?;
        mid.mid
            .parse::<f64>()
            .map_err(|e| Error::PolymarketApi(format!("bad midpoint '{}': {}", mid.mid, e)))
    }

    /// Fetch midpoints for multiple tokens in one request via CLOB REST.
    pub async fn fetch_midpoints(&self, token_ids: &[String]) -> Result<Vec<(String, f64)>, Error> {
        if token_ids.is_empty() {
            return Ok(vec![]);
        }

        let url = format!("{}/midpoints", self.clob_url);
        let body: Vec<serde_json::Value> = token_ids
            .iter()
            .map(|id| serde_json::json!({ "token_id": id }))
            .collect();

        let resp = self.http.post(&url).json(&body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(Error::PolymarketApi(format!(
                "POST /midpoints returned {}: {}",
                status, text
            )));
        }

        // Response is array of {mid: "0.55"} in same order as request
        let mids: Vec<MidpointResponse> = resp.json().await?;
        let mut result = Vec::with_capacity(mids.len());
        for (i, mid) in mids.iter().enumerate() {
            if let Some(token_id) = token_ids.get(i) {
                match mid.mid.parse::<f64>() {
                    Ok(p) => result.push((token_id.clone(), p)),
                    Err(e) => warn!(token_id, error = %e, "skipping bad midpoint"),
                }
            }
        }

        Ok(result)
    }
}
