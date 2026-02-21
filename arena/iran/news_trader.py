"""Iran news-reactive LLM trading bot."""

import json
import logging
import re
import time
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path

import openai

from bots.base import BaseAgent
from sybil_client import Block, BuyNo, BuyYes, OrderSpec, SellNo, SellYes
from sybil_client.types import NANOS_PER_DOLLAR

from .clock import SimulatedClock

log = logging.getLogger(__name__)


@dataclass
class Article:
    timestamp: datetime
    title: str
    source: str
    url: str
    full_text: str


@dataclass
class PriceSnapshot:
    block_height: int
    sim_time: datetime
    yes_price: float

    def to_dict(self) -> dict:
        return {
            "block": self.block_height,
            "sim_time": self.sim_time.isoformat(),
            "yes_price": self.yes_price,
        }


@dataclass
class TradeRecord:
    article: Article
    probability: float
    conviction: str  # "LOW" / "MEDIUM" / "HIGH"
    motivation: str
    orders: list[OrderSpec]
    sim_time: datetime
    llm_response: str = ""  # raw LLM output for debugging
    block_height: int = -1  # block in which this trade was submitted
    llm_duration_s: float = 0.0  # wall-clock seconds the LLM call took

    def to_dict(self) -> dict:
        return {
            "sim_time": self.sim_time.isoformat(),
            "block_height": self.block_height,
            "llm_duration_s": self.llm_duration_s,
            "article_title": self.article.title,
            "article_source": self.article.source,
            "article_url": self.article.url,
            "article_timestamp": self.article.timestamp.isoformat(),
            "probability": self.probability,
            "conviction": self.conviction,
            "motivation": self.motivation,
            "orders": [_describe_order(o) for o in self.orders],
            "llm_response": self.llm_response,
        }


def load_articles(phase1_path: str, texts_path: str) -> list[Article]:
    """Load phase1-YES articles that have full text available."""
    phase1_data = json.loads(Path(phase1_path).read_text())
    texts_data = json.loads(Path(texts_path).read_text())

    articles = []
    for item in phase1_data["results"]:
        if item.get("phase1") != "YES":
            continue
        text = texts_data.get(item["url"])
        if not text:
            continue
        articles.append(Article(
            timestamp=datetime.strptime(item["timestamp"], "%Y%m%dT%H%M%SZ"),
            title=item["title"],
            source=item["source"],
            url=item["url"],
            full_text=text,
        ))

    articles.sort(key=lambda a: a.timestamp)
    return articles


PHASE2_PROMPT = """\
You are a professional forecaster and prediction market trader specializing in US-Iran geopolitics.

You're trading on the market: "Will the United States carry out a military strike against Iran before March 31, 2026?"

Context:
USA-Iran tensions stem from long-standing issues like Iran's nuclear program and proxies, but escalated sharply after the June 2025 US strikes on Iranian nuclear sites during the Israel-Iran Twelve-Day War. They rose further in early January 2026 amid Iran's crackdown on anti-government protests, prompting President Trump to threaten military action and review strike options.

Market data:
Last batch YES price: {yes_price:.2f}

Your recent trades:
{recent_trades}

Your portfolio: {usdc:.2f} USDC, {yes_shares} YES shares, {no_shares} NO shares

You've just received this article from {source}:

"{title}"

{full_text}

Analyze this article. Use chain of thought:
1. What does this article signal about the likelihood of a US strike on Iran by March 31?
2. How significant is this signal? Is it a concrete development or speculation/opinion?
3. Consider the source credibility and potential bias.
4. How does this fit with your previous trades and current portfolio?

Then provide your conclusion in exactly this format:

MOTIVATION: [1-2 sentence thesis]
PROBABILITY: [your estimate, 0.00 to 1.00]
CONVICTION: [LOW / MEDIUM / HIGH]"""


class IranNewsTrader(BaseAgent):
    """LLM-powered news-reactive trader for the Iran strike market."""

    def __init__(
        self,
        client,
        account_id: int,
        articles: list[Article],
        clock: SimulatedClock,
        api_key: str,
        model_name: str = "moonshotai/kimi-k2",
        name: str | None = None,
        market_ids: list[int] | None = None,
    ):
        super().__init__(client, account_id, name or "IranNewsTrader", market_ids)
        self.articles = articles
        self.clock = clock
        self.api_key = api_key
        self.model_name = model_name
        self._article_index = 0
        self._llm_client: openai.AsyncOpenAI | None = None
        self.trade_log: list[TradeRecord] = []
        self.price_history: list[PriceSnapshot] = []

    def _get_llm_client(self) -> openai.AsyncOpenAI:
        if self._llm_client is None:
            self._llm_client = openai.AsyncOpenAI(
                base_url="https://openrouter.ai/api/v1",
                api_key=self.api_key,
            )
        return self._llm_client

    def _drain_arrived_articles(self) -> list[Article]:
        """Return all articles whose timestamp <= clock.now(), advance cursor."""
        arrived = []
        while self._article_index < len(self.articles):
            art = self.articles[self._article_index]
            if not self.clock.is_past(art.timestamp):
                break
            arrived.append(art)
            self._article_index += 1
        return arrived

    def _fills_for_trade(self, trade_idx: int) -> list:
        """Get AccountFill objects that resulted from a given trade."""
        rec = self.trade_log[trade_idx]
        if rec.block_height < 0 or not rec.orders:
            return []
        # Fills from this trade land in blocks after submission.
        # Attribute fills between this trade's block and the next trade's block.
        start = rec.block_height
        if trade_idx + 1 < len(self.trade_log):
            end = self.trade_log[trade_idx + 1].block_height
        else:
            end = float("inf")
        return [
            f for f in self._fill_history
            if start < f.block_height <= end
        ]

    def _format_recent_trades(self) -> str:
        if not self.trade_log:
            return "No trades yet."
        lines = []
        entries = self.trade_log[-5:]
        start_idx = len(self.trade_log) - len(entries)
        for i, rec in enumerate(entries):
            is_last = (start_idx + i == len(self.trade_log) - 1)
            lines.append(
                f"- [{rec.sim_time:%H:%M}] P={rec.probability:.2f} "
                f"{rec.conviction} | {rec.motivation}"
            )
            if not rec.orders:
                lines.append("  No orders")
            else:
                order_desc = ", ".join(
                    _describe_order(o) for o in rec.orders
                )
                lines.append(f"  Submitted: {order_desc}")
                if is_last:
                    lines.append("  (awaiting fill)")
                else:
                    fills = self._fills_for_trade(start_idx + i)
                    fill_desc = _format_fills(rec.orders, fills)
                    lines.append(f"  Filled: {fill_desc}")
            lines.append("")  # blank line between entries
        return "\n".join(lines).rstrip()

    async def _phase2_analyze(
        self, article: Article, block: Block
    ) -> tuple[float, str, str, str, float] | None:
        """Call LLM for probability/conviction/motivation.

        Returns (probability, conviction, motivation, raw_response, llm_duration_s) or None on failure.
        """
        market_id = next(iter(self.market_ids))
        yes_nanos, _ = self.filter_markets(block)[market_id]
        yes_price = yes_nanos / NANOS_PER_DOLLAR

        prompt = PHASE2_PROMPT.format(
            yes_price=yes_price,
            recent_trades=self._format_recent_trades(),
            usdc=self.current_balance,
            yes_shares=self.get_position(market_id, "YES"),
            no_shares=self.get_position(market_id, "NO"),
            source=article.source,
            title=article.title,
            full_text=article.full_text[:4000],  # truncate very long articles
        )

        try:
            llm = self._get_llm_client()
            t0 = time.monotonic()
            self.clock.pause()
            try:
                resp = await llm.chat.completions.create(
                    model=self.model_name,
                    messages=[{"role": "user", "content": prompt}],
                    temperature=0.3,
                    max_tokens=1024,
                )
            finally:
                self.clock.resume()
            llm_duration_s = time.monotonic() - t0
            text = resp.choices[0].message.content or ""
            log.info("[%s] LLM response for '%s' (%.1fs):\n%s", self.name, article.title[:60], llm_duration_s, text)
        except Exception as e:
            log.error("[%s] LLM call failed: %s", self.name, e)
            return None

        # Parse structured output
        prob_match = re.search(r"PROBABILITY:\s*([\d.]+)", text)
        conv_match = re.search(r"CONVICTION:\s*(LOW|MEDIUM|HIGH)", text)
        motiv_match = re.search(r"MOTIVATION:\s*(.+)", text)

        if not prob_match or not conv_match:
            log.warning("[%s] Failed to parse LLM output", self.name)
            return None

        probability = float(prob_match.group(1))
        conviction = conv_match.group(1)
        motivation = motiv_match.group(1).strip() if motiv_match else ""

        # Sanity check
        if not 0.0 <= probability <= 1.0:
            log.warning("[%s] Probability out of range: %s", self.name, probability)
            return None

        return probability, conviction, motivation, text, llm_duration_s

    def _phase3_execute(
        self, probability: float, conviction: str, block: Block,
        shadow_yes: int | None = None, shadow_no: int | None = None,
    ) -> list[OrderSpec]:
        """Mechanical trade execution from Phase 2 output.

        shadow_yes/shadow_no: if set, override get_position() to account for
        orders already generated earlier in the same batch.
        """
        market_id = next(iter(self.market_ids))
        yes_nanos, _ = self.filter_markets(block)[market_id]
        last_price = yes_nanos / NANOS_PER_DOLLAR

        # Step 2: Edge check
        edge = abs(probability - last_price)
        thresholds = {"LOW": 0.05, "MEDIUM": 0.03, "HIGH": 0.02}
        if edge < thresholds[conviction]:
            return []

        # Step 3: Risk budget
        current_yes = shadow_yes if shadow_yes is not None else self.get_position(market_id, "YES")
        current_no = shadow_no if shadow_no is not None else self.get_position(market_id, "NO")
        total_capital = (
            self.current_balance
            + current_yes * last_price
            + current_no * (1 - last_price)
        )

        risk_pcts = {"LOW": 0.05, "MEDIUM": 0.15, "HIGH": 0.30}
        risk_pct = risk_pcts[conviction]
        if edge > 0.15:  # bump tier
            bumped = {"LOW": 0.15, "MEDIUM": 0.30, "HIGH": 0.50}
            risk_pct = bumped[conviction]

        risk_budget = risk_pct * total_capital

        # Target position
        if probability > last_price:
            target_yes = int(risk_budget / probability) if probability > 0 else 0
            target_no = 0
        else:
            target_yes = 0
            target_no = int(risk_budget / (1 - probability)) if probability < 1 else 0

        # Step 4: Generate orders
        orders: list[OrderSpec] = []

        # Close wrong-side
        if target_no == 0 and current_no > 0:
            orders.append(SellNo.at_price(market_id, 1 - probability, current_no))
        if target_yes == 0 and current_yes > 0:
            orders.append(SellYes.at_price(market_id, probability, current_yes))

        # Adjust right-side
        if target_yes > current_yes:
            orders.append(BuyYes.at_price(market_id, probability, target_yes - current_yes))
        if target_no > current_no:
            orders.append(BuyNo.at_price(market_id, 1 - probability, target_no - current_no))

        return orders

    async def on_block(self, block: Block) -> list[OrderSpec]:
        # Track price every block
        market_id = next(iter(self.market_ids))
        prices = self.filter_markets(block)
        if market_id in prices:
            yes_nanos, _ = prices[market_id]
            self.price_history.append(PriceSnapshot(
                block_height=block.height,
                sim_time=self.clock.now(),
                yes_price=yes_nanos / NANOS_PER_DOLLAR,
            ))

        arrived = self._drain_arrived_articles()
        all_orders: list[OrderSpec] = []

        # Shadow positions track cumulative effect of orders within this batch,
        # preventing sells that exceed actual holdings when multiple articles
        # generate orders in the same on_block call.
        market_id = next(iter(self.market_ids))
        shadow_yes = self.get_position(market_id, "YES")
        shadow_no = self.get_position(market_id, "NO")

        for article in arrived:
            log.info(
                "[%s] Processing article: %s (%s)",
                self.name, article.title[:60], article.source,
            )

            result = await self._phase2_analyze(article, block)
            if result is None:
                self.trade_log.append(TradeRecord(
                    article=article,
                    probability=0.0,
                    conviction="LOW",
                    motivation="LLM parse failure",
                    orders=[],
                    sim_time=self.clock.now(),
                    block_height=block.height,
                ))
                continue

            probability, conviction, motivation, raw_response, llm_duration_s = result
            orders = self._phase3_execute(
                probability, conviction, block,
                shadow_yes=shadow_yes, shadow_no=shadow_no,
            )

            # Update shadow positions based on generated orders
            for order in orders:
                if isinstance(order, BuyYes):
                    shadow_yes += order.quantity
                elif isinstance(order, SellYes):
                    shadow_yes -= order.quantity
                elif isinstance(order, BuyNo):
                    shadow_no += order.quantity
                elif isinstance(order, SellNo):
                    shadow_no -= order.quantity

            self.trade_log.append(TradeRecord(
                article=article,
                probability=probability,
                conviction=conviction,
                motivation=motivation,
                orders=orders,
                sim_time=self.clock.now(),
                llm_response=raw_response,
                block_height=block.height,
                llm_duration_s=llm_duration_s,
            ))

            log.info(
                "[%s] P=%.2f %s edge=%.2f → %d orders | %s",
                self.name, probability, conviction,
                abs(probability - (self.filter_markets(block)[market_id][0] / NANOS_PER_DOLLAR)),
                len(orders), motivation,
            )

            all_orders.extend(orders)

        return all_orders


def _describe_order(order: OrderSpec) -> str:
    """Human-readable order description."""
    price = order.limit_price_nanos / NANOS_PER_DOLLAR
    if isinstance(order, BuyYes):
        return f"BuyYes {order.quantity} @ ${price:.2f}"
    elif isinstance(order, BuyNo):
        return f"BuyNo {order.quantity} @ ${price:.2f}"
    elif isinstance(order, SellYes):
        return f"SellYes {order.quantity} @ ${price:.2f}"
    elif isinstance(order, SellNo):
        return f"SellNo {order.quantity} @ ${price:.2f}"
    return str(order)


def _order_side(order: OrderSpec) -> tuple[str, str]:
    """Return (action, outcome) for an order. e.g. ('Buy', 'YES')."""
    if isinstance(order, BuyYes):
        return ("Buy", "YES")
    elif isinstance(order, BuyNo):
        return ("Buy", "NO")
    elif isinstance(order, SellYes):
        return ("Sell", "YES")
    elif isinstance(order, SellNo):
        return ("Sell", "NO")
    return ("?", "?")


def _format_fills(orders: list[OrderSpec], fills: list) -> str:
    """Match fills to submitted orders and format as a summary string.

    Groups fills by (action, outcome) to match against submitted orders.
    """
    if not fills:
        return "no fills"

    # Aggregate fills by (action, outcome): total_qty, total_cost
    fill_agg: dict[tuple[str, str], tuple[int, int]] = {}  # -> (qty, cost_nanos)
    for f in fills:
        for delta in f.position_deltas:
            action = "Buy" if delta.delta > 0 else "Sell"
            key = (action, delta.outcome)
            prev_qty, prev_cost = fill_agg.get(key, (0, 0))
            fill_agg[key] = (
                prev_qty + abs(delta.delta),
                prev_cost + abs(delta.delta) * f.fill_price_nanos,
            )

    # Match each order to its fill aggregate
    parts = []
    for order in orders:
        action, outcome = _order_side(order)
        key = (action, outcome)
        filled_qty, filled_cost = fill_agg.get(key, (0, 0))
        label = f"{action}{outcome.capitalize()}"
        if filled_qty == 0:
            parts.append(f"{label} 0/{order.quantity} unfilled")
        else:
            avg_price = filled_cost / filled_qty / NANOS_PER_DOLLAR
            partial = " (partial)" if filled_qty < order.quantity else ""
            parts.append(
                f"{label} {filled_qty}/{order.quantity} @ avg ${avg_price:.2f}{partial}"
            )

    return ", ".join(parts)
