---
id: truthful-read-outage
priority: p1
mode: controlled-fault
personas: public visitor,market researcher
routes: /leaderboard,/m/:market_id,/activity,/arena
fixtures: active market,nonempty public leaderboard
environments: mobile,instrumented-browser
---

# Distinguish public read outages from genuine empty data

## Intent

Confirm that independently failing public reads produce scoped, retryable, and
truthful states while unaffected cached data stays usable and small-screen
recovery controls remain reachable.

## Preconditions

- Bind an active market and verify the public leaderboard is nonempty before
  fault injection.
- Use an instrumented browser that can fail one named read family at a time and
  later restore it without changing server state.
- Run at a 390 by 844 viewport and start each fault from a freshly settled page.

## Steps

1. Open Leaderboard, record a visible ranked row, then fail only its refresh and
   inspect the cached-failure state and retry action.
2. Start a fresh Leaderboard load with the same read unavailable and compare the
   cold-failure state with the cached-failure state before restoring service.
3. Open the bound market, switch to advanced order entry, fail only open-batch
   data, and inspect price terminology, disabled or retained controls, and retry.
4. Open Activity and Arena in turn, fail one page-specific read family while
   leaving their other inputs available, and inspect partial-data provenance.
5. Restore each failed read, activate its visible retry where offered, and wait
   for a trustworthy populated state without reloading the whole application.

## Observable assertions

- A cold failure is an accessible unavailable state, never an empty leaderboard,
  no-history message, zero-bot count, or zero-activity summary.
- Cached data remains visible when safe, is labeled saved or stale, and never
  loses its warning merely because another read succeeds.
- Open-batch failure does not rename the committed last price as indicative and
  does not present an unsafe order decision as fully current.
- Each alert is scoped to the failed capability, has readable cause and recovery
  language, and leaves unrelated page data usable.
- Retry actions fit the mobile viewport, have usable touch size, work once the
  read is restored, and do not create duplicate alerts or requests.
- A successful retry removes stale warnings only after trustworthy replacement
  data is visible.

## Evidence

- Capture cached and cold Leaderboard failures, market open-batch failure,
  partial Activity and Arena states, and each recovered result.
- Record which read family was interrupted, whether cached data existed, and the
  exact visible empty, stale, or unavailable language.
- Record retry target size or clipping, unexpected console errors, and requests
  that still fail after the fault is removed.

## Cleanup

- Restore every intercepted read and remove all browser fault rules.
- Verify each visited page can reach a settled non-stale state.
- No server-side product state was changed.

## Stop conditions

- Stop before fault injection if the supposedly nonempty fixture is already
  empty or unavailable.
- Stop as failed if one scoped fault causes a global error boundary, false zero,
  hidden cached data, or an unreachable recovery action.
- Stop the run if fault interception affects mutations or any origin outside the
  declared devnet application and API.
