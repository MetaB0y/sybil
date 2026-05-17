import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClientProvider, QueryClient } from "@tanstack/react-query";
import { BotsView } from "./bots-view";

describe("BotsView", () => {
  it("renders the Bot Summaries heading before data loads", () => {
    const client = new QueryClient();
    const html = renderToStaticMarkup(
      <QueryClientProvider client={client}>
        <BotsView />
      </QueryClientProvider>
    );
    expect(html).toContain("Bot Summaries");
  });
  it("shows the DB-unavailable message when no data has loaded", () => {
    const client = new QueryClient();
    const html = renderToStaticMarkup(
      <QueryClientProvider client={client}>
        <BotsView />
      </QueryClientProvider>
    );
    expect(html).toContain("Arena decision database is not mounted.");
  });
});
