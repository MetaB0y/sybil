---
tags: [audit, release, private-devnet, operations, acceptance]
layer: cross-cutting
status: current
date: 2026-07-20
last_verified: 2026-07-20
---

# Private devnet launch readiness — 2026-07-20

## Outcome

Sybil's private devnet is launch-ready as a product-only deployment.

The complete stack was reset to fresh genesis, deployed from immutable
revision-tagged images, exercised through the real HTTPS/browser surfaces,
rolled back without rebuilding, restarted from durable state, load-tested,
soaked, and monitored. Account creation, fixed demo funding, passkey signing,
signed order/cancel, matching, settlement, history projection, frontend
activity, broad synthetic trading, and independently budgeted operator MMs all
work on the deployed service.

Launch-relevant code and evidence are pushed to `main`. The active chain is
healthy and advancing. Remaining work is either an explicit product/architecture
choice, an external provider condition, or a later security/production concern.

This report deliberately does not claim readiness for TEE, escape-hatch
maturity, real proving/circuits, Sepolia, real-money custody, or public
permissionless operation.

## What changed and why

### Validity and signed actions

- **#178 — zero-quantity fills and validity ownership.** Canonical/native/guest
  validity rejects zero-quantity fills. Diagnostic quality policy is not
  allowed to redefine protocol validity.
- **#179 — signing domains.** Signed actions use explicit, versioned domains so
  order, cancel, key, and bridge payloads cannot be confused merely because
  their fields happen to serialize similarly.

Decision: validity remains a small independent invariant surface. Diagnostics
may explain bad output, but they cannot make invalid output acceptable.

### Sequencer load and terminal failure

- **#184 — actor admission.** Read, ordinary-write, and operator-control RPCs
  have simple bounded permits before the canonical actor mailbox. Overload is
  fail-fast and typed; rejected work was never queued. Control capacity is
  reserved without introducing a scheduler/coordinator.
- **#185 — canonical ownership loss.** If supervised sequencer recovery cannot
  restore and respawn the canonical owner, the API publishes truthful terminal
  failure, drains, and exits nonzero. A live actorless API is not considered
  recovery.

Decision: use the process manager as the recovery layer for terminal ownership
failure. Do not invent a second in-process ownership protocol.

### Durable identity, provisioning, and user semantics

- **#188 — service provisioning.** Operator service accounts are provisioned
  with caller-stable, genesis-bound identities and durable receipts. Exact
  retries return the same account; conflicting reuse fails.
- **#167 — quota ownership.** Anonymous demo onboarding stock is independent
  from operator service accounts. The deployed public flow gives a fixed
  `$1,000` demo grant and does not let the browser choose arbitrary funding.
- **#129 — market-group identity.** Native market groups have canonical
  operator creation keys. Retry/recovery no longer discovers groups through
  titles or tags.

Decision: durable identity was added only at ambiguity boundaries. No generic
entity/registry layer was introduced. In user-facing product language, one
event card is one market; internally it may contain several sequencer component
markets.

### Trading metrics and product discovery

- **#145 — two execution ratios.** Trader execution is the share of admitted
  trader orders that receive a first positive fill. Maker hit rate is the share
  of worked maker quotes that are hit. They are separately named and exposed.
- **#163 — smoke fixtures.** Promotion smoke trades an existing product market
  and leaves no active order. It does not create durable fake markets in public
  discovery.

Decision: do not use one ambiguous "fill ratio" for two different cohorts, and
do not let release tooling mutate the product catalog.

### Immutable deployment

- **#65 — releases and rollback.** API, Arena, web, and Caddy use immutable
  image references. Release manifests record source revision, local/host image
  identity, expected service states, and verification time. Rollback activates
  a saved image set without rebuilding.
- Release verification was simplified while hardening it: it now checks every
  service, distinguishes long-running containers from the intentional
  `native-admin` `exited:0` job, and compares a portable image fingerprint
  across Docker stores.

Decision: a deployment is an identified artifact set, not "whatever the tag
currently points at."

### Post-soak storage simplification

The final audit found one real unbounded stream. The account-snapshot qMDB is a
fenced current-state A/B snapshot, but it retained its historical operation
journal. Live growth was about `22.2 KB/block`.

Commit `ea1e2719608b` prunes that journal to Commonware current-qMDB's safe
boundary after each durable snapshot commit. It does not change the A/B
keyspace, current root, redb fence, schema, or genesis identity. Typed-state
qMDB slots already rebuilt into fresh generations and removed old generations.

A 400-snapshot, 38-account regression verifies bounded retained sections and
recovery of both A/B slots. On the live chain, qMDB reclaimed from
`7,735,906 bytes / 12 data sections` to `723,720 bytes / 1 section` on the first
post-upgrade blocks, then to about `321 KB` after restart and further pruning.
#140 is closed.

## Source and issue disposition

The following required issues are closed with implementation and verification
comments:

| Issue | Disposition |
|---|---|
| #178 | Closed — zero fills invalid; diagnostics separated from validity |
| #179 | Closed — versioned action-domain separation |
| #184 | Closed — bounded pre-mailbox actor admission |
| #185 | Closed — terminal ownership loss reaches process boundary |
| #188 | Closed — retry-safe genesis-bound service provisioning |
| #129 | Closed — canonical market-group creation identity |
| #167 | Closed — service stock separate from anonymous demo quota |
| #145 | Closed — trader execution separated from maker hit rate |
| #163 | Closed — smoke fixtures removed from product discovery |
| #65 | Closed — immutable releases and no-rebuild rollback |
| #71 | Closed — 100-reader WebSocket acceptance passed |
| #140 | Closed — product soak passed and qMDB growth was bounded |

Launch validation produced evidence but intentionally did not guess the
remaining decisions:

| Issue | State and reason |
|---|---|
| #90 | Open — short projector outage semantics pass; overflow/completeness-floor and off-host archive policy remain a deliberate design |
| #172 | Open — breadth is healthy; mirror mark-quality invariant and solver/Arena calibration remain a product/economic decision |
| #196 | New, High/Todo — bound same-host backups and add acknowledged off-host copies plus restore drills |

#174, a public third-party signed MM bundle, was not implemented. The private
operator fleet already supports multiple independently budgeted MMs without it.

## Fresh genesis, backup, and releases

### Reset

- Pre-reset manifest backup:
  `/opt/sybil/backups/sybil-store-20260720T123427Z-764786`
- Pre-reset height: `27,307`
- A disposable restore drill validated that backup before reset.
- Fresh product genesis:
  `822393e4185b71d13fcb6d2ee24ce66ca58b3feef3fe7c3b7a79ce525ee5ac0c`

All incompatible application state was cleared: sequencer/history, native and
mirror mappings, Arena state, validity/L1 artifacts and cursors, and monitoring
history.

### Complete-stack release

The fresh-genesis complete stack was deployed as:

`20260720T130432Z-9b5432922f7b-all`

| Image | Host image identity |
|---|---|
| API/history/native/mirror | `sha256:81321a9eb38502b8fe9eab0a427c4ea63bdd997946618aa36bafc2491909eeb1` |
| Arena/dashboard | `sha256:01fcd857006c1e9ae332381500a985c3a4ce068d185d73197c1e59a5beabb3ee` |
| Web | `sha256:69cbc64f13e5ac2d4ae5353d855bb8932d3e48b0e232b3df209409f706469141` |
| Caddy | `sha256:5f5c8640aae01df9654968d946d8f1a56c497f1dd5c5cda4cf95ab7c14d58648` |

The complete saved image set was reactivated without a rebuild. The rollback
record is
`20260720T132117Z-rollback-to-20260720T130432Z-9b5432922f7b-all`;
`rebuilt_images=false`, all expected service states matched, and live smoke
passed afterward.

### Active storage hotfix release

The compatible qMDB fix was promoted without another state reset because it
changes no schema, root, fence, or chain identity:

`20260720T135614Z-49565d7e1072-api`

The active API-family image is:

- source revision:
  `49565d7e1072294688382cf62c2d876c528bde56`
- host image:
  `sha256:3a5003ae5ed4680ac20cec21678c0c6686b79d0c4c81d018d1f601e6bec5a7e5`

Arena and web remain the already-verified images above because the hotfix did
not touch those artifacts. Release verification confirms the running stack
exactly matches the active manifest.

## Automated validation

Repository and component gates passed during the launch run:

- `just check-fast`
- `just check-consensus`
- full `matching-sequencer`: 380 unit tests plus 25
  integration/economic/property tests
- `matching-sequencer` all-target clippy with warnings denied
- Arena: 352 tests
- frontend: 385 passed, 1 intentional skip; lint and type checking passed
- monitoring fixtures: 89 tests
- architecture/docs validation
- disposable exact signed-trading preflight before API promotion

The final deployed promotion smoke passed `55`, failed `0`, skipped `1`.
The only skip is proof freshness because product mode intentionally runs no
validity/prover profile. It verified:

- API health, state root, block advancement, and public replay/live handoff
- every expected container, including intentional `native-admin` completion
- deployed frontend shell and JavaScript assets
- CORS and passkey origin/RP configuration
- fixed-grant anonymous passkey onboarding
- active native and mirror markets plus fresh references
- signed crossing fills projected through the private history service
- trader execution metric attribution
- no active smoke-order residue
- service-token access matrix
- signed cancel and exact reservation release
- API-to-history query ownership
- Arena decision-store readability

The deployed browser suite passed `10`, failed `0`, skipped `1`, including the
passkey/account journey. The skip is physical-device-only. A focused passkey
journey also passed independently.

## Load, outage, restart, and soak evidence

### WebSocket replay/backpressure — #71

Normal production cadence:

- 100 subscribers, including 10 readers paused for 60 seconds
- 780-second run, height `269 -> 348`
- 79 blocks and exactly 7,900 deliveries
- every reader received one replay-complete boundary
- zero subscriber failures, retention gaps, or ordering gaps
- API RSS growth `10.77 MiB`; high-water growth `10.36 MiB`
- actor queue maximum `0`
- solve p99 `24.044 -> 72.187 ms`, under the 100 ms gate
- health p95 `120 -> 176 ms`, under the 250 ms gate

At the normal ten-second cadence, the 60-second pause did not overflow the
server buffer, which is expected. A disposable accelerated-cadence run forced
the lag path:

- 100 subscribers, 10 slow readers, 9,021 blocks
- all 10 slow readers observed lag, reconnected, and completed replay
- zero failures
- API RSS growth `18.25 MiB`

The harness itself was corrected during this run: it now installs one explicit
Rustls provider and gates DNS/TLS establishment to eight concurrent handshakes.
Measurement begins only after all 100 readers are connected, so connection
setup no longer accidentally load-tests the generator's resolver.

### History projector outage — #90

- Baseline height `220`; only `sybil-history` was stopped for 50 seconds.
- Canonical height advanced to `226`; trading did not depend on projection.
- Outbox grew monotonically from 1 row / 11,955 bytes at height 221 to 6 rows /
  30,364 bytes at height 226 (`oldest=221`, `newest=226`).
- History routes returned typed HTTP 503 `HISTORY_UNAVAILABLE`, never a false
  empty/complete result.
- After restart, the outbox drained to zero in under two seconds.
- History was healthy and contiguous through height 229, complete from height
  1, with no OOM or restart loop.

This proves the current isolation, durable buffering, truthful degraded
serving, and catch-up path. It does not settle the long-outage overflow policy.

### Product-only soak and cold restart — #140

The product soak sampled 23 times from `13:26:30Z` to `13:49:35Z`, height
`240 -> 378`:

- recent-block cache reached its intentional 100-block cap at height 309
- after the cap, API RSS stayed in roughly `92.4–95.7 MB`
- process high-water stayed flat at `108,724,224 bytes`
- redb stayed flat at `22,269,952 bytes`
- history redb stayed flat at `26,509,312 bytes`
- actor queue `0`, history outbox `0`, pending orders `1`
- restart count `0`, OOM `false`
- solve p99 roughly `20–43 ms`

The qMDB growth found by the soak was fixed and live-reclaimed as described
above. The upgraded API then cold-restarted from the pruned store and became
healthy in 9 seconds at `26.54 MiB`, with stable Docker restart count and no
OOM/boot loop. The same genesis and advancing height were preserved.

## Live product evidence

Final sampled state after the hotfix/restart:

- health: `ok`
- genesis: unchanged
- height: `442` and advancing
- user-visible market cards: `36`
- sequencer component markets: `206`
- traded cards: `36/36`
- traded components: `206/206`
- zero-volume components: `0`
- unique traders: `21`
- cumulative volume: about `$108,416.58`
- cumulative welfare: about `$11,101.81`
- trader execution: `10,592 / 20,256 = 52.29%`
- maker quote hit rate: `6,940 / 252,781 = 2.75%`
- representative recent blocks: about 630 orders, 36–47 fills, zero rejections

MM health in the measured launch window:

- native: 134 tracked/eligible/quoted, all two-sided; 402 active quote orders;
  account cap not binding; 100% submission success; zero failed compactions
- mirror: 72 tracked, 60 eligible/quoted/two-sided; 12 references outside the
  configured quote band; 180 active quote orders; account cap not binding;
  100% submission success; zero failed compactions or feed/sync failures

The independently budgeted MMs do not rotate and are not constrained by the old
12-account limit. Broad crossing-noise traders participate in both native and
mirrored markets.

### Mirror mark quality — #172

Breadth does not imply reference convergence. With all 72 references fresh:

- all mirrors: mean absolute difference `7.76c`, median `5.01c`; 36/72 above
  `5c`, 3/72 above `25c`
- quote-band mirrors: mean `7.22c`, median `5.00c`; 26/60 above `5c`, 3/60
  above `25c`
- largest observed differences:
  - market 181: Sybil `99c` vs reference `36.5c`
  - market 150: Sybil `27.315c` vs reference `1.25c`
  - market 146: Sybil `35.669c` vs reference `10.5c`

External references remain diagnostic and never enter consensus. #172 needs a
declared economic/product invariant before solver or synthetic-flow tuning.

## Operations and alerts

All application and operations containers are in their intended state:
long-running services are healthy; native admin completed successfully.
Release identity verification passes.

After safely pruning unused/unreferenced Docker images:

- root filesystem: about `17.95 GB` free (`64%` used)
- active API redb: `22,269,952 bytes`
- active qMDB: about `321 KB` after pruning/restart
- history volume: about `53 MB`
- API container limit: `1.25 GiB`
- API: healthy, zero OOM, zero restart loop

The former disk-low alert cleared. Three truthful alerts remain:

- `ArenaLlmProviderAuthOrCreditRejected` — firing
- `ArenaLlmProviderDegraded` — firing
- `SybilPriceDivergenceBroad` — pending in its 30-minute window

The Arena LLM condition is external: the configured OpenRouter key is present,
but the provider returns HTTP 402 for insufficient credit. The inexpensive
model configuration and exponential backoff are working; deterministic
crossing-noise, reference traders, and MMs continue trading. Replenishing
provider credit is required if LLM product behavior is expected in the demo.

The disk investigation found that same-host backup accumulation is the next
clear capacity risk. It is recorded as #196 rather than being silently deleted:
local pruning must follow an acknowledged encrypted off-host copy and a tested
restore policy.

## Final simplicity and boundary audit

Clear findings discovered during acceptance were fixed immediately:

1. Account qMDB accidentally retained history despite owning only current A/B
   snapshots. Native safe pruning now enforces that boundary.
2. Release verification assumed same-store Docker IDs and only checked a subset
   of services. It now uses portable fingerprints and exact per-service
   lifecycle expectations.
3. Browser acceptance and deployed account-menu semantics had drifted. The
   accessible account-menu contract and tests now agree.
4. WebSocket load setup had an implicit Rustls-provider choice and unbounded
   same-instant DNS/TLS setup. Both are explicit and bounded without reducing
   measured target concurrency.
5. The operator status script used the wrong Compose profile/env source and
   treated exact JSON nanos as a native number. It now reads active release env,
   preserves integer parsing, and reports LLM provider failures truthfully.
6. Disk pressure was initially an undifferentiated alert. Unreferenced images
   were safely reclaimed, while backup retention was separated into #196.

No further unambiguous local simplification was found after the hotfix,
restart, release verification, live smoke, and final state inspection.

The remaining items should not be "cleaned up" without decisions:

- #172: what a good Sybil mark means relative to an external venue
- #90: complete-history promise and overflow behavior during a long outage
- #196: off-host provider, retention, and RPO/RTO choice
- physical mobile/device acceptance
- 24-hour forecasting/product-quality experiment
- excluded security/validity work for any later public or real-value launch

## Recommended next work

1. Replenish OpenRouter credit and confirm LLM decisions recover; this is an
   external configuration action, not a code redesign.
2. Resolve #172 with the solver-context work already underway: state the
   canonical price/mark rule, simulate it, then calibrate alerts and flow.
3. Ratify a simple private-devnet history promise for #90 before a deliberately
   long projector outage.
4. Implement #196 before backups again become the dominant host disk consumer.
5. Run one physical mobile/passkey journey and a longer unattended product
   soak. Neither is a blocker for the current private devnet.
