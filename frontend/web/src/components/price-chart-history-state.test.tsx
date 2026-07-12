import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { PriceChart, PriceHistoryNotice } from "./price-chart";

const EMPTY_CHART = {
  drawn: [],
  byMarket: new Map(),
  mode: "area" as const,
  sinceMs: null,
  nowMs: 0,
};

describe("PriceChart history states", () => {
  it("does not describe pending or failed history as a real empty chart", () => {
    const pending = renderToStaticMarkup(
      <PriceChart {...EMPTY_CHART} historyPending />,
    );
    const failed = renderToStaticMarkup(
      <PriceChart {...EMPTY_CHART} historyUnavailable />,
    );

    expect(pending).toContain("loading clearing history…");
    expect(pending).not.toContain("no clearing history yet");
    expect(failed).toContain("clearing history unavailable — retry above.");
    expect(failed).not.toContain("no clearing history yet");
  });

  it("distinguishes incomplete history from a failed cached refresh", () => {
    const incomplete = renderToStaticMarkup(
      <PriceHistoryNotice
        failureCount={2}
        unavailableCount={1}
        retrying={false}
        onRetry={vi.fn()}
      />,
    );
    const stale = renderToStaticMarkup(
      <PriceHistoryNotice
        failureCount={1}
        unavailableCount={0}
        retrying
        onRetry={vi.fn()}
      />,
    );

    expect(incomplete).toContain('role="alert"');
    expect(incomplete).toContain(
      "failed to load price history for 2 outcomes · chart may be incomplete",
    );
    expect(incomplete).toContain(">retry</button>");
    expect(stale).toContain(
      "price history refresh failed for 1 outcome · showing saved data",
    );
    expect(stale).toContain('role="status"');
    expect(stale).toContain("disabled");
    expect(stale).toContain("retrying…");
  });

  it("does not call a cached empty history unavailable after refresh failure", () => {
    const cachedEmpty = renderToStaticMarkup(
      <>
        <PriceHistoryNotice
          failureCount={1}
          unavailableCount={0}
          retrying={false}
          onRetry={vi.fn()}
        />
        <PriceChart {...EMPTY_CHART} historyUnavailable={false} />
      </>,
    );

    expect(cachedEmpty).toContain("showing saved data");
    expect(cachedEmpty).toContain("no clearing history yet");
    expect(cachedEmpty).not.toContain("clearing history unavailable —");
  });
});
