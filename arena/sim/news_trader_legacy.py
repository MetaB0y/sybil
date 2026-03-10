"""News-reactive LLM trading bot (market-agnostic)."""

import asyncio
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
    conviction: int  # 1-10 scale
    motivation: str
    orders: list[OrderSpec]
    sim_time: datetime
    llm_response: str = ""  # raw LLM output for debugging
    block_height: int = -1  # block in which this trade was submitted
    llm_duration_s: float = 0.0  # wall-clock seconds the LLM call took
    # Trader state at decision time
    balance: float = 0.0
    yes_pos: int = 0
    no_pos: int = 0
    risk_pct: float = 0.0
    target_pos: int = 0  # target position on the chosen side
    belief: float = 0.0  # running Beta belief at decision time

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
            "balance": self.balance,
            "yes_pos": self.yes_pos,
            "no_pos": self.no_pos,
            "risk_pct": self.risk_pct,
            "target_pos": self.target_pos,
            "belief": self.belief,
        }


def load_articles(phase1_path: str) -> list[Article]:
    """Load phase1-YES articles that have full text available."""
    p = Path(phase1_path)
    if not p.exists():
        return []
    phase1_data = json.loads(p.read_text())

    articles = []
    for item in phase1_data["results"]:
        if item.get("phase1") != "YES":
            continue
        text = item.get("full_text")
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
{persona}

Market data:
Last batch YES price: {yes_price:.2f}

Your recent trades:
{recent_trades}

Your portfolio: {usdc:.2f} USDC, {yes_shares} YES shares, {no_shares} NO shares

You've just received this article from {source}:

"{title}"

{full_text}

Analyze this article. Use chain of thought:
1. {analysis_question}
2. How significant is this signal? Is it a concrete development or speculation/opinion?
3. Consider the source credibility and potential bias.
4. How does this fit with your previous trades and current portfolio?

Then provide your conclusion in exactly this format:

MOTIVATION: [1-2 sentence thesis]
PROBABILITY: [your estimate, 0.00 to 1.00]
CONVICTION: [1-10, where 1 = barely relevant noise, 5 = moderate signal, 10 = game-changing development]"""


class NewsTrader(BaseAgent):
    """LLM-powered news-reactive trader (market-agnostic)."""

    # Defaults — overridden by strategy dict if provided
    # Conviction is 1-10 scale. Belief strength and Kelly scale interpolate linearly.
    _DEFAULT_BELIEF_STRENGTH_RANGE = (1.0, 6.0)  # (conviction=1, conviction=10)
    _DEFAULT_BELIEF_WEIGHT_CAP = 30  # max alpha+beta before rescaling
    _DEFAULT_KELLY_RANGE = (0.05, 0.50)  # (conviction=1, conviction=10)
    _DEFAULT_MAX_KELLY_SCALE = 1.0
    _DEFAULT_MIN_EDGE = 0.02
    _DEFAULT_WARMUP_TRADES = 5

    def __init__(
        self,
        client,
        account_id: int,
        articles: list[Article],
        clock: SimulatedClock,
        api_key: str,
        persona: str,
        analysis_question: str,
        model_name: str = "moonshotai/kimi-k2",
        name: str | None = None,
        market_ids: list[int] | None = None,
        strategy: dict | None = None,
    ):
        super().__init__(client, account_id, name or "NewsTrader", market_ids)
        self.articles = articles
        self.clock = clock
        self.api_key = api_key
        self.model_name = model_name
        self.persona = persona
        self.analysis_question = analysis_question
        self._article_index = 0
        self._llm_client: openai.AsyncOpenAI | None = None
        self.trade_log: list[TradeRecord] = []
        self.price_history: list[PriceSnapshot] = []
        # Beta distribution belief state: belief = alpha / (alpha + beta)
        # Initialized from first observed clearing price in on_block()
        self._belief_alpha: float = 0.0
        self._belief_beta: float = 0.0
        self._belief_initialized: bool = False

        # Apply strategy overrides
        s = strategy or {}
        self._BELIEF_STRENGTH_RANGE = s.get("belief_strength_range", self._DEFAULT_BELIEF_STRENGTH_RANGE)
        self._BELIEF_WEIGHT_CAP = s.get("belief_weight_cap", self._DEFAULT_BELIEF_WEIGHT_CAP)
        self._KELLY_RANGE = s.get("kelly_range", self._DEFAULT_KELLY_RANGE)
        self._MAX_KELLY_SCALE = s.get("max_kelly_scale", self._DEFAULT_MAX_KELLY_SCALE)
        self._MIN_EDGE = s.get("min_edge", self._DEFAULT_MIN_EDGE)
        self._WARMUP_TRADES = s.get("warmup_trades", self._DEFAULT_WARMUP_TRADES)

    def snapshot_state(self) -> dict:
        """Capture cross-day state for multi-day simulations."""
        return {
            "alpha": self._belief_alpha,
            "beta": self._belief_beta,
            "initialized": self._belief_initialized,
            "trade_log": self.trade_log,
            "price_history": self.price_history,
        }

    def restore_state(self, state: dict) -> None:
        """Restore cross-day state from a previous day's snapshot."""
        self._belief_alpha = state["alpha"]
        self._belief_beta = state["beta"]
        self._belief_initialized = state["initialized"]
        self.trade_log = state["trade_log"]
        self.price_history = state["price_history"]

    def _get_llm_client(self) -> openai.AsyncOpenAI:
        if self._llm_client is None:
            self._llm_client = openai.AsyncOpenAI(
                base_url="https://openrouter.ai/api/v1",
                api_key=self.api_key,
                timeout=30.0,
            )
        return self._llm_client

    def _interp(self, lo_hi: tuple[float, float], conviction: int) -> float:
        """Linearly interpolate between lo (conviction=1) and hi (conviction=10)."""
        lo, hi = lo_hi
        t = (conviction - 1) / 9.0  # 1→0.0, 10→1.0
        return lo + t * (hi - lo)

    def _update_belief(self, probability: float, conviction: int) -> float:
        """Update Beta belief with new LLM signal. Returns updated belief."""
        total = self._belief_alpha + self._belief_beta
        if total > self._BELIEF_WEIGHT_CAP:
            scale = self._BELIEF_WEIGHT_CAP / total
            self._belief_alpha *= scale
            self._belief_beta *= scale
        s = self._interp(self._BELIEF_STRENGTH_RANGE, conviction)
        self._belief_alpha += s * probability
        self._belief_beta += s * (1 - probability)
        return self._belief_alpha / (self._belief_alpha + self._belief_beta)

    @property
    def belief(self) -> float:
        total = self._belief_alpha + self._belief_beta
        return self._belief_alpha / total if total > 0 else 0.5

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
    ) -> tuple[float, str, str, str, float] | str:
        """Call LLM for probability/conviction/motivation."""
        market_id = next(iter(self.market_ids))
        yes_nanos, _ = self.filter_markets(block)[market_id]
        yes_price = yes_nanos / NANOS_PER_DOLLAR

        prompt = PHASE2_PROMPT.format(
            persona=self.persona,
            analysis_question=self.analysis_question,
            yes_price=yes_price,
            recent_trades=self._format_recent_trades(),
            usdc=self.current_balance,
            yes_shares=self.get_position(market_id, "YES"),
            no_shares=self.get_position(market_id, "NO"),
            source=article.source,
            title=article.title,
            full_text=article.full_text[:4000],
        )

        llm = self._get_llm_client()
        text = ""
        llm_duration_s = 0.0
        try:
            t0 = time.monotonic()
            await self.client.pause()
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
                await self.client.resume()
            llm_duration_s = time.monotonic() - t0
            text = resp.choices[0].message.content or ""
            log.info("[%s] LLM response for '%s' (%.1fs):\n%s", self.name, article.title[:60], llm_duration_s, text)
        except Exception as e:
            log.warning("[%s] LLM call failed: %s", self.name, e)
            return f"API error: {e}"

        # Parse structured output
        prob_match = re.search(r"PROBABILITY:\s*([\d.]+)", text)
        conv_match = re.search(r"CONVICTION:\s*(\d+)", text)
        motiv_match = re.search(r"MOTIVATION:\s*(.+)", text)

        if not prob_match or not conv_match:
            log.warning("[%s] Failed to parse LLM output", self.name)
            return "parse error: no PROBABILITY/CONVICTION"

        probability = float(prob_match.group(1))
        conviction = max(1, min(10, int(conv_match.group(1))))
        motivation = motiv_match.group(1).strip() if motiv_match else ""

        if not 0.0 <= probability <= 1.0:
            log.warning("[%s] Probability out of range: %s", self.name, probability)
            return f"probability out of range: {probability}"

        return probability, conviction, motivation, text, llm_duration_s

    def _phase3_execute(
        self, conviction: int, block: Block,
        shadow_yes: int | None = None, shadow_no: int | None = None,
    ) -> list[OrderSpec]:
        """Mechanical trade execution using running belief + Kelly sizing."""
        market_id = next(iter(self.market_ids))
        yes_nanos, _ = self.filter_markets(block)[market_id]
        mkt_price = yes_nanos / NANOS_PER_DOLLAR
        b = self.belief

        current_yes = shadow_yes if shadow_yes is not None else self.get_position(market_id, "YES")
        current_no = shadow_no if shadow_no is not None else self.get_position(market_id, "NO")

        edge = abs(b - mkt_price)
        if edge < self._MIN_EDGE:
            self._last_exec_state = (self.current_balance, current_yes, current_no, 0.0, 0)
            return []

        total_capital = (
            self.current_balance
            + current_yes * mkt_price
            + current_no * (1 - mkt_price)
        )

        bullish = b > mkt_price
        experience = min(1.0, len(self.trade_log) / self._WARMUP_TRADES) if self._WARMUP_TRADES > 0 else 1.0
        kelly_scale = min(
            self._interp(self._KELLY_RANGE, conviction) * experience,
            self._MAX_KELLY_SCALE,
        )

        if bullish:
            kelly = edge / (1 - mkt_price) if mkt_price < 1 else 0
            bet_frac = min(kelly * kelly_scale, 1.0)
            risk_budget = bet_frac * total_capital
            target_yes = int(risk_budget / b) if b > 0 else 0
            target_no = 0
        else:
            kelly = edge / mkt_price if mkt_price > 0 else 0
            bet_frac = min(kelly * kelly_scale, 1.0)
            risk_budget = bet_frac * total_capital
            target_yes = 0
            target_no = int(risk_budget / (1 - b)) if b < 1 else 0

        target_pos = target_yes if b > mkt_price else target_no
        self._last_exec_state = (self.current_balance, current_yes, current_no, bet_frac, target_pos)

        orders: list[OrderSpec] = []
        cash = self.current_balance

        # Close wrong-side positions entirely
        if target_no == 0 and current_no > 0:
            orders.append(SellNo.at_price(market_id, 1 - b, current_no))
        if target_yes == 0 and current_yes > 0:
            orders.append(SellYes.at_price(market_id, b, current_yes))

        # Trim right-side positions down to target
        if 0 < target_yes < current_yes:
            orders.append(SellYes.at_price(market_id, b, current_yes - target_yes))
        if 0 < target_no < current_no:
            orders.append(SellNo.at_price(market_id, 1 - b, current_no - target_no))

        # Increase right-side toward target (capped by available cash)
        if target_yes > current_yes:
            want = target_yes - current_yes
            affordable = int(cash / b) if b > 0.01 else 0
            qty = min(want, affordable)
            if qty > 0:
                orders.append(BuyYes.at_price(market_id, b, qty))
        if target_no > current_no:
            want = target_no - current_no
            limit = 1 - b
            affordable = int(cash / limit) if limit > 0.01 else 0
            qty = min(want, affordable)
            if qty > 0:
                orders.append(BuyNo.at_price(market_id, limit, qty))

        return orders

    async def on_block(self, block: Block) -> list[OrderSpec]:
        market_id = next(iter(self.market_ids))
        prices = self.filter_markets(block)
        if market_id in prices:
            yes_nanos, _ = prices[market_id]
            yes_price = yes_nanos / NANOS_PER_DOLLAR
            self.price_history.append(PriceSnapshot(
                block_height=block.height,
                sim_time=self.clock.now(),
                yes_price=yes_price,
            ))
            if not self._belief_initialized and yes_price > 0:
                self._belief_alpha = yes_price
                self._belief_beta = 1.0 - yes_price
                self._belief_initialized = True

        arrived = self._drain_arrived_articles()
        market_id = next(iter(self.market_ids))

        if arrived:
            sim_t = self.clock.now().strftime("%H:%M")
            print(f"  [{self.name}] block {block.height} ({sim_t}): {len(arrived)} article(s)", flush=True)

        # Fire all LLM calls concurrently (clock pause is ref-counted)
        async def _analyze_one(article):
            print(f"    → LLM: \"{article.title[:60]}\" ({article.source})...", end="", flush=True)
            result = await self._phase2_analyze(article, block)
            return article, result

        results = await asyncio.gather(*[_analyze_one(a) for a in arrived])

        # Process results sequentially for belief updates
        analyses: list[tuple] = []
        best_conviction = 1

        for article, result in results:
            if isinstance(result, str):
                print(f" FAILED ({result})", flush=True)
                self.trade_log.append(TradeRecord(
                    article=article,
                    probability=0.0,
                    conviction=1,
                    motivation=result,
                    orders=[],
                    sim_time=self.clock.now(),
                    block_height=block.height,
                    balance=self.current_balance,
                    yes_pos=self.get_position(market_id, "YES"),
                    no_pos=self.get_position(market_id, "NO"),
                    belief=self.belief,
                ))
                continue

            probability, conviction, motivation, raw_response, llm_duration_s = result
            self._update_belief(probability, conviction)
            print(f" P={probability:.2f} C={conviction}/10 ({llm_duration_s:.1f}s) belief={self.belief:.3f}", flush=True)
            analyses.append((article, probability, conviction, motivation, raw_response, llm_duration_s))

            if conviction > best_conviction:
                best_conviction = conviction

        orders: list[OrderSpec] = []
        if analyses:
            orders = self._phase3_execute(best_conviction, block)
            if orders:
                order_desc = ", ".join(_describe_order(o) for o in orders)
                print(f"    → trade: {order_desc}", flush=True)
            else:
                print(f"    → no trade (edge too small)", flush=True)

        for i, (article, probability, conviction, motivation, raw_response, llm_duration_s) in enumerate(analyses):
            is_last = (i == len(analyses) - 1)
            article_orders = orders if is_last else []

            bal, yp, np_, rp, tp = getattr(self, '_last_exec_state', (0, 0, 0, 0, 0))
            self.trade_log.append(TradeRecord(
                article=article,
                probability=probability,
                conviction=conviction,
                motivation=motivation,
                orders=article_orders,
                sim_time=self.clock.now(),
                llm_response=raw_response,
                block_height=block.height,
                llm_duration_s=llm_duration_s,
                balance=bal,
                yes_pos=yp,
                no_pos=np_,
                risk_pct=rp if is_last else 0.0,
                target_pos=tp if is_last else 0,
                belief=self.belief,
            ))

            log.info(
                "[%s] P=%.2f %s belief=%.3f edge=%.3f → %d orders | %s",
                self.name, probability, conviction, self.belief,
                abs(self.belief - (self.filter_markets(block)[market_id][0] / NANOS_PER_DOLLAR)),
                len(article_orders), motivation,
            )

        return orders


def _describe_order(order: OrderSpec) -> str:
    """Human-readable order description."""
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
    """Match fills to submitted orders and format as a summary string."""
    if not fills:
        return "no fills"

    fill_agg: dict[tuple[str, str], tuple[int, int]] = {}
    for f in fills:
        for delta in f.position_deltas:
            action = "Buy" if delta.delta > 0 else "Sell"
            key = (action, delta.outcome)
            prev_qty, prev_cost = fill_agg.get(key, (0, 0))
            fill_agg[key] = (
                prev_qty + abs(delta.delta),
                prev_cost + abs(delta.delta) * f.fill_price_nanos,
            )

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
