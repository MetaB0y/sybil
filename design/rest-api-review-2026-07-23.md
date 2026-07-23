---
tags: [api, review, devex, trading, openapi]
status: unreviewed
date: 2026-07-23
---

# Complete REST API review

## Verdict

The API has more routes than a trader should have to think about, but the core
trading model is not conceptually bloated. The most important simplification is
to present a small trader SDK and a separate operator surface, not to merge
ordinary orders and atomic order groups into one polymorphic endpoint.

Keep these as two resources:

- an **ordinary order** has its own time-in-force, may rest across blocks, is
  reserved independently, and can be cancelled by order, market, or account;
- an **atomic order group** is one signed all-or-nothing quote set with one
  shared capital bound, one revision, one exact block fence, and one lifecycle.

They should share signing, nonce, retry, error, and client-library conventions.
They should not share a request union that makes callers and generated clients
branch over two different validity models.

The current one-active-group-per-account limit is a good launch constraint.
Multiple simultaneous strategy groups are not needed today and would change
aggregate budget enforcement, witness semantics, persistence, conflict rules,
and operational load. It is tracked only as the explicitly far-future
[GitHub #226](https://github.com/MetaB0y/sybil/issues/226).

The review did find several real prelaunch contract issues. The highest-value
ones are:

1. make 64-bit JSON values exact in JavaScript;
2. make OpenAPI truthfully describe service authentication and middleware
   errors;
3. expose owner-authorized signing/nonce state so every bot does not invent its
   own bootstrap and coordination policy;
4. make mutation bodies consistently reject unknown fields;
5. correct the authorized-action audit response, which still has a singular
   `order_id` even when one signed cancellation affects many orders;
6. organize generated clients by product domain and trust tier instead of raw
   Rust module names.

No additional API redesign was implemented as part of this review. These
recommendations intentionally require an explicit decision because several are
wire changes.

## Scope and evidence

This is a static review of the generated dev-inclusive OpenAPI document, all
four mounted route registries, shared request/response DTOs, the Rust client,
the frontend transport/signing layer, the two first-party market makers, and
the API conformance gate.

Current generated inventory:

| Item | Count |
|---|---:|
| OpenAPI paths, including dev | 73 |
| Operations, including dev | 79 |
| Operations in a locked non-dev runtime | 74 |
| Component schemas | 123 |
| GET / POST / PUT operations | 48 / 30 / 1 |
| Public route mounts, including OpenAPI and metrics | 41 |
| Owner-read mounts | 11 |
| Service mounts | 24 |
| Dev-only mounts | 5 |

The 74-operation runtime surface is not the same thing as a 74-operation trader
API. Most operations are product reads, bridge/operator integration, proof/DA
plumbing, or dev diagnostics. The directly relevant signed trader surface is
five operations:

```text
POST /v1/orders/signed
POST /v1/orders/cancel/signed
POST /v1/order-groups/signed
POST /v1/order-groups/replace/signed
POST /v1/order-groups/cancel/signed
```

The ordinary service batch route, `POST /v1/orders`, is an operator capability,
not an alternative public authentication model.

## Why one mega order endpoint would be worse

A superficially smaller API could accept a union such as:

```text
POST /v1/orders/signed
  action = ordinary | submit_group | replace_group | cancel_group
```

That would remove path names while moving the same concepts into a large tagged
body. It would also make the ordinary-order endpoint depend on group revision,
shared-capital, exact-next-block, and all-or-none error concepts. Generated
clients would expose a broad union, authorization domains would still remain
different, and middleware/telemetry would have to recover the hidden operation
type from the body.

The better minimum is one common client object with five clearly named methods:

```text
TradingClient
  submit_order(order, tif)
  cancel_orders(scope)
  submit_group(group)
  replace_group(group, expected_revision)
  cancel_group(group_id, expected_revision)
```

The methods share one signer, one nonce coordinator, one durable ambiguity
journal, one error taxonomy, and one retry policy. The wire resources remain
honest about their different state machines.

This matches the useful part of established practice:

- Polymarket tells market makers to post through its normal CLOB REST API and
  gives ordinary traders single, multiple, market, and all-order cancellation
  operations. Sybil should likewise keep normal MM orders on the normal order
  path; the extra Sybil group path exists only for the additional atomic shared
  capital guarantee. See the
  [Polymarket MM overview](https://docs.polymarket.com/market-makers/overview)
  and [cancellation model](https://docs.polymarket.com/trading/orders/cancel).
- Coinbase Advanced Trade has one order-creation resource and a distinct batch
  cancellation operation. Its FIX session is common for traders and market
  makers, while new, batch, cancel, and replace remain distinct message types.
  Unification is in session/authentication/client workflow, not one command
  shape. See the
  [Advanced Trade endpoint inventory](https://docs.cdp.coinbase.com/coinbase-business/advanced-trade-apis/rest-api)
  and [FIX order-entry messages](https://docs.cdp.coinbase.com/exchange/fix-api/order-entry-messages/order-entry-messages5).
- MEV-Boost relays expose specialized proposer, builder, and data APIs because
  a slot-bound atomic payload has different validation and replacement
  semantics from an ordinary transaction. Flashbots also notes that enabling
  builder submission cancellation adds validation cost. The analogy is not
  exact, but it supports keeping a validity-distinct atomic object explicit.
  See the
  [MEV-Boost relay architecture](https://github.com/flashbots/mev-boost-relay).

## Trader and market-maker scenarios

### Ordinary trader with GTC

1. The client signs and submits one ordinary GTC order.
2. The owner-scoped orders read shows its live order ID and remaining quantity.
3. The user cancels that order, every order in the displayed event's exact
   market set, or all of their resting orders.
4. The signed cancellation returns the exact sorted order IDs actually removed.

The API now supports this completely. The visible frontend still exposes only
its existing single-order cancellation interaction; no UI expansion was made
in the implementation pass.

Recommended UI:

- put **Cancel** on every open GTC row;
- provide one **Cancel event orders** action after expanding the event to exact
  market IDs client-side;
- provide **Cancel all open orders** in the portfolio/order-management view;
- show the scope and affected-order count in confirmation copy;
- show a temporary `Cancelling…` state, then reconcile from the owner orders
  read;
- do not claim success from optimistic removal alone.

All three interactions should call the same signed cancellation method. There
is no need for three new backend endpoints.

### Maker cold start

1. Load or atomically create the P256 trading key.
2. Load the lifecycle journal and require the same genesis and account.
3. Ensure the exact signer is registered on the account.
4. If the journal has a pending request, retry those exact bytes before making
   a new quote decision.
5. Otherwise submit revision zero for the exact next block.

The current first-party makers now do this. The journal is not a strategy
framework; it is the minimum state required to distinguish “the server did not
accept the request” from “the server accepted it but the response was lost.”

### Normal quote refresh

- No active group: submit revision zero.
- Same active group: replace `expected_revision` with the next revision.
- Accepted response: durably checkpoint the active identity and returned order
  IDs.
- Authoritative 4xx: clear the pending attempt and let policy decide a new
  action.
- Transport error, timeout, or 5xx: retain and retry the exact signed request.

### Restart after an ambiguous response

The maker must not invent a later nonce or revision until it has retried the
pending bytes. An exact server retry returns the recorded outcome. This is why
the durable journal is necessary even with idempotent server handling.

### Deliberate shutdown

The maker attempts a signed cancellation of the exact active group revision.
If the response is ambiguous, it retains the signed request for the next
startup. One-shot block expiry still prevents a stale quote set from becoming
long-lived.

### Concurrent strategies

Today, two independent strategies sharing one account will contend on:

- the account-global signing nonce;
- the one active group identity;
- group revision;
- the shared account capital/reservation state.

That inconvenience is intentional at launch. The clean solution today is one
supervising quoting policy per maker account, or separate funded accounts. A
server-side multi-strategy group registry would add protocol complexity before
there is demonstrated product need.

## Current maker DevEx: good parts and pain points

### Clear and useful

- The group is a real resource with an opaque client-chosen ID and explicit
  revision.
- Submit, replace, and cancel have distinct signature domains.
- The exact next-block fence prevents accidentally live stale quotes.
- One shared integer capital limit describes the actual atomic economic
  guarantee.
- Exact retries survive restart and distinguish conflicts.
- Stable 409 classes make stale nonce/revision and active-group conflicts
  machine-readable.
- Ordinary quote/order cancellation and atomic-group cancellation now use the
  same account key registry and signer types.

### Inconvenient or confusing

1. **There is no owner signing-state read.** A fresh bot cannot ask for the last
   accepted trading nonce. First-party makers seed from wall-clock
   milliseconds and then persist monotonic state. That is safe for one durable
   agent, but poor for recovery on another machine and awkward for multiple
   signers.

2. **The nonce is account-global.** Browser activity and multiple agents can
   race. A server read does not eliminate races, but it makes conflict recovery
   explicit and lets an SDK implement compare-and-advance behavior.

3. **The caller must understand ambiguous outcomes.** Retrying a newly signed
   request after a timeout is wrong. This should be solved once in the SDK, not
   rediscovered by every maker.

4. **Exact block fencing is latency-sensitive.** Slow WebAuthn interaction is a
   poor fit for continuous one-block quoting. Raw P256 agent keys are the
   appropriate maker credential; WebAuthn remains useful for human actions and
   key management.

5. **Replacements carry the full group.** This costs bandwidth and signature
   work, but keeps authorization and replay canonical. Delta replacement is not
   justified until group sizes or measured request load make it expensive.

6. **Order IDs change with a replacement.** Client code should treat
   `(group_id, revision)` as the stable lifecycle identity and returned order
   IDs as that revision's execution details.

7. **Complete-set compaction is still a service operation.** It is economically
   separate from retained-cash quoting and does not fit the group's shared
   capital budget. This is an explicit first-party privilege boundary, not a
   second quote-submission path.

### Potentially expensive

- one signature verification, admission pass, and acknowledged WAL record per
  accepted lifecycle update;
- full-group serialization and validation on every replacement;
- a failed block fence forces a new signed revision attempt;
- global per-account and global HTTP/actor flow limits can become the maker's
  throughput ceiling before CPU does.

These costs are bounded and observable. Do not add delta updates, multiple
active groups, partial group acceptance, or server-managed strategies until
measurements show a real problem.

## Findings and recommendations

### P0 — Exact 64-bit JSON policy

The generated contract advertises many `u64` values as JSON numbers with a
maximum of `18446744073709551615`. This includes account/order IDs, nonces,
block heights, timestamps, quantities, bridge units, and counters. JavaScript
cannot exactly represent every such integer above `2^53 - 1`.

The API already solved this correctly for every `*_nanos` value by using exact
decimal strings. Apply the same explicit classification to all other 64-bit
fields:

- protocol-exact/unbounded values become canonical decimal strings;
- retained JSON numbers receive an enforced semantic maximum no greater than
  `2^53 - 1`.

This is already tracked in
[GitHub #177](https://github.com/MetaB0y/sybil/issues/177). It is the most
important broad wire decision to make before launch.

### P0 — OpenAPI trust and error fidelity

The generated document references `bearer_service` on the authenticated v1
block WebSocket operation, but only defines the `bearer_read` security scheme.
Most service operations do not annotate service security at all even though the
entire service router is bearer-gated.

Middleware errors are also under-described:

- all five order writes can return 429 from the shared order limiter, but their
  OpenAPI responses omit 429;
- DA reads can return 429 from rate/concurrency limiting, but omit it;
- service routes have uneven 401/403 response declarations despite one common
  middleware boundary.

Recommendation: derive `security`, standard 401/403 responses, trust-tier
metadata, and middleware 429 responses from the declarative route registries
and middleware policy. Do not repeat them manually on dozens of handlers.

This is contract correctness, not cosmetic cleanup, and should be fixed before
publishing the generated SDK as stable. Tracked in
[GitHub #228](https://github.com/MetaB0y/sybil/issues/228).

### P0 — Authorized-action result is still singular

`SystemEventResponse.client_action_authorized` requires one `order_id`.
Ordinary cancellation can now resolve to many order IDs, while an atomic group
has a stable group ID and revision. Choosing one order ID loses audit meaning
and invites clients to infer the wrong scope.

Recommendation: make the response action-specific, for example:

```text
action =
  submitted_order { order_id }
  cancelled_orders { order_ids }
  submitted_group { group_id, revision, order_ids }
  replaced_group { group_id, revision, order_ids }
  cancelled_group { group_id, revision }
```

The wire response should be projected from the validity-bound action rather
than maintaining an independent lossy representation. Because this touches a
public response and possibly historical projection, it needs an explicit
migration decision. Tracked in
[GitHub #230](https://github.com/MetaB0y/sybil/issues/230).

### P1 — Owner signing-state endpoint

Add one owner-authenticated read such as:

```text
GET /v1/accounts/{id}/signing-state
```

Return at least:

- genesis hash;
- last committed/accepted trading nonce, with semantics named precisely;
- current active group ID and revision if present;
- possibly current keys/events digests, replacing the narrow public
  `keyop-state` read only if privacy and WebAuthn bootstrapping still work.

The response is recovery/coordination information, not permission to skip a
durable client journal. It materially improves bot recovery and makes nonce
conflicts diagnosable.

Do not introduce server-issued nonce leases or per-strategy nonce lanes yet.
Those are more complex protocol choices and are unnecessary for launch.
Tracked in [GitHub #233](https://github.com/MetaB0y/sybil/issues/233).

### P1 — Consistently closed mutation requests

Only 6 of 28 referenced JSON mutation schemas explicitly set
`additionalProperties: false`; 22 are open or unspecified. The newly added
onboarding, reference-price, cancellation, and group DTOs are strict, while
older order, account, key, bridge, and market mutations generally accept
misspelled or stale fields.

For an API used heavily by generated code and AI agents, silently ignored
fields are especially dangerous. A request that says `expire_at_block` instead
of `expires_at_block` should fail, not submit with a default.

Recommendation:

- deny unknown fields on every external command DTO;
- preserve open maps only for fields that are deliberately maps;
- add a contract test that every JSON command schema is closed;
- keep response schemas forward-additive unless a consumer explicitly needs a
  sealed union.

This is a behavior change but a desirable prelaunch one.
Tracked in [GitHub #232](https://github.com/MetaB0y/sybil/issues/232).

### P1 — Small generated trader surface

OpenAPI tags are currently raw module identifiers such as `routesorders`,
`routesaccounts`, and `routessystem`. The full generated client also exposes
public, owner, service, and dev operations together.

Recommendation:

- use stable product tags: `Trading`, `Accounts`, `Markets`, `History`,
  `Bridge`, `Blocks`, `Validity`, `Operator`, and `Development`;
- attach an `x-sybil-trust-tier` extension from the route registry;
- make the first-party SDK expose `TradingClient`, `OwnerClient`, and
  `OperatorClient` facades;
- generate public/owner documentation by default and put operator/dev
  operations in explicitly named sections.

Keep one runtime route registry and one canonical schema source. Do not fork
DTOs or manually maintain separate public/operator OpenAPI files.
Tracked in [GitHub #231](https://github.com/MetaB0y/sybil/issues/231).

### P1 — Cancellation UI

The backend cancellation model is now sufficient for ordinary traders and
makers. The remaining product work is one coherent UI, not more API:

- per-order cancel;
- event-scoped cancel using exact market IDs;
- account-wide cancel-all;
- clear confirmation of scope;
- pending/error/success reconciliation;
- GTC visibility in portfolio/open-orders views.

No frontend changes were made in this implementation pass, as requested.
Tracked in [GitHub #234](https://github.com/MetaB0y/sybil/issues/234).

### P2 — Collection and pagination consistency

Thirteen GET responses are top-level arrays without an OpenAPI `maxItems`.
Some handlers are runtime-limited, but `markets`, `market summaries`, market
groups, feeds, owner orders, and several operator collections are returned as
whole lists.

Pagination currently uses several domain-specific forms:

- `after` cursor for fills;
- `before` cursor for account events;
- `before_height` for blocks and price points;
- `before_ms` for candles;
- `offset` for market search.

Different directions and time domains are legitimate. The accidental part is
inconsistent envelope naming and missing advertised bounds.

Recommendation:

- first document/enforce a maximum on every collection;
- keep cursor direction and unit explicit (`after_cursor`, `before_height`,
  `before_ms`) instead of inventing one ambiguous generic cursor;
- migrate unbounded product collections to `{ items, next_* }` envelopes before
  public scale requires it;
- leave bounded key lists and dev diagnostics simple unless measurements justify
  pagination.

This is not required for the first launch if service-created market/account
stock remains deliberately small.
Tracked as after-launch scale work in
[GitHub #229](https://github.com/MetaB0y/sybil/issues/229).

### P2 — Error details should remain narrow but typed

The stable error envelope is good:

```text
{ code, error, details? }
```

Keep `code` as the machine branch and `error` as human diagnostics. Do not make
one enormous universal details object. Instead, introduce a small tagged or
code-correlated detail union when clients demonstrably need structured fields
such as:

- current/expected nonce;
- current/expected revision;
- group ID;
- affected order IDs;
- retry-after seconds.

The existing structured market ID/status precedent is sound.

## Schemathesis gate

Schemathesis is property-based API testing driven by OpenAPI. Instead of
maintaining only hand-written examples, it generates many schema-valid and
schema-invalid requests, sends them to a real disposable server, and verifies
that transport behavior agrees with the contract.

Sybil pins Schemathesis `4.23.0` and runs it through
`just api-contract-check`. The gate:

1. builds and starts a disposable dev API on an ephemeral port/data directory;
2. validates that every allowlist operation still exists and that every
   exception has an owner, durable reason, phase, and non-expired date;
3. runs deterministic positive fuzzing;
4. runs deterministic negative fuzzing;
5. checks status-code conformance, content type, response schema, acceptance of
   positive data, and rejection of negative data;
6. kills the server and deletes only its validated temporary directory.

Current run parameters are deliberately bounded:

| Setting | Current value |
|---|---:|
| Examples per operation/mode | 5 |
| Workers | 1 |
| Shrinking | disabled |
| Maximum failures | 20 |
| Positive allowlist entries | 22 |
| Negative allowlist entries | 2 |
| Allowlist expiry | 2026-10-31 |

The 22 positive exclusions are mostly stateful signed mutations: a random
schema-valid key is not necessarily a P256 point, and a random signed request
does not have a previously created/funded account, current nonce, matching
genesis, and valid signature. The two negative exclusions are WebSockets,
because this gate sends ordinary HTTP rather than performing an upgrade.

What the gate is good at:

- catching OpenAPI drift from actual JSON behavior;
- finding missing bounds and unexpected 500s;
- proving malformed path/query/body input uses the common error envelope;
- exercising generated combinations humans would not enumerate;
- preventing permanent “temporary” exclusions through expiry checks.

What it does **not** prove:

- signature correctness or canonical-byte agreement;
- sequencer/WAL/validity invariants;
- multi-step account → fund → market → order → block workflows;
- WebSocket message compatibility;
- meaningful positive success for the 22 excluded stateful operations;
- production middleware/profile behavior merely because dev mode passed.

The next improvement is not more random examples alone. Add dependency-aware
stateful sequences and OpenAPI links, as already tracked in
[GitHub #182](https://github.com/MetaB0y/sybil/issues/182). Keep handwritten
known-answer and crash/replay tests for cryptographic and persistence
boundaries. The WebSocket message contract remains separately tracked in
[GitHub #183](https://github.com/MetaB0y/sybil/issues/183).

## Deliberate duplication to keep

- `/v1/markets` and `/v1/markets/summary`: one is the full product object and
  one is an intentionally smaller polling read model. Measure before removing.
- `/v1/blocks/ws` and `/v2/blocks/ws`: v1 is the authenticated canonical stream;
  v2 is the privacy-filtered public tape. They are different trust products.
- service ordinary orders and public signed ordinary orders: same sequencer
  command family, different authorization/trust boundary.
- signed bridge withdrawals and service bridge withdrawals: self-custodied
  authorization versus operator integration.
- group submit/replace/cancel: separate lifecycle transitions with separate
  signature domains and idempotency rules.

## Accidental duplication or verbosity to remove

- repeated signer/auth/assertion fields without one SDK abstraction;
- hand-repeated security and middleware response annotations;
- raw module tags in the generated API;
- independently phrased client retry/conflict logic;
- open request DTOs with silent unknown fields;
- lossy singular authorized-action results;
- whole-list responses with no contract bound;
- broad generated clients that make normal traders see operator/prover/dev
  methods.

## Recommended decision set

### Approve before launch

1. Keep ordinary orders and atomic groups as separate resources.
2. Implement [#177](https://github.com/MetaB0y/sybil/issues/177) and coordinate
   its wire migration.
3. Fix OpenAPI service security and middleware error fidelity from route policy.
4. Add owner signing-state/nonce discovery.
5. Close all mutation DTOs to unknown fields.
6. Replace the singular authorized-action response with an action-specific
   result.
7. Rename tags and expose small trader/owner/operator SDK facades.
8. Implement the ordinary cancellation UI after the API decisions above land.

### Good after launch

- dependency-aware Schemathesis workflows
  ([#182](https://github.com/MetaB0y/sybil/issues/182));
- machine-readable WebSocket message schema
  ([#183](https://github.com/MetaB0y/sybil/issues/183));
- pagination/envelopes for collections that approach measured limits;
- structured error details for demonstrated SDK recovery needs.

### Explicitly defer

- multiple simultaneous atomic groups per account
  ([#226](https://github.com/MetaB0y/sybil/issues/226));
- server-managed maker strategies;
- delta group replacement;
- partial acceptance of atomic groups;
- nonce leasing or per-strategy nonce lanes;
- merging all trading actions into one request union;
- an ordinary order-replace protocol before queue-priority semantics are needed.

## Approval-sensitive consequences

The implementation preceding this review already makes these intentional
changes:

- public MM “bundle” terminology and routes become atomic order groups;
- witness format is v16 and requires a coordinated fresh-genesis deployment;
- ordinary signed cancellation can affect one order, many exact orders, exact
  markets, or all owned resting orders in one atomic action;
- first-party makers now persist a private P256 trading key and lifecycle
  journal beside their existing durable state;
- the one-active-group-per-account constraint remains;
- visible frontend cancellation UX is unchanged.

The review recommendations would add further wire changes if approved,
especially exact 64-bit strings, strict unknown-field rejection, a signing-state
response, and a corrected authorized-action response. They should be landed as
explicit migrations rather than hidden inside cosmetic API cleanup.
