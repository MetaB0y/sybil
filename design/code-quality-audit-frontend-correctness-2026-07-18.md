---
tags: [audit, code-quality, frontend, accessibility, realtime]
layer: interfaces
status: current
date: 2026-07-18
last_verified: 2026-07-18
---

# Frontend semantic correctness and accessibility audit — 2026-07-18

## Result

The audited production frontend now has one owner for bounded recent-block
bootstrap, truthfully distinguishes recent-trade loading/failure/empty states,
and shows every per-market traded clear. Direct index loads no longer depend on
visiting Activity first. The same monotonic store merge handles history-first,
live-first, replay, and retention-gap recovery orders.

No protocol, solver, backend persistence, deployment, or proving behavior
changed in this cluster.

## Scope and evidence boundary

Reviewed:

- all product routes and their data map;
- generated OpenAPI types and exact `*_nanos` parsing/formatting edges;
- React Query ownership and invalidation;
- Zustand REST/WebSocket hydration, replay, and recent-history merging;
- trade ticker and compact-card chart semantics;
- loading, unavailable, stale, empty, and ready presentation;
- keyboard/status semantics already covered by component tests;
- autonomous motion and reduced-motion handling;
- production mock/placeholder imports; and
- browser-level computer-use acceptance scenarios.

The explicitly mocked `/m-dev/[id]` diagnostic route was inventoried but not
treated as a product surface. GitHub #177 continues to own the broader
non-nanodollar 64-bit JavaScript policy, and #183 owns cross-runtime WebSocket
contract generation; neither was duplicated here.

Architecture and implementation sources:

- `frontend/web/AGENTS.md`;
- `frontend/DATA_MAP.md`;
- `WebSocket Block Stream`;
- `Block Data Boundaries`;
- `Historical Data Serving`;
- the repository-pinned Next 16 Server/Client Component, data-fetching, and
  Vitest documentation; and
- W3C WCAG 2.2 guidance for
  [status messages](https://www.w3.org/WAI/WCAG22/Understanding/status-messages.html)
  and
  [reduced motion](https://www.w3.org/WAI/WCAG22/Techniques/css/C39).

## Findings

### FE-1 — Recent trades had split history ownership and a false empty state

Severity: high. Disposition: fixed; GitHub #191.

`RealtimeProvider` hydrated only the latest block and prices. Activity alone
fetched 60 recent blocks and wrote them into the global store, so visiting
Activity changed whether the global markets header worked. The ticker then
removed first observations, flat clears, and moves under 0.05 percentage points
but labelled an empty filtered result `awaiting fills`.

Changes:

- the global realtime provider now owns one independent bounded
  `GET /v1/blocks` bootstrap;
- recent history has an explicit `idle/loading/ready/error` state;
- the history request does not gate the live WebSocket handshake;
- a generation fence rejects responses from a pre-recovery snapshot;
- monotonic store merging prevents late history from regressing the live head
  or price;
- Activity consumes the global ring and keeps only deep/incomplete page reads
  query-local; and
- a pure `deriveRecentTrades` includes every positive-volume per-market clear,
  shows the clearing price, uses no delta for the first observation, and shows
  `0.0pp` for a flat observation.

The empty language now distinguishes loading, history outage, genuine
no-trade windows, and aggregate fills lacking per-market attribution.

Executable evidence covers first, flat, sub-threshold, and material clears;
non-traded clears; deterministic ordering/capping; history failures; and
history-first versus live-first store convergence.

### FE-2 — Some autonomous motion ignored the user's reduced-motion preference

Severity: medium. Disposition: fixed in the reviewed surfaces.

The recent-trade marquee and several status pulses used inline animation
without a reduced-motion override. The research-service nudge stopped its slide
animation under reduced motion but continued replacing content every 2.6
seconds.

Changes:

- one shared pulse class now disables ticker, batch, and waiting-state pulses;
- the marquee animation and transform are disabled under reduced motion;
- the waiting alert reuses the shared pulse keyframe instead of installing a
  component-local keyframe; and
- the research nudge stops autonomous rotation entirely under reduced motion.

### FE-3 — Production code retained obsolete mock boundaries

Severity: low. Disposition: fixed.

The Portfolio history feed still accepted an unreachable `isMock` branch saying
the per-account events endpoint was pending, although it is backed by durable
`GET /v1/accounts/{id}/events`. Production market cards imported `lib/mock.ts`
only for count formatting; the file's actual mock-price generator had no
callers.

Changes:

- removed the dead Portfolio mock branch;
- moved compact count formatting into the ordinary format module with tests;
- deleted the unused product mock generator; and
- kept deterministic mocks isolated to the explicitly labelled legacy
  diagnostic route.

### FE-4 — Exact numeric boundaries are sound in the reviewed product paths

Severity: none. Disposition: accepted, with #177 retained.

`*_nanos` values enter through `parseNanos` and remain `bigint` for arithmetic.
Conversions to `number` in the reviewed product code are bounded probabilities,
display-only dollar/chart values, or already constrained counts. Unsafe numeric
legacy nanos are rejected. The generated schema continues to encode nanos as
decimal strings.

The broad policy for non-nanos `int64` identifiers remains a separate
cross-runtime design issue rather than an ad hoc conversion rewrite.

### FE-5 — Compact card chart semantics are intentionally conservative

Severity: none. Disposition: accepted.

Card sparklines render raw piecewise-linear observations without smoothing and
enforce a 20-percentage-point minimum Y span, clamped to the valid probability
domain. This prevents a 51–53% move from visually occupying the full card while
preserving the actual samples. Larger observed ranges remain uncompressed.

## Verification

The completion gate is:

- generated OpenAPI drift check;
- computer-use scenario validation;
- TypeScript;
- ESLint;
- all Vitest suites;
- Next production build; and
- repository documentation validation.

The live-deployment Playwright suite was not run because this change has not
been deployed and its default target is the public environment. The
`realtime-offline-recovery` computer-use contract now explicitly checks direct
index load, Activity-first navigation equivalence, first/flat traded clears,
and reconnect recovery.

## Residual risk and deliberate deferrals

- The bounded history bootstrap currently exposes failure until a fresh
  snapshot/reload; adding autonomous retry is a product/load policy, not
  required to avoid false data.
- The ticker is a per-market clear feed, not a side-attributed fill tape,
  because the public block shape intentionally omits account/fill detail.
- Cross-runtime generation for the v2 WebSocket envelope remains #183.
- A repository-wide JavaScript policy for non-nanos 64-bit values remains #177.
- The legacy `/m-dev/[id]` route is still intentionally mocked and visibly
  labelled; deleting or rebuilding the entire diagnostic surface is a product
  choice, not an unambiguous audit fix.

All remaining frontend items found in this pass are either owned by those
issues or involve an explicit product/load trade-off.
