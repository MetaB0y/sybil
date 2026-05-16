import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClientProvider, QueryClient } from "@tanstack/react-query";
import { OverviewView } from "./overview-view";

describe("OverviewView", () => {
  it("renders the stat labels before data loads", () => {
    const client = new QueryClient();
    const html = renderToStaticMarkup(
      <QueryClientProvider client={client}>
        <OverviewView />
      </QueryClientProvider>
    );
    expect(html).toContain("Markets");
    expect(html).toContain("Pending Orders");
  });
});
