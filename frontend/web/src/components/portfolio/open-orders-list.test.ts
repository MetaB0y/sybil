import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { OrderRow, type OpenRow } from "./open-orders-list";

const row: OpenRow = {
  order: {
    account_id: 7,
    created_at_block: 40,
    created_at_ms: 1_700_000_000_000,
    expires_at_block: 0,
    limit_price_nanos: "500000000",
    market_id: 12,
    order_id: 99,
    original_quantity: 2_000,
    remaining_quantity: 1_000,
    side: "BuyYes",
  },
  market: undefined,
  label: "Market #12",
  action: "BUY",
  outcome: "YES",
  placed: 2_000,
  filled: 1_000,
  remaining: 1_000,
  limitNanos: 500_000_000n,
  valueNanos: 500_000_000n,
  avgPriceNanos: 490_000_000n,
  fillCount: 1,
  createdMs: 1_700_000_000_000,
  createdBlock: 40,
  expiresAtBlock: 0,
};

describe("OrderRow", () => {
  it("keeps the Cancel button outside the market links", () => {
    const html = renderToStaticMarkup(
      createElement(OrderRow, {
        row,
        nowMs: 1_700_000_005_000,
        accountId: 7,
        publicKeyHex: "02" + "11".repeat(32),
        onCancelled: vi.fn(),
      }),
    );

    const button = html.indexOf("<button");
    const nearestAnchor = html.lastIndexOf("<a", button);
    expect(button).toBeGreaterThan(0);
    expect(nearestAnchor).toBeGreaterThan(0);
    expect(html.slice(nearestAnchor, button)).toContain("</a>");
    expect(html.match(/<a\b/g)).toHaveLength(1);
    expect(html).toContain('data-order-id="99"');
  });
});
