import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ get: vi.fn() }));

vi.mock("../api/client", () => ({ api: { GET: mocks.get } }));

import { fetchBatchBackfill } from "./use-batches";

describe("fetchBatchBackfill", () => {
  beforeEach(() => mocks.get.mockReset());

  it("throws on API errors instead of silently turning them into no rows", async () => {
    mocks.get.mockResolvedValue({ data: undefined, error: { status: 503 } });

    await expect(fetchBatchBackfill(60)).rejects.toThrow(
      "/v1/blocks backfill failed",
    );
  });

  it("preserves a successful empty chain as a real empty result", async () => {
    mocks.get.mockResolvedValue({ data: [], error: undefined });

    await expect(fetchBatchBackfill(60)).resolves.toEqual([]);
  });
});
