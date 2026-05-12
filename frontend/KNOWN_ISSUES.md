# Frontend known issues

Workarounds in place + open tickets for proper fixes.

---

## #1 — u64 / `_nanos` fields come over the wire as JSON numbers

**Status:** workaround active in frontend; backend fix pending.

**What it is.** Sybil represents money as nanos (1 dollar = 1,000,000,000 nanos). The Rust API serializes these `u64` fields as JSON numbers. JavaScript's `number` type can only represent integers up to **2^53 − 1 ≈ 9.007e15**. Once a value crosses that line, the precision is silently lost during JSON parsing — before our code ever sees it.

In practical Sybil terms, the danger zone in nanos is values above `~$9,007,199`. Per-trade prices are nowhere near; aggregate welfare totals, large account balances, and protocol-wide volume can cross it.

**What we did about it (frontend-only).**

1. `scripts/patch-bigints.mjs` rewrites the generated TypeScript schema so every `*_nanos: number` becomes `*_nanos: string`. Runs automatically after `pnpm types:generate`.
2. `src/lib/format/nanos.ts` exposes `parseNanos()`, `formatDollars()`, `formatProbability()`. `parseNanos` accepts `string | number | bigint` so it works whether the runtime value is a (possibly-rounded) JS number or a string.
3. All money arithmetic in app code must go through `bigint` and these helpers — **never** do `data.balance_nanos * 2` directly.

**Why this isn't enough.** The wire format is still JSON numbers, so a value of, say, `12,345,678,000,000,000` nanos (~$12.3M) is *already corrupted* by `JSON.parse` before our code receives it. We can't recover the lost digits client-side. The workaround prevents *further* corruption inside our code and forces correct arithmetic — that's the bound of what's possible from the frontend alone.

**The proper fix (backend ticket).** Configure `utoipa` in `crates/sybil-api` to emit `format: int64` u64 fields as JSON strings. When that lands:
- `pnpm types:generate` will produce `*_nanos: string` naturally → `scripts/patch-bigints.mjs` becomes a no-op and can be deleted.
- The wire format will be `"12345678000000000"` (string) instead of `12345678000000000` (number) → no precision loss in transit.
- `parseNanos` already handles both paths, so no app-code changes needed.

**Owner:** backend team. Track in the Rust repo, not here. This file should be updated when it ships.

---

## #2 — Polymarket mirror metadata stored off-block

**Status:** design choice for Phase 2 (planned, not yet shipped, currently paused). Plan-of-record: `frontend/PHASE_2_PLAN.md`.

**What it is.** The frontend cards need four fields the Sybil backend doesn't currently expose: event image, per-market image, expected end date, and category. The data exists in the Polymarket Gamma API response we already deserialize on every sync — we just throw it away. Phase 2 wires those four fields end-to-end (Gamma → `sybil-polymarket` mirror → `sybil-api` ref data → `MarketResponse` → frontend cards).

Several tradeoffs were chosen knowingly. Each is "good for now, revisit later" rather than a forever decision. Documenting here so the next person doesn't have to mine git history to find the why.

**Tradeoff 1 — Off-block storage.** Mirror-derived metadata lives in `MarketRefData` (in `crates/sybil-api/src/state.rs`), the same mutable off-block shelf that already holds `external_url`. It does **not** enter the block hash. Pros: backfill is trivial, Polymarket re-tags/image swaps don't perturb the block stream, no schema migration on the verifiable record. Cons: a third-party verifier can't cryptographically prove "this market was categorized as Sports at block N" — it's display chrome only. Revisit if/when the verification surface needs to grow to cover categorization or imagery.

**Tradeoff 2 — `end_date` is display-only.** Polymarket's `endDate` is the *expected* resolution date, not an enforced trading cutoff. Markets trade past it until the resolution actor signs an attestation. So we deliberately do **not** route it through the matching engine's `expiry_timestamp_ms` (which is a hard "stop accepting orders at T" cutoff). It lives in `MarketRefData.end_date_ms` for display only. Cost: the matching engine has no notion of when a mirrored market "should" close — only resolution events close them. That's already the de facto behavior; this just makes it explicit. Revisit when/if we want enforced trading windows.

**Tradeoff 3 — Backfill is a one-shot CLI flag, not a recurring loop.** `sybil-polymarket --backfill-metadata` walks every mirrored market, POSTs metadata, exits. Operators re-run it manually if Polymarket re-categorizes or replaces images. Pros: simplest possible surface, no permanent background actor. Cons: drift — a Polymarket re-tag won't propagate automatically. Revisit if drift becomes a real annoyance; the natural next step is promoting the backfill into a `BackfillActor` running at a slow cadence (daily-ish).

**Tradeoff 4 — Tag→category derivation is a hardcoded table in code.** Polymarket's `event.category` is always null in practice; the real category signal is in `event.tags[].label`. We collapse that long-tail of tags onto a fixed 16-bucket taxonomy via a hardcoded lookup in `crates/sybil-polymarket/src/categorize.rs`. New tag labels Polymarket introduces will fall to "Other" until the table is extended. Mitigation: every unmatched tag is logged at `info!` so we can grow the table. Revisit when the long tail becomes large or when ops shouldn't need a code change to retag.

**Tradeoff 5 — `MarketRefData` persists as JSON-on-disk, save-on-every-write.** Mirrors the existing `MappingStore` pattern in `crates/sybil-polymarket/src/mapping.rs:43-61`. Configurable path; empty path = volatile (current behavior). Cost: write-amplification on every metadata POST. Mitigated by the fact that metadata writes are rare (one per market per sync cycle, plus the one-shot backfill). Revisit if it ever becomes hot — promote to periodic-save-if-dirty or to a real database row.

**What would change our mind on any of these.** Common triggers:
- A real product need for "what was this market's category at block N" → flip Tradeoff 1 to on-block, add an `AmendMarketMetadata` sequencer event.
- We start wanting auto-closing markets on a schedule → flip Tradeoff 2, route `endDate` to `expiry_timestamp_ms`.
- We notice users seeing stale categories/images → flip Tradeoff 3 to a recurring backfill actor.
- We get tired of code changes to add a category mapping → flip Tradeoff 4 to a YAML/JSON config file loaded at startup.
- Metadata-write throughput climbs → flip Tradeoff 5 to periodic flush or move to the database the rest of state will eventually live in.

**Owner:** carries across `sybil-api` (response/state changes), `sybil-polymarket` (sync + backfill + categorize), and `frontend/web` (card wiring). Implementation lives in the same plan-of-record above.

---
