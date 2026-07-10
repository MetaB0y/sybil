import { describe, expect, it } from "vitest";
import {
  findDegenOrderId,
  findDegenPendingOrderId,
  priorMaxOrderId,
  resolveDegenBet,
  type DegenEvent,
  type DegenPendingOrder,
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
    const events = [
      ev({ type: "filled", orderId: 43, blockHeight: 101, qty: 5n }),
    ];
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

  it("skips a prior order at/below the id floor (repeat-bet isolation)", () => {
    // A previous bet's filled row sits at a height >= this submit; without the
    // floor it would re-bind and read as instantly filled. The floor (=42)
    // excludes it, and the fresh order (43) hasn't surfaced yet → null.
    const events = [ev({ type: "filled", orderId: 42, blockHeight: 101 })];
    expect(
      findDegenOrderId(events, { ...crit, minOrderIdExclusive: 42 }),
    ).toBeNull();
  });

  it("binds the new order above the id floor", () => {
    const events = [
      ev({ type: "filled", orderId: 42, blockHeight: 101 }),
      ev({ type: "placed", orderId: 43, blockHeight: 101 }),
    ];
    expect(findDegenOrderId(events, { ...crit, minOrderIdExclusive: 42 })).toBe(
      43,
    );
  });

  it("returns null when nothing matches", () => {
    expect(findDegenOrderId([], crit)).toBeNull();
  });
});

function po(p: Partial<DegenPendingOrder>): DegenPendingOrder {
  return {
    order_id: 1,
    market_id: 7,
    side: "BuyYes",
    created_at_block: 100,
    ...p,
  };
}

describe("findDegenPendingOrderId", () => {
  it("binds the matching buy-side pending order", () => {
    expect(findDegenPendingOrderId([po({ order_id: 55 })], crit)).toBe(55);
  });

  it("matches NO bets to the BuyNo side", () => {
    const noCrit = { marketId: 7, outcome: "NO" as const, submitHeight: 100 };
    const pending = [
      po({ order_id: 9, side: "BuyYes" }),
      po({ order_id: 10, side: "BuyNo" }),
    ];
    expect(findDegenPendingOrderId(pending, noCrit)).toBe(10);
  });

  it("ignores wrong market, side, and orders created before this bet", () => {
    const pending = [
      po({ order_id: 1, market_id: 8 }),
      po({ order_id: 2, side: "BuyNo" }),
      po({ order_id: 3, created_at_block: 99 }),
    ];
    expect(findDegenPendingOrderId(pending, crit)).toBeNull();
  });

  it("takes the newest (highest) id to isolate this bet from an earlier rest", () => {
    const pending = [
      po({ order_id: 70, created_at_block: 100 }),
      po({ order_id: 91, created_at_block: 101 }),
    ];
    expect(findDegenPendingOrderId(pending, crit)).toBe(91);
  });

  it("skips a still-resting prior order at/below the id floor", () => {
    const pending = [po({ order_id: 70, created_at_block: 100 })];
    expect(
      findDegenPendingOrderId(pending, { ...crit, minOrderIdExclusive: 70 }),
    ).toBeNull();
  });

  it("returns null for an empty pending list", () => {
    expect(findDegenPendingOrderId([], crit)).toBeNull();
  });
});

describe("priorMaxOrderId", () => {
  it("returns the highest id for the market across both feeds", () => {
    const events = [
      { market_id: 7, order_id: 10 },
      { market_id: 7, order_id: 42 },
      { market_id: 8, order_id: 99 }, // other market — ignored
    ];
    const pending = [{ market_id: 7, order_id: 41 }];
    expect(priorMaxOrderId(7, events, pending)).toBe(42);
  });

  it("ignores rows missing an order id", () => {
    const events = [
      { market_id: 7, order_id: null },
      { market_id: 7, order_id: 5 },
    ];
    expect(priorMaxOrderId(7, events, [])).toBe(5);
  });

  it("is null when neither feed has the market (first bet)", () => {
    expect(priorMaxOrderId(7, [{ market_id: 8, order_id: 3 }], [])).toBeNull();
    expect(priorMaxOrderId(7, [], [])).toBeNull();
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

  it("is expired when an expired row has zero fills", () => {
    const s = resolveDegenBet({ ...base, events: [ev({ type: "expired" })] });
    expect(s.phase).toBe("expired");
    expect(s.filledQty).toBe(0n);
    expect(s.avgPriceNanos).toBeNull();
  });

  it("is cancelled when the cancel flag is set and nothing filled", () => {
    const s = resolveDegenBet({ ...base, events: [], cancelled: true });
    expect(s.phase).toBe("cancelled");
    expect(s.filledQty).toBe(0n);
  });

  it("is partial when cancelled after some fills (filled portion stands)", () => {
    const s = resolveDegenBet({
      ...base,
      cancelled: true,
      events: [ev({ type: "partial_fill", qty: 7n, priceNanos: 500_000_000n })],
    });
    expect(s.phase).toBe("partial");
    expect(s.filledQty).toBe(7n);
  });

  it("honours a cancelled row from the events feed", () => {
    const s = resolveDegenBet({ ...base, events: [ev({ type: "cancelled" })] });
    expect(s.phase).toBe("cancelled");
  });

  it("prefers filled over a cancel that lands the same block", () => {
    const s = resolveDegenBet({
      ...base,
      cancelled: true,
      events: [ev({ type: "filled", qty: 20n, priceNanos: 500_000_000n })],
    });
    expect(s.phase).toBe("filled");
  });

  it("expires after the height passes and the order is no longer open", () => {
    const partial = resolveDegenBet({
      ...base,
      currentHeight: 104, // >= expiresAtBlock + 1
      orderOpen: false,
      events: [ev({ type: "partial_fill", qty: 5n, priceNanos: 5n })],
    });
    expect(partial.phase).toBe("partial");
    const expired = resolveDegenBet({
      ...base,
      currentHeight: 104,
      orderOpen: false,
      events: [],
    });
    expect(expired.phase).toBe("expired");
  });

  it("keeps tracking past the nominal expiry while the order is still open", () => {
    const s = resolveDegenBet({
      ...base,
      currentHeight: 104,
      orderOpen: true,
      events: [],
    });
    expect(s.phase).toBe("tracking");
  });

  it("waits for the open-orders feed before using the expiry backstop", () => {
    const s = resolveDegenBet({
      ...base,
      currentHeight: 104,
      orderOpen: null,
      events: [],
    });
    expect(s.phase).toBe("tracking");
  });
});
