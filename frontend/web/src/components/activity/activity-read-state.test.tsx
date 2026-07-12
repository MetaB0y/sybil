import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { AllTimeStats } from "@/lib/activity/types";
import { deriveActivityReadState } from "@/lib/activity/use-activity-overview";
import { HeroAllTime } from "./hero-all-time";
import { ActivityOverviewReadNotice } from "./overview-read-notice";

const unavailableAllTime: AllTimeStats = {
  matchedVolume: "—",
  welfare: "—",
  traders: null,
  ordersPlacedDistinct: null,
  ordersMatched: null,
  ordersUnmatched: null,
  totalBatches: null,
  liveMarkets: null,
};

describe("Activity overview read states", () => {
  it("distinguishes cold failures from loading and cached refresh failures", () => {
    expect(
      deriveActivityReadState([
        { hasData: false, isPending: true, error: null },
      ]),
    ).toBe("loading");
    expect(
      deriveActivityReadState([
        { hasData: false, isPending: false, error: new Error("offline") },
      ]),
    ).toBe("unavailable");
    expect(
      deriveActivityReadState([
        { hasData: true, isPending: false, error: new Error("refresh") },
      ]),
    ).toBe("stale");
    expect(
      deriveActivityReadState([
        { hasData: true, isPending: false, error: null },
      ]),
    ).toBe("ready");
  });

  it("never renders missing batch and market sources as real zeros", () => {
    const html = renderToStaticMarkup(
      <HeroAllTime allTime={unavailableAllTime} botCount={null} />,
    );

    expect(html).toContain("— batches · — live markets");
    expect(html).not.toContain("0 batches");
    expect(html).not.toContain("0 live markets");
  });

  it("renders accessible loading, unavailable, and stale notices", () => {
    const loading = renderToStaticMarkup(
      <ActivityOverviewReadNotice
        state="loading"
        retrying={false}
        onRetry={vi.fn()}
      />,
    );
    const unavailable = renderToStaticMarkup(
      <ActivityOverviewReadNotice
        state="unavailable"
        retrying={false}
        onRetry={vi.fn()}
      />,
    );
    const stale = renderToStaticMarkup(
      <ActivityOverviewReadNotice
        state="stale"
        retrying
        onRetry={vi.fn()}
      />,
    );

    expect(loading).toContain('role="status"');
    expect(unavailable).toContain('role="alert"');
    expect(unavailable).toContain("missing values are shown as —");
    expect(unavailable).toContain(">retry</button>");
    expect(stale).toContain('role="status"');
    expect(stale).toContain("showing saved data");
    expect(stale).toContain("disabled");
  });
});
