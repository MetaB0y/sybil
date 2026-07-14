---
tags: [design, arena, liquidity, market-maker, sequencing, operations]
layer: core
status: proposed
date: 2026-07-14
---

# Actor Liquidity v2 — implementation specification

> **Status:** proposed implementation plan. This document records the agreed
> product rules and the code-backed design needed to implement them. It does
> not describe current behavior and does not override code, ADRs, or the
> architecture vault.

## 1. Decision summary

Sybil will run one continuously reconciled active-market universe. In every
healthy block:

- the market maker contributes an accepted economic bid and ask to every
  active market;
- three persistent noise accounts each contribute one accepted short-lived
  order to every active market;
- LLM traders remain opportunistic and event-driven, with no coverage target.

The normal MM coverage target is 100%. Eighty percent is an emergency
availability floor and alert threshold, not an acceptable steady state.

This is not a configuration-only change. It requires:

1. an atomic, versioned market universe shared by the frontend and actors;
2. a service-scoped bulk actor submission path bound to one target block;
3. collateralized complete-set inventory for two-sided group-safe MM quotes;
4. a new all-market noise coordinator using IOC orders;
5. actor-aware observability, capacity tests, and a controlled migration.

## 2. Audited baseline

The baseline at the time of this proposal is:

| Set | Mirror | Native | Total |
|---|---:|---:|---:|
| Frontend-visible | 72 | 144 | 216 |
| Current actor catalogs | 7 | 127 | 134 |
| Visible but absent from actor catalogs | 65 | 17 | 82 |
| Reviewed next genesis | 72 | 134 | 206 |

The API currently returns 226 active markets. The frontend hides ten internal
smoke fixtures by a name-prefix rule, producing the visible count of 216. This
is not a safe source of truth: frontend discovery, mirror/native catalogs,
sequencer lifecycle, and actor membership can all disagree.

The native-catalog cleanup reviewed after that audit defines the migration
target: 134 canonical native children. It keeps the distinct eight-child
OpenRouter final-week event, removes nine duplicated legacy children and the
10M context-window rung, and requires explicit natural-language child-market
questions. Together with the 72 reviewed mirrors, the next-genesis universe is
therefore 206 markets. The old 216 count remains useful only as a pre-migration
observation.

The main liveness blockers are also independent:

- the MM and ordinary API admission both cap one submission at 64 orders;
- neutral MM quoting needs at least two orders per market;
- production noise is restricted to a small startup-frozen mirror cohort;
- noise uses GTC and an account-wide pending-order gate;
- a naive same-account two-sided strategy violates complete-set prevention on
  protocol market groups;
- native actors do not share one robust mark and Python does not receive native
  quote ranges;
- actors are not bound to the block for which their decisions were computed.

At the target size, a normal block starts with approximately:

```text
206 markets × 2 MM orders              = 412
206 markets × 3 noise accounts × 1     = 618
                                              ----
baseline system-actor orders           = 1,030
```

User, LLM, resting, and exceptional MM orders are additional. The current
ten-second block interval is therefore the capacity envelope for this design.

## 3. Goals, non-goals, and terminology

### Goals

- One exact active-market set across frontend, MM, noise, admission, and
  monitoring.
- Accepted MM liquidity and three distinct accepted noise participants on
  every active market in each healthy block.
- Two-sided, inventory-aware, budget-safe MM behavior on mirrors and natives.
- Native prices that react slowly to credible flow without following the
  actors' own synthetic prints in a feedback loop.
- Short-lived, reproducible, group-safe noise flow that buys and sells over
  time.
- No weakening of user admission, settlement, verification, or group
  self-trade-prevention invariants.
- Explicit degradation and recovery instead of silent omissions.

### Non-goals

- Forcing LLM activity on all markets.
- Guaranteeing a fill in every market-block. The system can guarantee order
  presence; fills remain a solver outcome. A separate fill-coverage KPI measures
  whether the chosen synthetic flow actually produces the desired activity.
- Enforcing native actor ranges on users or in matching validity.
- Treating frontend event grouping as a protocol `MarketGroup`.
- Hiding synthetic activity inside organic volume, rewards, or leaderboards.
- Adding a general public high-volume order endpoint.

### Terms

- **Protocol tradeable:** the sequencer permits orders on the unresolved
  market.
- **Discoverable:** the default frontend may display the market.
- **Actor enabled:** MM and noise are required to operate on the market.
- **Active market:** all three conditions above are true for the committed
  universe generation.
- **Actor epoch:** one actor's complete decision package for exactly one target
  block and one universe generation.
- **Quoted:** the order was accepted into that block's solve. Merely generating
  or HTTP-submitting it does not count.
- **Two-sided:** an accepted economic YES bid and YES ask exist. The underlying
  order shapes may be `SellNo` and `SellYes`.
- **Market-block:** one active market in one committed block; the denominator
  for coverage metrics.

Safety pauses remain coverage misses with typed reasons. They must not be
removed from the denominator merely to make the SLO appear healthy. Only a
market committed as suspended/resolved before the target block leaves the
denominator.

## 4. Architecture decisions

### 4.1 Versioned active-market universe

The frontend must not be the authority and actors must not scrape frontend
behavior. A controller in `sybil-polymarket` will derive a full desired
universe from checked-in stable source identities:

- explicit Polymarket condition ids for mirror children;
- stable native specification ids for native children;
- no name-prefix filtering and no implicit inclusion of newly discovered
  external children.

The initial migration must reproduce the exact reviewed 206-market target:
72 mirrors and 134 canonical native children. The count is a migration
assertion, not a permanent protocol constant. Later changes occur through
reviewed catalog changes and a new universe generation.

Each runtime snapshot contains at least:

```text
UniverseSnapshot
  generation: u64
  digest: [u8; 32]
  activated_at_height: u64
  entries: [MarketParticipationPolicy]

MarketParticipationPolicy
  stable_source_id
  sybil_market_id
  market_kind: mirror | native
  protocol_tradeable
  discoverable
  actor_enabled
  protocol_group_id?
  display_event_id?
  mirror_reference_identity?
  native_seed_nanos?
  actor_min_yes_nanos?
  actor_max_yes_nanos?
  coherence_rule?
```

Actor ranges and reference metadata are off-block policy. The committed core
stores only trading enabled/suspended state plus the active universe generation
and policy digest. This prevents a stale staged actor epoch from becoming valid
after a universe switch without making prices or ranges validity inputs.

Universe activation is two-phase and crash recoverable:

1. upload and validate the complete candidate policy snapshot;
2. stage one sequencer control action for the next block;
3. commit the trading-state transitions, generation, and digest in that block;
4. publish the candidate as active only after observing the committed digest;
5. retain the previous complete snapshot until activation succeeds.

Production startup fails closed for actor operation if the committed generation
and persisted snapshot are missing, corrupt, or have different digests. Public
user trading need not stop globally; only the actor endpoints remain unready.

Validation before activation:

- every source identity maps to exactly one existing Sybil market;
- all 206 migration entries have complete policy;
- all 134 native children expand from the reviewed catalog, including the
  restored eight-child OpenRouter final-week event and excluding retired
  legacy duplicates and the 10M context-window rung;
- all 65 legacy mirrors receive condition/token/reference mappings;
- `0 < min < seed < max < $1` for every native;
- the range is wide enough for at least one tick on both quote sides;
- categorical group bounds admit a coherent probability vector;
- threshold-ladder bounds admit the declared monotonic ordering;
- protocol groups activate or suspend as a unit, except for already resolved
  members;
- internal fixtures are explicitly `discoverable=false` and
  `actor_enabled=false`.

Removing a market does not pretend it resolved. At its activation block the
market becomes suspended, new orders are rejected, resting orders are evicted
with reservations released, synthetic orders disappear after their one-block
life, and history remains readable. Resolution remains possible while
suspended.

### 4.2 Service-scoped actor credentials

Bulk actor submission must not reuse the public route or globally raise its
64-order DoS guard.

Add a distinct actor route group authenticated by per-actor credentials. Each
credential is server-bound to one role and account id. The request does not get
to choose an arbitrary account or MM identity.

Roles needed initially:

- one `market_maker` principal;
- three `noise` principals.

Actor credentials can submit actor epochs and read actor-only universe/mark
snapshots. They cannot create accounts, fund arbitrary accounts, resolve
markets, mutate metadata, or use bridge/admin routes. Secrets live in mounted
files/environment and are never committed or logged.

The public 64-order cap, public rate limits, P256 behavior, and normal user
order semantics remain unchanged.

### 4.3 Target-block actor epochs

Introduce a dedicated service endpoint, conceptually:

```text
POST /v1/actor/epochs

ActorEpochRequest
  epoch_id
  target_height
  universe_generation
  observed_at_ms
  valid_until_ms
  market_intents[]
  mm_budget_nanos?       # MM role only

ActorMarketIntent
  market_id
  orders[]
  skip_reason?
```

Time-in-force is implicitly IOC. An epoch must target exactly `head + 1`; the
server never silently retargets a late package. Every active market must appear
exactly once as an order intent or an explicit typed omission.

Role limits are derived from active-market count and bounded by absolute
service caps:

- MM: up to `4 × M + headroom`, hard cap initially 1,024;
- noise: up to `M + headroom`, hard cap initially 256;
- request-body and decoded-memory byte limits apply before actor admission.

The sequencer stores actor epochs separately from ordinary pending bundles.
The key is `(actor principal, target height)`, not an append-only retry stream.

- retrying the same id and digest is idempotent;
- a different epoch for the same key atomically supersedes the old one before
  the cutoff;
- an expired or missed target is dropped, never carried forward;
- a wall-clock stale epoch cannot execute after block production resumes;
- crash recovery restores at most one epoch for that principal/height.

Structural errors—wrong generation, target, role, duplicate market intent, or
oversized payload—reject the epoch. A market-specific race or bad order rejects
only that intent and preserves the other markets. The response and final block
receipt report per-market accepted order ids and typed rejections.

All accepted orders in one MM epoch rebuild into exactly one `MmConstraint`
with one shared budget. Splitting or retrying must never create multiple
constraints that each receive the full budget.

Actor epochs are ordered deterministically during block preparation. They are
admission inputs, not a new resting book. Block production never waits
indefinitely for them and continues if actors are unhealthy.

### 4.4 Collateralized complete-set inventory

Complete-set STP remains enabled for every account, including the MM. We will
not evade it by rotating beneficial ownership across multiple MM identities.

To quote both sides of every grouped child, add a validity-checked per-market
complete-set collateralization action:

```text
collateralize(market, quantity):
  debit account cash by $1 × quantity
  credit equal YES and NO positions

redeem(market, quantity):
  debit equal YES and NO positions
  credit account cash by $1 × quantity
```

Only checked integer arithmetic is allowed. The v1 mechanism is per binary
market; capital-efficient group minting is deferred because it adds different
inventory and verification semantics.

The primitive is validity-critical even if its first route is actor-service
only. It must be represented in system actions/witnesses and reproduced by the
native verifier and ZK guest. It must not be counted as a trade, fill, organic
volume, or trader participation.

Normal inventory-backed MM quotes are:

- economic YES bid: sell held NO at `1 - bid_yes`;
- economic YES ask: sell held YES at `ask_yes`.

Sell orders do not add buy-side group coverage, so this gives two-sided direct
liquidity without weakening complete-set prevention. The MM replenishes a
bounded inventory floor and redeems excess matched complete sets. It may never
collateralize beyond configured per-market/global limits or available cash.

The risk engine must exclude neutral `min(YES, NO)` complete-set inventory from
directional gross exposure. Otherwise inventory provisioning itself would drive
the current gross-exposure budget to zero.

## 5. Market-maker v2 rules

### 5.1 Per-block sequence

After observing committed block `H`, and only after live replay is complete:

1. load universe generation and current group membership;
2. obtain a height-tagged balance/position snapshot;
3. update mirror/native marks and data-quality states;
4. reconcile complete-set inventory targets;
5. compute risk limits and one shared portfolio budget;
6. build every market intent for `H + 1`;
7. preflight actor ranges, group safety, and inventory sufficiency;
8. submit one MM epoch and retain its per-market receipt;
9. never generate quotes while replaying historical blocks.

Position-sync failure must not update the last-success height. The safe response
is risk-reducing-only quoting where state proves it is safe, otherwise a typed
pause. It is never a silent reuse of an arbitrarily old snapshot.

### 5.2 Mirror fair value

The anchor is a recently and successfully observed external YES midpoint.
Observation freshness is different from price-change age: a quiet market whose
unchanged price was just confirmed remains fresh.

Use a hysteretic quality state machine:

- **healthy:** ordinary spread and size;
- **degraded:** wider spread and smaller size for soft staleness, a moderate
  jump, or feed/local disagreement;
- **halted:** no new risk for invalid values, hard staleness, a confirmed
  extreme jump, source close, or terminal source inconsistency;
- **recovering:** require multiple sane observations before returning to
  healthy, preventing quote flapping.

Local/reference divergence is normally an arbitrage opportunity, not by itself
a reason to stop. Exact freshness, jump, recovery, and spread multipliers are
tuning parameters selected by replay/simulation and surfaced in metrics.

### 5.3 Native fair value

Every native starts from its configured seed. Its anchor is updated only from
qualifying committed flow:

- system-only MM/noise flow has zero or explicitly small weight;
- organic-involved flow receives the primary weight;
- tiny-notional prints below the configured threshold do not move the anchor;
- use a robust window statistic followed by EWMA;
- cap movement per block;
- apply weak mean reversion toward the seed when evidence is absent;
- clamp the final actor mark to its actor-only range.

This prevents a noise/MM loop from walking its own mark to a boundary. A native
market with no qualifying flow remains safely anchored; lack of flow is not a
staleness failure.

For protocol categorical groups, project unresolved-member marks jointly onto
their feasible bounded simplex. If a member resolves NO, remove it and
reproject. If a member resolves YES or receives a fractional payout, halt
unresolved siblings until lifecycle reconciliation makes the state coherent.

Threshold ladders are not protocol groups. They use explicit actor coherence
metadata and isotonic projection so easier thresholds cannot price below harder
ones. No group minting or complete-set rule is inferred from frontend
`event_id`.

### 5.4 Spread, size, inventory, and budget

For every healthy market:

- center on the fair mark plus inventory reservation-price skew;
- widen for volatility, reference degradation, and adverse inventory;
- round outward to integer nanos/ticks;
- clamp to actor ranges only after all adjustments;
- require `bid < ask`; a range too narrow for two sides is a policy validation
  error, not a crossed quote;
- size from available portfolio capacity and expected noise flow, not a fixed
  `$100 × market count` constant.

Risk degradation is local and side-specific:

1. reduce size;
2. widen spread;
3. remove only the risk-increasing side at a hard limit;
4. continue inventory-reducing flow;
5. globally halt only for genuine solvency, state-integrity, or service-wide
   failures.

Before activation, configured capital must cover the minimum per-market quote
floor and the expected aggressive noise notional with reserve for users/LLMs.
If it does not, readiness fails loudly. The solver's shared MM budget is still
the final fill constraint; order presence must never be presented as unlimited
executable depth.

## 6. Noise trader v2 rules

### 6.1 Coordinator and accounts

Replace the production crossing-noise topology with one `NoiseCoordinator`
managing exactly three durable account principals. It receives the same
universe generation and a resumable first-party block stream.

Account ids/credentials are restored on restart. Startup must not create a new
generation of anonymous accounts or abandon old orders/inventory. The rollout
must cancel all legacy synthetic GTC orders; a fresh-genesis deployment removes
them automatically.

The coordinator emits one separate epoch per noise principal and target block.
It does not use the generic `BaseAgent` account-wide pending gate. Legacy fast,
crossing, native-noise, and random strategies are disabled in production to
avoid double-running synthetic flow.

### 6.2 Coverage and randomness

For every active market and every block, each of the three accounts receives
one action. Randomness selects direction, exact price, and size; it never
selects whether the market is visited.

Use a reproducible seed derived from:

```text
(deployment seed, genesis hash, universe generation,
 target height, actor id, market id)
```

The coordinator assigns a common economic direction per market/block, with
different aggressiveness and size among the three accounts. This strongly
prefers MM-vs-noise execution over noise-vs-noise synthetic crossing. Direction
rotates over time and is biased toward reducing aggregate noise inventory.

At cold start, actions buy YES or NO. As holdings accumulate, sell probability
and size increase; sells are always capped by owned quantity. Per-market,
per-group, per-account, and per-block notional/exposure caps apply.

### 6.3 Group-safe construction

Use actual `/markets/groups` membership, never display-event membership.

For each account/group/block choose one deterministic uncovered outcome. Any
risk-increasing child orders are constructed so their union never covers that
outcome. For example, `BuyYes` is permitted on non-hole children and `BuyNo`
only on the hole; sells do not add coverage. Rotate holes across accounts and
blocks and cap duplicated group payoff exposure.

Run the same pure `GroupCoverageTracker` model in the coordinator before
submission and retain sequencer STP as the authoritative backstop.

### 6.4 Price and time-in-force

- Use a current actor mark tagged with universe generation and observed block.
- Apply the frontend Lite-shaped, price-dependent deviation with randomized
  amplitude; do not copy Lite's buy-only behavior or 12-block lifetime.
- At least one account normally uses a deviation intended to cross the accepted
  MM quote; the others are less aggressive.
- If the current MM quote receipt is unavailable, fall back to the latest fresh
  actor mark and configured normal spread rather than withholding the market.
- Apply native YES bounds `[min, max]` and NO bounds
  `[1 - max, 1 - min]` after deviation.
- If clamping removes marketability or no valid tick exists, retain a valid
  passive IOC or emit a typed skip; never submit an out-of-range system order.
- Every noise order is IOC for the target block. No GTC and no implicit
  long-lived GTD.

### 6.5 Sustainable synthetic capital

Continuous aggressive noise pays spread and will eventually deplete even a
large demo balance. Notional is therefore derived from an explicit synthetic
liquidity budget:

```text
per_order_notional <=
  allowed_daily_synthetic_loss /
  (blocks_per_day × active_markets × aggressive_accounts × expected_edge)
```

Production/devnet actor funding uses an audited synthetic treasury policy with
a daily and total cap. Refills are explicit service actions and metrics, never
silent infinite faucet behavior. Noise low-balance readiness alerts well before
coverage fails. A seven-day simulation must validate projected burn and
inventory saturation before deployment.

## 7. LLM traders

LLM forecasting, freshness, sizing, market selection, and news triggers remain
unchanged. The current production cohort remains a small reference-backed
mirror subset; native/all-market LLM participation is not promised by this
project.

Shared client/schema changes must have regression tests proving that ordinary
LLM topology, market cap, call cadence, TIF, and decision logging do not change.
LLM presence is reported separately but is never part of block-readiness or
coverage calculations.

## 8. Actor marks and policy distribution

Static actor policy comes from the active universe snapshot. Dynamic marks are
an off-block, in-memory snapshot produced by the mirror/MM service:

```text
ActorMark
  market_id
  universe_generation
  observed_height
  observed_at_ms
  fair_yes_nanos
  quality: healthy | degraded | halted
  source: external | native_seed | native_flow
```

The MM uses the same computed mark directly; noise reads it through an
actor-authenticated endpoint. Dynamic marks need not be written to disk every
ten seconds: mirror references can be restored from the provider and native
anchors can be rebuilt from a bounded committed-price window plus the seed.

A noise epoch may use the preceding block's mark within a bounded TTL. It must
record that degradation. A generation mismatch is never tolerated.

Future MM quote receipts are actor-only because exposing planned quotes before
the batch would create a public front-running surface.

## 9. Observability and operational contract

Maintain an actor-account registry so derived analytics can distinguish:

- MM;
- noise;
- LLM Arena actors;
- ordinary users;
- protocol MINT.

Noise and MM activity is labeled synthetic and excluded from organic volume,
organic trader count, rewards, and user leaderboards. LLM Arena metrics remain
separate and explicitly labeled rather than silently treated as noise.

For each market-block store or derive:

- universe generation and eligibility;
- intended, HTTP-accepted, block-placed, matched, and filled counts separately;
- MM bid present, ask present, sizes, spread, and fair-mark quality;
- distinct noise principals placed and filled;
- LLM presence;
- synthetic vs organic notional;
- actor epoch lateness, age, supersession, and target mismatch;
- per-market omission/rejection reason;
- balance, directional inventory, neutral complete-set inventory, and budget
  headroom.

### 9.1 Human-facing Dev Zone

The existing frontend Dev Zone is the primary human debugging surface for this
project. Grafana remains the paging/capacity tool; the Dev Zone must answer, in
plain language, “which markets were alive, what did each actor do, and why did
anything fail?” without requiring log or PromQL access.

Add a dedicated `/dev/liquidity` section to the existing Overview, Markets,
Blocks, Aggregates, MM & Accounts, and Bot Decisions sections.

The Liquidity overview contains:

- active universe generation/digest and FE/MM/noise market counts;
- a loud drift banner listing missing/extra ids when those sets differ;
- latest-block MM two-sided coverage, three-noise coverage, fill coverage, and
  organic-versus-synthetic volume;
- actor freshness/readiness for MM and each of the three noise principals;
- shared MM budget used/available, directional exposure, neutral inventory,
  and synthetic-treasury runway;
- top omission/rejection reasons and the last healthy block.

The central visualization is a filterable coverage heatmap:

```text
rows    = active markets
columns = recent committed blocks
cell    = MM sides + distinct noise placers + fill/LLM state
```

Cell states must remain distinguishable without relying on color alone:

- healthy: MM bid+ask and three noise placers;
- quoted/no fill: actor coverage succeeded but no trade occurred;
- degraded: one MM side or one noise principal missing;
- failed: fewer than two noise principals, no MM quote, or actor error;
- ineligible: suspended/resolved for that generation.

Filters include mirror/native, protocol group, market id/name, mark quality,
coverage state, actor, and reason code. Selecting a cell opens a block/market
drill-down containing:

- fair mark, mirror reference, clearing price, MM bid/ask, and native actor
  range over time;
- noise order prices/directions by principal and optional LLM presence;
- intended -> HTTP accepted -> block placed -> matched -> filled funnel;
- spread, size, budget/risk state, epoch age, target height, and universe
  generation;
- exact typed skip/rejection reasons with a short human explanation.

Existing pages also receive focused additions:

- **Overview:** coverage cards, latest actor health, and a small recent-block
  heatmap rather than only aggregate orders/fills;
- **Markets:** active/actor flags, MM bid/ask presence, noise count, mark
  source/quality, native range, last fill block, and link to the drill-down;
- **Blocks:** per-actor coverage/funnel, synthetic/organic split, epoch lateness,
  and reason counts for the selected block;
- **MM & Accounts:** MM neutral versus directional inventory, budget/risk
  headroom, three noise balances/inventory, treasury subsidy and projected
  runway;
- **Bot Decisions:** remains focused on LLM reasoning and does not mix routine
  synthetic noise rows into the decision feed.

The UI is truthful about data quality:

- intended, accepted, placed, matched, and filled are never conflated;
- missing/unavailable data renders as unknown/stale, never numeric zero;
- every panel shows its observed height/time and universe generation;
- stale last-known-good data remains visible with an explicit warning;
- suspended markets are distinct from actor failures;
- all synthetic volume is visibly labeled.

The current Dev Zone is public and consumes public read APIs. Therefore it may
show only committed, privacy-safe actor aggregates. It must never expose future
MM quote epochs, actor credentials, raw account-attributed orders, or private
rejection rows. If a future operator-only mode is added, it requires an
explicit authenticated boundary rather than hiding data with frontend code.

Serve the dashboard from a bounded derived read model, conceptually:

```text
GET /v1/activity/liquidity?blocks=N
GET /v1/activity/liquidity/markets/{id}?blocks=N
```

The response is typed in `sybil-api-types`, paged/bounded, and built from
committed actor receipts/block projections outside the sequencer mailbox. Do
not issue per-market account/API requests from the browser or scan canonical
state on each refresh. Default to a 30-block matrix, enforce a small maximum,
and refresh once per committed block.

Avoid unbounded Prometheus cardinality. Export platform ratios/counters and a
bounded per-market diagnostics table/log; do not label metrics with epoch ids
or order ids.

Initial service levels:

| Metric | Target | Warning | Critical |
|---|---:|---:|---:|
| Active-set FE/actor equality | 100% | any drift | any drift persists one block |
| MM two-sided quoted market-blocks | 100% | <99% for 2 blocks | <80% in any block |
| Markets with 3 distinct noise placers | 100% | <99% for 2 blocks | any market with <2 for 2 blocks |
| Stale actor epochs executed | 0 | — | any |
| Synthetic orders classified | 100% | <100% | persistent drift |

Fill coverage is a product KPI, not a validity SLO. The initial launch gate is
at least 80% of healthy market-blocks with nonzero fill in the calibrated
synthetic scenario, with no claim that production must force a fill when risk
or safety rules intervene.

Typed reason codes include at least:

```text
market_suspended, market_resolved, universe_mismatch, late_epoch,
stale_epoch, duplicate_or_superseded, missing_reference, stale_reference,
reference_jump, source_closed, invalid_actor_mark, range_too_narrow,
range_clamped_non_marketable, incoherent_group, position_snapshot_stale,
insufficient_inventory, insufficient_cash, per_market_risk_limit,
global_risk_limit, mm_budget_insufficient, complete_set_preflight,
sequencer_stp, actor_auth, actor_rate_limit, solver_failure, internal_error
```

## 10. Failure modes and required behavior

| Failure | Required behavior | Required test |
|---|---|---|
| Universe candidate has 206 discoverable entries but incomplete actor policy coverage | Refuse generation activation | Migration fixture/count test |
| Snapshot file missing/corrupt or digest mismatch | Keep last complete generation; actor readiness fails closed | Process restart/corruption test |
| Crash between policy upload and universe block commit | Resume two-phase activation without a mixed generation | Crash/WAL test |
| Market added/removed during epoch construction | Reject stale generation; refresh and target a later block | API integration race test |
| One market resolves after epoch staging | Reject only that market's orders; retain the rest and rebuild one MM constraint | Sequencer integration test |
| Duplicate actor HTTP retry | Return original receipt; do not add orders/budget | Idempotency + restart test |
| Replacement epoch crashes mid-write | Restore either old or new complete epoch, never both | Store crash test |
| Block production pauses | Drop epoch after target/`valid_until`; never execute stale quotes on resume | Pause/restart test |
| Actor misses target deadline | 409/typed late result; never retarget silently | API timing test |
| Mirror feed is quiet but healthy | Fresh observation keeps quoting | Feed unit test |
| Mirror feed is stale or jumps extremely | Degrade/halt with hysteresis and recover after sane observations | State-machine property test |
| Local price is far from fresh mirror reference | Widen/reduce unless integrity threshold is crossed | Quote unit test |
| Tiny synthetic native print tries to move mark | Ignore or heavily downweight; enforce max step | Native-mark property test |
| Native group ranges cannot satisfy simplex/monotonic rules | Refuse policy activation | Catalog property test |
| Group member resolves NO/YES/fractionally | Reproject or halt siblings according to lifecycle rule | Group transition tests |
| MM position snapshot fails | Do not mark sync successful; risk-reducing only or typed pause | MM actor test |
| MM complete-set inventory is insufficient | Atomically collateralize within cap or omit affected side; never settle negative | Engine/sequencer/verifier properties |
| Aggregate exposure reaches limit | Stop risk-increasing sides only; do not return before all markets are evaluated | Quote/risk tests |
| MM package exceeds public 64 cap | Actor route accepts within actor cap; public route still rejects | Route-policy test |
| Multiple MM chunks/retries | Exactly one budget constraint per MM epoch | Witness/verifier test |
| Noise restart | Restore same three principals, cursor, and inventory | Process test |
| Legacy GTC noise remains | Migration cancels it or fresh genesis eliminates it | Migration assertion |
| WS lag/reconnect | Replay state only, wait for `replay_complete`, then act on next live head | Python SDK integration test |
| Noise group actions complete coverage | Local preflight repairs/rejects; sequencer STP remains authoritative | Hypothesis + sequencer test |
| Noise balance drains | Reduce size/alert under treasury policy; no silent unlimited refill | Multi-day simulation |
| Actor token is used for another account/role | 403 before decoding/staging value-bearing work | Auth/route test |
| Actor endpoint is flooded | Body, count, rate, pending-epoch, and mailbox limits hold | Load/admission test |
| Solver or persistence exceeds block interval | Block production remains correct; alert and disable/cut actor load via kill switch | Capacity/failure-injection test |
| Synthetic rows pollute organic analytics | Classification tests keep rewards, trader count, and organic volume clean | Aggregate/history integration test |
| Dev Zone telemetry is stale/unavailable | Keep last good values visibly stale; never render missing values as zero | UI state and endpoint-failure tests |
| Dev Zone leaks pending actor information | Public DTO contains committed aggregates only, with no account ids or future quotes | Route/privacy contract test |
| DA/history disk grows too quickly | Measured bytes/day must fit retention budget; compress or reduce derived retention before launch | 10k-block storage soak |

## 11. Capacity and bottleneck plan

The performance test book must model the real shape, not only a random
1,030-order book:

- 206 binary markets: 72 mirrors and 134 natives;
- current protocol group-size distribution;
- one spanning 412-order MM constraint;
- three 206-order ordinary IOC noise bundles;
- native bounds, categorical groups, and threshold display cohorts;
- 0, 12, and stressed LLM/user additions;
- realistic resting user orders and a resolution/add/remove event;
- healthy, tight-budget, one-sided-flow, and reference-degraded variants.

Measure separately:

- actor HTTP parse/auth/admission and sequencer mailbox time;
- block build, solver, integer landing, verification, settlement, persistence,
  publication, and history-outbox time;
- peak RSS and allocator growth;
- canonical block, witness, DA payload, and history bytes per block/day;
- actor receipt and service-stream payload size;
- retained-cash convergence/certificate quality with the single broad MM;
- proof guest cycles/memory even though the current deployment uses a mock
  prover.

Provisional launch gates on production-equivalent hardware:

- p99 prepare-through-persist below 50% of the ten-second interval;
- no missed interval in the deterministic 10,000-block accelerated soak;
- actor epoch admission p99 below one second under concurrent four-epoch load;
- no unbounded pending epochs, resting synthetic orders, or RSS growth;
- seven-day retained canonical/DA/history projection fits the allocated disk
  with at least 2× safety margin;
- every returned block passes the native verifier and all economic invariants.

At 1,030 baseline orders and 8,640 blocks/day, the system processes roughly
8.9 million synthetic orders/day. Storage, history projection, and proof cost
may be a tighter bottleneck than the solver. Do not launch until exact bytes per
market-block are measured. Suppress nonessential derived per-order history for
MM/noise while retaining canonical witness/DA requirements and aggregate actor
receipts. Compression/retention changes require their own integrity review;
never drop validity material just to meet the target.

## 12. Test and simulation program

### 12.1 Pure/unit/property tests

`matching-engine`, `matching-sequencer`, `sybil-verifier`, and `sybil-zk`:

- collateralize/redeem round trips and checked rounding;
- cash/position conservation and no negative non-MINT state;
- collateralization commutes across independent markets;
- redeem cannot exceed either held outcome;
- MM inventory-backed sell fills remain nonnegative under any allowed fill;
- one MM epoch implies one complete shared constraint;
- generation/target/expiry comparisons are deterministic;
- group STP is unchanged for public and actor accounts.

`sybil-polymarket`:

- quote ordering, complement prices, range clamps, and narrow-range rejection;
- neutral inventory exclusion and risk-side classification;
- mirror quality-state hysteresis;
- native robust update, max step, seed pull, simplex projection, and isotonic
  projection;
- one bad market never aborts quote generation for other markets.

Arena with `pytest`/Hypothesis:

- every active market appears exactly once per principal/epoch;
- deterministic seeds reproduce identical epochs;
- different principals are decorrelated in price/size;
- no sell exceeds holdings;
- YES and complementary NO bounds are correct near 0/$1;
- every protocol group preserves an uncovered outcome;
- reconnect replay never emits historical trading actions;
- no GTC order is generated by Noise v2.

Frontend:

- public cards equal the active discoverable generation;
- fixtures and suspended markets are absent without name heuristics;
- closed/history detail remains reachable;
- liquidity heatmap and drill-down preserve the intended/accepted/placed/
  matched/filled distinction;
- unknown, stale, suspended, degraded, and healthy states render distinctly and
  accessibly without color-only meaning;
- actor dashboard responses contain committed aggregates only and cannot leak
  future quote epochs or account-attributed rows;
- bounded dashboard queries do not create an N+1 request per market;
- schema generation catches DTO drift.

### 12.2 API/sequencer integration and durability

- actor credential scope, body limits, role limits, and public-cap separation;
- exhaustive intent validation and per-market partial rejection;
- epoch idempotency, supersession, late target, stale wall clock, and generation
  mismatch;
- activation/suspension atomically evicts resting orders and releases reserves;
- restart after every acknowledged universe, epoch, and collateralization write;
- resolved-market race preserves the remaining MM budget mapping;
- receipts distinguish intended/accepted/placed/filled.

Any canonical state/witness change runs byte/golden, native-verifier, guest,
fingerprint, crash-harness, import, and process-restart coverage.

### 12.3 Deterministic solver and sequencer simulations

Add a named `active-liquidity-206` scenario to `matching-scenarios` for
solver-level capacity and a multi-block profile to `sequencer-sim` for actor
dynamics. Every run records seed, revision, config, machine profile, and all
failures; no failed book is silently dropped.

Required scenario matrix:

| Axis | Values |
|---|---|
| Noise principals | 0, 1, 3, 5 |
| Active markets | 134, 206, 300 |
| User/LLM additions | 0, 12, 100, 1,000 orders |
| MM budget | ample, expected, tight, exhausted |
| Flow | balanced, all bullish, all bearish, hot-market, heavy-tail |
| References | healthy, soft stale, hard stale, jump/recovery |
| Lifecycle | stable, add/remove, group shrink, resolution race |
| Restart | none, periodic actor reconnect, API/sequencer restart |

Simulation assertions:

- 100% intended/accepted coverage under the healthy ample-budget profile;
- no STP rejection in preflight-valid actor epochs;
- fill coverage meets the 80% launch KPI in the calibrated healthy profile;
- positions, cash, budget, and native marks remain bounded for 10,000 blocks;
- system-only native flow cannot cause boundary drift;
- projected seven-day synthetic subsidy remains under its configured cap;
- results are deterministic for the same seed.

### 12.4 Load, soak, and failure injection

Run the mutating load scenario only on a disposable stack; the existing
read-only `sybil-loadtest` contract remains read-only.

1. 10,000 accelerated in-process blocks for correctness and growth.
2. Production-cadence soak on production-equivalent hardware for timer,
   networking, file, and memory behavior.
3. Shadow MM/noise planning against live data without submitting orders.
4. Fault injection for feed disconnects, 429/5xx, delayed actor responses,
   dropped WS frames, API restart, storage latency, and solver timeout.
5. A 24-hour canary before full synthetic notional, followed by a seven-day
   capital/storage projection report.

## 13. Implementation sequence

Each phase has a kill switch and must meet its gate before the next phase.

### Phase 0 — ratification and catalog migration

- Record ADRs for the committed trading-universe generation, actor epochs, and
  complete-set collateralization boundary.
- Export and review the 72 mirror stable source identities.
- Backfill the 65 missing mirror mappings and freeze the 134 expanded native
  specifications, including their child titles, ranges, and source metadata.
- Explicitly retain the eight final-week OpenRouter children and retire the
  nine duplicated legacy children plus the 10M context-window rung.
- Add catalog validation and a golden migration fixture asserting 72/134/206.
- Decide and provision persistent MM/noise account identities and scoped
  credentials.

**Gate:** a dry-run reconciliation produces one complete, coherent 206-market
candidate with no inferred/default actor policy.

### Phase 1 — active universe and frontend

Likely ownership:

- `matching-sequencer`: committed trading enabled/suspended state, generation,
  control action, order eviction;
- `sybil-verifier`/`sybil-zk`: state transition and witness checks;
- `sybil-api-types`: universe DTOs;
- `sybil-api`: candidate store, activation API, public/actor reads;
- `sybil-polymarket`: desired-state reconciler;
- frontend: consume discoverable active generation and remove fixture-name
  filtering.

**Gate:** FE, API, MM shadow, and Arena shadow report the same generation and
market ids across add/remove/restart tests.

### Phase 2 — actor credentials and bulk epochs

- Add actor-scoped auth and role/account binding.
- Add target-height/generation actor epoch DTOs and route.
- Add durable idempotent/superseding actor-epoch storage.
- Add per-market receipts and partial poison isolation.
- Preserve one MM budget constraint and the public 64-order cap.
- Add Python/Rust client support and first-party resumable stream handling.

**Gate:** four concurrent epochs totaling 1,030 orders land in one target block,
survive retry/restart, and verify with no duplicate budget.

### Phase 3 — complete-set inventory and MM v2

- Implement per-market collateralize/redeem through integer shared helpers.
- Extend canonical state transitions, witness, native verifier, and guest.
- Replace gross inventory exposure with neutral-set plus directional risk.
- Implement inventory-backed two-sided quotes, mark quality states, native mark
  logic, group/simplex/threshold coherence, and local degradation.
- Add per-market reason codes and quote receipts.

**Gate:** all 206 markets receive group-safe two-sided accepted MM quotes for
10,000 healthy simulated blocks; every block verifies and inventory stays
bounded.

### Phase 4 — NoiseCoordinator v2

- Implement three persistent principals, deterministic all-market scheduling,
  group-hole planning, inventory-biased direction, Lite-shaped prices, actor
  range clamps, IOC epochs, and treasury metrics.
- Disable legacy production synthetic strategies.
- Add restart/cursor support and legacy GTC cleanup.

**Gate:** three distinct accepted noise principals on every active market in
the healthy simulation, no GTC, no STP rejection, no out-of-range actor order,
and bounded seven-day projected capital burn.

### Phase 5 — observability, performance, and rollout

- Add actor registry/classification and coverage dashboards/alerts.
- Add the `/dev/liquidity` heatmap, market/block drill-downs, MM/noise risk and
  treasury panels, and focused additions to existing Dev Zone pages.
- Add bounded committed-liquidity read models/endpoints outside the sequencer
  mailbox; prohibit future-epoch/account data from the public DTO.
- Add exact 206-market solver, sequencer, storage, and proof benchmarks.
- Run shadow, fault, soak, and production-equivalent capacity gates.
- Update architecture notes, `docs/SPEC.md`, runbooks, Compose preflight, and
  generated clients only as behavior lands.

**Gate:** all Definition-of-Done items below and an explicit launch report with
latency, memory, storage/day, fill coverage, and treasury burn.

## 14. Deployment and rollback

Because trading-state and collateralization change canonical state/witness
semantics, the recommended first deployment is a fresh-genesis devnet redeploy.
This also removes abandoned noise accounts and GTC orders cleanly. An in-place
chain migration is a separate project and must not be improvised during deploy.

Fresh rollout order:

1. deploy API/sequencer with actor endpoints disabled;
2. sync and verify all source markets and policy generation 1;
3. create/fund/register persistent actor principals;
4. activate the universe and verify FE equality;
5. run full-universe MM/noise shadow planning;
6. enable MM at minimal size with noise disabled;
7. enable one noise principal, then all three;
8. raise only notional parameters after the 24-hour canary report.

Rollback never stops block production:

- revoke/disable actor credentials or actor route group;
- IOC epochs disappear after their target block and stale epochs are dropped;
- user trading and resolution continue;
- burn redeemable neutral MM inventory where safe; directional positions remain
  ordinary account positions until traded or resolved;
- keep the last valid universe generation rather than publishing a partial
  rollback snapshot.

## 15. Definition of done

This project is complete only when all are true:

- the reviewed migration snapshot contains exactly the intended 206 markets
  and complete policy for each;
- FE, admission, MM, noise, and monitoring use one active generation;
- public order submissions remain capped at 64 while actor epochs support the
  measured all-market load;
- one MM epoch creates one shared budget constraint and accepted economic bid
  and ask on every healthy active market;
- complete-set STP remains unchanged and no beneficial-owner identity split is
  used to bypass it;
- three persistent noise principals place one IOC order on every active market
  in every healthy block;
- native actor orders respect complementary ranges and coherent group/ladder
  marks, while user orders remain unrestricted by those actor ranges;
- LLM trading behavior is regression-identical apart from additive schemas or
  transport reliability;
- actor retries, restarts, stream replay, universe changes, reference failures,
  and one-market poison cases have deterministic tested recovery;
- synthetic activity is excluded from organic metrics/rewards;
- the Dev Zone shows universe drift, per-market/block actor coverage, prices,
  marks, ranges, reason codes, MM risk, noise health, and synthetic/organic
  activity without exposing future quotes or private actor data;
- healthy simulation fill coverage is at least 80%;
- 10,000-block correctness/growth and production-cadence canary gates pass;
- p99 block time, RSS, DA/history bytes, proof cost, and projected actor capital
  burn fit documented budgets with headroom;
- relevant architecture notes, ADRs, runbooks, generated clients, and deploy
  configuration describe the landed behavior.

## 16. Validation commands after implementation

The exact command set grows with the touched crates, but the minimum final gate
is expected to include:

```bash
cargo test -p matching-engine
cargo test -p matching-sequencer
cargo test -p sybil-verifier
cargo test -p sybil-zk
cargo test -p sybil-api-types --all-features
cargo test -p sybil-api
cargo test -p sybil-polymarket
cargo test -p matching-scenarios
cargo test -p sequencer-sim

cd arena && uv run pytest tests/ -v
cd frontend/web && pnpm test && pnpm run types:check

just check-consensus
just arena-check
just frontend-check
just compose-smoke
just docs-check
```

Capacity/soak commands and artifacts must be added to a dedicated runbook; they
are explicit release evidence, not ordinary unit-test work.
