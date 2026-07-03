# Frontends and Visualization

**Surfaces:** `frontend/web` (Next.js), `crates/sybil-api/static/` (Alpine console), `frontend/handoff`, `frontend/archive`, `apps/composition-demo`, `viz/` (Streamlit), plus the arena Streamlit dashboard

## Verdict

Sybil has **five-plus distinct presentation surfaces with no single owner**, and the one users can actually reach is the worst of them. The Next.js app is high quality — strict TypeScript, bigint money discipline, a clean WebSocket state machine, 114 passing tests — but it is not deployed anywhere, while the deployed Alpine console includes a `/trade` page whose order signatures **can never verify**. The dominant problem is not code quality; it is proliferation and duplication of the same API across four independently hand-maintained clients.

## Architecture as built (the inventory)

1. **`frontend/web`** — Next.js 16 + React 19 + Tailwind v4, ~26.3k lines TS/TSX, entirely client-rendered (Next used as an SPA toolchain). **Not deployed** — no compose service, no Caddy route; the "live demo" is `pnpm dev` on localhost pointed at the prod devnet. Money is bigint-only via one `nanos.ts` module. A clean `BlockStream` WS client faithfully implements the vault's WebSocket note. Auth reimplements the `sybil-signing` borsh encoding in TS with test vectors copied from the Rust snapshots. Includes a **`/dev/*` "Dev Zone"** (~3,900 lines) that is a hand-synced React port of the Alpine console, self-documented as "re-sync by hand when the console changes."
2. **`crates/sybil-api/static/index.html` + `trade.html`** — ~3k lines of Alpine.js compiled into the API binary via `include_str!` and served at `/` and `/trade` on `:3000`. **This is the deployed UI.** `trade.html` hand-rolls a borsh canonical encoding that has **drifted from the Rust source** ([H11](01-critical-bugs.md)).
3. **`frontend/handoff`** — a static design spec (reference HTML + JSX data modules + token CSS).
4. **`frontend/archive` + root `BACKEND_*_PLAN.md`** — ~2,100 lines of completed plan documents.
5. **`apps/composition-demo`** — an **empty directory** on main (content lives only on a branch that committed `node_modules`).
6. **`viz/`** — a repo-root Streamlit solver-pipeline visualizer, local-only, last touched March.
7. The **`:8501` Streamlit** in compose is `arena/live/dashboard.py` — a *different* codebase from `viz/`.

**Type chain:** `sybil-api-types` → `/openapi.json` → `openapi-typescript` → committed `schema.d.ts` → post-processed by `patch-bigints.mjs` (because the backend still serializes u64 as JSON numbers). Manual mirrors of the same contracts exist in the WS types, the Dev Zone's loose types, and both Alpine pages.

**Doc drift:** no architecture note covers `frontend/web` at all; `Frequent Batch Auctions.md` says "1-second batches" while the FE hardcodes 10s and the API defaults to 500ms.

## Strengths

- **Bigint money discipline is real and enforced** in `frontend/web`: one money-math module, `parseNanos` at every wire boundary, no float on nanos outside the deliberately-excepted Dev Zone.
- The `BlockStream` WS client is a clean, well-commented state machine faithful to the docs (seeded handshake, replay/lagged/1008 handling, visibility reconnect).
- **Cross-language canonical-signing discipline:** `canonical.ts` mirrors `sybil-signing` with vectors copied from the Rust insta snapshots, so schema drift fails vitest — exactly the failure mode that bit `trade.html`.
- Mock-marker discipline (every placeholder is deterministic, centralized, visually flagged) and thorough decision documentation (STATUS.md, KNOWN_ISSUES.md state each workaround's bound and proper fix).
- 114 passing vitest tests over pure logic.

## Findings

| ID | Kind | Sev | Summary |
|----|------|-----|---------|
| [H11](01-critical-bugs.md) | bug | high | Deployed `/trade` page signs stale canonical bytes (omits `expires_at_block`) → every signed order rejected; its self-check vectors are equally stale so startup validation passes |
| FE-1 | bloat | high | Dev Zone hand-synced duplicate of the Alpine console (~3,900 lines re-implementing 1,727, with its own loose types and deliberate float math) |
| FE-2 | debt | high | u64-nanos-as-JSON-number corruption unresolved at the source; the patched TS schema now lies about request bodies, forcing double type-casts in `orders.ts` — see [12-api](12-api.md) |
| FE-3 | ops | medium | The real user frontend is not deployed; the reachable UI is the stale console + broken trade page |
| FE-4 | inconsistency | medium | Block cadence hardcoded in the FE (10s) and inconsistent repo-wide (10s compose / 500ms api default / 1s docs) |
| FE-5 | bloat | medium | Six unused npm deps (react-hook-form, zod, framer-motion, cva…) + create-next-app leftovers; styling contradicts its tooling (95 components use inline styles, Tailwind nearly unused) |
| FE-6 | bloat | medium | Dead code: unused mock modules, an unused store reset, unwired WS recovery events (the documented refetch-on-block-not-found contract is unimplemented) |
| FE-7 | test-gap | medium | CI never runs the 114-test vitest suite (incl. the canonical-drift tests that would catch a `trade.html` regression); the WS client and Alpine console have zero tests |
| FE-8 | inconsistency | low | Same endpoint fetched by three differently-keyed hooks → duplicate requests, cache fragmentation |
| FE-9 | inconsistency | low | IP-pinned devnet URL hardcoded as an in-code fallback in four places |
| FE-10 | doc-drift | low | `frontend/` root carries ~2,100 lines of completed plan docs + a stale STATUS.md |
| FE-11 | debt | low | `apps/composition-demo` is an empty directory; demo content marooned on a branch with committed `node_modules`/`dist` |
| FE-12 | debt | low | `viz/` is a maintained-in-name-only tool tied to a superseded solver-pipeline view |
| FE-13 | design | low | Type generation depends on the live devnet, not the checked-out backend; nothing in CI detects schema drift |

## Ambitious ideas

1. **One frontend, deployed.** Delete `crates/sybil-api/static/` (both Alpine pages) and the Dev Zone duplication in one move; deploy `frontend/web` behind Caddy as the devnet UI; port the two or three console-only views into `/dev` using the generated schema types; let `sybil-api` go back to being a pure API server. Net: ~3,000 lines of untyped embedded JS and ~1,500 lines of duplicated React deleted, one signing implementation instead of three, and the reachable UI becomes the good one.
2. **Make the Rust code the type oracle for every client.** Emit `openapi.json` from the workspace in CI, fix u64/Nanos serialization to JSON strings **once** in `sybil-api-types`, regenerate `schema.d.ts` as a checked build artifact, and export the `sybil-signing` insta snapshots as a shared JSON test-vector file both the TS suite and any SDK consume — no more "vectors copied verbatim," no `patch-bigints.mjs`, no silent >2^53 corruption.
3. **Extract `@sybil/client`** — a ~600-line TS package (canonical encoding, P-256 signing, openapi-fetch client, `BlockStream`, `parseNanos`) currently interleaved with React. It's the highest-quality code in the frontend and exactly what external traders/agents need; publishing it forces the protocol-client/UI boundary to stay clean.
4. **Cadence and config as API surface:** add `block_interval_ms` (and chain identity) to `/v1/health`; delete the hardcoded constant and the devnet-IP fallbacks so one build artifact works against any deployment.
5. **Prune the presentation strata deliberately:** `frontend/handoff` → `design/`; `frontend/archive` + the two `BACKEND_*_PLAN.md` → deleted; `apps/` → deleted; `viz/` → adopted-with-a-smoke-test or deleted. After this, `frontend/` contains exactly one thing and "which frontend is real?" stops needing a review to answer.
