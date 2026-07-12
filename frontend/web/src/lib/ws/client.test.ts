import { describe, expect, it } from "vitest";
import { DEFAULT_WS_BASE, resolveBlockStreamBase } from "./client";

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
