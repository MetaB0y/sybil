import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import {
  deriveEventHoldingsGate,
  EventHoldingsReadNotice,
} from "./event-holdings";

describe("EventHoldings read states", () => {
  it("only hides a connected empty panel after every read succeeds", () => {
    expect(
      deriveEventHoldingsGate({
        connected: false,
        hasAny: false,
        pendingWithoutData: true,
        failureCount: 0,
        missingFailureCount: 0,
      }),
    ).toBe("hidden");
    expect(
      deriveEventHoldingsGate({
        connected: true,
        hasAny: false,
        pendingWithoutData: true,
        failureCount: 0,
        missingFailureCount: 0,
      }),
    ).toBe("loading");
    expect(
      deriveEventHoldingsGate({
        connected: true,
        hasAny: true,
        pendingWithoutData: false,
        failureCount: 1,
        missingFailureCount: 1,
      }),
    ).toBe("unavailable");
    expect(
      deriveEventHoldingsGate({
        connected: true,
        hasAny: true,
        pendingWithoutData: false,
        failureCount: 1,
        missingFailureCount: 0,
      }),
    ).toBe("stale");
    expect(
      deriveEventHoldingsGate({
        connected: true,
        hasAny: false,
        pendingWithoutData: false,
        failureCount: 0,
        missingFailureCount: 0,
      }),
    ).toBe("hidden");
  });

  it("renders accessible loading, unavailable, and stale notices", () => {
    const loading = renderToStaticMarkup(
      <EventHoldingsReadNotice
        state="loading"
        retrying={false}
        onRetry={vi.fn()}
      />,
    );
    const unavailable = renderToStaticMarkup(
      <EventHoldingsReadNotice
        state="unavailable"
        retrying={false}
        onRetry={vi.fn()}
      />,
    );
    const stale = renderToStaticMarkup(
      <EventHoldingsReadNotice
        state="stale"
        retrying
        onRetry={vi.fn()}
      />,
    );

    expect(loading).toContain('role="status"');
    expect(loading).toContain("loading your positions &amp; orders…");
    expect(unavailable).toContain('role="alert"');
    expect(unavailable).toContain("no account data is shown as empty");
    expect(unavailable).toContain(">retry</button>");
    expect(stale).toContain('role="status"');
    expect(stale).toContain("showing saved data");
    expect(stale).toContain("disabled");
  });
});
