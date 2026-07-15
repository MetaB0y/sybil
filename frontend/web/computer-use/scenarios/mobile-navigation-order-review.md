---
id: mobile-navigation-order-review
priority: p0
mode: read-only
personas: signed-out mobile visitor
routes: /,/m/:market_id,/portfolio,/activity,/arena
fixtures: visible active market
environments: mobile,touch,keyboard
---

# Navigate and review an order safely on mobile

## Intent

Confirm that a visitor on a small screen can reach every primary public surface
and inspect the full order review experience without an accidental trade,
trapped focus, hidden control, or background interaction.

## Preconditions

- Use a fresh signed-out browser profile at 390 by 844 CSS pixels with touch
  input enabled, then repeat the narrowest checks at 320 by 568.
- Bind one visible active market whose public detail page offers order entry.
- Do not attach credentials or create an account during this scenario.

## Steps

1. Open the market index, open compact navigation, move through its controls by
   keyboard, and close it with Escape.
2. Open navigation again, search for the bound market, follow the visible
   result, and confirm navigation closes behind the new page.
3. Open the mobile order action, review both simple and advanced modes without
   entering a final confirmation, then close the order surface.
4. Visit Portfolio, Activity, and Arena through visible navigation and inspect
   their primary messages and controls while signed out.
5. Repeat compact navigation and the order surface at 320 by 568, scrolling each
   panel to its end before closing it.

## Observable assertions

- Opening a modal surface moves focus inside it, prevents background scrolling,
  keeps keyboard focus contained, and gives focus back to the opener on close.
- Navigation, search, outcome, amount, mode, close, and review controls remain
  visible, labeled, and comfortably operable by touch.
- The order surface clearly separates editing, review, and final confirmation;
  exploration while signed out cannot submit a trade.
- No primary content, dialog action, native select, tooltip, or error recovery
  control extends beyond the viewport at either size.
- Signed-out Portfolio and protected settings use a clear account prompt rather
  than false zero balances, empty positions, or an unexplained blank page.
- Hover is never the only way to discover information or operate an action.

## Evidence

- Capture compact navigation, the order review state, every signed-out primary
  message, and the 320-pixel recovery state with viewport dimensions visible in
  the run record.
- Record focus before opening, initial modal focus, Escape behavior, and focus
  after close for both navigation and order review.
- Record any horizontal overflow, clipped text, obscured action, or target that
  requires repeated taps.

## Cleanup

- Close every modal or sheet and return to the public market index.
- No server-side product state was changed.

## Stop conditions

- Stop immediately if any action appears to submit, fund, or create an account
  without an explicit confirmation step.
- Stop as blocked if no ordinary active market exposes the order review surface.
- Stop as failed if focus escapes behind a modal or the page remains scroll
  locked after a modal closes.
