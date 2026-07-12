import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { EventOutcome } from "@/lib/market-detail/use-event-group";
import { BuyBox } from "./buy-box";
import { DegenAmount } from "./degen-amount";
import { YesNoToggle } from "./yes-no-toggle";

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

  it("exposes the active order choices to assistive technology", () => {
    const pro = renderToStaticMarkup(
      <QueryClientProvider client={new QueryClient()}>
        <BuyBox outcome={OUTCOME} />
      </QueryClientProvider>,
    );
    const degen = renderToStaticMarkup(
      <YesNoToggle value="NO" onChange={() => undefined} />,
    );

    for (const label of [
      "Order direction",
      "Outcome side",
      "Order amount unit",
      "Time in force",
    ]) {
      expect(pro).toContain(`role="group" aria-label="${label}"`);
    }
    expect(pro).toMatch(
      /role="group" aria-label="Order direction"[^>]*>.*aria-pressed="true"[^>]*>buy<\/button>/,
    );
    expect(pro).toMatch(
      /role="group" aria-label="Outcome side"[^>]*>.*aria-pressed="true"[^>]*><span>YES<\/span>/,
    );
    expect(pro).toMatch(/aria-pressed="true"[^>]*>\$ amount<\/button>/);
    expect(pro).toMatch(/aria-pressed="true"[^>]*>GTC<\/button>/);
    expect(degen).toContain('role="group" aria-label="Outcome side"');
    expect(degen).toMatch(/aria-pressed="true"[^>]*>No<\/button>/);
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
