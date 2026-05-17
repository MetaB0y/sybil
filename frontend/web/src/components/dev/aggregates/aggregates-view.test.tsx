import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClientProvider, QueryClient } from "@tanstack/react-query";
import { AggregatesView } from "./aggregates-view";

describe("AggregatesView", () => {
  it("renders the major panel headings before data loads", () => {
    const client = new QueryClient();
    const html = renderToStaticMarkup(
      <QueryClientProvider client={client}>
        <AggregatesView />
      </QueryClientProvider>
    );
    expect(html).toContain("Per-Market Aggregates");
    expect(html).toContain("Open-Batch Indicative");
  });
});
