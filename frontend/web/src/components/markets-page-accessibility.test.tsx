import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ComponentPropsWithRef } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { Market } from "@/lib/markets/use-markets";
import { batchPillLabelColor } from "./batch-pill";
import { BinaryCard } from "./binary-card";
import { ClearingTicker } from "./clearing-ticker";
import { MultiCard } from "./multi-card";

vi.mock("next/link", async () => {
  const { createElement } = await import("react");
  return {
    default: ({
      prefetch,
      ...props
    }: ComponentPropsWithRef<"a"> & { prefetch?: boolean | null }) =>
      createElement("a", {
        ...props,
        "data-prefetch": String(prefetch),
      }),
  };
});

const UNCATEGORIZED_MARKET: Market = {
  market_id: 7,
  name: "Will the accessibility pass ship?",
  status: "open",
};

function withQueries(node: React.ReactNode): string {
  return renderToStaticMarkup(
    <QueryClientProvider client={new QueryClient()}>
      {node}
    </QueryClientProvider>,
  );
}

describe("markets page accessibility", () => {
  it("keeps live batch and empty-trades copy on AA contrast tokens", () => {
    const ticker = withQueries(<ClearingTicker marketsById={new Map()} />);

    expect(batchPillLabelColor(true)).toBe("var(--accent)");
    expect(batchPillLabelColor(false)).toBe("var(--warn)");
    expect(ticker).toContain('class="clearing-ticker"');
    expect(ticker).toMatch(
      /role="status"[^>]*color:var\(--fg-2\)[^>]*>loading recent trades…<\/span>/,
    );
  });

  it("uses a non-skipped heading level and readable empty category on cards", () => {
    const binary = withQueries(
      <BinaryCard market={UNCATEGORIZED_MARKET} price={undefined} />,
    );
    const multi = withQueries(
      <MultiCard
        groupName="Accessibility launch"
        markets={[UNCATEGORIZED_MARKET]}
        prices={{}}
      />,
    );

    for (const card of [binary, multi]) {
      expect(card).toContain("<h2");
      expect(card).not.toContain("<h3");
      expect(card).toMatch(/color:var\(--fg-3\)[^>]*>uncategorized<\/span>/);
    }
  });

  it("does not prefetch every market-detail route in the card grid", () => {
    const binary = withQueries(
      <BinaryCard market={UNCATEGORIZED_MARKET} price={undefined} />,
    );
    const multiMarkets: Market[] = [7, 8, 9].map((marketId) => ({
      ...UNCATEGORIZED_MARKET,
      market_id: marketId,
      event_id: "launch-event",
      event_title: "Launch event",
    }));
    const multi = withQueries(
      <MultiCard
        groupName="Accessibility launch"
        markets={multiMarkets}
        prices={{}}
      />,
    );

    const prefetchProps = `${binary}${multi}`.match(/data-prefetch="[^"]+"/g);
    expect(prefetchProps).toHaveLength(5);
    expect(prefetchProps).toEqual(
      Array.from({ length: 5 }, () => 'data-prefetch="false"'),
    );
    expect(multi.match(/class="mobile-action-link"/g)).toHaveLength(2);
  });
});
