---
tags: [audit, code-quality, testing, api, arena, frontend]
layer: cross-cutting
status: current-audit
date: 2026-07-17
last_verified: 2026-07-17
---

# Stateful API and client-conformance audit

Date: 2026-07-17  
Cluster: OpenAPI, HTTP runtime, WebSocket lifecycle, and generated/handwritten
Rust, Python, and TypeScript clients  
Primary technique: schema-derived contract testing plus dependency and
cross-runtime lifecycle analysis

## Verdict

The documented route inventory and all generated surfaces now agree on the
canonical full API: 69 paths, 75 uniquely identified operations, and 116
component schemas. The Python generator previously booted a production-profile
server and silently omitted all five dev operations; it now renders the same
deterministic full document as the frontend generator and emits 75 operation
modules.

The audit found three unsafe WebSocket client behaviors:

- Python reconnect replay was indistinguishable from live data, so `BaseAgent`
  and the live analyst could run strategies or LLM analysis for historical
  blocks.
- The browser dropped its cursor after `retention_gap` and automatically
  reconnected without replacing stale caches from REST.
- The Rust client deserialized the complete v2 enum before checking the
  envelope version/type, contradicting the protocol's forward-compatibility
  rule.

All three are fixed with regression tests. Python now exposes an event stream
with a replay flag and explicit boundary; trading agents update canonical state
during replay but do not invoke strategies. The frontend fail-stops after a
retention gap, clears derived state, hydrates a fresh REST snapshot, and only
then resumes at `snapshot_height + 1`. The Rust client reads the envelope
header first and ignores unknown versions or additive message types while
remaining strict for malformed known messages.

Schema-derived testing also found a broader API rejection-contract gap.
Framework extractor errors are often undocumented plain-text `400`/`422`
responses, and several string schemas are looser than their runtime semantic
validators. This needs one API-wide policy rather than endpoint patches and is
tracked in [GitHub #181](https://github.com/MetaB0y/sybil/issues/181).
Dependency-aware state sequences and a machine-readable WebSocket message
contract are tracked separately in
[GitHub #182](https://github.com/MetaB0y/sybil/issues/182) and
[GitHub #183](https://github.com/MetaB0y/sybil/issues/183).

## Evidence boundary

The audit covered:

- the full `sybil-api` route registries and generated OpenAPI document;
- OpenAPI request/response schemas and examples in `sybil-api-types`;
- runtime behavior on a disposable in-memory dev server;
- Rust `sybil-client` public block-stream decoding;
- the vendored generated Python substrate and handwritten ergonomic client;
- Arena bot and analyst consumption of reconnect replay;
- frontend generated HTTP types, handwritten WebSocket envelopes, hydration,
  Zustand state, and reconnect behavior; and
- architecture and data-map documentation for those boundaries.

The disposable server was not a live deployment. Optional history, L1, prover,
and persistent-storage services were absent, so their `503`/precondition
results were classified rather than treated as functional failures. The
Schemathesis runs were diagnostic campaigns, not a new passing CI gate. No
production state, guest executable, signing domain, or deployment was changed.

## Architecture context read

The review used the root and crate guidance for `sybil-api-types`,
`sybil-api`, `sybil-client`, Arena, and the frontend, together with:

- `REST API`
- `WebSocket Block Stream`
- `P256 Authentication`
- `Block Data Boundaries`
- `Python SDK`
- `Bot Framework`
- `Deployment Profiles`

The governing constraints were exact integer wire values, distinct
public/service/dev route profiles, public-v2 privacy, explicit replay
boundaries for side effects, forward-compatible versioned envelopes, and
fail-closed recovery when retained history is unavailable.

## Research basis and method

- The [RESTler paper](https://www.microsoft.com/en-us/research/wp-content/uploads/2021/03/RESTler.pdf)
  shows why producer/consumer dependencies and runtime feedback are necessary
  to explore stateful REST behavior rather than isolated endpoint shapes.
- [Schemathesis stateful testing](https://schemathesis.readthedocs.io/en/latest/guides/stateful-testing/)
  uses OpenAPI links to derive operation sequences. The Sybil document had no
  links, so isolated schema generation was kept separate from a future curated
  lifecycle harness.
- The [OpenAPI Link Object guide](https://learn.openapis.org/specification/links.html)
  informed the deferred producer/consumer work rather than inventing a second
  dependency format.
- [AsyncAPI message validation](https://www.asyncapi.com/docs/guides/message-validation)
  supports validating event payloads independently of transport. It motivated
  a separate issue for the currently handwritten Python/TypeScript WebSocket
  contract.

The procedure was:

1. Render and validate the canonical full OpenAPI document.
2. Inventory every method/path, schema, operation ID, link, and generated
   Python operation module.
3. Generate positive and negative HTTP cases against a disposable dev server.
4. Classify failures into false documentation, framework rejection policy,
   local schema looseness, and legitimate state/service preconditions.
5. Trace replay, lag, retention, unknown-version, and unknown-type behavior
   through the Rust, Python, and browser clients.
6. Fix bounded mismatches and add lifecycle tests in each affected runtime.
7. Regenerate clients from one canonical document and pin the inventory.
8. File coherent API-wide or message-contract work instead of adding divergent
   client workarounds.

## Contract inventory

| Surface | Before | Final evidence |
|---|---|---|
| Canonical full OpenAPI | Valid 3.1 document; 69 paths, 75 operations, 116 schemas | Same counts; all operation IDs present and unique; route/spec bidirectional pin passes |
| Runtime profiles | Production document omitted dev routes by design | Profile distinction retained |
| Generated Python operations | 70 modules because regen scraped production profile | 75 modules from deterministic full renderer; five dev modules pinned by import test |
| Generated TypeScript HTTP schema | Full canonical renderer | Full renderer retained; `types:check`, TypeScript, and frontend suite pass |
| OpenAPI links | 0 | Still 0; dependency work open as #182 |
| `GET /v1/blocks/{height}` | Runtime returned `410 RETENTION_GONE`; spec documented only 200/404 | 410 JSON response documented and drift-pinned |
| Public-key examples | Truncated non-key string | Valid compressed P-256 base-point example, parsed in test |
| Rust public WebSocket | Unknown version/type became protocol/JSON error | Two-stage header decode ignores unknown version/type; known messages remain strict |
| Python public WebSocket | Replay boundary discarded by `stream_blocks()` | `stream_block_events()`, replay flags, explicit boundary, all/live convenience views |
| Arena side effects | Bots and analyst could act on replay | Bots observe replay state without strategy calls; analyst consumes live-only view |
| Browser retention gap | Cursor cleared and reconnect scheduled without snapshot | Failed state until REST reset/hydration calls `recoverFromSnapshot()` |
| Browser initial replay classification | REST seed did not establish replay watermark | Seeded `H+1` handshake is `replaying` until `replay_complete` |

The OpenAPI operation distribution was:

| Tag | Operations |
|---|---:|
| accounts | 19 |
| aggregates | 3 |
| blocks | 5 |
| bots | 2 |
| bridge | 10 |
| DA | 2 |
| events | 2 |
| feeds | 2 |
| leaderboard | 1 |
| markets | 15 |
| orders | 6 |
| proofs | 1 |
| prover | 2 |
| system | 5 |

## Findings and disposition

| ID | Severity | Finding | Disposition |
|---|---|---|---|
| AC-1 | High | Python reconnect replay could invoke bot strategies, submit historical orders, and repeat analyst work. | Fixed with replay-aware events and side-effect-safe consumers. |
| AC-2 | High | Browser retention recovery could combine future live blocks with stale REST-derived caches. | Fixed with fail-stop, store reset, fresh hydration, and explicit recovery. |
| AC-3 | Medium | Rust rejected future envelope versions/types before applying the documented ignore policy. | Fixed with header-first decoding and strict-known/ignore-unknown tests. |
| AC-4 | Medium | Python SDK generation depended on runtime profile and omitted five documented dev operations. | Fixed by using `sybil-openapi`; regenerated and pinned all 75 modules. |
| AC-5 | Medium | `GET /v1/blocks/{height}` emitted an undocumented 410 retention response. | Fixed in the OpenAPI annotation and drift test. |
| AC-6 | Low | The public-key request example was not a valid P-256 point and made generated positive cases fail. | Fixed with a valid compressed point and semantic example test. |
| AC-7 | High | Axum extractor rejections and local string constraints do not match the declared error/schema contract. | Open as #181, Project 1 Todo/Backlog/High. |
| AC-8 | Medium | Zero OpenAPI links prevent automatic dependency-aware lifecycle generation. | Open as #182, Project 1 Todo/Backlog/Medium. |
| AC-9 | Medium | WebSocket messages lack a generated or cross-runtime-validated machine contract. | Open as #183, Project 1 Todo/Backlog/Medium. |
| AC-10 | Low | Frontend data-flow documentation still named the retired public v1 stream. | Fixed to `/v2/blocks/ws`. |

## Schema-derived runtime evidence

The full document passed `openapi-spec-validator`. A deterministic disposable
server then received two Schemathesis 4.23.0 campaigns:

- positive generation: 549 cases, 34 reported failures;
- negative generation: 74 cases, 35 unique reported failures.

The raw totals are not interpreted as 34 or 35 product bugs. Classification
showed:

- one false example, fixed in this cluster;
- one false response inventory, fixed in this cluster;
- systematic plain-text/undocumented extractor rejections, tracked in #181;
- schemas that express only `string` where the runtime requires decimal,
  fixed-length hex, or a bounded local language, tracked in #181; and
- expected state/profile failures where history, bridge, persistence, or a
  producer resource was unavailable.

This distinction matters: teaching Schemathesis to ignore every non-200 result
would erase the contract signal, while treating a missing optional history
service as an API implementation defect would be equally misleading.

## Implemented changes

- Documented the block-retention 410 response and pinned it.
- Replaced invalid P-256 examples and parse them in the drift suite.
- Added complete/unique operation-ID validation.
- Made Rust public-stream decoding header-first and forward-compatible.
- Added Python block/replay event types, all-block and live-only views, and
  replay-safe bot/analyst consumption.
- Removed a stale Python pending-order workaround comment that contradicted the
  current effective-expiry contract.
- Made browser retention recovery an explicit REST snapshot state transition.
- Correctly classified REST-seeded browser catch-up as replay.
- Replaced server boot/scrape in Python SDK regeneration with the canonical
  full OpenAPI renderer and regenerated the five missing modules.
- Updated SDK and frontend data-flow documentation.

## Verification

Passed:

- `openapi-spec-validator` on the rendered OpenAPI 3.1 document;
- `cargo test -p sybil-client` — 7 tests;
- `cargo test -p sybil-api --test openapi_drift` — 9 tests;
- `cargo test -p sybil-api --test ws_integration` — 7 tests;
- targeted Arena pytest — 50 tests;
- full Arena pytest — 318 tests, plus Ruff;
- full frontend Vitest — 375 passed, 1 skipped;
- frontend generated-type freshness and scenario checks;
- frontend TypeScript `--noEmit`;
- frontend ESLint;
- deterministic Python SDK regeneration with 75 operation modules; and
- strict Clippy for API types, API, and Rust client;
- Rust and Python formatting plus frontend Prettier; and
- `just docs-check`, including strict site build.

No deployment was performed.

## Residual risk and completion

The bounded cluster is complete for the concrete route/generator inventory and
replay/retention client failures. It does not claim that arbitrary stateful API
sequences conform: #181 must make rejection schemas executable, #182 must add
producer/consumer sequences, and #183 must make the event contract
cross-runtime and machine-readable.

Future work should use those issues rather than weakening the current tests or
adding language-specific exceptions.
