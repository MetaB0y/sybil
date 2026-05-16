import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClientProvider, QueryClient } from "@tanstack/react-query";
import { MarketsView } from "./markets-view";

describe("MarketsView", () => {
  it("renders the controls and table header before data loads", () => {
    const client = new QueryClient();
    const html = renderToStaticMarkup(
      <QueryClientProvider client={client}>
        <MarketsView />
      </QueryClientProvider>
    );
    expect(html).toContain("Market");
    expect(html).toContain("Search markets");
  });
});
