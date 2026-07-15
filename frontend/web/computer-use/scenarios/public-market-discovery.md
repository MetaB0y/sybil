---
id: public-market-discovery
priority: p0
mode: read-only
personas: signed-out visitor,first-time forecaster
routes: /,/m/:market_id
fixtures: visible active market,market with committed clearing history
environments: desktop,mobile
---

# Discover and understand a public market

## Intent

Confirm that a new visitor can find a relevant event, understand what is being
predicted, distinguish committed price history from live indications, and move
between the index and detail view without learning Sybil's implementation.

## Preconditions

- Use a fresh signed-out browser profile against the revision under test.
- Bind one ordinary visible active market and one market with committed
  clearing history; neither may be a deployment fixture or internal test row.
- Record the viewport, origin, revision, and the visible market names selected
  by the fixture provider.

## Steps

1. Open the market index and wait for its initial loading state to settle.
2. Scan the page hierarchy, category choices, sorting choice, search, and the
   visible information on several market cards.
3. Search using a distinctive word from the bound active market, then clear the
   search and use one category or status filter before returning to all markets.
4. Open the bound market from its visible card and identify the question,
   outcomes, resolution context, current price language, chart, activity, and
   order-entry affordance.
5. Inspect a recent point in the price chart and compare the tooltip language
   with the headline price language.
6. Repeat the index-to-detail path at a narrow mobile viewport using the compact
   navigation and search experience.

## Observable assertions

- Loading, unavailable, stale, and genuine empty results are visibly distinct;
  a failed read never becomes “no markets” or a zero statistic.
- Search and filters have an obvious active state, can be cleared, and do not
  surface deployment fixtures or internal deterministic test markets.
- Each card makes the event question, outcomes, price provenance, and status
  understandable without opening developer tools.
- The detail page does not call a committed last price “indicative,” and it does
  not invent a last price when no clearing history exists.
- Resolution wording and evidence links are presented as metadata, not as
  validity-proven exchange state.
- Desktop and mobile views retain one clear primary heading, readable controls,
  no horizontal page overflow, and no action that exists only on hover.

## Evidence

- Capture the settled index, filtered result, desktop detail, chart tooltip,
  compact navigation, and mobile detail states.
- Record the bound market names and the exact visible labels used for committed
  versus live or indicative prices.
- Record unexpected console errors, failed requests, and any layout clipping or
  obscured control with its viewport dimensions.

## Cleanup

- Clear search and filters so the browser is left on the ordinary market index.
- No server-side product state was changed.

## Stop conditions

- Stop as blocked if the environment cannot provide an ordinary active market
  and committed-history market without creating shared product state.
- Stop as failed if the page exposes a fixture market, silently substitutes
  mock values, or requires authentication for public discovery.
- Stop without bypassing browser security if the deployed origin or certificate
  is not the one declared for the run.
