import { describe, expect, it } from "vitest";
import { chartLineSelection } from "./chart-selection";

const AVAILABLE = new Set([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
const DEFAULTS = [1, 2, 3, 4, 5];

function select(over: Partial<Parameters<typeof chartLineSelection>[0]> = {}) {
  return chartLineSelection({
    selectedIds: null,
    visitedIds: [],
    activeId: 1,
    availableIds: AVAILABLE,
    defaultIds: DEFAULTS,
    max: 8,
    ...over,
  });
}

describe("chartLineSelection", () => {
  it("falls back to the favourite-first default", () => {
    expect(select()).toEqual(DEFAULTS);
  });

  it("appends the active outcome rather than reshuffling", () => {
    expect(select({ activeId: 9 })).toEqual([...DEFAULTS, 9]);
  });

  // The reported bug: picking an outcome in the Pro rail drew its line, then
  // switching outcome above the chart dropped it again.
  it("keeps an outcome you switched to after you switch away", () => {
    expect(select({ visitedIds: [9], activeId: 10 })).toEqual([
      ...DEFAULTS,
      9,
      10,
    ]);
  });

  it("does not duplicate the active outcome or a repeat visit", () => {
    expect(select({ visitedIds: [9, 9, 10], activeId: 9 })).toEqual([
      ...DEFAULTS,
      9,
      10,
    ]);
  });

  it("never re-adds an outcome already in the committed selection", () => {
    expect(select({ selectedIds: [3, 4], visitedIds: [3], activeId: 4 })).toEqual(
      [3, 4],
    );
  });

  it("drops defaults, not visited lines, at the cap", () => {
    const out = select({ visitedIds: [6, 7, 8, 9], activeId: 10 });
    expect(out).toHaveLength(8);
    expect(out.slice(-5)).toEqual([6, 7, 8, 9, 10]);
    expect(out.slice(0, 3)).toEqual([1, 2, 3]);
  });

  it("keeps the most recent visits when they alone exceed the cap", () => {
    const visited = [1, 2, 3, 4, 5, 6, 7, 8, 9];
    const out = select({ selectedIds: [10], visitedIds: visited, activeId: 9 });
    expect(out).toHaveLength(8);
    expect(out).not.toContain(1);
    expect(out).toContain(9);
  });

  it("drops ids from another event", () => {
    expect(select({ selectedIds: [99], visitedIds: [98], activeId: 2 })).toEqual(
      DEFAULTS,
    );
  });

  it("honours a committed selection over the defaults", () => {
    expect(select({ selectedIds: [7, 8], activeId: 7 })).toEqual([7, 8]);
  });
});
