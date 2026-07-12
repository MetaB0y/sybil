import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  clearStoredReadApiKey: vi.fn(),
  dispatchEvent: vi.fn(),
  readStoredReadApiKey: vi.fn(),
  use: vi.fn(),
}));

vi.mock("openapi-fetch", () => ({
  default: vi.fn(() => ({ use: mocks.use })),
}));

vi.mock("@/lib/account/storage", () => ({
  clearStoredReadApiKey: mocks.clearStoredReadApiKey,
  readStoredReadApiKey: mocks.readStoredReadApiKey,
}));

vi.stubGlobal("dispatchEvent", mocks.dispatchEvent);

import "./client";

describe("read-auth middleware", () => {
  beforeEach(() => {
    mocks.clearStoredReadApiKey.mockClear();
    mocks.dispatchEvent.mockClear();
    mocks.readStoredReadApiKey.mockClear();
  });

  it("invalidates only the read token after an authenticated GET 401", () => {
    const middleware = mocks.use.mock.calls[0]?.[0] as {
      onResponse: (args: { request: Request; response: Response }) => void;
    };
    const request = new Request("https://api.example.test/v1/account", {
      headers: { authorization: "Bearer sybk_stale" },
    });
    mocks.readStoredReadApiKey.mockReturnValue("sybk_stale");

    middleware.onResponse({
      request,
      response: new Response(null, { status: 401 }),
    });

    expect(mocks.clearStoredReadApiKey).toHaveBeenCalledOnce();
    expect(mocks.dispatchEvent).toHaveBeenCalledOnce();
  });

  it("does not erase a newer token when an old request finishes late", () => {
    const middleware = mocks.use.mock.calls[0]?.[0] as {
      onResponse: (args: { request: Request; response: Response }) => void;
    };
    const request = new Request("https://api.example.test/v1/account", {
      headers: { authorization: "Bearer sybk_stale" },
    });
    mocks.readStoredReadApiKey.mockReturnValue("sybk_fresh");

    middleware.onResponse({
      request,
      response: new Response(null, { status: 401 }),
    });

    expect(mocks.clearStoredReadApiKey).not.toHaveBeenCalled();
    expect(mocks.dispatchEvent).not.toHaveBeenCalled();
  });
});
