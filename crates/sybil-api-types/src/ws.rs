//! WebSocket block stream message schema.
//!
//! Versioned envelope used by `GET /v1/blocks/ws`. Clients should read
//! `v` first and ignore messages whose version they don't understand —
//! the server is allowed to add new message types or fields within the
//! same `v`, but will bump `v` for any breaking change.

use serde::{Deserialize, Serialize};

use crate::response::BlockResponse;

/// Current schema version for the block stream envelope.
pub const BLOCK_STREAM_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockStreamMessage {
    pub v: u32,
    #[serde(flatten)]
    pub payload: BlockStreamPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockStreamPayload {
    /// A committed block. Sent for every block during live streaming, and
    /// also for each replayed block when the client connects with
    /// `?from_block=N`.
    Block { data: Box<BlockResponse> },
    /// Sent once after a `?from_block=N` replay, signaling that all
    /// blocks up to and including `up_to_height` have been delivered and
    /// the connection is now following the live stream.
    ReplayComplete { up_to_height: u64 },
    /// Server-side broadcast buffer overflowed — client was too slow.
    /// This is the last message on the stream; the server closes the
    /// connection immediately after. Reconnect with
    /// `?from_block=<last_sent_height + 1>` to recover.
    Lagged {
        skipped: u64,
        last_sent_height: Option<u64>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_block_round_trip() {
        let msg = BlockStreamMessage {
            v: BLOCK_STREAM_VERSION,
            payload: BlockStreamPayload::Block {
                data: Box::new(BlockResponse {
                    height: 42,
                    parent_hash: "ab".into(),
                    state_root: "cd".into(),
                    events_root: "ef".into(),
                    order_count: 0,
                    fill_count: 0,
                    timestamp_ms: 0,
                    system_events: vec![],
                    fills: vec![],
                    clearing_prices_nanos: Default::default(),
                    rejections: vec![],
                    bridge: Default::default(),
                    total_welfare_nanos: 0,
                    total_volume_nanos: 0,
                    orders_filled: 0,
                }),
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""v":1"#));
        assert!(json.contains(r#""type":"block""#));
        assert!(json.contains(r#""height":42"#));

        let back: BlockStreamMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.v, 1);
        match back.payload {
            BlockStreamPayload::Block { data } => assert_eq!(data.height, 42),
            _ => panic!("expected Block variant"),
        }
    }

    #[test]
    fn envelope_replay_complete_shape() {
        let json = serde_json::to_string(&BlockStreamMessage {
            v: BLOCK_STREAM_VERSION,
            payload: BlockStreamPayload::ReplayComplete { up_to_height: 100 },
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"v":1,"type":"replay_complete","up_to_height":100}"#
        );
    }

    #[test]
    fn envelope_lagged_shape() {
        let json = serde_json::to_string(&BlockStreamMessage {
            v: BLOCK_STREAM_VERSION,
            payload: BlockStreamPayload::Lagged {
                skipped: 7,
                last_sent_height: Some(42),
            },
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"v":1,"type":"lagged","skipped":7,"last_sent_height":42}"#
        );
    }
}
