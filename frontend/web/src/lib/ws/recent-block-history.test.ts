import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ get: vi.fn() }));

vi.mock("../api/client", () => ({ api: { GET: mocks.get } }));

import {
  fetchRecentBlockHistory,
  RECENT_BLOCK_HISTORY_LIMIT,
} from "./recent-block-history";

describe("fetchRecentBlockHistory", () => {
  beforeEach(() => mocks.get.mockReset());

  it("owns one bounded recent-block request for global consumers", async () => {
    mocks.get.mockResolvedValue({ data: [], error: undefined });

    await expect(fetchRecentBlockHistory()).resolves.toEqual([]);
    expect(mocks.get).toHaveBeenCalledWith("/v1/blocks", {
      params: { query: { limit: RECENT_BLOCK_HISTORY_LIMIT } },
    });
  });

  it("surfaces read failures instead of manufacturing empty history", async () => {
    mocks.get.mockResolvedValue({ data: undefined, error: { status: 503 } });

    await expect(fetchRecentBlockHistory()).rejects.toThrow(
      "/v1/blocks recent history failed",
    );
  });
});
