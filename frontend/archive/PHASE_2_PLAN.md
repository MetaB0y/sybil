# Phase 2 — Polymarket mirror metadata enrichment

**Status:** ready to implement. Both prior blockers (prod SSH access, sybil-api OOM) are cleared.

**Previous draft archived at** `frontend/archive/PHASE_2_PLAN.md` (kept for context; outdated). The master implementation plan lives at `~/.claude/plans/typed-jingling-mitten.md` on the working machine; this file is the in-repo distillation so anyone reading on GitHub sees the current intent.

---

## Why

Several card fields on the markets index/detail pages are mocked because the Sybil backend doesn't expose them yet:

- Parent event identity (id + title) — needed to render obviously-grouped events (Fed Decision sub-markets, BTC threshold pairs) as one MultiCard instead of separate BinaryCards.
- Event image, per-market image — currently a deterministic colored tile.
- End dates — eyebrow "closes Mar 5" placeholder.
- Category — chip currently a hash-derived mock with a dotted-yellow MockValue underline.

The data already arrives every sync cycle from `https://gamma-api.polymarket.com/events` — `sybil-polymarket` deserializes the response, uses a sliver of it, and discards the rest. Phase 2 closes that gap.

## What we pull from Polymarket

Confirmed live by `curl`-ing Gamma:

- `event.id`, `event.title` — always present
- `event.image`, `event.icon` — S3 URLs; usually identical, occasionally differ at the market level
- `event.endDate`, `market.endDate` — ISO-8601; **can differ** (event may close 2025-12-31, child market 2026-04-01)
- `event.category` — null in practice; real signal is `event.tags[]` array of `{label, slug}`
- `market.image`, `market.icon` — per-market URLs

## Nine new ref-data fields per Sybil market

Stored in **`MarketRefData`** (off-block, same shelf as `external_url`):

| Field | Type | Source |
|---|---|---|
| `event_id` | `Option<String>` | Polymarket `event.id` — frontend grouping key |
| `event_title` | `Option<String>` | Polymarket `event.title` — MultiCard header |
| `event_image_url` | `Option<String>` | `event.image` |
| `event_icon_url` | `Option<String>` | `event.icon` — secondary image fallback |
| `event_end_date_ms` | `Option<u64>` | `event.endDate` parsed to epoch ms |
| `market_image_url` | `Option<String>` | `market.image` |
| `market_icon_url` | `Option<String>` | `market.icon` |
| `market_end_date_ms` | `Option<u64>` | `market.endDate` |
| `category` | `Option<String>` | derived from `event.tags[].label` |

All optional. All off-block. Nothing enters the block hash; matching engine is untouched.

## Tag → category derivation

Hardcoded priority-ordered table in a new `crates/sybil-polymarket/src/categorize.rs`. Walks top-to-bottom; first row whose labels intersect the event's tags wins. Unmatched → `None` (frontend shows no chip).

| # | Bucket | Tag labels (case-insensitive) |
|---|---|---|
| 1 | Politics | Trump · Congress · Senate |
| 2 | Geopolitics | Geopolitics |
| 3 | AI | AI |
| 4 | Tech | Tech |
| 5 | Economy | Economy |
| 6 | Culture | Culture · Movies · Music · Celebritards |
| 7 | Science | Science |
| 8 | World | World |
| 9 | Finance | Finance · Stocks · Earnings · IPOs · IPO |
| 10 | Business | Business |
| 11 | Weather | Weather |
| 12 | Mentions | Mentions |
| 13 | Sports | Sports · Soccer · NFL · NBA · NHL · MLB · UFC · Tennis · Boxing · Cricket · Chess · Hockey · Football · Golf · Formula 1 · Pickleball · EPL · MLS · PGA · Esports |
| 14 | Crypto | Crypto |
| 15 | Commodities | Commodities |

Worked examples: Kraken IPO event `[exchange, Tech, Crypto, Finance, Business, IPOs]` → **Tech**. MicroStrategy BTC event `[Finance, Economy, Business, Crypto, Stocks]` → **Economy**. Hypothetical `[NBA, Trump]` → **Politics**.

## Backend safety — what stays untouched

This is the part where care is critical.

1. **NegRisk `MarketGroup` semantics unchanged.** The matching engine's `GroupCoverageTracker` (`crates/matching-sequencer/src/sequencer.rs:425`) still rejects only NegRisk-set self-trades. We do **not** repurpose `MarketGroup` for frontend grouping — the new `event_id` field carries that signal independently.
2. **MM `in_group` flag unchanged.** `MmMessage::MarketMirrored.in_group = event.is_neg_risk()` stays; the MM continues to quote both YES and NO for independent binary markets.
3. **Nothing on-block.** `MarketMetadata` (block-hashed) untouched.
4. **Backend names of markets unchanged.** NegRisk markets keep their `"event title: outcome"` names; non-NegRisk keep raw questions. Frontend handles display with longest-common-prefix trimming.

## Files to change

### Backend (one PR `r/phase2-backend → main`):

```
crates/sybil-api-types/src/request.rs      extend SetMarketMetadataRequest with 9 fields
crates/sybil-api-types/src/response.rs     extend MarketResponse with 9 fields
crates/sybil-api/src/state.rs              extend MarketRefData + JSON persistence
crates/sybil-api/src/routes/markets.rs     pass MarketRefData snapshot to build_market_response
crates/sybil-polymarket/src/polymarket/types.rs   image, icon, tags on GammaEvent/Market
crates/sybil-polymarket/src/categorize.rs  NEW — priority table + unit tests
crates/sybil-polymarket/src/sync.rs        POST metadata after create_market
crates/sybil-polymarket/src/sybil/client.rs  add set_market_metadata helper
```

### Frontend (on `r/dev` after backend merges):

```
frontend/web/src/lib/api/schema.d.ts          regenerated (pnpm types:generate)
frontend/web/src/lib/markets/use-markets.ts   group by event_id
frontend/web/src/app/page.tsx                 drop MULTI_THRESHOLD; ≥2 markets per event → MultiCard
frontend/web/src/components/multi-card.tsx    use event_title + event_image; LCP label trim
frontend/web/src/components/binary-card.tsx   use market_image, real category, end date
frontend/web/src/components/market-thumb.tsx  two-step fallback: image → icon → tile
frontend/web/src/lib/mock.ts                  mockCategory becomes fallback only
frontend/STATUS.md, frontend/KNOWN_ISSUES.md  sync
```

## Deploy flow (fresh start — no backfill)

The user accepted starting from an empty mirror state, so there's no backfill module:

1. Ship the backend PR.
2. SSH to prod: stop `sybil-polymarket` and `sybil-api`.
3. `rm /data/polymarket_mapping.json /data/market_ref_data.json` to wipe state.
4. Set `MARKET_REF_DATA_PATH=/data/market_ref_data.json` on `sybil-api` (the `sybil-data:/data` volume is already mounted in `docker-compose.prod.yml`).
5. Restart both services.
6. Mirror re-syncs all events from scratch; every market is created with full metadata on first POST.

Trade-off accepted: old Sybil market IDs and their trading history are abandoned. On testnet with mostly-bot trading, that's fine.

## Verification

1. `cargo build -p sybil-api -p sybil-polymarket -p sybil-api-types`.
2. `cargo test -p sybil-polymarket categorize::tests` — table coverage + ambiguity.
3. Local: `just dev`, wait one sync cycle, `curl localhost:3000/v1/markets | jq '.[0]'` shows 9 new fields populated.
4. Restart sybil-api locally with a configured `MARKET_REF_DATA_PATH` — fields survive.
5. `cd frontend/web && pnpm dev` → `http://localhost:3000`:
   - Real S3 images on cards
   - No more dotted-yellow MockValue underline on category
   - Eyebrow "closes Mar 5" populated
   - Multi-market events render as MultiCard (Fed Decision, BTC threshold pairs)
   - Single-market events stay BinaryCard
6. Deploy + production smoke: `curl https://172-104-31-54.nip.io/v1/markets | jq '.[0] | {event_id, event_title, category, event_image_url, market_end_date_ms}'`.
7. Confirm no block-hash drift before/after a metadata POST.

## Tradeoffs we're knowingly making

Captured at the design-decision level in this plan and `KNOWN_ISSUES.md`. Headline summary:

1. **Off-block storage** — fast, cheap, but a verifier can't prove "category was X at block N." OK for display chrome.
2. **End-date display-only** — matching engine doesn't auto-close at endDate. That's already today's de facto behavior.
3. **Hardcoded tag→category table** — new tags fall to `None` until we extend it. Unmatched labels logged.
4. **JSON-on-disk persistence** — mirrors existing `MappingStore` pattern; saves on every write. Fine at current write rate.
5. **NegRisk MarketGroup separation** — frontend grouping (`event_id`) and matching-engine grouping (`MarketGroup`) deliberately decoupled.
6. **MM `in_group` flag stays NegRisk-only** — non-NegRisk shared-event markets still get YES + NO quotes.

## Out of scope

- On-block metadata storage / amend events.
- Recurring backfill loop to catch Polymarket re-tags.
- Externalized config file for the category table.
- CDN proxy for Polymarket S3 images.
- `expiry_timestamp_ms` enforcement at matching-engine level.
- Renaming non-NegRisk Sybil market names to event-relative labels (handled by frontend LCP trim instead).
