import { describe, expect, it } from "vitest";
import { DEGEN_BATCHES } from "./constants";
import { degenExpiry, degenQuantity } from "./degen";

const usd = (d: number): bigint => BigInt(Math.round(d * 1e9)); // $1 = 1e9 nanos
const cents = (c: number): bigint => BigInt(Math.round(c * 1e7));

describe("degenQuantity", () => {
  it("returns budget / limit as floored share-units", () => {
    expect(degenQuantity(usd(10), cents(50))).toBe(20_000n); // $10 / 50¢ = 20 shares
  });

  it("keeps fractional shares down to 0.001 without overspending", () => {
    expect(degenQuantity(usd(10), cents(30))).toBe(33_333n); // 33.333 shares
  });

  it("allows sub-share bets when the budget can afford one share-unit", () => {
    expect(degenQuantity(usd(0.1), cents(50))).toBe(200n); // 0.2 shares
  });

  it("guards against non-positive inputs (incl. negatives, which would otherwise floor toward -∞)", () => {
    expect(degenQuantity(0n, cents(50))).toBe(0n);
    expect(degenQuantity(usd(10), 0n)).toBe(0n);
    expect(degenQuantity(-1n, cents(50))).toBe(0n);
    expect(degenQuantity(usd(10), -1n)).toBe(0n);
  });
});

describe("degenExpiry", () => {
  it("keeps a degen bet live for 12 batches", () => {
    expect(DEGEN_BATCHES).toBe(12n);
  });

  it("is the latest height plus DEGEN_BATCHES", () => {
    expect(degenExpiry(100n)).toBe(100n + DEGEN_BATCHES);
  });
});
