import { describe, expect, it } from "vitest";
import { ONE_DOLLAR_NANOS } from "./constants";
import { resolveMarkNanos } from "./degen";

const FIFTY_CENTS = ONE_DOLLAR_NANOS / 2n;

describe("resolveMarkNanos", () => {
  it("prefers the history mark when present and positive", () => {
    expect(resolveMarkNanos(80_000_000n, 90_000_000n)).toBe(80_000_000n);
  });

  it("falls back to clearing when history is null", () => {
    expect(resolveMarkNanos(null, 90_000_000n)).toBe(90_000_000n);
  });

  it("falls back to clearing when history is zero", () => {
    expect(resolveMarkNanos(0n, 90_000_000n)).toBe(90_000_000n);
  });

  it("falls back to 50¢ when both are missing", () => {
    expect(resolveMarkNanos(null, null)).toBe(FIFTY_CENTS);
  });

  it("falls back to 50¢ when both are zero", () => {
    expect(resolveMarkNanos(0n, 0n)).toBe(FIFTY_CENTS);
  });

  it("treats a zero clearing as unavailable (null history, zero clearing -> 50¢)", () => {
    expect(resolveMarkNanos(null, 0n)).toBe(FIFTY_CENTS);
  });

  it("treats negative values as unavailable", () => {
    expect(resolveMarkNanos(-5n, -5n)).toBe(FIFTY_CENTS);
    expect(resolveMarkNanos(-5n, 90_000_000n)).toBe(90_000_000n);
  });
});
