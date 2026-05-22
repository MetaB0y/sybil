import { describe, expect, it } from "vitest";
import {
  findDegenOrderId,
  resolveDegenBet,
  type DegenEvent,
} from "./track";

function ev(p: Partial<DegenEvent>): DegenEvent {
  return {
    type: "filled",
    blockHeight: 0,
    marketId: 7,
    orderId: 1,
    side: "BUY",
    outcome: "YES",
    qty: 0n,
    priceNanos: 0n,
    ...p,
  };
}

const crit = { marketId: 7, outcome: "YES" as const, submitHeight: 100 };

describe("findDegenOrderId", () => {
  it("binds a matching placed row", () => {
    const events = [ev({ type: "placed", orderId: 42, blockHeight: 100 })];
    expect(findDegenOrderId(events, crit)).toBe(42);
  });

  it("binds a filled row when there is no placed (instant fill)", () => {
    const events = [ev({ type: "filled", orderId: 43, blockHeight: 101, qty: 5n })];
    expect(findDegenOrderId(events, crit)).toBe(43);
  });

  it("ignores wrong market, outcome, side, and pre-submit rows", () => {
    const events = [
      ev({ orderId: 1, marketId: 8 }),
      ev({ orderId: 2, outcome: "NO" }),
      ev({ orderId: 3, side: "SELL" }),
      ev({ orderId: 4, blockHeight: 99 }),
    ];
    expect(findDegenOrderId(events, crit)).toBeNull();
  });

  it("returns the earliest matching order id", () => {
    const events = [
      ev({ type: "filled", orderId: 9, blockHeight: 103 }),
      ev({ type: "placed", orderId: 8, blockHeight: 100 }),
    ];
    expect(findDegenOrderId(events, crit)).toBe(8);
  });

  it("returns null when nothing matches", () => {
    expect(findDegenOrderId([], crit)).toBeNull();
  });
});

describe("resolveDegenBet", () => {
  const base = { targetQty: 20n, currentHeight: 101, expiresAtBlock: 103 };

  it("is tracking with no events before expiry", () => {
    const s = resolveDegenBet({ ...base, events: [] });
    expect(s.phase).toBe("tracking");
    expect(s.filledQty).toBe(0n);
  });

  it("is filled when a filled row is present", () => {
    const s = resolveDegenBet({
      ...base,
      events: [ev({ type: "filled", qty: 20n, priceNanos: 530_000_000n })],
    });
    expect(s.phase).toBe("filled");
    expect(s.filledQty).toBe(20n);
    expect(s.avgPriceNanos).toBe(530_000_000n);
  });

  it("is filled when partial fills reach the target", () => {
    const s = resolveDegenBet({
      ...base,
      events: [
        ev({ type: "partial_fill", qty: 12n, priceNanos: 500_000_000n }),
        ev({ type: "partial_fill", qty: 8n, priceNanos: 600_000_000n }),
      ],
    });
    expect(s.phase).toBe("filled");
    expect(s.filledQty).toBe(20n);
    // volume-weighted: (12*5e8 + 8*6e8)/20 = 5.4e8
    expect(s.avgPriceNanos).toBe(540_000_000n);
  });

  it("is partial when an expired row follows some fills", () => {
    const s = resolveDegenBet({
      ...base,
      events: [
        ev({ type: "partial_fill", qty: 12n, priceNanos: 500_000_000n }),
        ev({ type: "expired" }),
      ],
    });
    expect(s.phase).toBe("partial");
    expect(s.filledQty).toBe(12n);
  });

  it("is none when expired with zero fills", () => {
    const s = resolveDegenBet({ ...base, events: [ev({ type: "expired" })] });
    expect(s.phase).toBe("none");
    expect(s.filledQty).toBe(0n);
    expect(s.avgPriceNanos).toBeNull();
  });

  it("falls back to height when the terminal row is missed", () => {
    const partial = resolveDegenBet({
      ...base,
      currentHeight: 104, // >= expiresAtBlock + 1
      events: [ev({ type: "partial_fill", qty: 5n, priceNanos: 5n })],
    });
    expect(partial.phase).toBe("partial");
    const none = resolveDegenBet({ ...base, currentHeight: 104, events: [] });
    expect(none.phase).toBe("none");
  });
});
