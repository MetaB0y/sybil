"""Live LLM-driven trader with Kelly-based position sizing.

Architecture (inspired by terminator2):
- LLM provides FAIR_VALUE estimates (probability analysis)
- Kelly criterion sizes positions mechanically (1/3 Kelly)
- Position management runs every block (active selling when edge shrinks)
- LLM is only called when new articles arrive; sizing is continuous

Key invariant: if MINT expected P&L = 0 (design/mint-pnl.typ), then
all capital flows are zero-sum among traders. Kelly sizing ensures
long-run growth while 1/3 scaling prevents ruin.
"""

import logging
import math
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
# Kelly sizing parameters
# --------------------------------------------------------------------------- #
KELLY_FRACTION = 1 / 3       # 1/3 Kelly — prevents ruin
MIN_EDGE = 0.02              # 2 cent minimum edge to trade
EXIT_EDGE = 0.005            # Below 0.5 cent edge → exit position
MAX_POSITION_FRAC = 0.30     # Max 30% of portfolio in any one market
MIN_CASH_FRAC = 0.20         # Keep at least 20% cash
REBALANCE_INTERVAL_S = 30.0  # Check positions every 30s even without articles


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


def kelly_target(
    fair_value: float,
    market_price: float,
    portfolio_value: float,
    kelly_frac: float = KELLY_FRACTION,
    max_position_frac: float = MAX_POSITION_FRAC,
) -> tuple[int, int]:
    """Compute Kelly-optimal target positions (target_yes, target_no).

    Returns the number of YES and NO shares to hold. Exactly one will
    be positive; the other will be zero.

    Uses fractional Kelly (default 1/3) to prevent ruin.
    """
    edge = fair_value - market_price
    if abs(edge) < MIN_EDGE:
        return (0, 0)

    if edge > 0:
        # Bullish: buy YES
        full_kelly = edge / (1 - market_price) if market_price < 1 else 0
        bet_value = full_kelly * kelly_frac * portfolio_value
        bet_value = min(bet_value, max_position_frac * portfolio_value)
        target_yes = int(bet_value / market_price) if market_price > 0 else 0
        return (max(target_yes, 0), 0)
    else:
        # Bearish: buy NO
        full_kelly = abs(edge) / market_price if market_price > 0 else 0
        bet_value = full_kelly * kelly_frac * portfolio_value
        bet_value = min(bet_value, max_position_frac * portfolio_value)
        no_price = 1 - market_price
        target_no = int(bet_value / no_price) if no_price > 0 else 0
        return (0, max(target_no, 0))


def position_orders(
    market_id: int,
    target_yes: int,
    target_no: int,
    current_yes: int,
    current_no: int,
    fair_value: float,
    market_price: float,
    available_cash: float,
) -> list[OrderSpec]:
    """Generate orders to move from current to target positions.

    Sells use market_price (willing to exit at current price).
    Buys use fair_value as limit (willing to pay up to FV).
    """
    orders: list[OrderSpec] = []

    # Exit wrong-side positions first (frees cash)
    if target_yes == 0 and current_yes > 0:
        # Sell all YES — use market price (willing to exit at market)
        orders.append(SellYes.at_price(market_id, market_price, current_yes))
    if target_no == 0 and current_no > 0:
        orders.append(SellNo.at_price(market_id, 1 - market_price, current_no))

    # Trim oversized positions
    if target_yes > 0 and current_yes > target_yes:
        excess = current_yes - target_yes
        orders.append(SellYes.at_price(market_id, market_price, excess))
    if target_no > 0 and current_no > target_no:
        excess = current_no - target_no
        orders.append(SellNo.at_price(market_id, 1 - market_price, excess))

    # Scale into target (buy)
    if target_yes > current_yes:
        deficit = target_yes - current_yes
        cost_per_share = market_price
        affordable = int(available_cash / cost_per_share) if cost_per_share > 0 else 0
        qty = min(deficit, affordable)
        if qty > 0:
            # Limit at fair_value — willing to pay up to our estimate
            orders.append(BuyYes.at_price(market_id, fair_value, qty))

    if target_no > current_no:
        deficit = target_no - current_no
        cost_per_share = 1 - market_price
        affordable = int(available_cash / cost_per_share) if cost_per_share > 0 else 0
        qty = min(deficit, affordable)
        if qty > 0:
            orders.append(BuyNo.at_price(market_id, 1 - fair_value, qty))

    return orders


# --------------------------------------------------------------------------- #
# System prompt — simplified, no ORDERS
# --------------------------------------------------------------------------- #
SYSTEM_PROMPT = """\
You are analyzing news articles for a prediction market. Your job is to estimate the probability of the event occurring, given the evidence.

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
- Only revise significantly for DIRECT evidence — tangential news warrants at most 1-2 cent adjustment
- Official actions > direct quotes > analysis > speculation > rumors
- Most events have genuine uncertainty — avoid extreme probabilities unless evidence is extraordinary

Always respond in English regardless of article language."""


# --------------------------------------------------------------------------- #
# LiveLlmTrader
# --------------------------------------------------------------------------- #
class LiveLlmTrader(BaseAgent):
    """LLM-driven analysis + Kelly-based execution.

    The LLM provides fair value estimates. Position sizing uses
    fractional Kelly criterion (1/3 Kelly by default). Positions
    are rebalanced every block based on current fair values and
    market prices — active selling when edge disappears.
    """

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
        self._last_rebalance: float = 0.0
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
                lines.append(f"- [{t}] FV={rec.fair_value:.2f} → {order_desc} | {rec.motivation}")
        return "\n".join(lines).rstrip()

    def _get_market_price(self, market_id: int, block: Block) -> float:
        """Get the best available price for a market (Polymarket > Sybil)."""
        poly_price = self.news_feed.polymarket_prices.get_price(market_id)
        prices = self.filter_markets(block)
        if market_id in prices:
            yes_nanos, _ = prices[market_id]
            sybil_price = yes_nanos / NANOS_PER_DOLLAR
        else:
            sybil_price = 0.0
        ref = poly_price if poly_price and poly_price > 0 else sybil_price
        return ref if ref > 0 else 0.0

    def _portfolio_value(self, block: Block) -> float:
        """Estimate total portfolio value (cash + positions at market prices)."""
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
        return max(pv, 0.01)  # avoid division by zero

    def _build_prompt(
        self, articles: list[LiveArticle], market: Market, block: Block
    ) -> str:
        market_id = market.id
        yes_price = self._get_market_price(market_id, block)
        if yes_price <= 0:
            return ""

        # Price context
        poly_price = self.news_feed.polymarket_prices.get_price(market_id)
        if poly_price and poly_price > 0:
            price_line = f"- Polymarket consensus: YES=${poly_price:.4f} | NO=${1 - poly_price:.4f}"
        else:
            price_line = f"- YES price: ${yes_price:.4f} | NO price: ${1 - yes_price:.4f}"

        history = self.price_history.get(market_id, [])
        recent_prices = [s.yes_price for s in history[-5:]]
        if recent_prices:
            price_line += f"\n- Recent prices: {', '.join(f'{p:.4f}' for p in recent_prices)}"

        # Portfolio context
        balance = self.current_balance
        yes_shares = self.get_position(market_id, "YES")
        no_shares = self.get_position(market_id, "NO")
        pv = self._portfolio_value(block)

        last_fv = self.fair_values.get(market_id)
        last_fv_line = f"\n- Your last fair value estimate: {last_fv:.2f}" if last_fv else ""

        # Market context
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

        return f"""{SYSTEM_PROMPT}

{self.persona}

Market: "{market.name}"{context}

Current state:
{price_line}
- Your portfolio: ${balance:.2f} cash, {yes_shares} YES shares, {no_shares} NO shares (~${pv:.0f} total){last_fv_line}

Recent trades:
{self._format_recent_trades(market_id)}

{article_section}

Analyze and respond in this EXACT format:

FAIR_VALUE: [Your probability estimate, 0.01-0.99]
MOTIVATION: [1 sentence — why this fair value]
ANALYSIS: [2-3 sentences max — key evidence from the article(s)]"""

    def _parse_fair_value(self, text: str) -> tuple[float, str, str] | None:
        """Parse FAIR_VALUE and MOTIVATION from LLM output."""
        fv_match = re.search(r"FAIR_VALUE:\s*([\d.]+)", text)
        if not fv_match:
            log.warning("Failed to parse FAIR_VALUE from LLM output")
            return None
        fair_value = float(fv_match.group(1))
        if not 0.01 <= fair_value <= 0.99:
            log.warning("FAIR_VALUE out of range: %s", fair_value)
            return None

        KEYWORDS = r"\nANALYSIS:|\nFAIR_VALUE:|\nEDGE:|\nORDERS:|\nMOTIVATION:|\Z"

        motiv_match = re.search(
            rf"MOTIVATION:\s*(.*?)(?={KEYWORDS})", text, re.DOTALL,
        )
        motivation = motiv_match.group(1).strip() if motiv_match else ""

        analysis_match = re.search(
            rf"ANALYSIS:\s*(.*?)(?={KEYWORDS})", text, re.DOTALL,
        )
        analysis = analysis_match.group(1).strip() if analysis_match else ""

        return (fair_value, motivation, analysis)

    def _rebalance_all(self, block: Block) -> list[OrderSpec]:
        """Rebalance all positions using Kelly targets.

        Runs every block. For each market with a fair_value:
        - Compute Kelly target from fair_value vs market_price
        - Generate orders to move toward target
        - Sell positions where edge has disappeared
        """
        pv = self._portfolio_value(block)
        cash = self.current_balance
        min_cash = MIN_CASH_FRAC * pv

        all_orders: list[OrderSpec] = []

        # Process all markets where we have a fair value or a position
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
                # No fair value → exit any positions (shouldn't hold what we can't value)
                if current_yes > 0:
                    all_orders.append(
                        SellYes.at_price(market_id, market_price, current_yes)
                    )
                if current_no > 0:
                    all_orders.append(
                        SellNo.at_price(market_id, 1 - market_price, current_no)
                    )
                continue

            edge = abs(fv - market_price)

            # If edge is below exit threshold, close position
            if edge < EXIT_EDGE:
                target_yes, target_no = 0, 0
            else:
                target_yes, target_no = kelly_target(fv, market_price, pv)

            # Available cash for buying (respect min cash reserve)
            available_cash = max(0, cash - min_cash)

            orders = position_orders(
                market_id, target_yes, target_no,
                current_yes, current_no,
                fv, market_price, available_cash,
            )

            # Deduct estimated buy costs from available cash
            for o in orders:
                if isinstance(o, (BuyYes, BuyNo)):
                    cost = o.quantity * (o.limit_price_nanos / NANOS_PER_DOLLAR)
                    cash -= cost

            all_orders.extend(orders)

        return all_orders

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
            if len(self.price_history[market_id]) > 500:
                self.price_history[market_id] = self.price_history[market_id][-500:]

        # Skip first block
        if not self._observed_first_block:
            self._observed_first_block = True
            return []

        # Check for new articles → LLM analysis → update fair values
        elapsed_llm = time.monotonic() - self._last_llm_call
        if elapsed_llm >= self.min_llm_interval_s or self._last_llm_call == 0:
            for market_id in list(self.market_ids or []):
                articles = await self.news_feed.drain(market_id)
                if not articles:
                    continue

                market = self.markets_info.get(market_id)
                if not market:
                    continue

                ref_price = self._get_market_price(market_id, block)
                if ref_price <= 0:
                    continue

                titles = "; ".join(f'"{a.title[:40]}"' for a in articles)
                log.info("[%s] %d article(s) for %s (price=%.2f): %s",
                         self.name, len(articles), market.name[:30], ref_price, titles)

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

                parsed = self._parse_fair_value(raw_text)
                if parsed is None:
                    log.warning("[%s] Failed to parse LLM output", self.name)
                    continue

                fair_value, motivation, analysis = parsed
                old_fv = self.fair_values.get(market_id)
                self.fair_values[market_id] = fair_value

                log.info("[%s] %s: FV %.2f→%.2f (market=%.2f, edge=%.2f) | %s",
                         self.name, market.name[:30],
                         old_fv or 0, fair_value, ref_price,
                         fair_value - ref_price, motivation)

                # DB logging
                if self.db:
                    article_urls = [
                        {"title": a.title, "url": a.url, "source": a.source}
                        for a in articles
                    ]
                    self.db.log_decision(
                        trader_name=self.name,
                        market_id=market_id,
                        market_name=market.name,
                        analysis=analysis,
                        fair_value=fair_value,
                        market_price=ref_price,
                        orders=[],  # filled in after rebalance
                        motivation=motivation,
                        raw_llm_response=raw_text,
                        llm_duration_s=llm_duration_s,
                        balance=self.current_balance,
                        yes_pos=self.get_position(market_id, "YES"),
                        no_pos=self.get_position(market_id, "NO"),
                        article_urls=article_urls,
                    )

        # Rebalance positions using Kelly targets (runs every block)
        elapsed_rebal = time.monotonic() - self._last_rebalance
        if elapsed_rebal >= REBALANCE_INTERVAL_S or self._last_rebalance == 0:
            rebalance_orders = self._rebalance_all(block)
            if rebalance_orders:
                order_desc = ", ".join(_describe_order(o) for o in rebalance_orders)
                log.info("[%s] Kelly rebalance: %s", self.name, order_desc)
            all_orders.extend(rebalance_orders)
            self._last_rebalance = time.monotonic()

        return all_orders
