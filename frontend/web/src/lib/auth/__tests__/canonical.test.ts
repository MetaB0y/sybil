import { describe, it, expect } from "vitest";
import {
  canonicalOrderBytes,
  canonicalCancelBytes,
  toHex,
  type CanonicalOrderInput,
} from "../canonical";

/**
 * Vectors copied VERBATIM from `crates/sybil-canonical/src/snapshots/*.snap`.
 *
 * To refresh: read the .snap files in that directory; the bottom line is the
 * authoritative hex.
 */
interface OrderVector {
  name: string;
  input: CanonicalOrderInput;
  expectedHex: string;
}

const ORDER_VECTORS: OrderVector[] = [
  {
    name: "buy_yes",
    input: {
      marketIds: [7],
      payoffs: [1, 0],
      limitPriceNanos: 550_000_000n,
      maxFill: 10n,
    },
    expectedHex:
      "07000000ffffffffffffffffffffffffffffffff010100000000000000000000000000000000000000000000000000000000000000028055c820000000000a000000000000000000",
  },
  {
    name: "sell_yes",
    input: {
      marketIds: [7],
      payoffs: [-1, 0],
      limitPriceNanos: 425_000_000n,
      maxFill: 3n,
    },
    expectedHex:
      "07000000ffffffffffffffffffffffffffffffff01ff000000000000000000000000000000000000000000000000000000000000000240fc54190000000003000000000000000000",
  },
  {
    name: "spread",
    input: {
      marketIds: [3, 9],
      payoffs: [0, -1, 1, 0],
      limitPriceNanos: 125_000_000n,
      maxFill: 5n,
    },
    expectedHex:
      "0300000009000000ffffffffffffffffffffffff0200ff01000000000000000000000000000000000000000000000000000000000004405973070000000005000000000000000000",
  },
  {
    name: "bundle",
    input: {
      marketIds: [1, 2, 4],
      payoffs: [0, 0, 0, 0, 0, 0, 0, 1],
      limitPriceNanos: 300_000_000n,
      maxFill: 2n,
    },
    expectedHex:
      "010000000200000004000000ffffffffffffffff0300000000000000010000000000000000000000000000000000000000000000000800a3e1110000000002000000000000000000",
  },
  {
    name: "conditional",
    input: {
      marketIds: [5],
      payoffs: [1, 0],
      limitPriceNanos: 610_000_000n,
      maxFill: 9n,
      condition: {
        market: 11,
        threshold: 490_000_000n,
        direction: "Above",
      },
    },
    expectedHex:
      "05000000ffffffffffffffffffffffffffffffff0101000000000000000000000000000000000000000000000000000000000000000280dc5b24000000000900000000000000010b00000080ce341d000000000000",
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
      expiresAtBlock: 1000n,
    });
    const withoutExpiry = canonicalOrderBytes({
      marketIds: [7],
      payoffs: [1, 0],
      limitPriceNanos: 550_000_000n,
      maxFill: 10n,
    });
    // With expiry: tail should be `01` + 8 LE bytes of 1000 (= 0xe803000000000000).
    // Without: tail should be `00`.
    expect(toHex(withExpiry).endsWith("01e803000000000000")).toBe(true);
    expect(toHex(withoutExpiry).endsWith("00")).toBe(true);
    // Length: with-expiry has 8 extra payload bytes (1 discriminator already present).
    expect(withExpiry.length - withoutExpiry.length).toBe(8);
  });
});

describe("canonicalCancelBytes", () => {
  it("encodes account_id + order_id as two u64 LE", () => {
    const got = toHex(canonicalCancelBytes(7n, 42n));
    // 7 LE u64: 0700000000000000
    // 42 LE u64: 2a00000000000000
    expect(got).toBe("07000000000000002a00000000000000");
  });

  it("encodes large account_id correctly", () => {
    const got = toHex(canonicalCancelBytes(0xdeadbeefn, 1n));
    expect(got).toBe("efbeadde000000000100000000000000");
  });
});
