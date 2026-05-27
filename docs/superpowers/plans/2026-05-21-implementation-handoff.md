# Implementation Handoff — 2026-05-21 Backend Batch

**You (post-compact Claude) are picking up to IMPLEMENT six already-planned backend changes.** The planning + investigation is done. Do not re-investigate or re-design — the plan files below are the source of truth and were written from verified code reads. Read the plan file for whatever you're implementing, then execute it.

## What this is

Six independent backend changes for the **Sybil** prediction-market repo (`/Users/r/pr/Sybil`), planned with the user this session. The user approved the approach and said "we'll implement after compact." Your job now is execution.

## The six plans (in `docs/superpowers/plans/`, build order)

1. **`2026-05-21-recent-blocks-and-created-at-ms.md`** — `GET /v1/blocks?limit=N` + `created_at_ms` on `PendingOrderResponse`.
2. **`2026-05-21-10s-batch-cadence.md`** — block cadence 2s→10s + FE countdown fixes. **Depends on #1** (open-orders uses `created_at_ms`).
3. **`2026-05-21-mm-liquidity.md`** — score MM flash orders into the liquidity ring.
4. **`2026-05-21-polymarket-full-json-cache.md`** — mirror PUTs full Gamma JSON → `sybil-api` folder, served at `GET /v1/events/{id}/raw`, wiped on restart.
5. **`2026-05-21-account-equity-series.md`** — `EquityTracker` sidecar + `GET /v1/accounts/{id}/equity?range=`.
6. **`2026-05-21-portfolio-history-feed.md`** — `AccountEventLog` + `GET /v1/accounts/{id}/events` (implements the spec `docs/superpowers/specs/2026-05-21-portfolio-history-feed-design.md`).

Build order: **1 → 2** (coupled). **3, 4, 5, 6 are independent** — any order / parallelizable.

## Decisions already made — do NOT re-litigate

- **#4c `OrderCancelled` is ALREADY implemented backend-side** (emitted in `sequencer.rs:1429` `cancel_pending_order`, in `events_root`, in `SystemEventResponse::OrderCancelled` at `response.rs:244`). Plan 6 reuses it; do not rebuild it.
- **Issue #17 / a separate `/closed` endpoint (#4b) is SUPERSEDED** by Plan 6's unified `/events` feed (realized P&L inline). Do not build `/closed`.
- **`created_at_ms` (#5) is a multi-struct change, not "one field"** — verified `RestingOrder.created_at` is a *block height* (`order_book.rs:40`), not ms. Plan 1 threads a wall-clock ms through `accept()` and its callers.
- **Plan 2 bridge withdrawal expiry** → `120_960` blocks (keeps the current 14-day wall-clock at 10s). GTC `order_ttl_blocks` + MM block-windows are deliberate no-ops (documented in the plan).
- **Plan 3** scores MM orders by quoted `max_fill` (user confirmed actual-fill-being-less is fine).
- **Plan 4 delivery** = mirror **PUTs to sybil-api** (user chose this over shared volume / mirror-served port). Full JSON via `Serialize`+`#[serde(flatten)]` for future-proofing.
- **Plans 5 & 6 sidecars are in-memory/volatile** ("since last restart" caveat, same as other off-block aggregates) — no `AnalyticsSnapshot` persistence plumbing in scope.

## Implementer notes to confirm during execution (not open questions)

Each plan's self-review lists a few "confirm this in-scope binding" notes — verify against live code as you go, don't assume:
- Plan 1 Step 9: the block-timestamp binding name in `produce_block_in_place` (`sequencer.rs` ~1930) for the batch-admit `created_at_ms`.
- Plan 2 Task 3: the relative-time formatter already imported in `open-orders-list.tsx`.
- Plan 3: `Order` import scope in `analytics.rs`.
- Plan 4 Task 4: the `self.sybil_client` field name in the mirror's `sync` struct.
- Plan 5/6: `AccountStore` / `Account` / `Order` accessor names; the `record_fills` "realized-before" capture must go *immediately before* `apply_fill` (Plan 6 §3b).

## How to execute

- **Use the `superpowers:executing-plans` skill** (or `superpowers:subagent-driven-development` if the user wants a fresh subagent per task with review between tasks — recommended for these independent plans). Ask the user which they prefer if unspecified.
- **TDD is mandatory per the plans:** write the failing test → run it and CONFIRM it fails → implement → run and CONFIRM it passes → commit. Do not skip the confirm-fail/confirm-pass steps.
- **`superpowers:verification-before-completion`:** run the actual commands and show output before claiming any task done. No "should work."

## Environment / process facts (these may be lost in the summary)

- **VCS is `jj` (Jujutsu), NOT git** (see `AGENTS.md`). Use `jj st`, `jj diff --git`, `jj new`, `jj describe -m "..."`. The plans' commit steps use `jj describe`. Currently on branch `r/dev`.
- **Build/test:** `just build`, `just test`, `just lint` (clippy), `just fmt`. One test: `cargo test -p <crate> <name>`. API integration: `cargo test -p sybil-api --test api_integration <name>`.
- **Run locally (the user wants to eyeball results):** `cargo run --release -p sybil-api -- --dev-mode --port 3001`. Mirror: `cargo run --release -p sybil-polymarket -- --sybil-url=http://localhost:3001 --max-events=5`. Each plan has a manual-verification task with exact `curl`s.
- **Frontend** (`frontend/web`, Next.js, pnpm): `pnpm lint`, `pnpm build` (typecheck), and `NEXT_PUBLIC_API_BASE=http://localhost:3001 pnpm types:generate` to regenerate `src/lib/api/schema.d.ts` from a running backend (needed by Plan 2 Task 3 after Plan 1 lands).
- **Conventions:** integer nanos only (1 dollar = 1e9 nanos), no floats; actor model (`SequencerHandle` ⇄ `SequencerMsg` in `crates/matching-sequencer/src/actor.rs`); off-block sidecars live in `AnalyticsState` (`analytics.rs`).
- **User preferences (memory):** propose before building large features; validate assumptions against actual code/data before implementing. The plans already reflect this; keep verifying as you implement.
- Set `/effort max` if you want the deepest reasoning (it's session-scoped, resets on compact).

## Status / open items

- The 6 plan files + the design doc are **on disk but may be uncommitted** — check `jj st`; commit them first (`jj describe`) if the user hasn't.
- Nothing in the actual crates has been changed yet. This is a clean starting point.

## Suggested first action

Confirm with the user: (a) execution mode (subagent-driven vs inline), (b) start with Plan 1. Then read `2026-05-21-recent-blocks-and-created-at-ms.md` and begin Task A1, Step 1.
