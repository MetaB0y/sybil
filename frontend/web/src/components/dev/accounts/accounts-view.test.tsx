import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClientProvider, QueryClient } from "@tanstack/react-query";
import { AccountsView } from "./accounts-view";

describe("AccountsView", () => {
  it("renders the major panel headings before data loads", () => {
    const client = new QueryClient();
    const html = renderToStaticMarkup(
      <QueryClientProvider client={client}>
        <AccountsView />
      </QueryClientProvider>
    );
    expect(html).toContain("Active Trading Accounts");
    expect(html).toContain("Participants");
  });
});
