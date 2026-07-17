import { describe, expect, it } from "vitest";
import { sparklineDomain } from "./sparkline";

describe("sparklineDomain", () => {
  it("gives small moves a 20 percentage-point minimum domain", () => {
    expect(sparklineDomain([510_000_000, 530_000_000])).toEqual([
      420_000_000, 620_000_000,
    ]);
  });

  it("preserves a wider observed domain", () => {
    expect(sparklineDomain([100_000_000, 900_000_000])).toEqual([
      100_000_000, 900_000_000,
    ]);
  });

  it("keeps the minimum domain inside probability bounds", () => {
    expect(sparklineDomain([10_000_000, 20_000_000])).toEqual([
      0, 200_000_000,
    ]);
    expect(sparklineDomain([980_000_000, 990_000_000])).toEqual([
      800_000_000, 1_000_000_000,
    ]);
  });
});
