"""Live LLM-driven trader for Polymarket-mirrored markets.

Adapted from sim/llm_trader.py — removes SimulatedClock and server pause/resume.
Articles come from NewsFeed (real-time RSS) instead of pre-loaded files.
"""

import logging
import re
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone

import openai

from bots.base import BaseAgent
from sybil_client import Block, BuyNo, BuyYes, OrderSpec, SellNo, SellYes
from sybil_client.types import NANOS_PER_DOLLAR, Market

from .db import DecisionDB
from .news_feed import LiveArticle, NewsFeed

log = logging.getLogger(__name__)


# --------------------------------------------------------------------------- #
# Data types
# --------------------------------------------------------------------------- #
@dataclass
class PriceSnapshot:
    block_height: int
    timestamp: datetime
    yes_price: float


@dataclass
class TradeRecord:
    market_id: int
    articles: list[LiveArticle]
    analysis: str
    fair_value: float
    orders: list[OrderSpec]
    motivation: str
    raw_llm_response: str
    llm_duration_s: float
    block_height: int
    timestamp: datetime
    balance: float
    yes_pos: int
    no_pos: int


# --------------------------------------------------------------------------- #
# Helpers (copied from sim/llm_trader.py)
# --------------------------------------------------------------------------- #
def _describe_order(order: OrderSpec) -> str:
    price = order.limit_price_nanos / NANOS_PER_DOLLAR
    if isinstance(order, BuyYes):
        return f"BuyYes {order.quantity} @ ${price:.4f}"
    elif isinstance(order, BuyNo):
        return f"BuyNo {order.quantity} @ ${price:.4f}"
    elif isinstance(order, SellYes):
        return f"SellYes {order.quantity} @ ${price:.4f}"
    elif isinstance(order, SellNo):
        return f"SellNo {order.quantity} @ ${price:.4f}"
    return str(order)


# --------------------------------------------------------------------------- #
# System prompt (from sim/llm_trader.py, verbatim)
# --------------------------------------------------------------------------- #
SYSTEM_PROMPT = """\
You are trading in a prediction market using Frequent Batch Auctions.
All orders in a batch are matched simultaneously at a single clearing price — no order book, no first-come advantage. Batches clear every ~10 minutes. Orders persist for 3 batches (TTL=3).

Pricing:
- YES + NO = $1.00 always. If YES=$0.80, NO=$0.20.
- BUY_YES costs the YES price; BUY_NO costs the NO price
- Your limit price is the WORST price you'd accept — you'll pay the clearing price, not your limit

Direction:
- FAIR_VALUE > YES price → BUY_YES or SELL_NO (bullish)
- FAIR_VALUE < YES price → BUY_NO or SELL_YES (bearish)
- SELL_NO = bullish (selling cheap NO). SELL_YES = bearish (selling expensive YES).

Trading:
- Deploy at most 10-20% of your cash per trade. Only exception: edge >30 cents, then up to 40%. This is a long day — prices will move, and you want cash for better opportunities later.
- Keep at least 20-30% cash in reserve at all times.
- Sell when your thesis weakens or counter-evidence appears. HOLD if already positioned and no new edge.
- Extreme FAIR_VALUE (>0.85) requires extraordinary evidence. Most events have genuine uncertainty — reflect that. Update based on the full picture, not just the latest article.

Evidence discipline:
- Base your FAIR_VALUE on the article(s) provided, the current market price, and your prior fair value estimate.
- If the article contains no NEW information relevant to the market question, keep your FV near your PRIOR fair value — do not reset to the market price. Irrelevant news is not a reason to change your view.
- Only revise your FV significantly when an article provides DIRECT evidence for or against the market question. Tangential news warrants at most a 1-2 cent adjustment.
- Do NOT inject outside knowledge. But DO maintain conviction from previous evidence unless new information contradicts it.

Always respond in English regardless of article language."""


# --------------------------------------------------------------------------- #
# LiveLlmTrader
# --------------------------------------------------------------------------- #
class LiveLlmTrader(BaseAgent):
    """LLM trader for live deployment — no SimulatedClock, no server pause."""

    def __init__(
        self,
        client,
        account_id: int,
        news_feed: NewsFeed,
        api_key: str,
        persona: str,
        model_name: str = "minimax/minimax-m2.7",
        market_ids: list[int] | None = None,
        markets_info: dict[int, Market] | None = None,
        db: DecisionDB | None = None,
        min_llm_interval_s: float = 60.0,
        name: str | None = None,
    ):
        super().__init__(client, account_id, name or "LiveLlmTrader", market_ids)
        self.news_feed = news_feed
        self.api_key = api_key
        self.model_name = model_name
        self.persona = persona
        self.markets_info = markets_info or {}
        self.db = db
        self.min_llm_interval_s = min_llm_interval_s

        self._llm_client: openai.AsyncOpenAI | None = None
        self._last_llm_call: float = 0.0
        self._observed_first_block = False

        # Per-market state
        self.price_history: dict[int, list[PriceSnapshot]] = {}
        self.trade_log: dict[int, list[TradeRecord]] = {}
        self.fair_values: dict[int, float] = {}

    def _get_llm_client(self) -> openai.AsyncOpenAI:
        if self._llm_client is None:
            self._llm_client = openai.AsyncOpenAI(
                base_url="https://openrouter.ai/api/v1",
                api_key=self.api_key,
                timeout=openai.Timeout(60.0, connect=10.0),
                max_retries=0,
            )
        return self._llm_client

    async def _call_llm_raw(self, prompt: str) -> tuple[str, float]:
        llm = self._get_llm_client()
        t0 = time.monotonic()
        resp = await llm.chat.completions.create(
            model=self.model_name,
            messages=[{"role": "user", "content": prompt}],
            temperature=0.3,
            max_tokens=1024,
        )
        text = resp.choices[0].message.content or ""
        return text, time.monotonic() - t0

    def _format_recent_trades(self, market_id: int) -> str:
        records = self.trade_log.get(market_id, [])
        if not records:
            return "No trades yet."
        lines = []
        for rec in records[-5:]:
            t = rec.timestamp.strftime("%H:%M")
            if not rec.orders:
                lines.append(f"- [{t}] FV={rec.fair_value:.2f} | {rec.motivation}")
                lines.append("  HOLD")
            else:
                order_desc = ", ".join(_describe_order(o) for o in rec.orders)
                lines.append(f"- [{t}] FV={rec.fair_value:.2f} | {rec.motivation}")
                lines.append(f"  Submitted: {order_desc}")
            lines.append("")
        return "\n".join(lines).rstrip()

    def _build_prompt(
        self, articles: list[LiveArticle], market: Market, block: Block
    ) -> str:
        market_id = market.id

        # Get Polymarket reference price (the "real" market consensus)
        poly_price = self.news_feed.polymarket_prices.get_price(market_id)

        # Get Sybil clearing price
        prices = self.filter_markets(block)
        if market_id in prices:
            yes_nanos, _ = prices[market_id]
            sybil_yes = yes_nanos / NANOS_PER_DOLLAR
        else:
            sybil_yes = 0.0

        # Use Polymarket price as primary reference, Sybil as secondary
        # The MM quotes around poly_price, so that's the actionable price
        yes_price = poly_price if poly_price and poly_price > 0 else sybil_yes
        if yes_price <= 0:
            return ""  # No price data at all — skip
        no_price = 1 - yes_price

        # Price info line
        if poly_price and poly_price > 0:
            price_line = (
                f"- Polymarket consensus: YES=${poly_price:.4f} | NO=${1 - poly_price:.4f} "
                f"(this is the deep-market reference price)"
            )
            if sybil_yes > 0 and abs(sybil_yes - poly_price) > 0.01:
                price_line += f"\n- Sybil clearing price: YES=${sybil_yes:.4f} (may differ due to low liquidity)"
        else:
            price_line = f"- YES price: ${yes_price:.4f} | NO price: ${no_price:.4f}"

        # Price trend
        history = self.price_history.get(market_id, [])
        recent_prices = [s.yes_price for s in history[-5:]]
        if recent_prices:
            price_line += f"\n- Recent Sybil prices: {', '.join(f'{p:.4f}' for p in recent_prices)}"

        balance = self.current_balance
        yes_shares = self.get_position(market_id, "YES")
        no_shares = self.get_position(market_id, "NO")
        portfolio_value = balance + yes_shares * yes_price + no_shares * no_price
        cash_pct = (balance / portfolio_value * 100) if portfolio_value > 0 else 100

        last_fv = self.fair_values.get(market_id)
        last_fv_line = f"\n- Your last fair value estimate: {last_fv:.2f}" if last_fv else ""

        # Market context from Polymarket metadata
        context = ""
        if market.description:
            context += f"\n{market.description[:500]}"
        if market.resolution_criteria:
            context += f"\nResolution: {market.resolution_criteria[:200]}"

        # Articles
        if len(articles) == 1:
            art = articles[0]
            text = art.full_text[:3000] if art.full_text else "(text unavailable)"
            article_section = (
                f'New article from {art.source}:\n'
                f'"{art.title}"\n\n{text}'
            )
        else:
            budget_per = max(500, 6000 // len(articles))
            parts = ["New articles this batch:\n"]
            for idx, art in enumerate(articles, 1):
                text = art.full_text[:budget_per] if art.full_text else "(text unavailable)"
                parts.append(f'[{idx}] From {art.source}: "{art.title}"\n{text}\n')
            article_section = "\n".join(parts)

        # Available actions — use poly price for cost estimates
        actions = []
        if balance >= 0.01:
            max_yes = int(balance / yes_price) if yes_price > 0 else 0
            max_no = int(balance / no_price) if no_price > 0 else 0
            actions.append(f"- BUY_YES <qty> @ <price>: costs ~${yes_price:.2f}/share (up to ~{max_yes} shares)")
            actions.append(f"- BUY_NO <qty> @ <price>: costs ~${no_price:.2f}/share (up to ~{max_no} shares)")
        else:
            actions.append("- BUY_YES / BUY_NO: not available (no cash)")
        if yes_shares > 0:
            actions.append(f"- SELL_YES <qty> @ <price>: sell {yes_shares} YES shares")
        if no_shares > 0:
            actions.append(f"- SELL_NO <qty> @ <price>: sell {no_shares} NO shares")
        actions.append("- HOLD: do nothing")
        actions_block = "\n".join(actions)

        analyze_word = "these articles" if len(articles) > 1 else "this article"

        return f"""{SYSTEM_PROMPT}

{self.persona}

Market: "{market.name}"{context}

Current state:
{price_line}
- Your portfolio: ${balance:.2f} cash ({cash_pct:.0f}% of portfolio), {yes_shares} YES shares, {no_shares} NO shares
- Estimated portfolio value: ~${portfolio_value:.2f}{last_fv_line}

Recent trades:
{self._format_recent_trades(market_id)}

{article_section}

Available actions:
{actions_block}

Analyze {analyze_word} and decide your trade. Respond in this exact format:

ANALYSIS: [Your analysis of what {analyze_word} signals, 2-4 sentences]
FAIR_VALUE: [Your probability estimate, 0.01-0.99]
EDGE: [Calculate: |FAIR_VALUE - Polymarket price| = edge per share. Only trade if edge > $0.03]
ORDERS: [Choose from available actions, or HOLD if no edge. Set your LIMIT PRICE to cross the market maker's spread:
For BUY_YES: set limit 1-2 cents ABOVE the Polymarket YES price (to ensure fill).
For BUY_NO: set limit 1-2 cents ABOVE the Polymarket NO price (to ensure fill).
For SELL: set limit 1-2 cents BELOW the market price.]
MOTIVATION: [1-2 sentence thesis]"""

    def _parse_orders(
        self, text: str, market_id: int
    ) -> tuple[str, float, list[OrderSpec], str] | None:
        """Parse structured LLM output. Adapted from sim/llm_trader.py."""
        KEYWORDS = r"\nANALYSIS:|\nFAIR_VALUE:|\nEDGE:|\nORDERS:|\nMOTIVATION:|\Z"

        analysis_match = re.search(
            rf"ANALYSIS:\s*(.*?)(?={KEYWORDS})", text, re.DOTALL,
        )
        analysis = analysis_match.group(1).strip() if analysis_match else ""

        fv_match = re.search(r"FAIR_VALUE:\s*([\d.]+)", text)
        if not fv_match:
            log.warning("Failed to parse FAIR_VALUE from LLM output")
            return None
        fair_value = float(fv_match.group(1))
        if not 0.01 <= fair_value <= 0.99:
            log.warning("FAIR_VALUE out of range: %s", fair_value)
            return None

        motiv_match = re.search(
            rf"MOTIVATION:\s*(.*?)(?={KEYWORDS})", text, re.DOTALL,
        )
        motivation = motiv_match.group(1).strip() if motiv_match else ""

        orders_match = re.search(
            rf"ORDERS:\s*(.*?)(?={KEYWORDS})", text, re.DOTALL,
        )
        orders_text = orders_match.group(1).strip() if orders_match else ""

        orders: list[OrderSpec] = []
        if "HOLD" in orders_text.upper() and not re.search(r"(BUY|SELL)", orders_text.upper()):
            return (analysis, fair_value, [], motivation)

        order_map = {
            "BUY_YES": BuyYes,
            "BUY_NO": BuyNo,
            "SELL_YES": SellYes,
            "SELL_NO": SellNo,
        }
        for m in re.finditer(
            r"(BUY_YES|BUY_NO|SELL_YES|SELL_NO)\s+(\d+)\s*@\s*\$?([\d.]+)", orders_text
        ):
            side = m.group(1)
            qty = int(m.group(2))
            price = float(m.group(3))
            cls = order_map[side]
            if qty > 0:
                orders.append(cls.at_price(market_id, price, qty))

        return (analysis, fair_value, orders, motivation)

    def _validate_orders(
        self, orders: list[OrderSpec], market_id: int, block: Block
    ) -> list[OrderSpec]:
        """Clip orders to affordable/held amounts. Adapted from sim/llm_trader.py."""
        # Use Polymarket price as reference for validation when Sybil price is 0
        poly_price = self.news_feed.polymarket_prices.get_price(market_id)
        prices = self.filter_markets(block)
        if market_id in prices:
            yes_nanos, _ = prices[market_id]
            yes_price = yes_nanos / NANOS_PER_DOLLAR
        else:
            yes_price = 0.0
        if yes_price <= 0 and poly_price and poly_price > 0:
            yes_price = poly_price
        if yes_price <= 0:
            return []
        no_price = 1 - yes_price

        yes_held = self.get_position(market_id, "YES")
        no_held = self.get_position(market_id, "NO")
        cash = self.current_balance
        portfolio_value = cash + yes_held * yes_price + no_held * no_price
        max_order_value = portfolio_value * 0.25

        valid: list[OrderSpec] = []
        for order in orders:
            price = order.limit_price_nanos / NANOS_PER_DOLLAR
            price = max(0.01, min(0.99, price))
            qty = order.quantity

            if isinstance(order, SellYes):
                qty = min(qty, yes_held)
                yes_held -= qty
            elif isinstance(order, SellNo):
                qty = min(qty, no_held)
                no_held -= qty
            elif isinstance(order, (BuyYes, BuyNo)):
                max_qty_by_conc = int(max_order_value / price) if price > 0 else 0
                qty = min(qty, max_qty_by_conc)
                cost = qty * price
                if cost > cash:
                    qty = int(cash / price) if price > 0 else 0
                cash -= qty * price

            if qty <= 0:
                continue
            cls = type(order)
            valid.append(cls.at_price(market_id, price, qty))

        return valid

    async def on_block(self, block: Block) -> list[OrderSpec]:
        all_orders: list[OrderSpec] = []
        prices = self.filter_markets(block)
        now = datetime.now(timezone.utc)

        # Record price snapshots
        for market_id, (yes_nanos, _) in prices.items():
            yes_price = yes_nanos / NANOS_PER_DOLLAR
            self.price_history.setdefault(market_id, []).append(
                PriceSnapshot(block.height, now, yes_price)
            )
            # Cap history at 500 entries per market
            if len(self.price_history[market_id]) > 500:
                self.price_history[market_id] = self.price_history[market_id][-500:]

        # Skip first block
        if not self._observed_first_block:
            self._observed_first_block = True
            return []

        # Rate limit
        elapsed = time.monotonic() - self._last_llm_call
        if elapsed < self.min_llm_interval_s and self._last_llm_call > 0:
            return []

        # Check for new articles across all tracked markets
        for market_id in list(self.market_ids or []):
            articles = await self.news_feed.drain(market_id)
            if not articles:
                continue

            market = self.markets_info.get(market_id)
            if not market:
                continue

            # Get reference price: prefer Polymarket, fall back to Sybil clearing
            poly_price = self.news_feed.polymarket_prices.get_price(market_id)
            if market_id in prices:
                yes_nanos, _ = prices[market_id]
                sybil_price = yes_nanos / NANOS_PER_DOLLAR
            else:
                sybil_price = 0.0
            ref_price = poly_price if poly_price and poly_price > 0 else sybil_price
            if ref_price <= 0:
                log.info("[%s] Skipping %s — no price data", self.name, market.name[:30])
                continue

            titles = "; ".join(f'"{a.title[:40]}"' for a in articles)
            log.info("[%s] %d article(s) for %s (poly=%.2f): %s",
                     self.name, len(articles), market.name[:30],
                     poly_price or 0, titles)

            prompt = self._build_prompt(articles, market, block)
            if not prompt:
                continue

            try:
                raw_text, llm_duration_s = await self._call_llm_raw(prompt)
                self._last_llm_call = time.monotonic()
                log.info("[%s] LLM response (%.1fs):\n%s", self.name, llm_duration_s, raw_text)
            except Exception as e:
                log.warning("[%s] LLM call failed: %s", self.name, e)
                continue

            parsed = self._parse_orders(raw_text, market_id)
            if parsed is None:
                log.warning("[%s] Failed to parse LLM output", self.name)
                continue

            analysis, fair_value, orders, motivation = parsed
            orders = self._validate_orders(orders, market_id, block)

            # Directional consistency filter (use reference price)
            consistent = []
            for o in orders:
                if isinstance(o, (BuyYes, SellNo)) and fair_value < ref_price:
                    continue
                if isinstance(o, (BuyNo, SellYes)) and fair_value > ref_price:
                    continue
                consistent.append(o)
            orders = consistent

            # Update fair value
            self.fair_values[market_id] = fair_value

            # Log trade
            record = TradeRecord(
                market_id=market_id,
                articles=articles,
                analysis=analysis,
                fair_value=fair_value,
                orders=orders,
                motivation=motivation,
                raw_llm_response=raw_text,
                llm_duration_s=llm_duration_s,
                block_height=block.height,
                timestamp=now,
                balance=self.current_balance,
                yes_pos=self.get_position(market_id, "YES"),
                no_pos=self.get_position(market_id, "NO"),
            )
            self.trade_log.setdefault(market_id, []).append(record)

            # DB logging
            if self.db:
                order_dicts = [
                    {"side": type(o).__name__, "qty": o.quantity,
                     "price": o.limit_price_nanos / NANOS_PER_DOLLAR}
                    for o in orders
                ]
                self.db.log_decision(
                    trader_name=self.name,
                    market_id=market_id,
                    market_name=market.name,
                    analysis=analysis,
                    fair_value=fair_value,
                    market_price=ref_price,
                    orders=order_dicts,
                    motivation=motivation,
                    raw_llm_response=raw_text,
                    llm_duration_s=llm_duration_s,
                    balance=self.current_balance,
                    yes_pos=self.get_position(market_id, "YES"),
                    no_pos=self.get_position(market_id, "NO"),
                )

            if orders:
                order_desc = ", ".join(_describe_order(o) for o in orders)
                log.info("[%s] %s: FV=%.2f -> %s", self.name, market.name[:30], fair_value, order_desc)
            else:
                log.info("[%s] %s: FV=%.2f -> HOLD", self.name, market.name[:30], fair_value)

            all_orders.extend(orders)

        return all_orders
