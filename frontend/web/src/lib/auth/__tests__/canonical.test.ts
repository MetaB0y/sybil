import { describe, it, expect } from "vitest";
import {
  canonicalOrderBytes,
  canonicalCancelBytes,
  toHex,
  type CanonicalOrderInput,
} from "../canonical";

/**
 * Vectors copied VERBATIM from `crates/sybil-signing/src/snapshots/*.snap`.
 *
 * To refresh: read the .snap files in that directory; the bottom line is the
 * authoritative hex.
 */
interface OrderVector {
  name: string;
  input: CanonicalOrderInput;
  expectedHex: string;
}

const GENESIS_HASH = new Uint8Array(32).fill(0xab);
const GENESIS_HEX = "ab".repeat(32);

const ORDER_VECTORS: OrderVector[] = [
  {
    name: "buy_yes",
    input: {
      marketIds: [7],
      payoffs: [1, 0],
      limitPriceNanos: 550_000_000n,
      maxFill: 10n,
      nonce: 7n,
      genesisHash: GENESIS_HASH,
    },
    expectedHex:
      GENESIS_HEX +
      "07000000ffffffffffffffffffffffffffffffff010100000000000000000000000000000000000000000000000000000000000000028055c820000000000a0000000000000000000700000000000000",
  },
  {
    name: "sell_yes",
    input: {
      marketIds: [7],
      payoffs: [-1, 0],
      limitPriceNanos: 425_000_000n,
      maxFill: 3n,
      nonce: 7n,
      genesisHash: GENESIS_HASH,
    },
    expectedHex:
      GENESIS_HEX +
      "07000000ffffffffffffffffffffffffffffffff01ff000000000000000000000000000000000000000000000000000000000000000240fc541900000000030000000000000000000700000000000000",
  },
  {
    name: "spread",
    input: {
      marketIds: [3, 9],
      payoffs: [0, -1, 1, 0],
      limitPriceNanos: 125_000_000n,
      maxFill: 5n,
      nonce: 7n,
      genesisHash: GENESIS_HASH,
    },
    expectedHex:
      GENESIS_HEX +
      "0300000009000000ffffffffffffffffffffffff0200ff010000000000000000000000000000000000000000000000000000000000044059730700000000050000000000000000000700000000000000",
  },
  {
    name: "bundle",
    input: {
      marketIds: [1, 2, 4],
      payoffs: [0, 0, 0, 0, 0, 0, 0, 1],
      limitPriceNanos: 300_000_000n,
      maxFill: 2n,
      nonce: 7n,
      genesisHash: GENESIS_HASH,
    },
    expectedHex:
      GENESIS_HEX +
      "010000000200000004000000ffffffffffffffff0300000000000000010000000000000000000000000000000000000000000000000800a3e11100000000020000000000000000000700000000000000",
  },
  {
    name: "conditional",
    input: {
      marketIds: [5],
      payoffs: [1, 0],
      limitPriceNanos: 610_000_000n,
      maxFill: 9n,
      nonce: 7n,
      condition: {
        market: 11,
        threshold: 490_000_000n,
        direction: "Above",
      },
      genesisHash: GENESIS_HASH,
    },
    expectedHex:
      GENESIS_HEX +
      "05000000ffffffffffffffffffffffffffffffff0101000000000000000000000000000000000000000000000000000000000000000280dc5b24000000000900000000000000010b00000080ce341d0000000000000700000000000000",
  },
];

describe("canonicalOrderBytes", () => {
  for (const vec of ORDER_VECTORS) {
    it(`matches Rust snapshot: ${vec.name}`, () => {
      const got = toHex(canonicalOrderBytes(vec.input));
      expect(got).toBe(vec.expectedHex);
    });
  }

  it("encodes expires_at_block when provided", () => {
    const withExpiry = canonicalOrderBytes({
      marketIds: [7],
      payoffs: [1, 0],
      limitPriceNanos: 550_000_000n,
      maxFill: 10n,
      nonce: 7n,
      genesisHash: GENESIS_HASH,
      expiresAtBlock: 1000n,
    });
    const withoutExpiry = canonicalOrderBytes({
      marketIds: [7],
      payoffs: [1, 0],
      limitPriceNanos: 550_000_000n,
      maxFill: 10n,
      nonce: 7n,
      genesisHash: GENESIS_HASH,
    });
    // With expiry: `01` + 8 LE bytes of 1000, then nonce 7.
    // Without expiry: `00`, then nonce 7.
    expect(toHex(withExpiry).endsWith("01e8030000000000000700000000000000")).toBe(true);
    expect(toHex(withoutExpiry).endsWith("000700000000000000")).toBe(true);
    // Length: with-expiry has 8 extra payload bytes (1 discriminator already present).
    expect(withExpiry.length - withoutExpiry.length).toBe(8);
  });
});

describe("canonicalCancelBytes", () => {
  it("encodes account_id + order_id + nonce as three u64 LE", () => {
    const got = toHex(canonicalCancelBytes(7n, 42n, 11n, GENESIS_HASH));
    // 7 LE u64: 0700000000000000
    // 42 LE u64: 2a00000000000000
    // 11 LE u64: 0b00000000000000
    expect(got).toBe(
      GENESIS_HEX + "07000000000000002a000000000000000b00000000000000",
    );
  });

  it("encodes large account_id correctly", () => {
    const got = toHex(canonicalCancelBytes(0xdeadbeefn, 1n, 2n, GENESIS_HASH));
    expect(got).toBe(
      GENESIS_HEX + "efbeadde0000000001000000000000000200000000000000",
    );
  });
});
