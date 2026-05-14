use std::collections::HashMap;

use reqwest::Client;
use tracing::{debug, warn};

use super::types::{GammaEvent, MidpointResponse};
use crate::error::Error;

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum MidpointsResponse {
    Array(Vec<MidpointResponse>),
    Map(HashMap<String, serde_json::Value>),
}

fn parse_midpoint_value(token_id: &str, value: &serde_json::Value) -> Result<Option<f64>, Error> {
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(raw) => {
            if raw.is_empty() {
                Ok(None)
            } else {
                raw.parse::<f64>().map(Some).map_err(|e| {
                    Error::PolymarketApi(format!("bad midpoint '{raw}' for {token_id}: {e}"))
                })
            }
        }
        serde_json::Value::Number(number) => number.as_f64().map(Some).ok_or_else(|| {
            Error::PolymarketApi(format!("bad midpoint number for {token_id}: {number}"))
        }),
        other => Err(Error::PolymarketApi(format!(
            "unsupported midpoint payload for {token_id}: {other}"
        ))),
    }
}

fn decode_midpoints_response(body: &str, chunk: &[String]) -> Result<Vec<(String, f64)>, Error> {
    let response: MidpointsResponse = serde_json::from_str(body)?;
    let mut all_results = Vec::with_capacity(chunk.len());

    match response {
        MidpointsResponse::Array(mids) => {
            for (i, mid) in mids.iter().enumerate() {
                if let Some(token_id) = chunk.get(i) {
                    match mid.mid.parse::<f64>() {
                        Ok(price) => all_results.push((token_id.clone(), price)),
                        Err(error) => warn!(token_id, error = %error, "skipping bad midpoint"),
                    }
                }
            }
        }
        MidpointsResponse::Map(mids) => {
            for token_id in chunk {
                let Some(value) = mids.get(token_id) else {
                    continue;
                };
                match parse_midpoint_value(token_id, value) {
                    Ok(Some(price)) => all_results.push((token_id.clone(), price)),
                    Ok(None) => {}
                    Err(error) => warn!(token_id, error = %error, "skipping bad midpoint"),
                }
            }
        }
    }

    Ok(all_results)
}

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
        excluded_categories: &[String],
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
                if !event.matches_category_filters(categories, excluded_categories) {
                    debug!(
                        event_id = event.id,
                        title = event.title,
                        include = ?categories,
                        exclude = ?excluded_categories,
                        "skipping event by category filters"
                    );
                    continue;
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

    /// Fetch closed (potentially resolved) events. Used by the resolution
    /// actor to reconcile mirrored markets against Polymarket settlement.
    pub async fn fetch_closed_events(&self, max_events: usize) -> Result<Vec<GammaEvent>, Error> {
        let mut all_events = Vec::new();
        let mut offset = 0;
        let page_size = 100;

        loop {
            let url = format!("{}/events", self.gamma_url);
            let resp = self
                .http
                .get(&url)
                .query(&[
                    ("closed", "true"),
                    ("limit", &page_size.to_string()),
                    ("offset", &offset.to_string()),
                    ("order", "endDate"),
                    ("ascending", "false"),
                ])
                .send()
                .await?;

            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                return Err(Error::PolymarketApi(format!(
                    "GET /events?closed=true returned {}: {}",
                    status, body
                )));
            }

            let events: Vec<GammaEvent> = resp.json().await?;
            let page_len = events.len();
            for event in events {
                all_events.push(event);
                if all_events.len() >= max_events {
                    return Ok(all_events);
                }
            }
            if page_len < page_size {
                break;
            }
            offset += page_size;
        }

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

    /// Fetch midpoints for multiple tokens via CLOB REST, batching to stay under payload limits.
    pub async fn fetch_midpoints(&self, token_ids: &[String]) -> Result<Vec<(String, f64)>, Error> {
        if token_ids.is_empty() {
            return Ok(vec![]);
        }

        let url = format!("{}/midpoints", self.clob_url);
        let mut all_results = Vec::with_capacity(token_ids.len());

        // Polymarket rejects large payloads. Batch into chunks of 200 tokens.
        for chunk in token_ids.chunks(200) {
            let body: Vec<serde_json::Value> = chunk
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

            let text = resp.text().await?;
            all_results.extend(decode_midpoints_response(&text, chunk)?);
        }

        Ok(all_results)
    }
}

#[cfg(test)]
mod tests {
    use super::decode_midpoints_response;

    #[test]
    fn decode_midpoints_array_response() {
        let chunk = vec!["1".to_string(), "2".to_string()];
        let decoded =
            decode_midpoints_response(r#"[{"mid":"0.55"},{"mid":"0.42"}]"#, &chunk).unwrap();
        assert_eq!(
            decoded,
            vec![("1".to_string(), 0.55), ("2".to_string(), 0.42)]
        );
    }

    #[test]
    fn decode_midpoints_map_response() {
        let chunk = vec!["1".to_string(), "2".to_string(), "3".to_string()];
        let decoded =
            decode_midpoints_response(r#"{"1":"0.55","2":0.42,"3":null}"#, &chunk).unwrap();
        assert_eq!(
            decoded,
            vec![("1".to_string(), 0.55), ("2".to_string(), 0.42)]
        );
    }
}
