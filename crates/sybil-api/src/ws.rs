//! WebSocket block stream handler — see `docs/architecture/WebSocket Block Stream.md`.

use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use serde::Deserialize;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::{Instant, MissedTickBehavior, interval};

use matching_sequencer::{SequencerError, SequencerHandle};
use sybil_api_types::ws::{
    BLOCK_STREAM_VERSION, BlockStreamMessage, BlockStreamPayload, PUBLIC_BLOCK_STREAM_VERSION,
    PublicBlockStreamMessage, PublicBlockStreamPayload,
};

use crate::convert::{block_to_response, public_block_to_response};

const PING_INTERVAL: Duration = Duration::from_secs(30);
/// WebSocket close code for policy violations and stream-level errors.
/// Chosen so browser `onclose` handlers can distinguish a server-initiated
/// close from transport failures (code 1006 is reserved for abnormal close).
const CLOSE_POLICY: u16 = 1008;

#[derive(Debug, Default, Deserialize)]
pub struct WsQuery {
    /// If set, replay every committed block from this height up to the
    /// current head, then follow the live stream. Used for reconnects.
    pub from_block: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockStreamVisibility {
    Public,
    Service,
}

pub async fn handle_block_ws(
    mut socket: WebSocket,
    handle: &SequencerHandle,
    query: WsQuery,
    visibility: BlockStreamVisibility,
    client_idle_timeout: Duration,
) {
    // Subscribe BEFORE fetching head so that the live channel already
    // holds any block committed while we replay, and we can dedupe by
    // `last_sent_height` after the handoff.
    let mut rx = match handle.subscribe_blocks().await {
        Ok(rx) => rx,
        Err(e) => {
            close_with_reason(&mut socket, format!("subscribe failed: {e}")).await;
            return;
        }
    };

    let mut last_sent_height: Option<u64> = None;

    if let Some(from) = query.from_block {
        match replay(&mut socket, handle, from, visibility).await {
            ReplayOutcome::Streamed(high_water) => {
                last_sent_height = Some(high_water);
            }
            ReplayOutcome::NoBlocks => {
                // Chain hasn't produced anything yet; straight to live.
            }
            ReplayOutcome::ClientGone => return,
            ReplayOutcome::Error(msg) => {
                close_with_reason(&mut socket, msg).await;
                return;
            }
        }
    }

    // Heights <= replay_watermark may still be in the broadcast buffer
    // from before/during replay. Skip them in the live loop to avoid dupes.
    let replay_watermark = last_sent_height;

    let mut ping_timer = interval(PING_INTERVAL);
    ping_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // interval() fires immediately on first tick — consume that so the
    // first real ping is PING_INTERVAL away, not instant.
    ping_timer.tick().await;
    let mut last_activity = Instant::now();

    loop {
        tokio::select! {
            recv = rx.recv() => match recv {
                Ok(block) => {
                    let h = block.canonical.header.height;
                    if replay_watermark.is_some_and(|w| h <= w) {
                        continue;
                    }
                    if !send_block(&mut socket, &block, visibility).await {
                        return;
                    }
                    last_sent_height = Some(h);
                }
                Err(RecvError::Lagged(n)) => {
                    send_lagged_and_close(&mut socket, n, last_sent_height, visibility).await;
                    return;
                }
                Err(RecvError::Closed) => break,
            },
            msg = socket.recv() => match msg {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(Message::Ping(data))) => {
                    last_activity = Instant::now();
                    if socket.send(Message::Pong(data)).await.is_err() {
                        break;
                    }
                }
                Some(Ok(_)) => {
                    // Pong or any other frame counts as liveness.
                    last_activity = Instant::now();
                }
            },
            _ = ping_timer.tick() => {
                if last_activity.elapsed() > client_idle_timeout {
                    close_with_reason(&mut socket, "client idle timeout".into()).await;
                    return;
                }
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
        }
    }
}

enum ReplayOutcome {
    Streamed(u64),
    NoBlocks,
    ClientGone,
    Error(String),
}

async fn replay(
    socket: &mut WebSocket,
    handle: &SequencerHandle,
    from: u64,
    visibility: BlockStreamVisibility,
) -> ReplayOutcome {
    let head = match handle.get_latest_block().await {
        Ok(Some(b)) => b.canonical.header.height,
        Ok(None) => return ReplayOutcome::NoBlocks,
        Err(e) => return ReplayOutcome::Error(format!("get_latest_block failed: {e}")),
    };
    if from > head {
        // Client claims to be ahead of us. Nothing to replay; just announce
        // completion at our current head and let them pick up the live stream.
        if !send_replay_complete(socket, head, visibility).await {
            return ReplayOutcome::ClientGone;
        }
        return ReplayOutcome::Streamed(head);
    }

    let mut h = from;
    while h <= head {
        let block = match handle.get_block(h).await {
            Ok(b) => b,
            Err(SequencerError::BlockPruned {
                requested_height,
                retention_min_height,
            }) => {
                if !send_retention_gap(
                    socket,
                    requested_height,
                    retention_min_height,
                    head,
                    visibility,
                )
                .await
                {
                    return ReplayOutcome::ClientGone;
                }
                return ReplayOutcome::Error(format!(
                    "replay requested height {requested_height} older than retention min {retention_min_height}"
                ));
            }
            Err(e) => return ReplayOutcome::Error(format!("replay failed at height {h}: {e}")),
        };
        if !send_block(socket, &block, visibility).await {
            return ReplayOutcome::ClientGone;
        }
        h += 1;
    }
    if !send_replay_complete(socket, head, visibility).await {
        return ReplayOutcome::ClientGone;
    }
    ReplayOutcome::Streamed(head)
}

async fn send_block(
    socket: &mut WebSocket,
    block: &matching_sequencer::block::SealedBlock,
    visibility: BlockStreamVisibility,
) -> bool {
    match visibility {
        BlockStreamVisibility::Public => {
            send_public_envelope(
                socket,
                PublicBlockStreamPayload::Block {
                    data: Box::new(public_block_to_response(block)),
                },
            )
            .await
        }
        BlockStreamVisibility::Service => {
            send_envelope(
                socket,
                BlockStreamPayload::Block {
                    data: Box::new(block_to_response(block)),
                },
            )
            .await
        }
    }
}

async fn send_replay_complete(
    socket: &mut WebSocket,
    up_to_height: u64,
    visibility: BlockStreamVisibility,
) -> bool {
    match visibility {
        BlockStreamVisibility::Public => {
            send_public_envelope(
                socket,
                PublicBlockStreamPayload::ReplayComplete { up_to_height },
            )
            .await
        }
        BlockStreamVisibility::Service => {
            send_envelope(socket, BlockStreamPayload::ReplayComplete { up_to_height }).await
        }
    }
}

async fn send_envelope(socket: &mut WebSocket, payload: BlockStreamPayload) -> bool {
    let msg = BlockStreamMessage {
        v: BLOCK_STREAM_VERSION,
        payload,
    };
    let Ok(json) = serde_json::to_string(&msg) else {
        return false;
    };
    socket.send(Message::Text(json.into())).await.is_ok()
}

async fn send_public_envelope(socket: &mut WebSocket, payload: PublicBlockStreamPayload) -> bool {
    let msg = PublicBlockStreamMessage {
        v: PUBLIC_BLOCK_STREAM_VERSION,
        payload,
    };
    let Ok(json) = serde_json::to_string(&msg) else {
        return false;
    };
    socket.send(Message::Text(json.into())).await.is_ok()
}

async fn send_retention_gap(
    socket: &mut WebSocket,
    requested_height: u64,
    retention_min_height: u64,
    head_height: u64,
    visibility: BlockStreamVisibility,
) -> bool {
    match visibility {
        BlockStreamVisibility::Public => {
            send_public_envelope(
                socket,
                PublicBlockStreamPayload::RetentionGap {
                    requested_height,
                    retention_min_height,
                    head_height,
                },
            )
            .await
        }
        BlockStreamVisibility::Service => {
            send_envelope(
                socket,
                BlockStreamPayload::RetentionGap {
                    requested_height,
                    retention_min_height,
                    head_height,
                },
            )
            .await
        }
    }
}

async fn send_lagged_and_close(
    socket: &mut WebSocket,
    skipped: u64,
    last_sent_height: Option<u64>,
    visibility: BlockStreamVisibility,
) {
    let _ = match visibility {
        BlockStreamVisibility::Public => {
            send_public_envelope(
                socket,
                PublicBlockStreamPayload::Lagged {
                    skipped,
                    last_sent_height,
                },
            )
            .await
        }
        BlockStreamVisibility::Service => {
            send_envelope(
                socket,
                BlockStreamPayload::Lagged {
                    skipped,
                    last_sent_height,
                },
            )
            .await
        }
    };
    let reason = match last_sent_height {
        Some(h) => format!(
            "stream lagged; reconnect with ?from_block={}",
            h.saturating_add(1)
        ),
        None => "stream lagged".to_string(),
    };
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code: CLOSE_POLICY,
            reason: reason.into(),
        })))
        .await;
}

async fn close_with_reason(socket: &mut WebSocket, reason: String) {
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code: CLOSE_POLICY,
            reason: reason.into(),
        })))
        .await;
}
