import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { LeaderboardReadNotice, LeaderboardTable } from "./leaderboard-table";

describe("LeaderboardTable read states", () => {
  it("does not turn a cold transport failure into an empty leaderboard", () => {
    const html = renderToStaticMarkup(
      <LeaderboardTable
        rows={[]}
        isLoading={false}
        isRetrying={false}
        readState="unavailable"
        errorMessage="Leaderboard window predates available historical data"
        onRetry={() => {}}
        myAccountId={null}
      />,
    );

    expect(html).toContain('role="alert"');
    expect(html).toContain("leaderboard unavailable");
    expect(html).toContain("window predates available historical data");
    expect(html).toContain(">retry<");
    expect(html).not.toContain("no ranked traders yet");
  });

  it("keeps saved rows visible when a refresh fails", () => {
    const html = renderToStaticMarkup(
      <LeaderboardTable
        rows={[
          {
            rank: 1,
            accountId: 42,
            label: "Trader #42",
            pnlNanos: 1_000_000_000n,
            roiBps: 100,
            marketsTraded: 3,
            equityNanos: 101_000_000_000n,
          },
        ]}
        isLoading={false}
        isRetrying={false}
        readState="stale"
        onRetry={() => {}}
        myAccountId={42}
      />,
    );

    expect(html).toContain('role="status"');
    expect(html).toContain("showing saved rankings");
    expect(html).toContain("Trader #42");
  });

  it("disables retry while the request is in flight", () => {
    const html = renderToStaticMarkup(
      <LeaderboardReadNotice stale={false} retrying onRetry={() => {}} />,
    );

    expect(html).toContain("disabled");
    expect(html).toContain("retrying…");
  });
});
