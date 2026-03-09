# Iran Strike Market — Bot Logic

Market: "Will US strike Iran by March 31?"
Period: 2026-01-01 to 2026-02-18

## Bot Architecture

Each bot has two modes:
1. **Reactive** — triggered by incoming news articles
2. **Periodic** — every 2 hours, portfolio rebalancing

Reactive pipeline has 3 phases:
- Phase 1: Relevance filter (headline only, yes/no)
- Phase 2: Analysis + probability estimate (full context)
- Phase 3: Trade execution (mechanical, no LLM)

---

## Phase 1: Relevance Filter

**Goal**: Cheaply filter out irrelevant news. Headline-only, binary answer.

**Input**: News headline, source name

**Prompt**:
```
You are a prediction market trader specializing in US-Iran geopolitics.

You're monitoring the market: "Will the United States carry out a military strike against Iran before March 31, 2026?"

A new headline just appeared:

"[HEADLINE]" — [SOURCE]

Could this news plausibly shift the probability of a US military strike on Iran?
Focus on: military moves, threats or warnings between US and Iran, diplomatic signals, sanctions policy, nuclear developments, regional proxy conflicts, or direct US-Iran confrontation.
Ignore news that mentions Iran only in passing, covers unrelated topics, or is primarily about domestic economics, culture, or entertainment.

Answer only YES or NO.
```

**Notes**:
- No chain of thought needed — keep tokens minimal
- "Focus on" with explicit categories prevents over-broad reasoning chains
- Explicit ignore list (economics, culture, entertainment) gives the model permission to say NO on tangential stuff
- Source name included because credibility matters (Fox News vs random blog)

---

## Phase 2: Analysis + Probability Estimate

**Goal**: Given a relevant article, estimate the true probability of the event and conviction level. This output feeds directly into mechanical Phase 3.

**Input**:
- Full article text + source (headline passed Phase 1, now we fetch the full article)
- Static context paragraph (geopolitical background — same for all calls)
- Market price data (last batch price + last 7 days)
- Bot's last 3 trades with timestamp, size and motivation
- Bot's current portfolio (USDC, YES shares, NO shares)
- Bot's resting (unfilled) orders in the orderbook

**Prompt**:
```
You are a professional forecaster and prediction market trader specializing in US-Iran geopolitics.

You're trading on the market: "Will the United States carry out a military strike against Iran before March 31, 2026?"

Context:
USA-Iran tensions stem from long-standing issues like Iran's nuclear program and proxies, but escalated sharply after the June 2025 US strikes on Iranian nuclear sites during the Israel-Iran Twelve-Day War. They rose further in early January 2026 amid Iran's crackdown on anti-government protests, prompting President Trump to threaten military action and review strike options.

Market data:
Last batch YES price: [PRICE]
Last 7 days: [D1, D2, D3, D4, D5, D6, D7]

Your recent trades:
[RECENT_TRADES or "No trades yet."]

Your resting orders:
[RESTING_ORDERS or "None."]

Your portfolio: [USDC] USDC, [YES] YES shares, [NO] NO shares

You've just received this article from [SOURCE]:

"[HEADLINE]"

[FULL_ARTICLE_TEXT]

Analyze this article. Use chain of thought:
1. What does this article signal about the likelihood of a US strike on Iran?
2. How significant is this signal? Is it a concrete development or speculation/opinion?
3. Consider the source credibility and potential bias.
4. How does this fit with the recent price trend and your previous trades?

Then provide your conclusion in exactly this format:

MOTIVATION: [1-2 sentence thesis]
PROBABILITY: [your estimate, 0.00 to 1.00]
CONVICTION: [LOW / MEDIUM / HIGH]
```

**Output parsing**: We extract PROBABILITY and CONVICTION mechanically. MOTIVATION is logged for debugging and fed back as context in future trades.

**Open questions**:
- Full article fetching: GDELT only gives headlines + URLs. Need to scrape article text. Strategy TBD (pre-fetch all articles? fetch on demand? handle paywalls/failures?)

**Notes**:
- Full article text is key — the bot reads the actual article, not just the headline. This is the whole point.
- Context paragraph grounds the model in geopolitical background, saving tokens on every call (model doesn't need to reconstruct history each time)
- Chain of thought before conclusion prevents anchoring on the current price
- Resting orders prevent duplicate order placement (bot won't place a new buy YES if one is already sitting in the book)
- Portfolio state is included so the model is aware of existing exposure
- Recent trades with motives prevent flip-flopping — the model sees its own reasoning history and can infer its previous probability estimates from them
- We do NOT ask the model to make a trade — that's Phase 3's job. Clean separation of analysis from execution.

---

## Phase 3: Trade Execution (Mechanical)

**Goal**: Convert Phase 2 output (PROBABILITY, CONVICTION) into concrete orders. No LLM — pure deterministic rules.

**Input**:
- PROBABILITY (0.00–1.00) from Phase 2
- CONVICTION (LOW / MEDIUM / HIGH) from Phase 2
- Last batch YES price
- Current portfolio: USDC, YES shares, NO shares
- Current resting orders

### Step 0: Cancel contradictory resting orders

Cancel ALL resting orders. The bot recomputes from scratch every time it reacts to news. This avoids all edge cases around stale/contradictory orders.

### Step 1: Compute total capital

```
total_capital = USDC + YES_shares × last_price + NO_shares × (1 - last_price)
```

This is the bot's total wealth measured at current market prices.

### Step 2: Check edge — is it worth trading?

```
edge = |PROBABILITY - last_price|
```

Minimum edge thresholds by conviction:
- LOW: edge > 0.05
- MEDIUM: edge > 0.03
- HIGH: edge > 0.02

If edge is below threshold → do nothing. The bot doesn't think the mispricing is large enough to justify the trade given its confidence level.

### Step 3: Compute target position

Risk budget as percentage of total capital, by conviction:
- LOW: 5%
- MEDIUM: 15%
- HIGH: 30%

If edge is very large (> 0.15), bump risk one tier: LOW→MEDIUM, MEDIUM→HIGH, HIGH→50%.

```
risk_budget = risk_pct × total_capital

if PROBABILITY > last_price:
    target_yes = risk_budget / PROBABILITY    # number of YES shares
    target_no  = 0
else:
    target_yes = 0
    target_no  = risk_budget / (1 - PROBABILITY)  # number of NO shares
```

### Step 4: Generate orders

Compare target position to current position and emit orders:

```
# Close wrong-side positions
if target_no == 0 and current_no > 0:
    → Sell current_no NO shares at limit (1 - PROBABILITY)

if target_yes == 0 and current_yes > 0:
    → Sell current_yes YES shares at limit PROBABILITY

# Adjust right-side positions
if target_yes > current_yes:
    → Buy (target_yes - current_yes) YES at limit PROBABILITY

if target_no > current_no:
    → Buy (target_no - current_no) NO at limit (1 - PROBABILITY)

# If target equals current → do nothing (already correctly positioned)
```

All limit prices are set to PROBABILITY (for YES) or 1-PROBABILITY (for NO). This means "I'll trade up to what I think fair value is." In FBA the clearing price is uniform, so this is just the bot's maximum willingness to pay.

### Examples

**Example 1: Fresh bot, bullish signal**
- PROB=0.25, CONVICTION=MEDIUM, price=0.15
- Portfolio: 1000 USDC, 0 YES, 0 NO
- Edge: 0.10 > 0.03 threshold → trade
- total_capital = 1000
- risk_budget = 15% × 1000 = 150 USDC
- target_yes = 150 / 0.25 = 600 shares
- → Buy 600 YES at limit 0.25

**Example 2: Holding YES, bearish reversal**
- PROB=0.10, CONVICTION=HIGH, price=0.20
- Portfolio: 500 USDC, 200 YES, 0 NO
- Edge: 0.10 > 0.02 threshold → trade
- total_capital = 500 + 200×0.20 = 540
- risk_budget = 30% × 540 = 162 USDC
- target_yes = 0, target_no = 162 / 0.90 = 180 shares
- → Sell 200 YES at limit 0.10
- → Buy 180 NO at limit 0.90

**Example 3: Already positioned correctly, conviction increase**
- PROB=0.30, CONVICTION=HIGH, price=0.20
- Portfolio: 700 USDC, 100 YES, 0 NO
- Edge: 0.10 > 0.02 → trade
- total_capital = 700 + 100×0.20 = 720
- risk_budget = 30% × 720 = 216 USDC
- target_yes = 216 / 0.30 = 720 shares
- → Buy 620 more YES at limit 0.30 (720 target - 100 current)

**Example 4: Edge too small**
- PROB=0.16, CONVICTION=LOW, price=0.15
- Edge: 0.01 < 0.05 threshold → do nothing

### Notes
- All arithmetic in the actual implementation will use integer nanos (1 dollar = 1,000,000,000 nanos), not floats. Examples use decimals for readability.
- The "cancel all resting orders" approach is simple but aggressive. Alternative: only cancel contradictory orders. Starting with cancel-all for simplicity, can refine later.
- The sell + buy in Example 2 both go into the same FBA batch, so they execute atomically.
- 50% max risk (HIGH conviction + large edge) ensures the bot never goes all-in.

---

## Periodic Rebalancing

TODO

---

## Market Maker

**Goal**: Provide continuous two-sided liquidity. Anchors prices via spread quoting, manages inventory risk via skew. Does NOT use external price feeds — quotes purely off last batch price.

**Starting state**: 10,000 YES shares + 10,000 NO shares + USDC for quoting.

### Logic (every batch)

```
mid = last_batch_clearing_price

# Inventory skew: shift mid away from heavy side to attract rebalancing flow
# If yes_pos > no_pos (excess YES), shift mid DOWN → cheaper YES attracts buyers
# If no_pos > yes_pos (excess NO), shift mid UP → cheaper NO attracts buyers
skew = (yes_pos - no_pos) * SKEW_FACTOR * 0.01
adjusted_mid = clamp(mid + skew, 0.05, 0.95)

# Multi-level quoting: multiple price levels for depth
half_spread = HALF_SPREAD_BPS / 10000      # e.g. 100 bps = 0.01
level_spacing = LEVEL_SPACING_BPS / 10000  # e.g. 50 bps = 0.005

for level in 0..NUM_LEVELS:
    offset = half_spread + level * level_spacing
    buy_yes_price = adjusted_mid - offset
    buy_no_price  = (1 - adjusted_mid) - offset
    → Buy QUOTE_SIZE YES at buy_yes_price
    → Buy QUOTE_SIZE NO  at buy_no_price
```

### Parameters

| Parameter | Value | Notes |
|-----------|-------|-------|
| HALF_SPREAD_BPS | 100 | Total spread = 2% (200 bps) |
| NUM_LEVELS | 2-3 | Depth levels |
| LEVEL_SPACING_BPS | 50-100 | Gap between levels |
| QUOTE_SIZE | 10-50 | Shares per level per side |
| SKEW_FACTOR | 0.05-0.15 | How aggressively to rebalance |

### How minting works with MM quotes

When MM posts BuyYes at 0.14 and a trader posts BuyNo at 0.87, they match via minting (0.14 + 0.87 ≥ 1.00). The MM gets YES shares, the trader gets NO shares. Total cost $1 split between them.

Similarly, when the MM has accumulated YES shares, traders buying YES from MM is done by the trader posting BuyYes that matches with MM's existing sell orders, or the MM can post SellYes orders directly.

### Existing code

`arena/bots/market_maker.py` already has:
- **`SimpleMarketMaker`** — basic version, quotes both sides, sells excess inventory via SellYes/SellNo, has `max_position` cap. No skew.
- **`FlashMarketMaker`** — inventory skew via `_compute_skew()`, multi-level quoting, uses `mm_budget_nanos` flash liquidity (capital efficient but may not be needed).
- **`WideFlashMM(FlashMarketMaker)`** — pre-configured with 200bps spread, 2 levels, 100bps spacing, quote_size=8, skew=0.05.
- **`TightFlashMM(FlashMarketMaker)`** — 50bps spread, 4 levels, 25bps spacing, more aggressive.

All extend `BaseAgent` from `arena/bots/base.py` which handles the run loop, position tracking, and fill notifications.

### What we need to adjust for our simulation

1. **Starting inventory**: Current bots start flat. We want 10k YES + 10k NO. Either pre-mint via the API or modify the bot to track initial inventory.
2. **Flash liquidity**: `FlashMarketMaker` uses `mm_budget_nanos` which lets the solver pick optimal fill subsets. For simplicity we may want to start with `SimpleMarketMaker` style (per-order balance) and switch to flash later if needed.
3. **Initial price**: First batch has no clearing price. Need to seed with a starting price (e.g. Polymarket price on Jan 1, or a reasonable prior like 0.10).

---

## Noise Traders

**Goal**: Add volume and make the orderbook realistic. No intelligence, no directional bias.

**Implementation**: 10 instances of `RandomTrader` from `arena/bots/random_trader.py`. No new bot class needed.

### Per-instance config

| Parameter | Value | Notes |
|-----------|-------|-------|
| Starting balance | $20 | $200 total noise capital |
| trade_probability | 0.5–0.8 | Tune for desired volume |
| min_size | 1 | |
| max_size | 5 | Under $10 per trade at typical prices |
| seed | unique per instance | Reproducibility |

### Behavior (per batch, per instance)

1. Coin flip at `trade_probability` — if fail, do nothing
2. Pick one random action from available:
   - **buy_yes** — always available
   - **buy_no** — always available
   - **sell_yes** — only if holding YES shares
   - **sell_no** — only if holding NO shares
3. Random price near last clearing price (multiplicative noise: 0.9–1.05x for buys, 0.95–1.1x for sells)
4. Random size in `[min_size, max_size]`
5. Emit single order

### Volume math

- 10 instances × `trade_probability` = avg orders per batch
- At 0.5: ~5/batch. At 0.8: ~8/batch.
- Starts buy-heavy (no positions to sell), gradually mixes in sells as positions accumulate
- 50/50 YES/NO bias when flat (2 buy actions equally weighted)
