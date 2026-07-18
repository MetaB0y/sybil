import { beforeEach, describe, expect, it } from "vitest";

import type { Block } from "../ws/types";
import { useStore } from "./index";

function block(height: number, yes: string): Block {
  return {
    bridge: { deposit_count: 0, deposit_root_hex: "" },
    clearing_prices_nanos: {
      "7": [yes, String(1_000_000_000n - BigInt(yes))],
    },
    events_root: "",
    fill_count: 0,
    height,
    order_count: 0,
    orders_filled: 0,
    parent_hash: "",
    rejection_count: 0,
    state_root: "",
    timestamp_ms: height * 1000,
    total_volume_nanos: "0",
    total_welfare_nanos: "0",
  };
}

describe("recent block store", () => {
  beforeEach(() => {
    useStore.getState().resetForFreshSnapshot();
  });

  it("does not let a late history bootstrap regress the live head or price", () => {
    useStore.getState().applyBlock(block(62, "620000000"));
    useStore
      .getState()
      .applyBlocks(
        Array.from({ length: 60 }, (_, index) => block(index + 1, "500000000")),
      );

    const state = useStore.getState();
    expect(state.latestBlock?.height).toBe(62);
    expect(state.pricesByMarketId[7]?.yes).toBe(620_000_000n);
    expect(state.recentBlocks.map((item) => item.height)).toEqual([
      62,
      ...Array.from({ length: 60 }, (_, index) => 60 - index),
    ]);
  });

  it("converges to the same ordered history whether live or history lands first", () => {
    const history = [block(2, "520000000"), block(1, "510000000")];
    const live = block(3, "530000000");

    useStore.getState().applyBlocks(history);
    useStore.getState().applyBlock(live);
    const historyFirst = useStore
      .getState()
      .recentBlocks.map((item) => item.height);

    useStore.getState().resetForFreshSnapshot();
    useStore.getState().applyBlock(live);
    useStore.getState().applyBlocks(history);
    const liveFirst = useStore
      .getState()
      .recentBlocks.map((item) => item.height);

    expect(liveFirst).toEqual(historyFirst);
    expect(liveFirst).toEqual([3, 2, 1]);
  });
});
