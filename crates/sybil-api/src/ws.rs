use axum::extract::ws::{CloseFrame, Message, WebSocket};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use matching_sequencer::SequencerHandle;

use crate::convert::block_to_response;

pub async fn handle_block_ws(mut socket: WebSocket, handle: &SequencerHandle) {
    let rx = match handle.subscribe_blocks().await {
        Ok(rx) => rx,
        Err(e) => {
            let _ = socket
                .send(Message::Close(Some(CloseFrame {
                    code: 1011,
                    reason: format!("Failed to subscribe: {e}").into(),
                })))
                .await;
            return;
        }
    };

    let mut stream = BroadcastStream::new(rx);

    loop {
        tokio::select! {
            Some(result) = stream.next() => {
                match result {
                    Ok(block) => {
                        let response = block_to_response(&block);
                        let json = match serde_json::to_string(&response) {
                            Ok(j) => j,
                            Err(_) => continue,
                        };
                        if socket.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => continue, // lagged, skip
                }
            }
            Some(msg) = socket.recv() => {
                match msg {
                    Ok(Message::Close(_)) | Err(_) => break,
                    Ok(Message::Ping(data)) => {
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            else => break,
        }
    }
}
