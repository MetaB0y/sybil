//! Typed client for Arena-owned analytics.
//!
//! Arena owns its SQLite database and schema. The public API proxies stable
//! JSON documents from Arena's private read service; it never mounts or opens
//! the Python-owned database.

use std::sync::Arc;
use std::time::Duration;

use reqwest::StatusCode;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::config::ApiConfig;

#[derive(Clone)]
pub struct ArenaReadClient {
    base_url: Arc<str>,
    token: Arc<str>,
    http: reqwest::Client,
}

#[derive(Debug, thiserror::Error)]
pub enum ArenaReadError {
    #[error("Arena read request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("Arena read service returned {status}: {body}")]
    Response { status: StatusCode, body: String },
}

impl ArenaReadClient {
    pub fn from_config(config: &ApiConfig) -> Result<Option<Self>, reqwest::Error> {
        let base_url = config.arena_read_url.trim().trim_end_matches('/');
        if base_url.is_empty() {
            return Ok(None);
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.arena_read_timeout_ms.max(1)))
            .build()?;
        Ok(Some(Self {
            base_url: Arc::from(base_url),
            token: Arc::from(config.arena_read_token.trim()),
            http,
        }))
    }

    pub async fn decisions<Q: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        query: &Q,
    ) -> Result<R, ArenaReadError> {
        self.get("v1/decisions", query).await
    }

    pub async fn equity_series<Q: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        query: &Q,
    ) -> Result<R, ArenaReadError> {
        self.get("v1/equity-series", query).await
    }

    async fn get<Q: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        path: &str,
        query: &Q,
    ) -> Result<R, ArenaReadError> {
        let response = self
            .http
            .get(format!("{}/{path}", self.base_url))
            .bearer_auth(self.token.as_ref())
            .query(query)
            .send()
            .await?;
        let status = response.status();
        if status.is_success() {
            return response.json().await.map_err(Into::into);
        }
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "unreadable response".to_string());
        Err(ArenaReadError::Response { status, body })
    }
}
