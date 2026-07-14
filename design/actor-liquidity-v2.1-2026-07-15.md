---
tags: [design, arena, liquidity, market-maker, native-markets, frontend, operations]
layer: core
status: proposed
date: 2026-07-15
supersedes: actor-liquidity-v2-2026-07-14 sections 5.3, 5.4, 6, 7.2, 8, 9, 10, and 11 where they conflict
---

# Actor Liquidity v2.1 — sparse noise, robust native marks, and truthful PnL

## 1. Status and scope

This is the implementation specification for the post-soak corrections to
Actor Liquidity v2. It does not authorize deployment. The implementation must
pass the simulations and local soak gates in this document before the existing
production deployment workflow is used.

The change has four coupled goals:

1. stop MM/noise-only flow from walking native marks into actor guardrails;
2. replace three all-market noise accounts with fifteen sparse, independent
   accounts whose aggregate order coverage is about 25% of markets per block;
3. restore genuinely two-sided MM liquidity on almost every healthy market;
4. make Dev/Arena account roles and PnL cohorts truthful and Sybil-marked.

LLM decision cadence and market selection remain unchanged. Public user order
rules, settlement, verification, and the public 64-order submission cap remain
unchanged.

## 2. Decisions that resolve overlapping requirements

Some requested rules came from the old three-noise/all-market topology. The
following precedence rules make the target internally consistent.

### 2.1 Fifteen actors supersede the old three-actor wording

There is one MM principal and exactly fifteen durable noise principals. Rules
such as “one crosser and the other two vary” are reinterpreted per
noise-active market-block, not as a permanent three-account formation.

### 2.2 Aggression is randomized from the prior committed mark

Noise orders touch about 25% of markets per block. Each selected order is
independently aggressive with 60% probability and passive with 40% probability.
Both are priced from the previous committed Sybil mark: the traded clearing
price when the market filled, otherwise the two-sided book midpoint, otherwise
the carried mark. Noise actors never inspect MM orders for the upcoming batch.

Aggressive buys move above the mark and aggressive sells below it; passive
orders move in the opposite direction. The randomized distance is bounded by
the exact frontend Lite-tax envelope. Crosses and the requested 10–20% realized
fill coverage are emergent calibration outcomes, not coordinated guarantees.

### 2.3 Account and actor PnL are Sybil-only

The later requirement to base account accounting on Sybil data supersedes the
earlier proposal to display account PnL under both Sybil and external marks.

- `/dev/accounts`, `/dev/overview`, and the headline `/arena` PnL use canonical
  account `portfolio_value_nanos` and `pnl_nanos` from Sybil.
- No frontend code recalculates account PnL from market reference prices.
- External reference prices remain valid market/feed/MM diagnostics, but are
  never presented as account portfolio value or account PnL.

### 2.4 Roles are discovered, not inferred from permanent numeric ranges

On a fresh local chain the first topology is expected to be MM account `1`,
noise accounts `2..=16`, and the first six LLM sizer accounts `17..=22`.
Those ranges are not a product contract: Arena restarts and non-local account
allocation can change LLM ids, while production actor ids come from the
credential file. Frontends must join runtime role metadata to account ids and
must not hard-code `1`, `2..4`, or `5..10`.

## 3. Target block behavior

For an active universe of `M` markets (206 in the current reviewed catalog), a
healthy block has:

```text
MM:       up to 2 economic sides × M                 ≈ 412 orders
Noise:    15 actors × M × 0.01899 selection chance   ≈ 59 orders
LLM/user: opportunistic                               additional
```

The target is order presence, not guaranteed execution:

- MM: 100% operational target; at least 98% two-sided coverage in healthy
  operation, with every omission carrying a typed reason;
- noise: approximately 25% of active markets receive at least one noise order
  per block, measured over a rolling window;
- aggressive noise orders: approximately 60% of selected noise orders;
- realized market fill coverage: 10–20% over a rolling window;
- LLM: unchanged and opportunistic.

## 4. Native fair value and MM boundary recovery

This work is a prerequisite for enabling sparse noise. The current native
algorithm consumes every published clearing vector, including zero-volume and
MM/noise-only blocks. Because a clearing vector can exist without a trade, the
current “quiet block” branch almost never runs. System actors therefore feed
their own prints back into the MM anchor until the configured boundary removes
one quote side.

### 4.1 Derived flow attribution

Add non-consensus, aggregate attribution to each `BlockMarketStats`:

```text
mm_matched_orders
noise_matched_orders
organic_matched_orders
mm_fill_notional_nanos
noise_fill_notional_nanos
organic_fill_notional_nanos
```

“Organic” means an order that did not enter through an MM or noise actor epoch;
it includes manual and LLM accounts. The values are derived analytics only:
they do not enter the state root, witness, verifier, solver objective, or
settlement. Order origin is known while actor epochs are merged and can be
joined to filled order ids when block analytics are built.

The notional fields are role-attributed filled-order notionals, not a promise
that their sum equals the block's unique economic trade notional. Native mark
qualification uses only `organic_matched_orders > 0` and the organic notional
threshold; product volume retains its existing definition.

### 4.2 Qualifying native observations

A native clearing price may influence its actor mark only when all conditions
hold:

- the market has positive matched volume;
- at least one organic order matched;
- organic filled-order notional meets a configurable minimum (initial local
  default: `$1.00`);
- the price is finite, nonterminal, and inside the native actor range;
- the market/group lifecycle is active and coherent.

MM/noise-only flow has exactly zero mark weight. A zero-volume clearing vector
has exactly zero mark weight. Merely placing an organic order is insufficient.

### 4.3 Robust update

Each native child keeps:

```text
seed_yes
current_actor_mark
recent qualifying observations (price, organic notional, height)
last_qualifying_height
mark_quality = seed | organic | reverting | halted
```

Initial tuning contract:

1. retain the latest 20 qualifying observations;
2. compute a notional-capped weighted median so one large order cannot own the
   entire window;
3. cap candidate movement to 2 percentage points per block;
4. apply an EWMA weight of 0.15 to the capped candidate;
5. without qualifying evidence, move 0.2% of the remaining distance toward the
   configured seed per block;
6. apply group/ladder projection;
7. clamp to the quoteable interior, not the raw actor boundary.

All constants are configuration with validated safe bounds and metrics. They
are calibrated in simulation; they are not protocol validity rules.

### 4.4 Coherence projection

For each categorical protocol group, project the complete mark vector onto the
bounded simplex:

```text
sum(mark_yes_i) = 1
quoteable_min_i <= mark_yes_i <= quoteable_max_i
```

The checked-in catalog must itself contain categorical seed vectors summing to
1 within one actor-price tick. The current catalog has 17 incoherent enabled
categorical seed vectors, although every group has feasible min/max bounds;
those seeds must be corrected as part of this migration. Loader validation
then rejects future incoherent seed vectors.

For threshold cohorts, project marks isotonicly according to their declared
direction. For an `above` ladder, an easier threshold may not have a lower YES
mark than a harder threshold. Threshold cohorts remain display/coherence
groups, not protocol `MarketGroup`s.

### 4.5 Quoteable interior and two-sided MM construction

For a native range `[min, max]`, derive an interior that leaves room for an
integer bid, ask, configured minimum spread, and one crossing tick:

```text
quoteable_min = min + required_edge_room
quoteable_max = max - required_edge_room
```

Catalog validation fails if no such interior exists. The reservation mark is
clamped to that interior before quote construction. Quote construction then
clamps the economic YES bid and ask asymmetrically to the actor range and must
produce `bid < ask` after integer rounding.

The underlying group-safe MM shapes remain:

- economic YES ask: `SellYes`;
- economic YES bid: `SellNo` at the complementary NO price.

A healthy, funded native market therefore emits both sides even if the last
public clearing price sits at a raw guardrail. One-sided quoting is allowed only
for a typed hard inventory/risk state, never as an unnoticed rounding result.

### 4.6 Mirror MM availability

Mirrors retain external references:

- fresh reference: normal quote;
- soft stale reference: smaller, wider quote using the last valid observation;
- hard stale, invalid, terminal, or confirmed extreme reference: typed skip;
- recovery requires multiple sane observations to avoid flapping.

The production `--mm-max-orders-per-block=64` setting must be removed/disabled;
the actor route's dedicated MM cap remains the protection. The public order cap
is unchanged.

## 5. Committed mark pricing and MM diagnostics

Noise prices use only the prior committed Sybil mark. The serving mark is:

1. the clearing price when the market had a fill;
2. otherwise the midpoint of the committed two-sided book;
3. otherwise the previous carried mark;
4. the native seed/50¢ only before any mark exists.

The actor-authenticated MM snapshot remains available for Dev diagnostics:

```text
GET /v1/actor/mm-quotes?target_height=H

MmQuoteSnapshot
  target_height
  universe_generation
  observed_at_ms
  markets[]
    market_id
    yes_bid_nanos?
    yes_ask_nanos?
    bid_quantity?
    ask_quantity?
    quote_state: two_sided | one_sided | skipped
    skip_reason?
```

The API derives the economic bid/ask from the accepted MM actor epoch and
retains only a bounded recent receipt cache. Tokens, account-private inventory,
and raw order ids are not returned. The coordinator never calls this endpoint.
The API may compare accepted noise orders with the cached MM shape after both
exist to report naturally marketable orders; that observation cannot influence
the submitted noise price.

## 6. Fifteen-account sparse noise model

### 6.1 Identities and capital

- exactly fifteen unique durable noise principals and accounts;
- one coordinator process may manage them, but each submits a separately
  authenticated epoch and remains independently observable;
- no account creation or refill during normal startup;
- local total noise capital remains `$300,000`, split to `$20,000` per account;
- production provisioning declares an explicit total equal to the approved
  pre-migration aggregate and divides it across fifteen accounts;
- no fivefold increase in total synthetic capital or allowed loss.

### 6.2 Deterministic independent randomness

Every random lane is derived from:

```text
(deployment seed, genesis hash, universe generation, target height,
 actor principal id, market id, lane name)
```

Actor principal id is mandatory for selection, direction, group hole, size,
aggressiveness, and sell decisions. Replaying the same inputs reproduces the
same plan; different actors do not share direction or group holes by accident.

Each actor has a stable personality derived from its principal id:

- activity multiplier in a bounded range around 1, normalized across the
  cohort so expected aggregate coverage is unchanged;
- size multiplier;
- small bounded directional bias;
- passive-distance and aggressiveness preference.

Personalities create heterogeneity but may not override range, solvency,
inventory, or group-safety rules.

### 6.3 Market selection and aggregate coverage

For fifteen independent actors, the baseline per actor/market selection
probability is:

```text
p0 = 1 - (1 - 0.25)^(1 / 15) ≈ 0.01899
```

At 206 markets this produces:

- about 3.9 selected markets per actor per block;
- about 59 noise orders per block;
- about 51.5 markets (25%) with at least one noise order;
- usually one noise actor on a touched market, sometimes two, rarely three.

Selection is independent per actor after applying its normalized personality
multiplier. It never depends on iteration order or process timing.

### 6.4 Anti-starvation

The coordinator stores, keyed by genesis hash and universe generation, the last
height at which any accepted noise order selected each market. The selection
probability remains `p0` for an ordinary drought and then rises gradually after
eight untouched blocks, with a validated cap. It never forces every market in
one block and must preserve 22–28% rolling aggregate coverage after tuning.

State is updated only from accepted actor receipts. If the durable state is
missing after a restart, anti-starvation age resets and emits a degraded metric;
base random selection continues safely. A generation change starts a new map.

### 6.5 Direction and inventory reduction

At zero inventory, YES and NO economic directions have equal base probability
apart from the actor's small stable personality bias. As inventory accumulates:

- the probability of selecting the over-held outcome for a sale increases
  monotonically with marked inventory value;
- the probability of buying further into an over-held outcome decreases;
- sell quantity is always capped by owned share-units;
- expected inventory drift above the soft cap must point toward zero;
- hard per-market, per-group, per-account, and total notional caps apply.

The exact curve is a pure function under test. It must not use raw share count
alone: cheap YES and expensive NO orders produce very different share counts
for the same dollars. Bias and caps use Sybil-marked dollar exposure.

### 6.6 Group safety

Use protocol `/markets/groups` membership, never frontend event membership.
For every selected actor/group/block:

- derive an actor-specific uncovered outcome (group hole);
- risk-increasing buys may not cover the hole;
- `BuyYes` is allowed only on non-hole selected children;
- `BuyNo` is allowed only on the hole;
- sells do not add payoff coverage but remain inventory-capped;
- run the same pure coverage tracker before submission;
- sequencer self-trade prevention remains authoritative.

Sparse selection means most actors touch zero or one child in a group, but the
full rule is still required for collision and anti-starvation cases.

### 6.7 Aggressive and passive orders

For each selected actor/market/block, independently draw aggression with 60%
probability. Use the selected outcome's previous committed mark and calculate
the frontend Lite deviation:

```text
max_deviation = 0.04 × (4 × mark × (1 - mark)) ^ 1.3
```

Draw a heterogeneous distance inside that envelope using the actor personality
and per-order random lane. Aggressive buys add the distance and aggressive sells
subtract it. Passive buys subtract the distance and passive sells add it. Every
final price is integer nanos and is clamped to the actor-only YES/NO range.
No order reads, targets, or claims knowledge of an upcoming MM quote.

Order notional is randomized inside an exact `$7`–`$150` range. Every actor can
draw the full range, while its stable size personality shifts probability mass
toward smaller or larger orders. The inventory-reduction scale is `$800`, so
the larger orders do not immediately force maximum sell bias. No zero-notional
or below-minimum order may be emitted. MM depth is `$200` per side so a
marketable noise or LLM order is not artificially capped by the previous local
`$1` quote.

### 6.8 Time in force

Every noise order is IOC for exactly its target FBA block. A one-block explicit
expiry is equivalent. No noise order becomes GTC or survives into a second
batch.

## 7. Actor API and credential changes

### 7.1 Credential contract

The credential file must contain exactly:

- one `market_maker` actor;
- fifteen `noise` actors;
- sixteen unique principals, accounts, and tokens;
- no MINT/system account;
- tokens of at least 32 bytes.

Any partial or legacy one-plus-three file fails actor readiness closed. Error
messages, config comments, deployment docs, Compose interpolation messages,
and smoke fixtures must say one-plus-fifteen rather than four actors.

### 7.2 Sparse noise epochs

MM epochs retain exact one-intent-per-active-market coverage so every omission
has a typed reason. Noise epochs become sparse on the wire:

- supplied market ids must be unique members of the active generation;
- a supplied noise intent contains at most one order;
- an empty supplied intent requires a typed exceptional skip;
- omitted markets mean `random_not_selected` and are not serialized;
- a noise epoch with zero selected markets is a valid heartbeat;
- hard cap: 32 noise orders per actor epoch;
- noise may not attach an MM budget.

The API still constructs `ActorEpochSubmission.covered_market_ids` from the
entire committed universe, preserving the sequencer's exact-generation and
full-decision-package invariant. The sequencer remains unaware of HTTP sparsity
and retains IOC target-height semantics.

Receipts add aggregate counts (`considered`, `selected`, `accepted`, `skipped`)
and return rows only for supplied intents. API observations store the MM's full
market shape but only selected/exceptional noise markets plus one actor
heartbeat, avoiding `15 × 206 × 256` map entries.

### 7.3 Runtime actor-role metadata

Expose a secret-free diagnostics read, either as an additive section of
`/v1/liquidity/health` or a dedicated `/v1/liquidity/actors` route:

```text
account_id
principal_id
role = market_maker | noise
last_observed_height?
ready
```

Never expose bearer tokens. Frontend role labels use this response.

Arena portfolio snapshots add `account_id` and
`participant_kind = llm | noise | legacy` so current LLM accounts can also be
joined without name-prefix heuristics. SQLite migration is additive and the bot
feed exposes both fields on summaries.

## 8. Frontend accounting and Dev UI

### 8.1 Canonical PnL definitions

For an account:

- Cash = `balance_nanos`;
- Sybil Portfolio = `portfolio_value_nanos`;
- Sybil PnL = `pnl_nanos`;
- position Sybil Value = `position.value_nanos`.

For a cohort, sum those API fields across the exact role-resolved account set.
Do not rebuild a portfolio from prices in TypeScript.

Delete the account `positionRefValue` helper and every account-level
reference-PnL aggregate. Market-level reference price comparisons elsewhere in
Dev Zone remain separate and explicitly named.

### 8.2 `/dev/overview`

Replace the single misleading `MM Ref PnL` card with four Sybil-marked cards:

- MM PnL — the one runtime MM account (local fresh chain: account 1);
- Noise PnL — all fifteen runtime noise accounts;
- LLM PnL — latest active account for each current LLM trader;
- All Actors PnL — union of MM, noise, and current LLM accounts, deduplicated.

Each card also shows summed Sybil Portfolio and account count. Account zero and
unclassified manual/user accounts are excluded from All Actors and shown as a
separate “Other accounts” count if present. Missing role metadata renders `—`
with a data-quality warning; it must never silently fall back to “all active.”

The `/arena` headline PnL becomes LLM-only. Noise snapshots remain available
for risk diagnostics but do not masquerade as Arena competitor performance.

### 8.3 `/dev/accounts`

Remove:

- Ref Portfolio;
- Ref PnL;
- Ref Mark;
- all mixed reference/Sybil fallback text.

Display:

- Cash;
- Sybil Portfolio;
- Sybil PnL;
- total position count;
- YES position count, quantity in share-units/shares, and Sybil value;
- NO position count, quantity in share-units/shares, and Sybil value.

Role labels are `MM`, `Noise`, `LLM`, `Other`, or `System`, joined from runtime
metadata. The position table is titled **Top 25 Positions by Sybil Value** and
has an `All / YES / NO` filter. Filtering occurs before sorting and taking 25.
The table may later become paginated, but the title and outcome summaries are
required now so the slice cannot be mistaken for the full portfolio.

Account fetching uses the union of ids from actor metadata, current bot
summaries, pending orders, and already loaded participant data. The existing
fixed `0..47` scan is not an identity source.

## 9. Liquidity health and observability

Replace three-noise/full-market success metrics with topology-neutral fields.
Keep old fields only during a short schema transition if generated clients
require it; this early-dev migration may remove them once all clients update.

### 9.1 MM metrics

Per market:

- economic bid present;
- economic ask present;
- two-sided state;
- quote prices and sizes in actor-authenticated diagnostics;
- typed skip/degradation reason;
- native mark source/quality and distance to guardrails.

Aggregates:

- any-side MM coverage;
- two-sided MM coverage;
- one-sided and skipped counts by reason;
- quote-receipt age;
- MM budget and inventory degradation counts.

### 9.2 Noise metrics

- configured, observed, and successfully submitted noise actors (`15/15`);
- noise orders and unique noise markets in the latest block;
- aggregate noise market coverage bps;
- naturally MM-marketable noise orders, measured post-submission;
- realized markets with noise fills;
- selected markets per actor histogram;
- markets with 1, 2, and 3+ noise actors;
- per-market blocks since last accepted noise order;
- inventory value and soft/hard-cap counts per actor;
- actor balance, PnL, and projected loss runway;
- partial epoch failure and retry counts.

### 9.3 Initial health bands

Measured over a rolling 100-block window unless stated otherwise:

| Metric | Healthy | Degraded |
| --- | ---: | ---: |
| MM two-sided coverage | `>=98%` | `95–98%` |
| Noise actors observed | `15/15` | `13–14/15` |
| Noise market coverage | `22–28%` | `15–22%` or `28–35%` |
| Realized fill coverage | `10–20%` | `5–10%` or `20–30%` |
| P99 per-market noise drought | `<=18 blocks` | `19–30` |

Single blocks are noisy and should not page merely for missing the rolling
band. Credential failure, stale universe generation, global MM budget failure,
or zero observed noise actors are immediate hard failures.

## 10. Capacity and failure handling

### 10.1 Expected load

Compared with the current three-all-market topology, solver order count falls
from roughly 1,030 baseline actor orders to roughly 471. HTTP submissions rise
from four actor epochs to sixteen, while actual noise order count falls from
618 to about 59.

The coordinator may parallelize account reads and submissions, but must bound
concurrency and keep a deadline before the target block. Universe and market
group metadata are cached by generation rather than fetched fifteen times per
block. Account inventories remain current enough for sell safety; the API is
authoritative and rejects stale oversells.

The receipt cache and health observation cache store sparse noise state. No
full `15 × market × retained_height` object graph is permitted.

### 10.2 Failure modes

| Failure | Required behavior |
| --- | --- |
| Legacy 1+3 credential file | Actor routes unready; explicit migration error |
| One noise epoch fails | Other epochs remain valid; bounded retry before cutoff; actor shown missing |
| Coordinator process fails | All noise coverage drops to zero and alerts; MM/user/LLM continue |
| MM diagnostic receipt missing | Noise behavior is unchanged; post-hoc crossing metric is unavailable |
| MM one-sided market | Noise may use valid available side but crossing-availability metric degrades |
| Native actor-only fills | Native mark does not move |
| Clearing vector with zero volume | Native mark does not move |
| Organic micro-print below threshold | Recorded but does not move mark |
| Group projection infeasible | Catalog/startup readiness fails closed |
| Anti-starvation state lost | Base randomness continues; degraded state metric; no invented history |
| Noise balance near floor | Inventory-reducing sells only, then typed local pause; no refill |
| Universe generation changes | Discard stale plans/state; refresh metadata; submit only new generation |
| API restart loses quote receipts | Crosses degrade for at most the recovery window; no guessed quote |
| Arena restart creates new LLM ids | Latest bot snapshot mapping replaces old active mapping; no hard-coded ranges |

## 11. Implementation work packages

### WP1 — Native mark and MM quote correctness

Primary files:

- `crates/sybil-polymarket/src/mm.rs`
- `crates/sybil-polymarket/src/mm/quotes.rs`
- `crates/sybil-polymarket/src/native.rs`
- `crates/sybil-polymarket/native_markets.json`
- matching-sequencer derived analytics and API conversion files

Deliver actor/organic fill attribution, robust qualifying updates, seed pull,
simplex/isotonic projection, quoteable-interior validation, and two-sided native
quotes. Disable the production 64-order MM rotation/cap.

### WP2 — Actor API and receipt contract

Primary files:

- `crates/sybil-api/src/state.rs`
- `crates/sybil-api/src/routes/actors.rs`
- `crates/sybil-api-types/src/request.rs`
- `crates/sybil-api-types/src/response.rs`
- Rust/Python/TypeScript generated clients

Deliver 1+15 credential validation, sparse noise epochs, 32-order cap, MM quote
snapshot, actor role metadata, and topology-neutral health fields. Preserve the
sequencer's full covered-universe invariant.

### WP3 — Noise coordinator v2.1

Primary files:

- `arena/live/noise_coordinator.py`
- `arena/live/runner.py`
- `arena/live/db.py`
- `arena/live/queries.py`
- `arena/tests/test_noise_coordinator.py`

Deliver independent RNG, personalities, 1.899% selection, anti-starvation,
inventory-aware action selection, group safety, committed-mark Lite-bounded
pricing, randomized size/aggressiveness, sparse payloads, and actor/account
snapshot metadata.

### WP4 — Provisioning and profiles

Primary files:

- `scripts/local-soak.sh`
- `scripts/local-soak-bootstrap.sh`
- `docker-compose.soak.yml`
- `docker-compose.prod.yml`
- `DEPLOY.md`
- `arena/.env.example`
- profile smoke tests and runbook

Generate/mount sixteen credentials, create fixed local accounts 1–16, split
the existing `$300k` local noise capital, and update fail-closed checks and
operator documentation.

### WP5 — Dev/Arena frontend truth

Primary files:

- `frontend/web/src/components/dev/accounts/accounts-view.tsx`
- `frontend/web/src/components/dev/overview/overview-view.tsx`
- `frontend/web/src/components/arena/arena-view.tsx`
- `frontend/web/src/lib/dev/derive.ts`
- Dev/Arena fetchers, types, derivations, and tests

Remove account reference valuation, add dynamic roles and outcome summaries,
split Sybil PnL cohorts, filter the top-25 table, and make Arena PnL LLM-only.

### WP6 — Architecture and operations docs

Update the maintained architecture notes for REST API, Bot Framework, Block
Lifecycle derived analytics, and Deployment Profiles. Regenerate OpenAPI and
both generated clients. Update this spec's status only after implementation and
verification.

## 12. Tests and simulations

### 12.1 Pure/unit tests

Noise:

- identical input/actor is deterministic;
- different actor ids change selection, direction, hole, and personality;
- 100,000 simulated market-blocks converge to 25% aggregate coverage within a
  tight statistical tolerance;
- per-actor selected count averages about 3.9 at 206 markets;
- group coverage never becomes complete for risk-increasing orders;
- sell probability is monotone in marked inventory and sells never exceed
  holdings;
- every price stays within the frontend Lite envelope around the committed mark;
- aggressive/passive selection converges to 60%/40% without MM quote input;
- every native YES/NO order respects complementary guardrails;
- all orders are IOC and each sparse intent has at most one order;
- anti-starvation improves the drought tail without lifting rolling coverage
  above its band.

Native/MM:

- zero-volume clearing prices do not update native marks;
- MM/noise-only fills do not update native marks;
- qualifying organic flow updates by no more than the configured step;
- absent evidence reverts toward the seed;
- 10,000 actor-only blocks cannot walk any mark to a guardrail;
- every categorical projected vector is bounded and sums to one;
- every threshold projection is bounded and isotonic;
- every healthy native range produces an integer bid and ask;
- inventory hard limits remove only the risk-increasing side with a typed
  reason;
- catalog loading rejects incoherent or non-quoteable policies.

Frontend/API:

- credential validation accepts exactly 1+15 and rejects partial/duplicate
  sets;
- sparse noise epochs reject duplicate/out-of-universe markets and remain IOC;
- MM epochs still require exact market coverage and a budget;
- quote snapshots expose economic sides without secrets;
- role joins are dynamic and deduplicated;
- `/dev/accounts` renders no Ref Portfolio, Ref PnL, or Ref Mark text;
- cohort PnL equals exact sums of API `pnl_nanos` fixtures;
- MM PnL cannot include noise accounts;
- Arena PnL excludes tagged noise snapshots;
- outcome filtering happens before top-25 truncation.

### 12.2 Full-universe simulation

Run at least 10,000 blocks with 206 markets, one MM, fifteen noise accounts,
and representative LLM/manual injections. Record:

- solver and block latency p50/p95/p99;
- actor payload size, request latency, and cutoff misses;
- MM two-sided, noise order, naturally marketable, and fill coverage;
- per-market drought distribution;
- MM/noise balances, PnL, and inventory distributions;
- native distance-to-seed and distance-to-guardrail distributions;
- categorical simplex and threshold isotonic violations (must be zero);
- rejects, oversells, group coverage failures, and budget degradations.

Compare deterministic reruns byte-for-byte at the noise plan level.

### 12.3 Local soak gate

The fixed-account migration conflicts with the existing 1+3 local volume. A
fresh isolated local-soak chain is mandatory; `local-soak clean` may be used
only after the user explicitly chooses to discard that local test state.

After rebuild, soak for at least two hours for functional acceptance and six
hours for the deployment candidate. The frontend, actor dashboard, Arena DB,
API health, Grafana, history memory, and raw account portfolios must agree on
roles and cohort PnL.

No production deployment proceeds if:

- any native market remains guardrail-pinned from system-only flow;
- rolling MM two-sided coverage is below 98% without a diagnosed external
  outage/risk reason;
- rolling noise coverage is outside 22–28%;
- fill coverage is outside 10–20% after calibration;
- any actor identity or capital total differs across credentials, API, Arena,
  and Dev UI;
- account PnL cards fail exact API reconciliation;
- history or API memory exhibits unbounded growth.

## 13. Migration and rollout

1. Normalize all categorical native seeds and add catalog validation.
2. Implement derived actor/organic fill attribution and robust native marks.
3. Implement two-sided boundary-safe MM quotes and diagnostic quote receipts.
4. Implement 1+15 credentials, sparse noise epochs, new health DTOs, and
   generated clients.
5. Implement the coordinator in shadow-plan mode and run statistical tests.
6. Implement Sybil-only frontend accounting and runtime role joins.
7. Provision a fresh local topology and run the 10,000-block simulation plus
   local soak gates.
8. Provision fifteen production noise accounts and a new secret file without
   increasing aggregate capital.
9. Deploy with noise disabled; verify MM/native marks.
10. Enable five noise actors, then fifteen, checking rolling coverage and
    capital after each stage.
11. Keep rollback capable of disabling noise immediately without disabling MM,
    LLM, or user trading.

The migration must never relabel existing LLM accounts as noise merely because
their ids overlap the new fresh-chain range. Actor role is credential-bound;
LLM identity is Arena-snapshot-bound.

## 14. Acceptance summary

The change is complete only when all of the following are true:

- native marks ignore zero-volume and system-only flow, remain coherent, and
  do not boundary-lock under 10,000-block stress;
- the MM submits accepted economic bids and asks on at least 98% of active
  markets in healthy rolling operation;
- exactly fifteen durable noise actors are ready, independently randomized,
  inventory-aware, group-safe, and IOC-only;
- rolling aggregate noise order coverage is 22–28%, 60% of selected orders are
  aggressive relative to the prior committed mark, and realized fill coverage
  is 10–20%;
- total noise capital is unchanged from the approved pre-migration total;
- Dev/Arena roles come from runtime metadata;
- every displayed account/actor PnL is Sybil-marked and reconciles exactly to
  API portfolio fields;
- `/dev/accounts` contains no account-level reference valuation and clearly
  identifies its top-25 slice.
