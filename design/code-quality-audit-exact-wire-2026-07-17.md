---
tags: [audit, code-quality, api, wire-format, testing, codex]
layer: api
status: current-audit
date: 2026-07-17
last_verified: 2026-07-17
---

# Exact integer wire-contract audit

Date: 2026-07-17  
Cluster: cross-language API and generated-client fidelity  
Scope: Rust REST/WebSocket DTOs, OpenAPI, the Rust client, the generated Python
SDK and thin Arena client, and the TypeScript frontend

## Verdict

The nanodollar wire contract is now internally consistent and executable:

- Rust keeps `u64`/`i64` nanodollar values internally but emits exact decimal
  JSON strings.
- Deserializers accept decimal strings and legacy integer JSON tokens, while
  rejecting floating-point tokens and out-of-range values.
- Every public numeric nanodollar field has a `*_nanos` name. Five semantic
  aliases that escaped the original suffix-based migration were corrected.
- OpenAPI describes 108 nanodollar component properties and three manually
  declared query parameters as strings. A recursive drift test pins scalars,
  nullable scalars, maps, arrays, and named path/query parameters.
- Generated Python and TypeScript types were regenerated from that contract.
- The frontend refuses unsafe legacy JavaScript numbers instead of converting
  an already rounded value to `bigint`.

This closes the nanodollar-specific cluster. It does **not** establish exact
interchange for every other 64-bit protocol integer. Quantities, nonces, token
base units, identifiers, heights, cursors, and timestamps still need an
explicit string-or-bounded-number policy; that follow-up is
[GitHub #177](https://github.com/MetaB0y/sybil/issues/177).

## Evidence boundary

The working change already contained a broad nanodollar string migration when
this audit began. This audit treated that change as shared, in-flight work:
it reconciled the generated artifacts and tests, traced semantic values across
the language boundaries, repaired omissions, and strengthened the executable
invariant. It did not attribute all 89 changed files to this audit.

The code review was static plus local execution. No deployment or live-network
claim is made.

## Why this cluster was selected

JSON, OpenAPI, Rust, Python, and JavaScript do not share one integer model:

- [RFC 8259 section 6](https://datatracker.ietf.org/doc/html/rfc8259#section-6)
  says implementations agree exactly on integers only within
  `[-(2^53)+1, (2^53)-1]`.
- [RFC 7493 section 2.2](https://www.rfc-editor.org/rfc/rfc7493.html#section-2.2)
  recommends a string representation when an application needs greater
  precision or range than interoperable JSON numbers provide.
- The [OpenAPI `uint64` format
  registry](https://spec.openapis.org/registry/format/uint64) recommends strings
  for values outside the 53-bit range. A format annotation alone does not force
  generators or JavaScript runtimes to preserve the value.

Sybil explicitly treats integer prices, settlement, commitments, and
verification as protocol truth. A JSON token that silently rounds before
signing, display, or resubmission therefore violates a repository invariant,
even if one particular deployment is unlikely to reach the maximum Rust value.

## Review method

The audit used a repository-aware contract path rather than relying on a
free-form AI review:

1. Read the crate guidance and the REST, WebSocket, authentication, block-data,
   and deployment architecture notes.
2. Inventory integer fields in `sybil-api-types`, including values whose names
   did not originally contain `nanos`.
3. Trace request and response mappings through `sybil-api`, `sybil-client`,
   Arena's thin wrapper and generated SDK, and frontend parsing.
4. Compare the generated OpenAPI schema with the mounted route declarations.
5. Exercise values at and beyond JavaScript's safe-integer boundary and at Rust
   extrema.
6. Turn the invariant into deterministic schema and runtime tests, regenerate
   both clients, and run the focused language gates.

This ordering follows the strongest evidence found in the research:

- Repository-aware, validator-backed data-flow review is more useful than
  context-free suggestions; [RepoAudit](https://arxiv.org/abs/2501.18160)
  combines repository paths with validation rather than accepting a model's
  initial claim.
- Schema-derived and dependency-aware API testing reaches contract failures
  that examples miss. The [Schemathesis
  evaluation](https://arxiv.org/abs/2112.10328) and
  [RESTler](https://arxiv.org/abs/1806.09739) support generated boundary cases
  and stateful request sequences.
- Mutation testing remains valuable as a test-oracle check, but it is secondary
  here: a cross-runtime equality test directly measures wire fidelity, while a
  surviving mutation only says that the available mutation operators found a
  sensitivity gap.

## Findings and disposition

| ID | Severity | Finding | Disposition |
|---|---|---|---|
| EW-1 | High | Nanodollar JSON numbers above `2^53 - 1` can be silently rounded by JavaScript. | Remediated by the in-flight string migration and completed by this audit. |
| EW-2 | Medium | Five nanodollar values had semantic aliases, so a suffix-only inventory missed them. | Remediated; names, serde/OpenAPI types, handlers, clients, and generated artifacts now agree. |
| EW-3 | Medium | `parseNanos(number)` converted unsafe or fractional JavaScript numbers with `BigInt`, preserving an already incorrect value. | Remediated; only safe integer numbers are accepted. |
| EW-4 | Medium | Many non-nanodollar `u64`/`i64` values remain OpenAPI/JSON numbers without a proved safe bound. | Open in GitHub #177; deliberately not folded into this already broad migration. |
| EW-5 | Low | The first OpenAPI invariant covered component properties but not manually declared query parameters. | Remediated during final verification with a separate pinned parameter traversal. |

### EW-1 — exact nanodollar serialization

The reusable `wire_integer` adapters cover:

- required signed and unsigned scalar values;
- optional scalar values;
- `HashMap<String, Vec<u64>>` clearing-price arrays; and
- `HashMap<u32, u64>` reference-price maps.

Serialization always emits strings. Deserialization accepts string or legacy
integer tokens to avoid a flag-day migration for Rust/Python clients. It does
not accept a JSON float. Unit tests pin `i64::MAX`, `u64::MAX`, nested map/array
values, legacy integer inputs, and float rejection.

The compatibility choice is intentionally asymmetric:

```text
legacy integer token ─┐
decimal string ───────┴─> exact Rust integer ─> canonical decimal string
float token ──────────────────────────────────> rejection
```

This is a wire migration policy, not permission to use floating point in
protocol state.

### EW-2 — semantic aliases

The original migration selected `*_nanos` names, but the following values were
also nanodollars:

| Old public name | Correct public name | Shape |
|---|---|---|
| `min_yes_price` | `min_yes_price_nanos` | optional search query |
| `max_yes_price` | `max_yes_price_nanos` | optional search query |
| `min_volume` | `min_volume_nanos` | optional search query |
| `prices` | `prices_nanos` | market-id to reference-price map |
| `reserved_balance_released` | `reserved_balance_released_nanos` | signed response scalar |

`MarketSearchParams` and `SetReferencePricesRequest` now deny unknown fields.
The obsolete search/reference-price names are rejected, preventing a stale
client from apparently succeeding while the filter or update is ignored.

The search route's manual OpenAPI parameters previously omitted the minimum and
maximum YES-price filters. All three nanodollar filters are now documented as
strings and generated into the Python and TypeScript contracts.

### EW-3 — fail closed on unsafe frontend compatibility values

The frontend's `parseNanos` continues to accept a JavaScript `number` for local
fixtures and old payloads, but only when `Number.isSafeInteger` is true.
Previously, `BigInt(9007199254740993 as number)` could encode the rounded
`9007199254740992` with false confidence. Unsafe integers and fractions now
raise `RangeError`; normal application arithmetic remains `bigint`.

Conversions from exact `bigint` to `number` still exist for bounded
probabilities and intentionally approximate chart/display calculations. They
are not used as an exact wire representation.

### EW-4 — remaining 64-bit policy gap

The suffix invariant must not be generalized into a false claim that all public
integers are safe. Concrete unresolved classes include:

- signed-order `max_fill`, order/fill/position quantities, and signed deltas;
- replay nonces and block expiries;
- L1 `amount_token_units`, chain heights, and withdrawal/deposit identifiers;
- account/order/feed ids, block heights, cursors, counts, and timestamps.

Some may have practical or semantic bounds below `2^53`; others are exact
protocol values and should become strings. The required decision, field
inventory, boundary corpus, cross-runtime round trips, and acceptance criteria
are recorded in #177. The issue is in Project 1 with Stage `Backlog`, Priority
`Medium`, and status `Todo`.

### EW-5 — schema-test coverage gap

Component DTO properties and route parameters occupy different OpenAPI
locations. The drift test now:

- recursively validates component properties ending in `*_nanos`;
- follows string, nullable-string, array-item, and map-value schemas;
- separately traverses OpenAPI path parameter objects whose `name` ends in
  `*_nanos`; and
- pins both counts, so adding or removing a field forces deliberate review.

This does not infer semantics from arbitrary names. The semantic inventory and
the naming correction are what make the mechanical suffix rule useful.

## Implemented changes

- Added reusable exact-integer serde support for the reference-price map.
- Renamed and typed the five semantic aliases.
- Added missing market-search OpenAPI parameters.
- Made the Rust client construct the typed reference-price request DTO.
- Updated the Arena search wrapper and regenerated the vendored Python SDK.
- Regenerated the TypeScript schema directly from the live OpenAPI document.
- Hardened `parseNanos` against unsafe numeric compatibility values.
- Added high-boundary, legacy-input, obsolete-name, generated-schema, and
  frontend regression tests.
- Updated the REST architecture note to state the exact string convention and
  the `prices_nanos` contract.

## Verification

All relevant functional and generation gates passed on 2026-07-17:

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo test -p sybil-api-types --all-features` | 11 passed |
| `cargo test -p sybil-client` | 4 passed |
| `cargo test -p sybil-api` | full crate suite passed |
| `cargo clippy -p sybil-api-types -p sybil-client -p sybil-api --all-targets --all-features` | pass with four pre-existing `matching-solver` dependency dead-code warnings |
| same Clippy command with `-D warnings` | blocked only by those four out-of-scope dependency warnings |
| `uv run ruff check .` in `arena/` | pass |
| `uv run pytest -q` in `arena/` | 315 passed |
| `pnpm types:check` | generated TypeScript is current |
| `pnpm exec tsc --noEmit` | pass |
| `pnpm lint` | pass |
| `pnpm test -- --run` | 373 passed, 1 skipped |
| `pnpm scenarios:check` | 7 scenarios valid |
| `pnpm build` | production build passed |
| `just docs-check` | pins, sync, vault, and strict site build passed |

The strict Clippy result is recorded rather than hidden: the selected API
packages cause `matching-solver` to compile with a narrower dependency feature
set in which four research helpers are unused. The ordinary all-target/all-
feature selected-package lint succeeds. Cleanup belongs in the dedicated
lint/dead-code cluster, not in this wire-contract patch.

Follow-up on 2026-07-17: the economic-property cluster found the same helpers
were used only by the direct-dual conic feature and put them behind that exact
feature boundary. The strict API Clippy command above now passes with
`-D warnings`; the table retains the original audit-time result for provenance.

## Completion gate and residual risk

The exact-nanodollar cluster is complete when this report and the generated
artifacts remain in the same change and the final drift tests pass. It should be
archived as a dated reference once #177 either:

1. establishes exact strings for every unbounded 64-bit wire value; or
2. establishes and enforces JSON-safe maxima for retained numeric values.

Until then, this report remains `current-audit` because users could reasonably
misread the nanodollar fix as a repository-wide integer guarantee.

## Future audit directions

The highest-value next cluster is test-oracle effectiveness in the validity
core: targeted mutation testing of verifier, settlement, canonical encoding,
and state-transition checks, with every surviving mutant classified as
equivalent, uncovered, or invalid. Other queued clusters are maintained in the
[code-quality audit program](code-quality-audit-program-2026-07.md).
