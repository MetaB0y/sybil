/**
 * BlockStream — singleton WebSocket client for the public /v2/blocks/ws tape.
 *
 * Owns the lifecycle of a single connection to the Sybil block stream.
 * No React / Zustand coupling here — Milestone B wires this to a store.
 *
 * Behavior matches docs/architecture/WebSocket Block Stream.md:
 *   - On reconnect, requests `?from_block=lastSeenHeight+1` to replay missed
 *     blocks. The server emits Block envelopes for each replayed block, then
 *     a single ReplayComplete envelope. After that, live blocks follow.
 *   - On `lagged` envelope (server-side buffer overflow), the server closes
 *     with code 1008. We reconnect with from_block.
 *   - On a retention gap, automatic reconnect stops. Consumers must fetch a
 *     fresh REST snapshot and explicitly resume from that snapshot height.
 *   - The server pings every 30s; the browser auto-pongs. We don't need
 *     to send anything; just track that messages keep arriving.
 *   - On document.visibilitychange === "visible" with a stale connection,
 *     we force-reconnect.
 */

import type {
  ConnectionState,
  ConnectionTransitionReason,
  WsEnvelope,
  WsEvent,
  WsEventType,
  WsListener,
} from "./types";

const VISIBILITY_STALE_MS = 30_000; // tab returned + no message for 30s → reconnect
const INITIAL_BACKOFF_MS = 1_000;
const MAX_BACKOFF_MS = 30_000;

export class BlockStream {
  private readonly wsBase: string;

  private ws: WebSocket | null = null;
  private state: ConnectionState = "idle";
  private lastSeenHeight: number | null = null;
  private lastMessageAt: number | null = null;
  private replayWatermark: number | null = null;

  private backoffMs = INITIAL_BACKOFF_MS;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private explicitlyDisconnected = false;
  private needsColdResync = false;

  private readonly listeners = new Map<WsEventType, Set<WsListener>>();
  private visibilityHandler: (() => void) | null = null;

  constructor(wsBase: string) {
    this.wsBase = wsBase.replace(/\/+$/, "");
  }

  // ── Public API ────────────────────────────────────────────────────────

  connect(): void {
    if (typeof window === "undefined") {
      // SSR / build-time call. No-op; reconnect on client mount.
      return;
    }
    this.explicitlyDisconnected = false;
    if (this.needsColdResync) return;
    this.attachVisibilityListener();
    if (this.state === "idle" || this.state === "failed") {
      this.openSocket("initial-connect");
    }
  }

  disconnect(): void {
    this.explicitlyDisconnected = true;
    this.cancelReconnect();
    this.detachVisibilityListener();
    if (this.ws) {
      try {
        this.ws.close(1000, "client disconnect");
      } catch {
        // ignore
      }
      this.ws = null;
    }
    this.setState("idle", "user-disconnect");
  }

  on<T extends WsEventType>(type: T, listener: WsListener<T>): () => void {
    const bucket = this.listeners.get(type) ?? new Set<WsListener>();
    const erased = listener as unknown as WsListener;
    bucket.add(erased);
    this.listeners.set(type, bucket);
    return () => {
      bucket.delete(erased);
    };
  }

  getState(): ConnectionState {
    return this.state;
  }

  getLastSeenHeight(): number | null {
    return this.lastSeenHeight;
  }

  /** Seed the height handshake from a REST snapshot. The next connect()
   *  will pass `?from_block=height+1`, so the server replays missed blocks. */
  seedLastSeenHeight(height: number): void {
    if (!Number.isFinite(height) || height < 0) return;
    this.lastSeenHeight = height;
    this.replayWatermark = height;
  }

  /** Resume only after consumers have replaced their state from REST. */
  recoverFromSnapshot(height: number): void {
    if (!Number.isFinite(height) || height < 0) return;
    const staleSocket = this.ws;
    this.ws = null;
    if (staleSocket) {
      staleSocket.onclose = null;
      try {
        staleSocket.close(1000, "cold snapshot recovered");
      } catch {
        // ignore
      }
    }
    this.cancelReconnect();
    this.lastSeenHeight = height;
    this.replayWatermark = height;
    this.needsColdResync = false;
    this.backoffMs = INITIAL_BACKOFF_MS;
    this.openSocket("snapshot-recovered");
  }

  // ── Internal: socket lifecycle ────────────────────────────────────────

  private openSocket(reason: ConnectionTransitionReason): void {
    if (typeof window === "undefined") return;
    if (this.ws) return; // already an active socket
    const url = this.buildUrl();
    this.setState("connecting", reason);
    let ws: WebSocket;
    try {
      ws = new WebSocket(url);
    } catch {
      this.scheduleReconnect("transport-error");
      return;
    }
    this.ws = ws;

    ws.onopen = () => {
      this.lastMessageAt = Date.now();
      // Don't promote to "live" yet — wait for first envelope. If we sent
      // from_block, the first envelopes will be replayed Blocks.
    };

    ws.onmessage = (event) => {
      this.lastMessageAt = Date.now();
      let envelope: WsEnvelope;
      try {
        envelope = JSON.parse(event.data) as WsEnvelope;
      } catch {
        return; // ignore malformed
      }
      if (envelope.v !== 2) return; // unknown version → skip per spec
      this.handleEnvelope(envelope);
    };

    ws.onerror = () => {
      // Don't act — wait for onclose; that's where we have a reason and code.
    };

    ws.onclose = (event) => {
      this.ws = null;
      if (this.explicitlyDisconnected) return;
      this.handleClose(event.code, event.reason);
    };
  }

  private buildUrl(): string {
    const base = `${this.wsBase}/v2/blocks/ws`;
    if (this.lastSeenHeight != null) {
      const from = this.lastSeenHeight + 1;
      return `${base}?from_block=${from}`;
    }
    return base;
  }

  private handleEnvelope(envelope: WsEnvelope): void {
    switch (envelope.type) {
      case "block": {
        const block = envelope.data;
        this.lastSeenHeight = block.height;
        // First envelope of a replayed reconnect → mark as replaying.
        if (this.state === "connecting") {
          const replay =
            this.lastSeenHeight != null && this.replayWatermark != null;
          this.setState(replay ? "replaying" : "live", "first-envelope");
        }
        this.emit({ type: "block", block });
        break;
      }
      case "replay_complete": {
        this.replayWatermark = null;
        this.setState("live", "replay-complete");
        this.backoffMs = INITIAL_BACKOFF_MS;
        this.emit({
          type: "replay-complete",
          upToHeight: envelope.up_to_height,
        });
        break;
      }
      case "lagged": {
        // Server will close immediately after; the close handler will
        // reconnect with from_block=lastSentHeight+1.
        if (envelope.last_sent_height != null) {
          this.lastSeenHeight = envelope.last_sent_height;
        }
        this.emit({
          type: "lagged",
          skipped: envelope.skipped,
          lastSentHeight: envelope.last_sent_height,
        });
        break;
      }
      case "retention_gap": {
        this.requireColdResync(
          envelope.requested_height,
          envelope.retention_min_height,
          envelope.head_height,
        );
        break;
      }
    }
  }

  private handleClose(code: number, reason: string): void {
    if (code === 1008 && /block not found/i.test(reason)) {
      // Compatibility with older servers that closed without first sending
      // the structured retention_gap envelope.
      this.requireColdResync(null, null, null, "block-not-found");
      return;
    }
    if (this.needsColdResync) return;
    // Normal close (server initiated, transport error, lagged forced close).
    if (this.lastSeenHeight != null) {
      this.replayWatermark = this.lastSeenHeight;
    }
    this.scheduleReconnect(code === 1008 ? "lagged" : "transport-close");
  }

  private scheduleReconnect(reason: ConnectionTransitionReason): void {
    if (this.needsColdResync) return;
    this.cancelReconnect();
    this.setState("reconnecting", reason);
    const delay = this.backoffMs;
    this.backoffMs = Math.min(this.backoffMs * 2, MAX_BACKOFF_MS);
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.openSocket(reason);
    }, delay);
  }

  private cancelReconnect(): void {
    if (this.reconnectTimer != null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }

  private requireColdResync(
    requestedHeight: number | null,
    retentionMinHeight: number | null,
    headHeight: number | null,
    reason: ConnectionTransitionReason = "retention-gap",
  ): void {
    if (this.needsColdResync) return;
    this.needsColdResync = true;
    this.cancelReconnect();
    this.lastSeenHeight = null;
    this.replayWatermark = null;
    this.setState("failed", reason);
    this.emit({
      type: "retention-gap",
      requestedHeight,
      retentionMinHeight,
      headHeight,
    });
  }

  // ── Internal: state + event emission ──────────────────────────────────

  private setState(
    state: ConnectionState,
    reason: ConnectionTransitionReason,
  ): void {
    if (this.state === state) return;
    this.state = state;
    this.emit({ type: "connection", state, reason });
  }

  private emit(event: WsEvent): void {
    const bucket = this.listeners.get(event.type);
    if (!bucket) return;
    for (const listener of bucket) {
      try {
        listener(event as never);
      } catch (err) {
        // Listener errors must not break the dispatch loop.
        console.error("[blockStream] listener threw:", err);
      }
    }
  }

  // ── Internal: visibility handling ─────────────────────────────────────

  private attachVisibilityListener(): void {
    if (this.visibilityHandler) return;
    if (typeof document === "undefined") return;
    this.visibilityHandler = () => {
      if (document.visibilityState !== "visible") return;
      if (this.needsColdResync) return;
      const stale =
        this.lastMessageAt == null ||
        Date.now() - this.lastMessageAt > VISIBILITY_STALE_MS;
      const disconnected =
        this.state === "reconnecting" || this.state === "failed";
      if (stale || disconnected) {
        if (this.ws) {
          try {
            this.ws.close(1000, "visibility-stale");
          } catch {
            // ignore
          }
          this.ws = null;
        }
        this.cancelReconnect();
        this.backoffMs = INITIAL_BACKOFF_MS;
        this.openSocket("visibility-stale");
      }
    };
    document.addEventListener("visibilitychange", this.visibilityHandler);
  }

  private detachVisibilityListener(): void {
    if (!this.visibilityHandler) return;
    if (typeof document === "undefined") return;
    document.removeEventListener("visibilitychange", this.visibilityHandler);
    this.visibilityHandler = null;
  }
}

// ── Singleton ─────────────────────────────────────────────────────────────

export const DEFAULT_WS_BASE = "wss://172-104-31-54.nip.io";

export function resolveBlockStreamBase(configured: string | undefined): string {
  const trimmed = configured?.trim();
  return trimmed || DEFAULT_WS_BASE;
}

function makeSingleton(): BlockStream {
  return new BlockStream(
    resolveBlockStreamBase(process.env.NEXT_PUBLIC_WS_BASE),
  );
}

// Lazy so SSR doesn't choke on missing env vars at module load.
let _instance: BlockStream | null = null;
export function getBlockStream(): BlockStream {
  if (!_instance) _instance = makeSingleton();
  return _instance;
}
