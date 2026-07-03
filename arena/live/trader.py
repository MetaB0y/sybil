"""Live LLM-driven trader with pluggable sizing strategy.

Architecture:
- LLM provides FAIR_VALUE estimates (probability analysis)
- Sizing strategy computes target positions (Kelly, Flat, etc.)
- Position management runs periodically (active selling when edge shrinks)
- LLM is only called when new articles arrive; sizing is continuous
"""

import logging
import re
import time
from dataclasses import dataclass
from datetime import datetime, timezone

import openai

from bots.base import BaseAgent
from sybil_client import Block, OrderSpec
from sybil_client.types import NANOS_PER_DOLLAR, Market

from .db import DecisionDB
from .news_feed import LiveArticle, NewsFeed, NewsSubscription
from .strategy import RESOLVED_HIGH, RESOLVED_LOW, KellyStrategy, SizingStrategy, position_orders

log = logging.getLogger(__name__)

MIN_CASH_FRAC = 0.20  # Keep at least 20% cash


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
# Helpers
# --------------------------------------------------------------------------- #
def _describe_order(order: OrderSpec) -> str:
    from sybil_client import BuyNo, BuyYes, SellNo, SellYes
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


def _order_to_log_dict(order: OrderSpec) -> dict:
    from sybil_client import BuyNo, BuyYes, SellNo, SellYes

    if isinstance(order, BuyYes):
        side = "BUY_YES"
    elif isinstance(order, BuyNo):
        side = "BUY_NO"
    elif isinstance(order, SellYes):
        side = "SELL_YES"
    elif isinstance(order, SellNo):
        side = "SELL_NO"
    else:
        side = type(order).__name__.upper()

    return {
        "market_id": getattr(order, "market_id", None),
        "side": side,
        "qty": getattr(order, "quantity", 0),
        "price": getattr(order, "limit_price_nanos", 0) / NANOS_PER_DOLLAR,
    }


# --------------------------------------------------------------------------- #
# System prompt
# --------------------------------------------------------------------------- #
SYSTEM_PROMPT = """\
You are analyzing news articles for a prediction market. Your job is to estimate the probability
of the event occurring, given the evidence.

You will be given:
- A market question
- Current market price (from Polymarket)
- Your previous fair value estimate (if any)
- Your current portfolio
- One or more news articles

Respond with your probability estimate and brief reasoning. Be concise.

Key principles:
- Base your estimate on the article evidence + prior fair value, not just the market price
- If the article contains no NEW information, keep your estimate near your prior fair value
- Only revise significantly for DIRECT evidence — tangential news warrants at most 1-2 cent
  adjustment
- Official actions > direct quotes > analysis > speculation > rumors
- Most events have genuine uncertainty — avoid extreme probabilities unless evidence is
  extraordinary

Always respond in English regardless of article language."""


# --------------------------------------------------------------------------- #
# LiveLlmTrader
# --------------------------------------------------------------------------- #
class LiveLlmTrader(BaseAgent):
    """LLM-driven analysis + pluggable sizing strategy.

    The LLM provides fair value estimates. The strategy computes target
    positions. Position management runs periodically based on strategy's
    rebalance_interval_s.
    """

    def __init__(
        self,
        client,
        account_id: int,
        news_feed: NewsFeed,
        api_key: str,
        persona: str,
        strategy: SizingStrategy | None = None,
        model_name: str = "deepseek/deepseek-v4-flash",
        market_ids: list[int] | None = None,
        markets_info: dict[int, Market] | None = None,
        db: DecisionDB | None = None,
        min_llm_interval_s: float = 60.0,
        name: str | None = None,
    ):
        super().__init__(client, account_id, name or "LiveLlmTrader", market_ids)
        self.news_feed = news_feed
        # SYB-192: each trader drains its OWN subscriber view so the Kelly and
        # Flat arms both see every article. When the feed is wired after
        # construction (the runner passes news_feed=None), attach_news_feed
        # registers the subscription then.
        self.news_sub: NewsSubscription | None = (
            news_feed.subscribe(name=name) if news_feed is not None else None
        )
        self.api_key = api_key
        self.model_name = model_name
        self.persona = persona
        self.strategy = strategy or KellyStrategy()
        self.markets_info = markets_info or {}
        self.db = db
        self.min_llm_interval_s = min_llm_interval_s

        self._llm_client: openai.AsyncOpenAI | None = None
        self._last_llm_call: float = 0.0
        self._last_rebalance: float = 0.0
        self._observed_first_block = False

        # Per-market state
        self.price_history: dict[int, list[PriceSnapshot]] = {}
        self.trade_log: dict[int, list[TradeRecord]] = {}
        self.fair_values: dict[int, float] = {}
        self._pending_order_logs: list[dict] = []

    def attach_news_feed(self, feed: NewsFeed) -> None:
        """Wire in the shared feed and register this trader's own subscription.

        Used by the runner, which constructs traders before the feed exists.
        """
        self.news_feed = feed
        self.news_sub = feed.subscribe(name=self.name)

    def _record_trade(
        self,
        market_id: int,
        market_name: str,
        fair_value: float,
        orders: list[OrderSpec],
        motivation: str,
        analysis: str,
        raw_llm_response: str,
        llm_duration_s: float,
        market_price: float,
        block_height: int,
        timestamp: datetime,
        articles: list[LiveArticle] | None = None,
    ) -> None:
        yes_pos = self.get_position(market_id, "YES")
        no_pos = self.get_position(market_id, "NO")
        record = TradeRecord(
            market_id=market_id,
            articles=articles or [],
            analysis=analysis,
            fair_value=fair_value,
            orders=orders,
            motivation=motivation,
            raw_llm_response=raw_llm_response,
            llm_duration_s=llm_duration_s,
            block_height=block_height,
            timestamp=timestamp,
            balance=self.current_balance,
            yes_pos=yes_pos,
            no_pos=no_pos,
        )
        records = self.trade_log.setdefault(market_id, [])
        records.append(record)
        if len(records) > 200:
            self.trade_log[market_id] = records[-200:]

        if self.db:
            article_urls = [
                {"title": a.title, "url": a.url, "source": a.source}
                for a in (articles or [])
            ]
            self.db.log_decision(
                trader_name=self.name,
                market_id=market_id,
                market_name=market_name,
                analysis=analysis,
                fair_value=fair_value,
                market_price=market_price,
                orders=[_order_to_log_dict(order) for order in orders],
                motivation=motivation,
                raw_llm_response=raw_llm_response,
                llm_duration_s=llm_duration_s,
                balance=self.current_balance,
                yes_pos=yes_pos,
                no_pos=no_pos,
                article_urls=article_urls,
            )

    # -- LLM --

    def _get_llm_client(self) -> openai.AsyncOpenAI:
        if self._llm_client is None:
            self._llm_client = openai.AsyncOpenAI(
                base_url="https://openrouter.ai/api/v1",
                api_key=self.api_key,
                timeout=openai.Timeout(60.0, connect=10.0),
                max_retries=0,
            )
        return self._llm_client

    async def _call_llm(self, prompt: str) -> tuple[str, float]:
        llm = self._get_llm_client()
        t0 = time.monotonic()
        resp = await llm.chat.completions.create(
            model=self.model_name,
            messages=[{"role": "user", "content": prompt}],
            temperature=0.3,
            max_tokens=2048,
            extra_body={"reasoning": {"max_tokens": 1024}},
        )
        text = resp.choices[0].message.content or ""
        duration = time.monotonic() - t0
        if resp.usage:
            log.info(
                "[%s] tokens: prompt=%d completion=%d (%.1fs)",
                self.name, resp.usage.prompt_tokens,
                resp.usage.completion_tokens, duration,
            )
            if self.db:
                self.db.log_token_usage(
                    self.name, resp.usage.prompt_tokens,
                    resp.usage.completion_tokens, self.model_name, duration,
                )
        return text, duration

    # -- Price helpers --

    def _get_market_price(self, market_id: int, block: Block) -> float:
        # AR-1: prefer the freshly polled Polymarket mid so the sizing engine
        # sees the same price the LLM prompt is shown. The startup
        # reference_price_nanos snapshot is only a fallback — it used to win
        # here and froze sizing prices at process start.
        poly_price = self.news_feed.polymarket_prices.get_price(market_id)
        if poly_price and poly_price > 0:
            return poly_price

        market = self.markets_info.get(market_id)
        if market and market.reference_price_nanos is not None and market.reference_price_nanos > 0:
            return market.reference_price_nanos / NANOS_PER_DOLLAR

        if market_id in block.clearing_prices:
            yes_nanos, _ = block.clearing_prices[market_id]
            return yes_nanos / NANOS_PER_DOLLAR

        return 0.0

    def _observed_market_prices(self, block: Block) -> dict[int, tuple[int, int]]:
        """Prices worth recording for live trading, without synthetic 50/50s."""
        market_ids = self.market_ids or set(block.clearing_prices.keys())
        prices: dict[int, tuple[int, int]] = {}
        for market_id in market_ids:
            ref = self._get_market_price(market_id, block)
            if ref > 0:
                yes_nanos = int(ref * NANOS_PER_DOLLAR)
                prices[market_id] = (yes_nanos, NANOS_PER_DOLLAR - yes_nanos)
        return prices

    def _portfolio_value(self, block: Block) -> float:
        pv = self.current_balance
        for (mid, outcome), qty in self.positions.items():
            if qty == 0:
                continue
            price = self._get_market_price(mid, block)
            if price <= 0:
                continue
            if outcome == "YES":
                pv += qty * price
            else:
                pv += qty * (1 - price)
        return max(pv, 0.01)

    # -- Prompt building --

    def _format_recent_trades(self, market_id: int) -> str:
        records = self.trade_log.get(market_id, [])
        if not records:
            return "No trades yet."
        lines = []
        for rec in records[-5:]:
            t = rec.timestamp.strftime("%H:%M")
            if not rec.orders:
                lines.append(f"- [{t}] FV={rec.fair_value:.2f} | {rec.motivation}")
            else:
                order_desc = ", ".join(_describe_order(o) for o in rec.orders)
                lines.append(f"- [{t}] FV={rec.fair_value:.2f} -> {order_desc} | {rec.motivation}")
        return "\n".join(lines).rstrip()

    def _build_prompt(
        self, articles: list[LiveArticle], market: Market, block: Block
    ) -> str:
        market_id = market.id
        yes_price = self._get_market_price(market_id, block)
        if yes_price <= 0:
            return ""

        poly_price = self.news_feed.polymarket_prices.get_price(market_id)
        if poly_price and poly_price > 0:
            price_line = f"- Polymarket consensus: YES=${poly_price:.4f} | NO=${1 - poly_price:.4f}"
        else:
            price_line = f"- YES price: ${yes_price:.4f} | NO price: ${1 - yes_price:.4f}"

        history = self.price_history.get(market_id, [])
        recent_prices = [s.yes_price for s in history[-5:]]
        if recent_prices:
            price_line += f"\n- Recent prices: {', '.join(f'{p:.4f}' for p in recent_prices)}"

        balance = self.current_balance
        yes_shares = self.get_position(market_id, "YES")
        no_shares = self.get_position(market_id, "NO")
        pv = self._portfolio_value(block)

        last_fv = self.fair_values.get(market_id)
        last_fv_line = f"\n- Your last fair value estimate: {last_fv:.2f}" if last_fv else ""
        portfolio_line = (
            f"- Your portfolio: ${balance:.2f} cash, {yes_shares} YES shares, "
            f"{no_shares} NO shares (~${pv:.0f} total){last_fv_line}"
        )

        context = ""
        if market.description:
            context += f"\n{market.description[:500]}"
        if market.resolution_criteria:
            context += f"\nResolution: {market.resolution_criteria[:200]}"

        if len(articles) == 1:
            art = articles[0]
            text = art.full_text[:3000] if art.full_text else "(text unavailable)"
            article_section = f'New article from {art.source}:\n"{art.title}"\n\n{text}'
        else:
            budget_per = max(500, 6000 // len(articles))
            parts = ["New articles this batch:\n"]
            for idx, art in enumerate(articles, 1):
                text = art.full_text[:budget_per] if art.full_text else "(text unavailable)"
                parts.append(f'[{idx}] From {art.source}: "{art.title}"\n{text}\n')
            article_section = "\n".join(parts)

        return f"""{SYSTEM_PROMPT}

{self.persona}

Market: "{market.name}"{context}

Current state:
{price_line}
{portfolio_line}

Recent trades:
{self._format_recent_trades(market_id)}

{article_section}

Analyze and respond in this EXACT format:

FAIR_VALUE: [Your probability estimate, 0.01-0.99]
MOTIVATION: [1 sentence — why this fair value]
ANALYSIS: [2-3 sentences max — key evidence from the article(s)]"""

    # -- LLM output parsing --

    def _parse_fair_value(self, text: str) -> tuple[float, str, str] | None:
        fv_match = re.search(r"FAIR_VALUE:\s*([\d.]+)", text)
        if not fv_match:
            log.warning("Failed to parse FAIR_VALUE from LLM output")
            return None
        raw_fair_value = fv_match.group(1)
        try:
            fair_value = float(raw_fair_value.rstrip("."))
        except ValueError:
            log.warning("Invalid FAIR_VALUE: %s", raw_fair_value)
            return None
        if not 0.01 <= fair_value <= 0.99:
            log.warning("FAIR_VALUE out of range: %s", fair_value)
            return None

        KEYWORDS = r"\nANALYSIS:|\nFAIR_VALUE:|\nEDGE:|\nORDERS:|\nMOTIVATION:|\Z"
        motiv_match = re.search(rf"MOTIVATION:\s*(.*?)(?={KEYWORDS})", text, re.DOTALL)
        motivation = motiv_match.group(1).strip() if motiv_match else ""
        analysis_match = re.search(rf"ANALYSIS:\s*(.*?)(?={KEYWORDS})", text, re.DOTALL)
        analysis = analysis_match.group(1).strip() if analysis_match else ""

        return (fair_value, motivation, analysis)

    # -- Position management --

    def _rebalance_all(self, block: Block) -> list[OrderSpec]:
        """Rebalance all positions using the sizing strategy."""
        pv = self._portfolio_value(block)
        cash = self.current_balance
        min_cash = MIN_CASH_FRAC * pv

        all_orders: list[OrderSpec] = []

        markets_to_check = set(self.fair_values.keys())
        for (mid, _outcome), qty in self.positions.items():
            if qty > 0:
                markets_to_check.add(mid)

        for market_id in markets_to_check:
            market_price = self._get_market_price(market_id, block)
            if market_price <= 0:
                continue

            current_yes = self.get_position(market_id, "YES")
            current_no = self.get_position(market_id, "NO")
            fv = self.fair_values.get(market_id)

            if fv is None:
                # No fair value or drawdown → exit positions
                from sybil_client import SellNo, SellYes
                if current_yes > 0:
                    all_orders.append(SellYes.at_price(market_id, market_price, current_yes))
                if current_no > 0:
                    all_orders.append(SellNo.at_price(market_id, 1 - market_price, current_no))
                continue

            target_yes, target_no = self.strategy.target(
                fv, market_price, pv, current_yes, current_no, market_id=market_id,
            )

            available_cash = max(0, cash - min_cash)
            orders = position_orders(
                market_id, target_yes, target_no,
                current_yes, current_no,
                fv, market_price, available_cash,
            )

            from sybil_client import BuyNo, BuyYes
            for o in orders:
                if isinstance(o, (BuyYes, BuyNo)):
                    cost = o.quantity * (o.limit_price_nanos / NANOS_PER_DOLLAR)
                    cash -= cost

            all_orders.extend(orders)

        return all_orders

    # -- Main loop --

    async def on_block(self, block: Block) -> list[OrderSpec]:
        all_orders: list[OrderSpec] = []
        prices = self._observed_market_prices(block)
        now = datetime.now(timezone.utc)
        self._pending_order_logs = []

        for market_id, (yes_nanos, _) in prices.items():
            yes_price = yes_nanos / NANOS_PER_DOLLAR
            self.price_history.setdefault(market_id, []).append(
                PriceSnapshot(block.height, now, yes_price)
            )
            if len(self.price_history[market_id]) > 500:
                self.price_history[market_id] = self.price_history[market_id][-500:]

        if not self._observed_first_block:
            self._observed_first_block = True
            return []

        # LLM analysis on new articles.
        #
        # AR-6: the min interval is enforced per LLM *call*, not per block. A
        # single block can surface articles for many markets; the old loop-entry
        # gate let one block fire an unbounded burst of sequential LLM calls.
        # Here we stop draining once the per-trader budget is spent and leave the
        # remaining markets' articles pending for a later block.
        for market_id in list(self.market_ids or []):
            elapsed_llm = time.monotonic() - self._last_llm_call
            if self._last_llm_call != 0 and elapsed_llm < self.min_llm_interval_s:
                break

            articles = await self.news_sub.drain(market_id) if self.news_sub else []
            if not articles:
                continue

            market = self.markets_info.get(market_id)
            if not market:
                continue

            ref_price = self._get_market_price(market_id, block)
            if ref_price <= 0:
                continue

            # Skip resolved markets — don't waste LLM calls
            if ref_price >= RESOLVED_HIGH or ref_price <= RESOLVED_LOW:
                if market_id in self.fair_values:
                    log.info("[%s] %s resolved (price=%.2f), clearing FV",
                             self.name, market.name[:30], ref_price)
                    del self.fair_values[market_id]
                continue

            titles = "; ".join(f'"{a.title[:40]}"' for a in articles)
            log.info("[%s] %d article(s) for %s (price=%.2f): %s",
                     self.name, len(articles), market.name[:30], ref_price, titles)

            prompt = self._build_prompt(articles, market, block)
            if not prompt:
                continue

            try:
                raw_text, llm_duration_s = await self._call_llm(prompt)
                self._last_llm_call = time.monotonic()
                log.info("[%s] LLM response (%.1fs):\n%s", self.name, llm_duration_s, raw_text)
            except Exception as e:
                log.warning("[%s] LLM call failed: %s", self.name, e)
                continue

            parsed = self._parse_fair_value(raw_text)
            if parsed is None:
                log.warning("[%s] Failed to parse LLM output", self.name)
                continue

            fair_value, motivation, analysis = parsed
            old_fv = self.fair_values.get(market_id)
            self.fair_values[market_id] = fair_value

            log.info("[%s] %s: FV %.2f->%.2f (market=%.2f, edge=%.2f) | %s",
                     self.name, market.name[:30],
                     old_fv or 0, fair_value, ref_price,
                     fair_value - ref_price, motivation)

            self._record_trade(
                market_id=market_id,
                market_name=market.name,
                fair_value=fair_value,
                orders=[],
                motivation=motivation,
                analysis=analysis,
                raw_llm_response=raw_text,
                llm_duration_s=llm_duration_s,
                market_price=ref_price,
                block_height=block.height,
                timestamp=now,
                articles=articles,
            )

        # Position management via strategy
        elapsed_rebal = time.monotonic() - self._last_rebalance
        if elapsed_rebal >= self.strategy.rebalance_interval_s or self._last_rebalance == 0:
            rebalance_orders = self._rebalance_all(block)
            if rebalance_orders:
                order_desc = ", ".join(_describe_order(o) for o in rebalance_orders)
                log.info("[%s] %s rebalance: %s", self.name, self.strategy.name, order_desc)
                orders_by_market: dict[int, list[OrderSpec]] = {}
                for order in rebalance_orders:
                    orders_by_market.setdefault(order.market_id, []).append(order)
                for market_id, market_orders in orders_by_market.items():
                    market = self.markets_info.get(market_id)
                    market_name = market.name if market else str(market_id)
                    market_price = self._get_market_price(market_id, block)
                    fair_value = self.fair_values.get(market_id, market_price)
                    if market_id in self.fair_values:
                        motivation = f"{self.strategy.name} rebalance to target position"
                    else:
                        motivation = f"{self.strategy.name} exit without an active fair value"
                    self._pending_order_logs.append({
                        "market_id": market_id,
                        "market_name": market_name,
                        "fair_value": fair_value,
                        "orders": market_orders,
                        "motivation": motivation,
                        "market_price": market_price,
                        "block_height": block.height,
                        "timestamp": now,
                    })
            all_orders.extend(rebalance_orders)
            self._last_rebalance = time.monotonic()

        return all_orders

    async def on_orders_submitted(self, block: Block, orders: list[OrderSpec]) -> None:
        for entry in self._pending_order_logs:
            self._record_trade(
                market_id=entry["market_id"],
                market_name=entry["market_name"],
                fair_value=entry["fair_value"],
                orders=entry["orders"],
                motivation=entry["motivation"],
                analysis="",
                raw_llm_response="",
                llm_duration_s=0.0,
                market_price=entry["market_price"],
                block_height=entry["block_height"],
                timestamp=entry["timestamp"],
            )
        self._pending_order_logs = []
