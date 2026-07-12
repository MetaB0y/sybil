//! Public WebSocket block stream integration tests (`/v2/blocks/ws`).
//!
//! Spins up a real HTTP server on an ephemeral port so we can exercise the
//! WebSocket upgrade flow end-to-end with `tokio-tungstenite`.

mod common;

use std::net::SocketAddr;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use matching_sequencer::{SequencerConfig, SequencerHandle};
use sybil_api_types::ws::{PublicBlockStreamMessage, PublicBlockStreamPayload};
use sybil_client::SybilClient;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::protocol::Message;

use common::{test_app, test_app_with_store_config};

async fn spawn_server() -> (SocketAddr, SequencerHandle) {
    let (app, handle) = test_app(true).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, handle)
}

async fn spawn_store_server(config: SequencerConfig) -> (SocketAddr, SequencerHandle) {
    let (app, handle) = test_app_with_store_config(true, config).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, handle)
}

async fn recv_envelope<S>(stream: &mut S) -> PublicBlockStreamMessage
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let msg = timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("timeout waiting for ws message")
        .expect("stream ended")
        .expect("ws error");
    let text = match msg {
        Message::Text(t) => t.to_string(),
        other => panic!("expected text, got {:?}", other),
    };
    serde_json::from_str(&text).expect("valid envelope")
}

#[tokio::test]
async fn ws_streams_live_block_envelope() {
    let (addr, handle) = spawn_server().await;
    let url = format!("ws://{}/v2/blocks/ws", addr);
    let (ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let (_sink, mut stream) = ws.split();

    // Give the handler a tick to subscribe before we emit a block.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let block = handle.produce_block().await.unwrap();

    let msg = recv_envelope(&mut stream).await;
    assert_eq!(msg.v, 2);
    match msg.payload {
        PublicBlockStreamPayload::Block { data } => {
            assert_eq!(data.height, block.canonical.header.height);
            let value = serde_json::to_value(&data).unwrap();
            for forbidden in [
                "fills",
                "rejections",
                "system_events",
                "derived_view_sidecar",
            ] {
                assert!(
                    value.get(forbidden).is_none(),
                    "public v2 stream leaked {forbidden}"
                );
            }
            assert!(value["bridge"].get("consumed_deposits").is_none());
            assert!(value["bridge"].get("withdrawal_leaves").is_none());
        }
        other => panic!("expected block envelope, got {:?}", other),
    }
}

#[tokio::test]
async fn ws_from_block_replays_history_then_goes_live() {
    let (addr, handle) = spawn_server().await;

    // Produce three blocks before any client connects.
    let b0 = handle.produce_block().await.unwrap();
    let b1 = handle.produce_block().await.unwrap();
    let _b2 = handle.produce_block().await.unwrap();

    let from = b0.canonical.header.height;
    let head_at_connect = _b2.canonical.header.height;
    let url = format!("ws://{}/v2/blocks/ws?from_block={}", addr, from);
    let (ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let (_sink, mut stream) = ws.split();

    // Expect three block envelopes followed by replay_complete, then a live block.
    let mut seen_heights = vec![];
    for _ in 0..3 {
        let msg = recv_envelope(&mut stream).await;
        match msg.payload {
            PublicBlockStreamPayload::Block { data } => seen_heights.push(data.height),
            other => panic!("expected block during replay, got {:?}", other),
        }
    }
    assert_eq!(
        seen_heights,
        vec![
            b0.canonical.header.height,
            b1.canonical.header.height,
            _b2.canonical.header.height
        ]
    );

    let complete = recv_envelope(&mut stream).await;
    match complete.payload {
        PublicBlockStreamPayload::ReplayComplete { up_to_height } => {
            assert_eq!(up_to_height, head_at_connect);
        }
        other => panic!("expected replay_complete, got {:?}", other),
    }

    // Now produce one live block and make sure we receive it, not a duplicate.
    let live = handle.produce_block().await.unwrap();
    let msg = recv_envelope(&mut stream).await;
    match msg.payload {
        PublicBlockStreamPayload::Block { data } => {
            assert_eq!(data.height, live.canonical.header.height)
        }
        other => panic!("expected live block after replay, got {:?}", other),
    }
}

#[tokio::test]
async fn ws_from_block_replays_store_history_beyond_hot_ring() {
    let (addr, handle) = spawn_store_server(SequencerConfig {
        block_history_capacity: 1,
        block_interval: Duration::from_secs(60),
        ..SequencerConfig::default()
    })
    .await;

    let b0 = handle.produce_block().await.unwrap();
    let b1 = handle.produce_block().await.unwrap();
    let b2 = handle.produce_block().await.unwrap();
    let recent = handle.get_recent_blocks(10).await.unwrap();
    assert_eq!(recent.len(), 1, "hot ring should retain only block 3");
    assert_eq!(
        recent[0].canonical.header.height,
        b2.canonical.header.height
    );

    let url = format!(
        "ws://{}/v2/blocks/ws?from_block={}",
        addr, b0.canonical.header.height
    );
    let (ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let (_sink, mut stream) = ws.split();

    let mut seen_heights = vec![];
    for _ in 0..3 {
        let msg = recv_envelope(&mut stream).await;
        match msg.payload {
            PublicBlockStreamPayload::Block { data } => seen_heights.push(data.height),
            other => panic!("expected durable replay block, got {:?}", other),
        }
    }
    assert_eq!(
        seen_heights,
        vec![
            b0.canonical.header.height,
            b1.canonical.header.height,
            b2.canonical.header.height
        ]
    );

    let complete = recv_envelope(&mut stream).await;
    match complete.payload {
        PublicBlockStreamPayload::ReplayComplete { up_to_height } => {
            assert_eq!(up_to_height, b2.canonical.header.height);
        }
        other => panic!("expected replay_complete, got {:?}", other),
    }
}

#[tokio::test]
async fn ws_from_block_older_than_retention_sends_gap_envelope() {
    let (addr, handle) = spawn_store_server(SequencerConfig {
        block_history_capacity: 1,
        block_history_retention_blocks: 1,
        history_prune_interval_blocks: 1,
        history_prune_max_rows: 10,
        block_interval: Duration::from_secs(60),
        ..SequencerConfig::default()
    })
    .await;

    let b0 = handle.produce_block().await.unwrap();
    handle.produce_block().await.unwrap();
    let b2 = handle.produce_block().await.unwrap();

    let url = format!(
        "ws://{}/v2/blocks/ws?from_block={}",
        addr, b0.canonical.header.height
    );
    let (ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let (_sink, mut stream) = ws.split();

    let msg = recv_envelope(&mut stream).await;
    match msg.payload {
        PublicBlockStreamPayload::RetentionGap {
            requested_height,
            retention_min_height,
            head_height,
        } => {
            assert_eq!(requested_height, b0.canonical.header.height);
            assert_eq!(retention_min_height, b2.canonical.header.height);
            assert_eq!(head_height, b2.canonical.header.height);
        }
        other => panic!("expected retention_gap, got {:?}", other),
    }
}

#[tokio::test]
async fn sybil_client_stream_blocks_uses_ws_from_block_resume() {
    let (addr, handle) = spawn_server().await;
    let b0 = handle.produce_block().await.unwrap();
    let b1 = handle.produce_block().await.unwrap();

    let client = SybilClient::with_defaults(format!("http://{addr}"), None);
    let block_stream = client
        .stream_blocks_from_block(Some(b0.canonical.header.height))
        .await
        .unwrap();
    tokio::pin!(block_stream);

    let first = timeout(Duration::from_secs(5), block_stream.next())
        .await
        .expect("timeout waiting for first replay block")
        .expect("stream ended")
        .expect("stream error");
    assert_eq!(first.height, b0.canonical.header.height);

    let second = timeout(Duration::from_secs(5), block_stream.next())
        .await
        .expect("timeout waiting for second replay block")
        .expect("stream ended")
        .expect("stream error");
    assert_eq!(second.height, b1.canonical.header.height);

    let live = handle.produce_block().await.unwrap();
    let live_msg = timeout(Duration::from_secs(5), block_stream.next())
        .await
        .expect("timeout waiting for live block")
        .expect("stream ended")
        .expect("stream error");
    assert_eq!(live_msg.height, live.canonical.header.height);
}

#[tokio::test]
async fn ws_from_block_ahead_of_head_announces_complete() {
    let (addr, _handle) = spawn_server().await;

    // No blocks produced — head is None; any from_block value should result in
    // no replay blocks and the handler transitioning straight to the live loop.
    let url = format!("ws://{}/v2/blocks/ws?from_block=5", addr);
    let (ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let (mut sink, mut stream) = ws.split();

    // Nothing should arrive in the first 100ms. We don't block long here —
    // just ensure the connection is idle rather than dumping events.
    let quick = timeout(Duration::from_millis(100), stream.next()).await;
    assert!(
        quick.is_err(),
        "did not expect any message when head is None and from_block > head, got {:?}",
        quick
    );

    // Clean up.
    sink.send(Message::Close(None)).await.ok();
}
