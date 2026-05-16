import { describe, expect, it } from "vitest";
import { mergeBlocks } from "./use-recent-blocks";
import type { DevBlock } from "./types";

const b = (height: number): DevBlock => ({ height });

describe("mergeBlocks", () => {
  it("dedupes by height and sorts ascending", () => {
    expect(mergeBlocks([b(3), b(1)], [b(2), b(3)], 80).map((x) => x.height)).toEqual([1, 2, 3]);
  });
  it("caps to the window, keeping the newest", () => {
    const out = mergeBlocks([b(1), b(2), b(3), b(4)], [], 2);
    expect(out.map((x) => x.height)).toEqual([3, 4]);
  });
  it("live block wins over backfill at the same height", () => {
    const out = mergeBlocks([{ height: 5, fill_count: 0 }], [{ height: 5, fill_count: 9 }], 80);
    expect(out[0]?.fill_count).toBe(9);
  });
});
