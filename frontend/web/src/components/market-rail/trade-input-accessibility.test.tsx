import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { EventOutcome } from "@/lib/market-detail/use-event-group";
import { BuyBox } from "./buy-box";
import { DegenAmount } from "./degen-amount";

const OUTCOME: EventOutcome = {
  marketId: 7,
  closed: false,
  label: "Will the launch happen?",
  shortLabel: "Launch",
  yesPriceNanos: 600_000_000n,
  noPriceNanos: 400_000_000n,
  yesCents: 60,
  delta24Cents: 2,
  volume24hNanos: 1_000_000_000n,
  createdAtMs: null,
  endDateMs: null,
};

describe("trade input accessibility", () => {
  it("programmatically names Pro amount and limit controls", () => {
    const html = renderToStaticMarkup(
      <QueryClientProvider client={new QueryClient()}>
        <BuyBox outcome={OUTCOME} />
      </QueryClientProvider>,
    );

    expect(html).toContain('aria-label="Order amount in dollars"');
    expect(html).toContain('aria-label="Limit price in cents"');
    expect(html).toContain('aria-label="Limit price slider in cents"');
    expect(html.match(/inputMode="decimal"/g)).toHaveLength(2);
  });

  it("programmatically names the Degen bet amount", () => {
    const html = renderToStaticMarkup(
      <DegenAmount
        amount="25"
        setAmount={() => undefined}
        maxFill={50_000n}
        availableDollars={100}
      />,
    );

    expect(html).toContain('aria-label="Bet amount in dollars"');
    expect(html).toContain('inputMode="decimal"');
  });
});
