/**
 * Wire types for the privacy-preserving /v2/blocks/ws envelope. Mirrors
 * `crates/sybil-api-types/src/ws.rs` (BlockStreamMessage / BlockStreamPayload).
 *
 * The serde annotation `#[serde(flatten)]` on the payload + `tag = "type"`
 * on the enum produces a flat JSON shape:
 *   {"v": 2, "type": "block",            "data": {...PublicBlockResponse}}
 *   {"v": 2, "type": "replay_complete",  "up_to_height": N}
 *   {"v": 2, "type": "lagged",           "skipped": N, "last_sent_height": N | null}
 */

import type { components } from "../api/schema";

export type Block = components["schemas"]["PublicBlockResponse"];

export type WsEnvelope =
  | { v: number; type: "block"; data: Block }
  | { v: number; type: "replay_complete"; up_to_height: number }
  | {
      v: number;
      type: "lagged";
      skipped: number;
      last_sent_height: number | null;
    }
  | {
      v: number;
      type: "retention_gap";
      requested_height: number;
      retention_min_height: number;
      head_height: number;
    };

/** High-level connection state visible to the rest of the app. */
export type ConnectionState =
  | "idle" // never connected
  | "connecting" // socket open requested, no envelope received yet
  | "replaying" // receiving historical blocks after from_block reconnect
  | "live" // caught up; following the live feed
  | "reconnecting" // socket closed; backoff scheduled
  | "failed"; // gave up reconnecting (e.g. block-not-found on too-old replay)

/** Reasons for transitions (telemetry / debugging). */
export type ConnectionTransitionReason =
  | "initial-connect"
  | "open"
  | "first-envelope"
  | "replay-complete"
  | "lagged"
  | "block-not-found"
  | "transport-close"
  | "transport-error"
  | "visibility-stale"
  | "user-disconnect";

/** Public events emitted to subscribers. */
export type WsEvent =
  | { type: "block"; block: Block }
  | { type: "replay-complete"; upToHeight: number }
  | {
      type: "lagged";
      skipped: number;
      lastSentHeight: number | null;
    }
  | {
      type: "connection";
      state: ConnectionState;
      reason: ConnectionTransitionReason;
    };

export type WsEventType = WsEvent["type"];
export type WsEventOf<T extends WsEventType> = Extract<WsEvent, { type: T }>;
export type WsListener<T extends WsEventType = WsEventType> = (
  event: WsEventOf<T>
) => void;
