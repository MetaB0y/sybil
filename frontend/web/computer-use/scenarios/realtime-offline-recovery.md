---
id: realtime-offline-recovery
priority: p1
mode: controlled-fault
personas: market watcher
routes: /,/m/:market_id,/activity
fixtures: active market,advancing devnet blocks
environments: desktop,instrumented-browser
---

# Recover truthful realtime state after going offline

## Intent

Confirm that the product preserves the last trustworthy market and block state
during a temporary network loss, labels the interruption honestly, and resumes
without gaps, duplicates, false zeroes, or a forced page reload.

## Preconditions

- Bind an active market and a devnet whose block height is advancing during the
  test window.
- Use an instrumented browser that can disable and restore all network traffic
  without changing server state.
- Record the initial visible block height, market price label, recent-trades
  entries, and connection state after the page has fully settled.

## Steps

1. Open the market index directly and wait for Recent trades to settle without
   visiting Activity first. Record its visible traded clears, then open the
   bound market, observe at least one live update or countdown change, and visit
   Activity to record the latest visible batch height.
2. Disable network traffic while keeping the page open and interact with the
   market and Activity views long enough for freshness indicators to react.
3. Inspect headline prices, charts, batch counts, retry or connection language,
   and controls while offline; do not reload the page.
4. Restore network traffic and wait for the connection and read states to settle
   without manually clearing storage or refreshing.
5. Revisit the bound market and Activity, then compare the recovered latest
   height and visible history with the before-offline observations. Return to
   the market index and compare Recent trades with the same loaded block window.

## Observable assertions

- The last trustworthy values may remain visible, but stale or disconnected
  provenance is explicit and they are not silently relabeled as current.
- A transport failure never creates a zero price, zero activity total, empty
  market list, or “no history” claim.
- Actions that require fresh signed state do not remain deceptively ready while
  required account or market reads are unavailable.
- Recovery resumes automatically, advances beyond the recorded height, and does
  not duplicate batches, chart points, toasts, or order rows.
- Recent trades loads on direct index entry, includes first and flat traded
  clears with their clearing prices, and does not depend on visiting Activity.
  Navigation order and reconnect recovery produce the same loaded-window feed.
- If the retained range cannot bridge the interruption, the product exposes a
  clear recovery or unavailable state rather than concealing a gap.
- Console and network diagnostics contain no unbounded reconnect loop or burst
  that continues after recovery.

## Evidence

- Capture the settled pre-fault state, offline/stale market and Activity states,
  connection recovery, and post-recovery latest height.
- Record before/offline/after heights and price labels plus reconnect timing.
- Preserve unexpected console errors and failed-request counts, grouping
  expected offline failures separately from failures after restoration.

## Cleanup

- Restore normal network conditions and verify the product reports a settled
  connected state before ending the run.
- No server-side product state was changed.

## Stop conditions

- Stop as blocked if blocks are not advancing before fault injection.
- Stop as failed if recovery requires deleting account data, refreshing the
  page, or accepting a hidden history gap.
- Stop the browser if reconnect traffic remains unbounded after normal network
  conditions return.
