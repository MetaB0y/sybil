import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { type Market, useMarket } from "./use-market";

describe("useMarket hydration", () => {
  it("keeps the server loading state even when the shared list cache is warm", () => {
    const client = new QueryClient();
    client.setQueryData(
      ["markets", "all"],
      [{ market_id: 60 }] as Market[],
    );

    function Probe() {
      const market = useMarket(60).data;
      return <span>{market ? `market ${market.market_id}` : "loading market"}</span>;
    }

    const html = renderToStaticMarkup(
      <QueryClientProvider client={client}>
        <Probe />
      </QueryClientProvider>,
    );

    expect(html).toBe("<span>loading market</span>");
  });
});
