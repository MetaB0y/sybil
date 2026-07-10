import { describe, expect, it } from "vitest";
import { humanizeOrderError } from "./order-errors";

describe("humanizeOrderError", () => {
  it("maps an insufficient-balance rejection to friendly copy", () => {
    const raw = new Error(
      "submit_signed failed (HTTP 400): order 18293722 rejected: InsufficientBalance { required: 1009762112570, available: 1000000000000 }",
    );
    expect(humanizeOrderError(raw, "bet")).toBe("Not enough balance for this bet.");
    expect(humanizeOrderError(raw, "order")).toBe(
      "Not enough balance for this order.",
    );
  });

  it("maps insufficient position (sell) without leaking internals", () => {
    const raw = new Error(
      "submit_signed failed (HTTP 400): order 1 rejected: InsufficientPosition { market: 5, outcome: 0, required: 10, available: 2 }",
    );
    expect(humanizeOrderError(raw)).toBe("You don't have enough shares to sell.");
  });

  it("maps an expired order", () => {
    const raw = new Error(
      "order 7 rejected: Expired { current_block: 100, expires_at_block: 99 }",
    );
    expect(humanizeOrderError(raw, "bet")).toMatch(/didn't make it into a batch/i);
  });

  it("maps transport / signer errors", () => {
    expect(humanizeOrderError(new Error("invalid P256 signature"))).toMatch(
      /verify your bet/i,
    );
    expect(humanizeOrderError(new Error("rate limited; retry after 3s"))).toMatch(
      /too many orders/i,
    );
    expect(humanizeOrderError(new Error("mempool full"))).toMatch(/network is busy/i);
  });

  it("maps a 409 replay-nonce rejection to a retry nudge", () => {
    const raw = new Error(
      "submit_signed failed (HTTP 409): replay nonce is stale or duplicate",
    );
    expect(humanizeOrderError(raw, "order")).toBe(
      "This order was already submitted — try again.",
    );
    expect(humanizeOrderError(raw, "order")).not.toMatch(/HTTP|nonce|replay/);
  });

  it("tells a complete-set rejection what to cancel, not just that it failed", () => {
    const raw = new Error(
      "submit_signed failed (HTTP 400): order 692574 rejected: CompleteSetFormation",
    );
    const out = humanizeOrderError(raw, "bet");
    expect(out).toMatch(/cover the other outcomes/i);
    expect(out).toMatch(/cancel one/i);
    expect(out).not.toMatch(/CompleteSetFormation|HTTP/);
  });

  it("falls back to a generic line and never echoes the raw string", () => {
    const raw = new Error("submit_signed failed (HTTP 500): kaboom internal panic");
    const out = humanizeOrderError(raw, "bet");
    expect(out).toBe("Couldn't place your bet. Try again.");
    expect(out).not.toMatch(/HTTP|panic|kaboom/);
  });
});
