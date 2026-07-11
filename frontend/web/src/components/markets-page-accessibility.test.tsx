import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Market } from "@/lib/markets/use-markets";
import { batchPillLabelColor } from "./batch-pill";
import { BinaryCard } from "./binary-card";
import { ClearingTicker } from "./clearing-ticker";
import { MultiCard } from "./multi-card";

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
    const ticker = renderToStaticMarkup(
      <ClearingTicker marketsById={new Map()} />,
    );

    expect(batchPillLabelColor(true)).toBe("var(--accent)");
    expect(batchPillLabelColor(false)).toBe("var(--warn)");
    expect(ticker).toMatch(/color:var\(--fg-2\)[^>]*>awaiting fills…<\/span>/);
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
});
