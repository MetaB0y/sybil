import { describe, expect, it } from "vitest";
import { DEGEN_BATCHES } from "./constants";
import { degenExpiry, degenQuantity } from "./degen";

const usd = (d: number): bigint => BigInt(Math.round(d * 1e9)); // $1 = 1e9 nanos
const cents = (c: number): bigint => BigInt(Math.round(c * 1e7));

describe("degenQuantity", () => {
  it("returns budget / limit as a floored share count", () => {
    expect(degenQuantity(usd(10), cents(50))).toBe(20n); // $10 / 50¢ = 20
  });

  it("floors fractional shares (does not overspend)", () => {
    expect(degenQuantity(usd(10), cents(30))).toBe(33n); // 33.33 -> 33
  });

  it("returns 0 when the budget cannot afford one share", () => {
    expect(degenQuantity(usd(0.1), cents(50))).toBe(0n);
  });

  it("guards against non-positive inputs (incl. negatives, which would otherwise floor toward -∞)", () => {
    expect(degenQuantity(0n, cents(50))).toBe(0n);
    expect(degenQuantity(usd(10), 0n)).toBe(0n);
    expect(degenQuantity(-1n, cents(50))).toBe(0n);
    expect(degenQuantity(usd(10), -1n)).toBe(0n);
  });
});

describe("degenExpiry", () => {
  it("is the latest height plus DEGEN_BATCHES", () => {
    expect(degenExpiry(100n)).toBe(100n + DEGEN_BATCHES);
  });
});
