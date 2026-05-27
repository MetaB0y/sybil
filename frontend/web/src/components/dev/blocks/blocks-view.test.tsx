import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClientProvider, QueryClient } from "@tanstack/react-query";
import { BlocksView } from "./blocks-view";

describe("BlocksView", () => {
  it("renders both panel headings before data loads", () => {
    const client = new QueryClient();
    const html = renderToStaticMarkup(
      <QueryClientProvider client={client}>
        <BlocksView />
      </QueryClientProvider>
    );
    expect(html).toContain("Chain Blocks");
    expect(html).toContain("Selected Block");
  });
});
