import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ get: vi.fn() }));

vi.mock("../api/client", () => ({ api: { GET: mocks.get } }));

import { fetchBlockPage } from "./use-batches";

describe("fetchBlockPage", () => {
  beforeEach(() => mocks.get.mockReset());

  it("throws on API errors instead of silently turning them into no rows", async () => {
    mocks.get.mockResolvedValue({ data: undefined, error: { status: 503 } });

    await expect(fetchBlockPage(101, 60)).rejects.toThrow(
      "/v1/blocks page failed",
    );
  });

  it("preserves a successful empty chain as a real empty result", async () => {
    mocks.get.mockResolvedValue({ data: [], error: undefined });

    await expect(fetchBlockPage(101, 60)).resolves.toEqual([]);
  });
});
