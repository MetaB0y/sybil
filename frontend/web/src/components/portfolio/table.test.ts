import { describe, expect, it } from "vitest";
import {
  bodyRowGrid,
  cmpBig,
  cmpNullableBig,
  headerRowGrid,
  nextSort,
  ROW_GAP,
  ROW_PADDING,
} from "./table";

describe("nextSort", () => {
  it("opens numeric columns high→low and text columns A→Z", () => {
    expect(nextSort(null, "value", true)).toEqual({
      key: "value",
      dir: "desc",
    });
    expect(nextSort(null, "market", false)).toEqual({
      key: "market",
      dir: "asc",
    });
  });

  it("flips direction when the active column is clicked again", () => {
    const first = nextSort(null, "value", true);
    expect(nextSort(first, "value", true)).toEqual({
      key: "value",
      dir: "asc",
    });
  });

  it("restarts at the column's own default when switching columns", () => {
    const active = { key: "market", dir: "desc" } as const;
    expect(nextSort(active, "value", true)).toEqual({
      key: "value",
      dir: "desc",
    });
  });
});

describe("cmpNullableBig", () => {
  it("sorts nulls below every known value", () => {
    expect(cmpNullableBig(null, 1n)).toBe(-1);
    expect(cmpNullableBig(1n, null)).toBe(1);
    expect(cmpNullableBig(null, null)).toBe(0);
    expect(cmpNullableBig(undefined, null)).toBe(0);
  });

  it("otherwise matches a plain bigint comparison", () => {
    expect(cmpNullableBig(2n, 1n)).toBe(cmpBig(2n, 1n));
    expect(cmpNullableBig(-5n, 5n)).toBe(cmpBig(-5n, 5n));
    expect(cmpNullableBig(7n, 7n)).toBe(0);
  });
});

describe("row grids", () => {
  // The tabs drifted apart precisely because each owned its own padding/gap.
  it("give the header and body rows identical metrics", () => {
    const columns = "28px minmax(0, 1fr) 60px";
    const head = headerRowGrid(columns);
    const body = bodyRowGrid(columns);
    for (const key of [
      "gridTemplateColumns",
      "gap",
      "padding",
      "fontSize",
      "fontFamily",
    ] as const) {
      expect(body[key]).toBe(head[key]);
    }
    expect(head.gap).toBe(ROW_GAP);
    expect(head.padding).toBe(ROW_PADDING);
  });

  it("puts the divider on body rows only", () => {
    expect(bodyRowGrid("1fr").borderTop).toBe("1px solid var(--border-1)");
    expect(headerRowGrid("1fr").borderTop).toBeUndefined();
  });
});
