import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BlockStream, DEFAULT_WS_BASE, resolveBlockStreamBase } from "./client";

class MockWebSocket {
  static instances: MockWebSocket[] = [];

  readonly url: string;
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: ((event: { code: number; reason: string }) => void) | null = null;

  constructor(url: string) {
    this.url = url;
    MockWebSocket.instances.push(this);
  }

  close(code = 1000, reason = ""): void {
    this.onclose?.({ code, reason });
  }

  message(value: unknown): void {
    this.onmessage?.({ data: JSON.stringify(value) });
  }

  serverClose(code: number, reason: string): void {
    this.onclose?.({ code, reason });
  }
}

beforeEach(() => {
  MockWebSocket.instances = [];
  vi.stubGlobal("window", {});
  vi.stubGlobal("document", {
    visibilityState: "visible",
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  });
  vi.stubGlobal("WebSocket", MockWebSocket);
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("resolveBlockStreamBase", () => {
  it("uses the shared devnet for a source production build without local env", () => {
    expect(resolveBlockStreamBase(undefined)).toBe(DEFAULT_WS_BASE);
    expect(resolveBlockStreamBase("   ")).toBe(DEFAULT_WS_BASE);
  });

  it("preserves an explicitly configured WebSocket origin", () => {
    expect(resolveBlockStreamBase(" wss://api.example.test ")).toBe(
      "wss://api.example.test",
    );
  });
});

describe("BlockStream replay and retention recovery", () => {
  it("classifies the REST-seeded handshake as replay until replay_complete", () => {
    const stream = new BlockStream("wss://api.example.test");
    const states: string[] = [];
    stream.on("connection", (event) => states.push(event.state));

    stream.seedLastSeenHeight(10);
    stream.connect();

    const socket = MockWebSocket.instances[0]!;
    expect(socket.url).toBe(
      "wss://api.example.test/v2/blocks/ws?from_block=11",
    );
    socket.message({ v: 2, type: "block", data: { height: 11 } });
    expect(stream.getState()).toBe("replaying");
    socket.message({ v: 2, type: "replay_complete", up_to_height: 11 });
    expect(stream.getState()).toBe("live");
    expect(states).toContain("replaying");
  });

  it("fails closed on retention gaps until a REST snapshot is supplied", () => {
    vi.useFakeTimers();
    const stream = new BlockStream("wss://api.example.test");
    const gaps: number[] = [];
    stream.on("retention-gap", (event) => {
      gaps.push(event.retentionMinHeight ?? -1);
    });

    stream.seedLastSeenHeight(4);
    stream.connect();
    const staleSocket = MockWebSocket.instances[0]!;
    staleSocket.message({
      v: 2,
      type: "retention_gap",
      requested_height: 5,
      retention_min_height: 10,
      head_height: 20,
    });
    staleSocket.serverClose(1008, "retention gap");
    vi.runAllTimers();

    expect(stream.getState()).toBe("failed");
    expect(stream.getLastSeenHeight()).toBeNull();
    expect(MockWebSocket.instances).toHaveLength(1);
    expect(gaps).toEqual([10]);

    stream.recoverFromSnapshot(20);

    expect(MockWebSocket.instances).toHaveLength(2);
    expect(MockWebSocket.instances[1]!.url).toBe(
      "wss://api.example.test/v2/blocks/ws?from_block=21",
    );
    expect(stream.getState()).toBe("connecting");
  });
});
