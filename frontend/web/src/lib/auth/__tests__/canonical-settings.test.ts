import { describe, it, expect } from "vitest";
import {
  canonicalApiKeyCreateBytes,
  canonicalApiKeyRevokeBytes,
  canonicalKeyRevocationBytes,
  canonicalProfileUpdateBytes,
  fromHex,
  toHex,
} from "../canonical";

/**
 * Structural encoding tests for the SYB-60 account-management canonical
 * payloads. These mirror `crates/sybil-signing/src/lib.rs`. Exact hex vectors
 * are not snapshotted in Rust for these, so we assert the borsh structure
 * directly: u64 little-endian layout, Option None vs Some tag + payload, and
 * Vec<u8> length prefixes. Each field is exercised.
 */

const u64le = (n: bigint): string => {
  let out = "";
  for (let i = 0; i < 8; i++) {
    out += Number((n >> BigInt(8 * i)) & 0xffn)
      .toString(16)
      .padStart(2, "0");
  }
  return out;
};

// borsh string: u32 LE length prefix + utf8 bytes.
const borshString = (s: string): string => {
  const bytes = new TextEncoder().encode(s);
  let len = "";
  for (let i = 0; i < 4; i++) {
    len += ((bytes.length >>> (8 * i)) & 0xff).toString(16).padStart(2, "0");
  }
  return len + toHex(bytes);
};

describe("canonicalProfileUpdateBytes", () => {
  it("encodes account_id, Some/Some, nonce in order", () => {
    const got = toHex(canonicalProfileUpdateBytes(7n, "alice", "seed-1", 42n));
    const expected =
      u64le(7n) +
      "01" +
      borshString("alice") +
      "01" +
      borshString("seed-1") +
      u64le(42n);
    expect(got).toBe(expected);
  });

  it("encodes None (null) fields with a 0x00 tag and no payload", () => {
    const got = toHex(canonicalProfileUpdateBytes(7n, null, null, 42n));
    const expected = u64le(7n) + "00" + "00" + u64le(42n);
    expect(got).toBe(expected);
  });

  it("distinguishes None from Some (empty string is still Some)", () => {
    const none = toHex(canonicalProfileUpdateBytes(1n, null, null, 1n));
    const someEmpty = toHex(canonicalProfileUpdateBytes(1n, "", null, 1n));
    expect(none).not.toBe(someEmpty);
    // Some("") = tag 01 + u32 length 0.
    expect(someEmpty).toContain("0100000000");
  });

  it("changes when any single field changes (all fields covered)", () => {
    const base = toHex(canonicalProfileUpdateBytes(1n, "a", "b", 2n));
    expect(base).not.toBe(toHex(canonicalProfileUpdateBytes(9n, "a", "b", 2n)));
    expect(base).not.toBe(toHex(canonicalProfileUpdateBytes(1n, "z", "b", 2n)));
    expect(base).not.toBe(toHex(canonicalProfileUpdateBytes(1n, "a", "z", 2n)));
    expect(base).not.toBe(toHex(canonicalProfileUpdateBytes(1n, "a", "b", 3n)));
  });
});

describe("canonicalKeyRevocationBytes", () => {
  const target = fromHex("02" + "ab".repeat(32)); // 33-byte compressed pubkey

  it("encodes account_id, Vec<u8> (len prefix + bytes), nonce", () => {
    const got = toHex(canonicalKeyRevocationBytes(7n, target, 42n));
    // Vec<u8> = u32 LE length (33 = 0x21) + the 33 raw bytes.
    const expected = u64le(7n) + "21000000" + toHex(target) + u64le(42n);
    expect(got).toBe(expected);
  });

  it("is deterministic and covers the target bytes", () => {
    const a = toHex(canonicalKeyRevocationBytes(7n, target, 42n));
    const b = toHex(canonicalKeyRevocationBytes(7n, target, 42n));
    expect(a).toBe(b);
    const other = fromHex("03" + "cd".repeat(32));
    expect(a).not.toBe(toHex(canonicalKeyRevocationBytes(7n, other, 42n)));
  });
});

describe("canonicalApiKeyCreateBytes", () => {
  it("encodes account_id, Some(label), nonce", () => {
    const got = toHex(canonicalApiKeyCreateBytes(7n, "grafana", 42n));
    const expected = u64le(7n) + "01" + borshString("grafana") + u64le(42n);
    expect(got).toBe(expected);
  });

  it("encodes a None label with a 0x00 tag", () => {
    const got = toHex(canonicalApiKeyCreateBytes(7n, null, 42n));
    expect(got).toBe(u64le(7n) + "00" + u64le(42n));
  });
});

describe("canonicalApiKeyRevokeBytes", () => {
  it("encodes three u64 LE in order", () => {
    const got = toHex(canonicalApiKeyRevokeBytes(7n, 5n, 42n));
    expect(got).toBe(u64le(7n) + u64le(5n) + u64le(42n));
  });

  it("is sensitive to each field", () => {
    const base = toHex(canonicalApiKeyRevokeBytes(1n, 2n, 3n));
    expect(base).not.toBe(toHex(canonicalApiKeyRevokeBytes(9n, 2n, 3n)));
    expect(base).not.toBe(toHex(canonicalApiKeyRevokeBytes(1n, 9n, 3n)));
    expect(base).not.toBe(toHex(canonicalApiKeyRevokeBytes(1n, 2n, 9n)));
  });
});
