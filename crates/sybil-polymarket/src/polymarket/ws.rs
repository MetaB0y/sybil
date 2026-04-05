use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::watch;
use tokio::time::{interval, Instant};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};

use super::types::ClobWsMessage;
use crate::feed::PriceSnapshot;

/// Connect to Polymarket CLOB WebSocket, subscribe to token_ids, and push
/// price updates into the watch channel.
///
/// Returns on disconnect — caller should reconnect with backoff.
pub async fn run_ws_feed(
    ws_url: &str,
    token_ids: &[String],
    price_tx: &watch::Sender<PriceSnapshot>,
) -> Result<(), crate::error::Error> {
    if token_ids.is_empty() {
        info!("no token IDs to subscribe, skipping WebSocket");
        return Ok(());
    }

    let (ws_stream, _) = connect_async(ws_url)
        .await
        .map_err(|e| crate::error::Error::WebSocket(e.to_string()))?;

    info!(
        url = ws_url,
        tokens = token_ids.len(),
        "WebSocket connected"
    );

    let (mut sink, mut stream) = ws_stream.split();

    // Subscribe to all token IDs
    let subscribe_msg = serde_json::json!({
        "assets_ids": token_ids,
        "type": "market",
        "custom_feature_enabled": true,
    });
    sink.send(Message::Text(subscribe_msg.to_string().into()))
        .await
        .map_err(|e| crate::error::Error::WebSocket(e.to_string()))?;

    debug!(tokens = token_ids.len(), "subscribed to price updates");

    let mut ping_interval = interval(Duration::from_secs(10));
    let reconnect_deadline = Instant::now() + Duration::from_secs(15 * 60); // 15 min

    loop {
        tokio::select! {
            _ = ping_interval.tick() => {
                if Instant::now() >= reconnect_deadline {
                    info!("proactive reconnect after 15 minutes");
                    return Ok(());
                }
                if let Err(e) = sink.send(Message::Ping(vec![].into())).await {
                    warn!(error = %e, "PING failed");
                    return Err(crate::error::Error::WebSocket(e.to_string()));
                }
            }
            msg = stream.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if text == "PONG" || text.is_empty() {
                            continue;
                        }
                        // Try parsing as array (some messages come batched)
                        if let Ok(messages) = serde_json::from_str::<Vec<ClobWsMessage>>(&text) {
                            let mut snapshot = price_tx.borrow().clone();
                            for ws_msg in messages {
                                if let Some((token_id, price)) = ws_msg.midpoint() {
                                    snapshot.midpoints.insert(token_id, price);
                                }
                            }
                            snapshot.last_updated_ms = now_ms();
                            snapshot.source = crate::feed::PriceSource::WebSocket;
                            let _ = price_tx.send(snapshot);
                        } else if let Ok(ws_msg) = serde_json::from_str::<ClobWsMessage>(&text) {
                            if let Some((token_id, price)) = ws_msg.midpoint() {
                                let mut snapshot = price_tx.borrow().clone();
                                snapshot.midpoints.insert(token_id, price);
                                snapshot.last_updated_ms = now_ms();
                                snapshot.source = crate::feed::PriceSource::WebSocket;
                                let _ = price_tx.send(snapshot);
                            }
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Close(_))) => {
                        info!("WebSocket closed by server");
                        return Ok(());
                    }
                    Some(Err(e)) => {
                        warn!(error = %e, "WebSocket error");
                        return Err(crate::error::Error::WebSocket(e.to_string()));
                    }
                    None => {
                        info!("WebSocket stream ended");
                        return Ok(());
                    }
                    _ => {} // Binary, Frame — ignore
                }
            }
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
