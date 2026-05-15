# Phase 2 — Polymarket mirror metadata enrichment (off-block)

> **Status: paused, not started.** Approved as a plan but not yet implemented. Paused because (a) the prod server (`root@172.104.31.54`) was wedged when we went to verify deploy access, and (b) the current user's SSH key isn't in `authorized_keys` on the box. Pick this up once SSH access is granted and the API container is healthy again — see "Pre-flight before resuming" at the bottom.
>
> Design tradeoffs locked in this plan are summarized in `STATUS.md` ("Active design tradeoffs") and detailed in `KNOWN_ISSUES.md` #2.

## Context

The markets index is built and shipped, but four card fields are mocked in the frontend because the backend doesn't expose them: event image, market image, end date, and category. The data exists in Polymarket's Gamma API — we already deserialize the JSON in `SyncActor::sync_once`, we just throw it away. Live confirmation by hitting `https://gamma-api.polymarket.com/events`:

- `event.image`, `event.icon` — S3 PNG URLs, always populated on active events
- `market.image`, `market.icon` — distinct per-market URLs (matters on NegRisk events where each outcome has its own picture)
- `event.endDate`, `market.endDate` — ISO-8601 strings. Per user: this is the *expected* resolution date, not a hard trading cutoff. Trading continues past `endDate` until the resolution actor signs an attestation.
- `event.category` — always `null` in practice. Real category signal lives in `event.tags[].label` ("World Elections", "NBA", "Trump", …) and needs a transformation table to collapse onto our 16-bucket taxonomy.

This phase wires those four fields end-to-end: pulled from Gamma on every sync, persisted on sybil-api, surfaced in `MarketResponse`, and rendered on the frontend (replacing the `<MockValue>` wrappers and deterministic-tile fallbacks).

## Design decisions (all "good for now, revisit later")

These are tradeoffs we're consciously accepting. Documented in `STATUS.md` under "Active design tradeoffs" and in `KNOWN_ISSUES.md` #2 so they don't get forgotten.

1. **Off-block storage.** Mirror-derived metadata (image, category, tags-from-Gamma, end_date) goes into `MarketRefData` — the same off-block shelf that already holds `external_url`. **It does not enter the block hash.** Cleanest for now: a Polymarket re-tag or image swap doesn't perturb our block stream, and backfill is trivial. Cost: a third-party verifier can't prove "this market was categorized as Sports at block N". For display chrome that's acceptable. Revisit if the verification surface needs to grow to include this metadata.

2. **`end_date` is display-only.** Per user clarification, Polymarket's `endDate` is the expected resolution date, not a trading cutoff — trading continues past it until the resolution actor closes the market. So we do **not** route it through the matching engine's `expiry_timestamp_ms` (which is a hard "stop accepting orders at T" cutoff). It lives in `MarketRefData.end_date_ms` for display only. Cost: the matching engine has no notion of when a market "should" close — only resolution events close mirrored markets. That's already the de facto behavior today; this just makes it explicit. Revisit if/when we want enforced trading windows.

3. **Backfill is a one-shot CLI flag, not a recurring loop.** `sybil-polymarket --backfill-metadata` walks every mirrored market, POSTs metadata, exits. Re-run manually if Polymarket re-categorizes or replaces images. Simpler. Cost: a Polymarket re-categorization won't propagate automatically — we'd need to remember to re-run the backfill (or cron it from outside the binary). Revisit if drift becomes a real annoyance.

4. **Tag → category derivation is a hardcoded table in code, not config.** Per user's mapping spec. New tags Polymarket adds will fall to "Other" until we extend the table. Every unmatched tag is logged so we can grow the table. Revisit when the long tail becomes large.

5. **JSON-on-disk persistence for `MarketRefData`.** Mirrors the existing `MappingStore` pattern in `crates/sybil-polymarket/src/mapping.rs:43-61`. Save on every write, load on startup, configurable path (empty = volatile in-memory only, current behavior). Simple and matches a pattern already in the codebase. Cost: write-amplification on every metadata POST. Mitigated by the fact that metadata writes are rare (one per market per sync cycle, plus one-shot backfill). Revisit if it ever becomes hot.

## Files to modify

### sybil-api (backend)

- **`crates/sybil-api-types/src/request.rs`** — extend `SetMarketMetadataRequest` (currently only `external_url`) with five new optional fields: `event_image_url`, `market_image_url`, `end_date_ms`, `category`, `tags_raw`.
- **`crates/sybil-api-types/src/response.rs`** — add three new optional fields to `MarketResponse`: `event_image_url`, `market_image_url`, `end_date_ms`. (Existing `category` and `tags` stay; merge rule below.) Optionally add `event_image_url` to `MarketSummaryResponse` (only the event image — keep summary lean).
- **`crates/sybil-api/src/state.rs:16-18`** — extend `MarketRefData` struct with the same five fields. Add `load_from_disk(path)` and `save_to_disk(path)` methods modeled on `MappingStore::load`/`save`. New `Config` flag `market_ref_data_path: String` (default empty = volatile).
- **`crates/sybil-api/src/routes/markets.rs`**
  - `set_market_metadata` (line 648): persist after mutation when path configured. Stays dev-mode-only.
  - `build_market_response` (line 32): pull the new fields from `MarketRefData`. **Merge rule for category/tags**: if `ref_data.category` is Some, it wins over `metadata.category`; same for tags. (Mirror-native sybil markets keep on-block values; mirrored markets get ref-data values.)
  - All read paths that consume `external_url` from `market_ref_data` (lines 115, 212, 541) need to also read the new fields. The cleanest way: pass the whole `MarketRefData` snapshot into `BuildMarketResponseArgs` instead of just `external_url`.

### sybil-polymarket (mirror)

- **`crates/sybil-polymarket/src/categorize.rs`** (NEW) — small module with the hardcoded tag→category table per user spec, exposing `fn derive_category(tags: &[GammaTag]) -> Option<String>`. Case-insensitive. Priority-ordered (Sports > Politics > Geopolitics > … > Other) so the same set of tags always yields the same category regardless of Polymarket's array order. Logs every unmatched tag label at `info!` level.
- **`crates/sybil-polymarket/src/polymarket/types.rs:62`** — extend `GammaEvent` with `image: Option<String>` and `icon: Option<String>`. Tags already covered by `tags: Vec<GammaTag>` (need to define `GammaTag { label: String, slug: String }` with `#[serde(default)]` since some entries omit fields). Already has `endDate` as `Option<String>` on both event and market — parse to `Option<u64>` ms-since-epoch via a helper.
- **`crates/sybil-polymarket/src/polymarket/types.rs:87`** — extend `GammaMarket` with `image: Option<String>` and `icon: Option<String>`. `endDate` already deserialized.
- **`crates/sybil-polymarket/src/sync.rs:163-211`** — after a successful `create_market`, build a `SetMarketMetadataRequest` and POST it. Failure of metadata POST is non-fatal — logged + retried on the next sync cycle (idempotent on the api side).
- **`crates/sybil-polymarket/src/sybil/client.rs`** — add `set_market_metadata(market_id, req)` method.
- **`crates/sybil-polymarket/src/backfill.rs`** (NEW) — one-shot reconciler.
  - Walks `MappingStore.all_condition_mappings()` (already exists, mapping.rs:145).
  - Chunks condition_ids into batches of 100.
  - Per batch: `GET /markets?condition_ids=...&condition_ids=...` (confirmed batched; the response includes the embedded `events` array, so we get event-level image + tags in the same call without a second roundtrip).
  - For each market: derive category, build `SetMarketMetadataRequest`, POST.
  - Exits when done. Logs a summary (`updated=N, skipped=M, failed=K`).
- **`crates/sybil-polymarket/src/main.rs`** and **`crates/sybil-polymarket/src/config.rs`** — add `--backfill-metadata` CLI flag. When set: skip actor startup, run backfill, exit.

### Frontend

- **`frontend/web/src/lib/api/schema.d.ts`** — regenerated from backend OpenAPI after the type changes ship (existing codegen toolchain).
- **`frontend/web/src/components/market-thumb.tsx`** — accept `imageUrl?: string` and render an `<img>` with the deterministic tile as `onError` fallback. (Component already designed for this; just needs to be wired.)
- **`frontend/web/src/components/binary-card.tsx`** and **`multi-card.tsx`** — pass `market.market_image_url` to `<MarketThumb>`. Drop `<MockValue>` wrap on the category chip when `market.category` is present. Render `end_date_ms` in the eyebrow as "closes Mar 5" when present (small text-mono, fg-3).
- **`frontend/web/src/lib/mock.ts`** — keep `mockCategory` as a fallback for markets without backend category (e.g. cold-start before mirror runs); flip card components to prefer real data.
- **`STATUS.md` + `KNOWN_ISSUES.md`** — already updated with the design tradeoffs above (done before pausing).

## Tag → category mapping (per user spec)

| Category | Tag labels (case-insensitive, first match wins by priority order below) |
|---|---|
| Sports | Sports, Soccer, NFL, NBA, NHL, MLB, UFC, Tennis, Boxing, Cricket, Chess, Hockey, Football, football, Golf, Formula 1, Pickleball, EPL, MLS, PGA, Esports |
| Politics | Politics, Trump, Congress, Senate |
| Geopolitics | Geopolitics |
| Crypto | Crypto |
| World | World |
| Culture | Culture, Movies, Music, Celebrities, YouTube, Awards |
| Economy | Economy |
| Finance | Finance, Stocks, Earnings, IPOs, IPO |
| Commodities | Commodities |
| Business | Business |
| Mentions | Mentions |
| Weather | Weather |
| Tech | Tech |
| Science | Science |
| AI | AI |

Priority order (when an event has tags from multiple categories): Sports > Politics > Geopolitics > Crypto > World > Culture > Economy > Finance > Commodities > Business > Mentions > Weather > Tech > Science > AI. Locked in `categorize.rs` constants and documented inline. Untagged or all-unmatched events → `None` (frontend renders as "Other" or omits the chip). Every unmatched tag logged at `info!` to grow the table.

## Edge cases & mitigations

1. **NegRisk events have one event image + N market images.** Both stored. Cards use `market.image`, group/event views use `event.image`.
2. **Sybil-native (non-mirrored) markets.** Untouched. They keep using on-block `MarketMetadata.category`. Merge rule in `build_market_response` falls back to on-block when ref-data is None.
3. **Tag mapping miss.** Logged. Frontend renders `category: null` as "Other" (or omits the chip). Raw tag labels stored in `tags_raw` for a future debug tooltip.
4. **Image URL becomes invalid.** Polymarket-hosted S3 URLs occasionally 404. `<img onError>` fallback to deterministic tile already in `<MarketThumb>`.
5. **`MarketRefData` snapshot file missing/corrupt at startup.** Same handling as `MappingStore::load`: if file is missing, start empty; if corrupt, log warn and start empty. Recoverable via re-running backfill.
6. **Metadata POST during high-throughput sync cycle.** Sync currently creates at most ~50 markets per cycle (config `max_events`). Each adds one metadata POST. Negligible. No batching needed.
7. **Backfill rate-limit.** Gamma is 4000 req/10s. 150 markets / 100 batch size = 2 requests. No concern.
8. **Backfill collides with running sync.** Both write to the same `MarketRefData` entries. Idempotent in both directions — last writer wins, which is fine because both pull from the same upstream (Gamma).
9. **Backfill before any markets are mirrored.** `MappingStore::all_condition_mappings()` returns empty; backfill logs "nothing to do" and exits cleanly.
10. **Category mapping ambiguity.** A market with both "NBA" and "Trump" tags would otherwise be ambiguous. Resolved by category priority order (Sports beats Politics). Priority order locked in `categorize.rs` constants and documented inline.
11. **Mirror DOWN at sybil-api restart.** With JSON persistence chosen, sybil-api loads from disk and shows correct images/categories even with the mirror offline. No regression vs. today's behavior (today they're mocked anyway).
12. **Polymarket changes a tag.** Won't propagate automatically (per design decision 3 — recurring backfill deferred). Document in STATUS.md.
13. **Frontend OpenAPI regen drift.** Schema bump requires `pnpm <codegen>` to be re-run. Add to deploy checklist.
14. **Dev-mode guard on `POST /v1/markets/{id}/metadata`.** Endpoint is currently dev-mode-only. Prod already runs with `SYBIL_DEV_MODE: "true"` per `docker-compose.yml:8`, so this is fine. Pre-existing constraint, not new — but flag it.

## Verification

End-to-end smoke test:

1. **Backend builds and starts**: `cargo build -p sybil-api -p sybil-polymarket && just dev` (or equivalent local stack).
2. **Forward sync wiring**: trigger one sync cycle (`SYNC_INTERVAL_SECS=10`), confirm a freshly-created market has `event_image_url` / `market_image_url` / `end_date_ms` / `category` populated in `GET /v1/markets/{id}` response.
3. **Backfill**: `sybil-polymarket --backfill-metadata --sybil-url=http://localhost:3000 --mapping-store-path=/tmp/mapping.json`. Confirm exit code 0, summary log line, and `GET /v1/markets` shows populated fields on all mirrored markets.
4. **Persistence**: restart sybil-api with `MARKET_REF_DATA_PATH=/tmp/ref.json`. Confirm fields survive the restart without re-running backfill.
5. **Frontend**: `cd frontend/web && pnpm dev` → `/`. Confirm cards show real images (S3 PNGs), category chips no longer have dotted-yellow underlines, eyebrow shows "closes …" dates. Confirm fallback tile still appears for any market whose image URL 404s.
6. **Category derivation unit test**: `cargo test -p sybil-polymarket categorize::tests` covering each user-spec mapping rule and the priority ordering for ambiguous tag sets.
7. **Type-checking + lint**: `cargo clippy --workspace -- -D warnings` and `cd frontend/web && pnpm tsc --noEmit && pnpm lint`.
8. **No block-hash drift**: hash the head block before + after a metadata POST and confirm identical (off-block invariant).

## Out of scope (deferred to follow-ups)

- On-block metadata storage (block-hash-committed category/image). Revisit if verification scope expands.
- Recurring backfill actor to catch Polymarket re-tags.
- Externalizing the tag→category table to a config file.
- Proxying/caching Polymarket S3 images on our own CDN.
- Real `expiry_timestamp_ms` enforcement (matching-engine auto-close at endDate).
- Backfilling `MarketMetadata.expiry_timestamp_ms` for existing markets (would require an on-block amend event).

## Pre-flight before resuming

When picking this back up:

1. **Confirm prod box is healthy**: `curl https://172-104-31-54.nip.io/v1/health` returns 200 in <2s. If not, fix the underlying memory/CPU pressure first (probable OOM on the 2GB Linode — `docker compose logs sybil-api | grep -iE 'killed|oom|panic'`).
2. **Confirm SSH access**: `ssh root@172.104.31.54 'echo ok'` returns `ok`. If not, send your `~/.ssh/id_ed25519.pub` to whoever owns prod (current pubkey fingerprint: `SHA256:Ce/giqRyMLs81FnJGaFkRydKZmDd0fL73qHxvMBGsW0`) to be appended to `root@/.ssh/authorized_keys`.
3. **Confirm Gamma is reachable from the prod box**: `ssh root@172.104.31.54 'docker compose exec sybil-polymarket curl -sS https://gamma-api.polymarket.com/events?limit=1 | head -c 200'` — should return JSON.
4. **Re-read this plan** — design tradeoffs may be stale by then. Especially #3 (one-shot backfill) and #4 (hardcoded category map) are likely to grow tired.
