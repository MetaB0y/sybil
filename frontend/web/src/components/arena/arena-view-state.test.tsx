import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  feed: {} as Record<string, unknown>,
  equity: {} as Record<string, unknown>,
  history: {} as Record<string, unknown>,
  drift: {} as Record<string, unknown>,
  useDecisionHistory: vi.fn(),
}));

vi.mock("@/lib/arena/use-arena-feed", () => ({
  useArenaFeed: () => mocks.feed,
  useArenaEquitySeries: () => mocks.equity,
  useArenaDecisionHistory: mocks.useDecisionHistory,
}));

vi.mock("@/lib/activity/use-activity-overview", () => ({
  useActivityOverview: () => ({
    allTime: { matchedVolume: "—", welfare: "—" },
    last24h: { matchedVolume: "—", welfare: "—" },
    isLoading: false,
  }),
}));

vi.mock("@/lib/dev/use-recent-blocks", () => ({
  useDevRecentBlocks: () => ({
    blocks: [],
    latestBlock: null,
    isBackfilling: false,
  }),
}));

import { ArenaView } from "./arena-view";

const refetch = vi.fn();
const readyFeed = {
  db_available: true,
  error: null,
  stats: {
    articles: 0,
    decisions: 0,
    snapshots: 0,
    token_usage: 0,
    traders: 1,
    latest_decision_timestamp: null,
  },
  summaries: [{ trader_name: "Alice (Flat)", decision_count: 0 }],
  decisions: [],
  token_usage: [],
};
const readyEquity = {
  db_available: true,
  error: null,
  points: [],
  downsampled: false,
  stride: 1,
  source_rows: 0,
};
const readyHistory = {
  ...readyFeed,
  summaries: [],
  stats: { ...readyFeed.stats, traders: 0 },
};

function query(data: unknown) {
  return {
    data,
    isPending: false,
    isError: false,
    isFetching: false,
    refetch,
  };
}

beforeEach(() => {
  Object.assign(mocks.feed, query(readyFeed));
  Object.assign(mocks.equity, query(readyEquity));
  Object.assign(mocks.history, query(readyHistory));
  Object.assign(mocks.drift, query(undefined));
  mocks.useDecisionHistory.mockReset();
  mocks.useDecisionHistory
    .mockReturnValueOnce(mocks.history)
    .mockReturnValueOnce(mocks.drift);
  refetch.mockClear();
});

describe("ArenaView query-state integration", () => {
  it("does not render dashboard totals while the primary feed is pending", () => {
    Object.assign(mocks.feed, query(undefined), { isPending: true });

    const html = renderToStaticMarkup(<ArenaView />);

    expect(html).toContain("Loading Arena");
    expect(html).not.toContain("Est. LLM Cost");
  });

  it("does not render dashboard totals after an initial transport failure", () => {
    Object.assign(mocks.feed, query(undefined), { isError: true });

    const html = renderToStaticMarkup(<ArenaView />);

    expect(html).toContain("Arena feed unavailable");
    expect(html).toContain("Retry Arena feed");
    expect(html).not.toContain("Est. LLM Cost");
  });

  it("keeps cached dashboard data visible after a refresh failure", () => {
    Object.assign(mocks.feed, query(readyFeed), { isError: true });

    const html = renderToStaticMarkup(<ArenaView />);

    expect(html).toContain("Arena refresh failed");
    expect(html).toContain("Est. LLM Cost");
  });

  it("marks article pills as coarse-pointer action links", () => {
    Object.assign(
      mocks.feed,
      query({
        ...readyFeed,
        decisions: [
          {
            id: 17,
            trader_name: "Alice (Flat)",
            market_id: 7,
            market_name: "Accessible market",
            article_urls: ["https://example.com/evidence"],
            orders: [],
          },
        ],
      }),
    );

    const html = renderToStaticMarkup(<ArenaView />);

    expect(html).toContain(
      'class="mobile-action-link" href="https://example.com/evidence"',
    );
  });

  it("does not render totals when the API reports its database unavailable", () => {
    Object.assign(
      mocks.feed,
      query({
        ...readyFeed,
        db_available: false,
        error: "decision database cannot be opened",
      }),
    );

    const html = renderToStaticMarkup(<ArenaView />);

    expect(html).toContain("decision database cannot be opened");
    expect(html).not.toContain("Est. LLM Cost");
  });

  it("does not call an equity failure empty history", () => {
    Object.assign(mocks.equity, query(undefined), { isError: true });

    const html = renderToStaticMarkup(<ArenaView />);

    expect(html).toContain("Equity history unavailable");
    expect(html).toContain("Retry history");
    expect(html).not.toContain("No equity history yet");
  });

  it("surfaces an equity database failure without fake chart values", () => {
    Object.assign(
      mocks.equity,
      query({
        ...readyEquity,
        db_available: false,
        error: "equity table cannot be read",
      }),
    );

    const html = renderToStaticMarkup(<ArenaView />);

    expect(html).toContain("equity table cannot be read");
    expect(html).not.toContain("Latest Equity</div>");
  });

  it("keeps cached equity visible when its background refresh fails", () => {
    Object.assign(mocks.equity, query(readyEquity), { isError: true });

    const html = renderToStaticMarkup(<ArenaView />);

    expect(html).toContain("Equity history refresh failed");
    expect(html).toContain("Latest Equity");
  });

  it("does not call a decision-history failure empty FV history", () => {
    Object.assign(mocks.history, query(undefined), { isError: true });

    const html = renderToStaticMarkup(<ArenaView />);

    expect(html).toContain("Bot decision history unavailable");
    expect(html).not.toContain("No FV drift history yet");
  });

  it("surfaces a drift database failure instead of an empty FV history", () => {
    Object.assign(
      mocks.history,
      query({
        ...readyHistory,
        decisions: [
          {
            id: 1,
            trader_name: "Alice (Flat)",
            market_id: 1,
            market_name: "Test market",
            article_urls: [],
            orders: [],
          },
        ],
      }),
    );
    Object.assign(
      mocks.drift,
      query({
        ...readyHistory,
        db_available: false,
        error: "drift table cannot be read",
      }),
    );

    const html = renderToStaticMarkup(<ArenaView />);

    expect(html).toContain("drift table cannot be read");
    expect(html).not.toContain("No FV drift history yet");
  });
});
