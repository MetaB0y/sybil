# Iran Simulation — Gap Analysis

Goal: Run a 6-hour simulation (Jan 2 08:00–17:00) with 1 market maker, 10 noise traders, and 1 American Trader bot reacting to 12 pre-accepted news articles.

## What EXISTS and works

1. **Sybil matching engine** (Rust) — `cargo run --release -p sybil-api -- --dev-mode`
2. **sybil_client** — Python SDK, order types (BuyYes/BuyNo/SellYes/SellNo), SSE block streaming
3. **BaseAgent** — run loop, position tracking, fill notifications
4. **RandomTrader** — works as-is for noise traders, matches bot_logic.md spec
5. **FlashMarketMaker / WideFlashMM** — has skew + multi-level quoting, matches bot_logic.md
6. **BacktestRunner / SimulatedClock / NewsScheduler** — time-compressed backtesting framework
7. **Phase 1 results + article texts** — pre-fetched for Jan 2 08:00-17:00 (12 accepted, 1 skip, 7 rejected)

## What's MISSING (must build)

### 1. American Trader bot (the news-reactive LLM trader)

No code exists. Need a new bot class that:
- Extends `BacktestAgent` (needs `on_news` + `on_block`)
- Phase 1: calls Kimi API with headline → YES/NO filter
- Phase 2: calls Kimi API with full article + market state → PROBABILITY/CONVICTION/MOTIVATION
- Phase 3: mechanical execution (cancel all → edge check → target position → orders)
- Tracks recent trades with motivations (fed back into Phase 2 prompt)

**Shortcut for first test**: since we already pre-ran Phase 1 and have article texts, skip Phase 1 at runtime and feed the 12 pre-accepted articles directly, only calling Kimi for Phase 2.

### 2. Dataset adapter

The existing `Dataset` / `NewsItem` schema is sports-oriented:
- `Event` has `home_team`, `away_team`, `moneyline`, `actual_outcome`
- `NewsItem.source` is typed as `Literal["lineup", "injury", "in_game", "weather", "other"]`
- Our Iran market is a single market with GDELT articles (headline, source, URL, full text)

Options:
- Adapt the existing schema (awkward, source type doesn't fit)
- Write a simpler runner that skips the sports abstractions

### 3. Runner adaptation

`BacktestRunner` assumes multiple sports events with resolution. We need:
- Single market creation with a seed price (~0.10 for YES)
- MM starting with 10k YES + 10k NO (current bots start flat)
- No mid-sim resolution
- Different agent setup (MM doesn't need `clock`/`event_market_map`)

### 4. MM starting inventory

bot_logic.md says MM starts with 10,000 YES + 10,000 NO. Neither `SimpleMarketMaker` nor `FlashMarketMaker` supports this. Options:
- Pre-mint via the API (create account, mint shares somehow)
- Give MM lots of USDC and let it accumulate inventory naturally
- Modify the MM to work without starting inventory

### 5. Initial price seeding

First batch has no clearing price. `filter_markets()` defaults to 50/50 (0.50). bot_logic.md says we need ~0.10.
- Need a "seed trade" in the first batch to establish the initial price
- Or set initial price via the API if supported

## Inconsistencies: bot_logic.md vs existing code

| bot_logic.md | Existing code | Gap |
|---|---|---|
| Phase 3: cancel ALL resting orders | No cancel order API in sybil_client | Orders expire after 3 blocks (TTL), may be sufficient |
| Phase 3: limit price at PROBABILITY | InformedTrader bids at `market_prob + 0.01` | Different logic — need new bot |
| Phase 3: target-position-based sizing | InformedTrader uses fixed `order_size` | Completely different approach |
| MM starts with 10k YES + 10k NO | MMs start flat | Need pre-minting |
| MM quotes off last_batch_price only | FlashMarketMaker does this already | OK — consistent |
| Noise: 10 instances, $20 each | RandomTrader exists, configurable | OK — just config |

## Shortest path to a working 6-hour demo

Skip the full BacktestRunner. Write a Jupyter notebook that:

1. Start sybil-api (`cargo run --release -p sybil-api -- --dev-mode --port 3001`)
2. Create 1 market, seed initial price at 0.10
3. Create MM account (give lots of USDC, or pre-mint 10k+10k if API supports)
4. Create 10 noise trader accounts ($20 each)
5. Create 1 American Trader account ($1000)
6. Feed the 12 pre-accepted articles sequentially:
   - For each article: call Kimi Phase 2 → run Phase 3 mechanically → submit orders
   - Wait for batch → log results (clearing price, fills, portfolio changes)
   - MM and noise traders submit orders on every batch in between
7. After all articles: print final state (positions, PnL, price history)

This avoids the sports-oriented BacktestRunner entirely and lets us see:
- Does Phase 2 produce sensible PROBABILITY/CONVICTION for each article?
- Does Phase 3 generate correct orders?
- Do trades actually execute against MM + noise?
- How does the price evolve over 12 articles?
