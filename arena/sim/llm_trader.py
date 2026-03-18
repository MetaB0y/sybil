"""LLM-driven trader that delegates all trading decisions to the language model."""

import json
import logging
import re
import time
from dataclasses import dataclass, field
from datetime import datetime, timedelta
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
    articles: list[Article]
    analysis: str
    fair_value: float
    orders: list[OrderSpec]
    motivation: str
    raw_llm_response: str
    llm_duration_s: float
    block_height: int
    sim_time: datetime
    balance: float
    yes_pos: int
    no_pos: int

    def to_dict(self) -> dict:
        articles_list = [
            {
                "title": a.title,
                "source": a.source,
                "url": a.url,
                "timestamp": a.timestamp.isoformat(),
            }
            for a in self.articles
        ]
        # Backward-compat top-level fields from first article
        first = self.articles[0] if self.articles else None
        return {
            "sim_time": self.sim_time.isoformat(),
            "block_height": self.block_height,
            "llm_duration_s": self.llm_duration_s,
            "article_title": first.title if first else "",
            "article_source": first.source if first else "",
            "article_url": first.url if first else "",
            "article_timestamp": first.timestamp.isoformat() if first else "",
            "articles": articles_list,
            "analysis": self.analysis,
            "fair_value": self.fair_value,
            "orders": [_describe_order(o) for o in self.orders],
            "motivation": self.motivation,
            "raw_llm_response": self.raw_llm_response,
            "balance": self.balance,
            "yes_pos": self.yes_pos,
            "no_pos": self.no_pos,
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
- Extreme FAIR_VALUE (>0.85) requires extraordinary evidence. Most geopolitical events have genuine uncertainty — reflect that. Update based on the full picture, not just the latest article.

Always respond in English regardless of article language."""


class LlmTrader(BaseAgent):
    """LLM-driven trader that delegates all trading decisions to the language model."""

    def __init__(
        self,
        client,
        account_id: int,
        articles: list[Article],
        clock: SimulatedClock,
        api_key: str,
        persona: str,
        market_question: str,
        context: str = "",
        model_name: str = "google/gemini-3.1-flash-lite-preview",
        name: str | None = None,
        market_ids: list[int] | None = None,
    ):
        super().__init__(client, account_id, name or "LlmTrader", market_ids)
        self.articles = articles
        self.clock = clock
        self.api_key = api_key
        self.model_name = model_name
        self.persona = persona
        self.market_question = market_question
        self.context = context
        self._article_index = 0
        self._llm_client: openai.AsyncOpenAI | None = None
        self.trade_log: list[TradeRecord] = []
        self.price_history: list[PriceSnapshot] = []
        self._observed_first_block = False
        self._last_rebalance_time: datetime | None = None

    def snapshot_state(self) -> dict:
        """Capture cross-day state for multi-day simulations."""
        return {
            "trade_log": self.trade_log,
            "price_history": self.price_history,
            "_last_rebalance_time": self._last_rebalance_time,
        }

    def restore_state(self, state: dict) -> None:
        """Restore cross-day state from a previous day's snapshot."""
        self.trade_log = state["trade_log"]
        self.price_history = state["price_history"]
        self._last_rebalance_time = state.get("_last_rebalance_time")

    def _get_llm_client(self) -> openai.AsyncOpenAI:
        if self._llm_client is None:
            self._llm_client = openai.AsyncOpenAI(
                base_url="https://openrouter.ai/api/v1",
                api_key=self.api_key,
                timeout=openai.Timeout(60.0, connect=10.0),
                max_retries=0,
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
            art_count = f" ({len(rec.articles)} articles)" if len(rec.articles) > 1 else ""
            lines.append(
                f"- [{rec.sim_time:%H:%M}] FV={rec.fair_value:.2f}{art_count} | {rec.motivation}"
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

    def _format_last_reasoning(self) -> str:
        """Format the last trade's reasoning for self-reflection."""
        # Find last trade with actual analysis (skip API/parse errors)
        last = None
        for rec in reversed(self.trade_log):
            if rec.analysis and rec.fair_value > 0:
                last = rec
                break
        if last is None:
            return ""

        current_price = self.price_history[-1].yes_price if self.price_history else None
        if current_price is not None:
            price_at_trade = None
            for snap in self.price_history:
                if snap.block_height >= last.block_height:
                    price_at_trade = snap.yes_price
                    break
            if price_at_trade is not None:
                move = current_price - price_at_trade
                move_str = f"YES moved {move:+.4f} since then"
            else:
                move_str = "price movement unknown"
        else:
            move_str = "no price data"

        return (
            f"Your last reasoning (FV={last.fair_value:.2f}, {move_str}):\n"
            f'"{last.analysis}"\n'
            f"Reflect: does the new evidence support or contradict your prior thesis?"
        )

    def _build_prompt(self, articles: list[Article], block: Block) -> str:
        """Assemble the full prompt for the LLM."""
        market_id = next(iter(self.market_ids))
        yes_nanos, _ = self.filter_markets(block)[market_id]
        yes_price = yes_nanos / NANOS_PER_DOLLAR

        # Price trend from last 5 snapshots
        recent_prices = [s.yes_price for s in self.price_history[-5:]]
        price_trend = ", ".join(f"{p:.4f}" for p in recent_prices) if recent_prices else "n/a"

        balance = self.current_balance
        yes_shares = self.get_position(market_id, "YES")
        no_shares = self.get_position(market_id, "NO")
        portfolio_value = balance + yes_shares * yes_price + no_shares * (1 - yes_price)
        cash_pct = (balance / portfolio_value * 100) if portfolio_value > 0 else 100

        context_line = f"\n{self.context}" if self.context else ""

        # Build article section
        if len(articles) == 1:
            art = articles[0]
            article_section = (
                f'New article from {art.source}:\n'
                f'"{art.title}"\n\n'
                f'{art.full_text[:3000]}'
            )
        else:
            # Distribute truncation budget across articles
            budget_per = max(500, 6000 // len(articles))
            parts = ["New articles this batch:\n"]
            for idx, art in enumerate(articles, 1):
                parts.append(
                    f'[{idx}] From {art.source}: "{art.title}"\n'
                    f'{art.full_text[:budget_per]}\n'
                )
            article_section = "\n".join(parts)

        # Build dynamic available-actions block
        no_price = 1 - yes_price
        actions = []
        if balance >= 0.01:
            max_yes = int(balance / yes_price) if yes_price > 0 else 0
            max_no = int(balance / no_price) if no_price > 0 else 0
            actions.append(f"- BUY_YES <qty> @ <price>: costs ~${yes_price:.2f}/share (up to ~{max_yes} shares)")
            actions.append(f"- BUY_NO <qty> @ <price>: costs ~${no_price:.2f}/share (up to ~{max_no} shares)")
        else:
            actions.append("- BUY_YES / BUY_NO: not available (no cash)")
        if yes_shares > 0:
            actions.append(f"- SELL_YES <qty> @ <price>: sell {yes_shares} YES shares at ~${yes_price:.2f}/share (only if BEARISH, FV < {yes_price:.2f})")
        else:
            actions.append("- SELL_YES: NOT available (you hold 0 YES shares)")
        if no_shares > 0:
            actions.append(f"- SELL_NO <qty> @ <price>: sell {no_shares} NO shares at ~${no_price:.2f}/share (only if BULLISH, FV > {yes_price:.2f})")
        else:
            actions.append("- SELL_NO: NOT available (you hold 0 NO shares)")
        actions.append("- HOLD: do nothing")
        actions_block = "\n".join(actions)

        analyze_word = "these articles" if len(articles) > 1 else "this article"

        last_reasoning = self._format_last_reasoning()
        reflection_section = f"\nPrevious reasoning:\n{last_reasoning}\n" if last_reasoning else ""

        return f"""{SYSTEM_PROMPT}

{self.persona}

Market: "{self.market_question}"{context_line}

Current state:
- YES price: ${yes_price:.4f} | NO price: ${no_price:.4f} (last 5 YES: {price_trend})
- Your portfolio: ${balance:.2f} cash ({cash_pct:.0f}% of portfolio), {yes_shares} YES shares, {no_shares} NO shares
- Estimated portfolio value: ~${portfolio_value:.2f}

Recent trades:
{self._format_recent_trades()}
{reflection_section}
{article_section}

Available actions:
{actions_block}

Analyze {analyze_word} and decide your trade. Respond in this exact format:

ANALYSIS: [Your analysis of what {analyze_word} signals, 2-4 sentences]
FAIR_VALUE: [Your probability estimate, 0.01-0.99]
EDGE: [Calculate: |FAIR_VALUE - YES price| = edge per share. edge × quantity = expected profit. Only trade if edge > $0.03]
ORDERS: [Choose from available actions, or HOLD if no edge. LIMIT PRICE — do NOT just use the current market price.
For BUY_YES: set limit between YES price and your FAIR_VALUE. For BUY_NO: set limit between NO price and (1 - FAIR_VALUE). Example: if YES=$0.70, NO=$0.30, and your FV=0.15, your NO fair value is $0.85 — bid NO at $0.50-0.60, NOT $0.30.
For SELL_YES: set limit between your FAIR_VALUE and YES price (lower = more likely to fill; FBA guarantees you get the clearing price, not your limit). For SELL_NO: set limit between (1 - FAIR_VALUE) and NO price.
Routine news → limit closer to market. Breaking news → limit closer to your fair value.]
MOTIVATION: [1-2 sentence thesis]"""

    def _parse_orders(self, text: str) -> tuple[str, float, list[OrderSpec], str] | None:
        """Parse structured LLM output into (analysis, fair_value, orders, motivation)."""
        KEYWORDS = r"\nANALYSIS:|\nFAIR_VALUE:|\nEDGE:|\nORDERS:|\nMOTIVATION:|\Z"

        # Parse ANALYSIS (everything until next keyword)
        analysis_match = re.search(
            rf"ANALYSIS:\s*(.*?)(?={KEYWORDS})",
            text, re.DOTALL,
        )
        analysis = analysis_match.group(1).strip() if analysis_match else ""

        # Parse FAIR_VALUE
        fv_match = re.search(r"FAIR_VALUE:\s*([\d.]+)", text)
        if not fv_match:
            log.warning("Failed to parse FAIR_VALUE from LLM output")
            return None
        fair_value = float(fv_match.group(1))
        if not 0.01 <= fair_value <= 0.99:
            log.warning("FAIR_VALUE out of range: %s", fair_value)
            return None

        # Parse MOTIVATION (everything after keyword until end or next keyword)
        motiv_match = re.search(
            rf"MOTIVATION:\s*(.*?)(?={KEYWORDS})",
            text, re.DOTALL,
        )
        motivation = motiv_match.group(1).strip() if motiv_match else ""

        # Parse ORDERS
        orders_match = re.search(
            rf"ORDERS:\s*(.*?)(?={KEYWORDS})",
            text, re.DOTALL,
        )
        orders_text = orders_match.group(1).strip() if orders_match else ""

        orders: list[OrderSpec] = []
        if "HOLD" in orders_text.upper() and not re.search(r"(BUY|SELL)", orders_text.upper()):
            return (analysis, fair_value, [], motivation)

        market_id = next(iter(self.market_ids))
        order_map = {
            "BUY_YES": BuyYes,
            "BUY_NO": BuyNo,
            "SELL_YES": SellYes,
            "SELL_NO": SellNo,
        }
        for m in re.finditer(r"(BUY_YES|BUY_NO|SELL_YES|SELL_NO)\s+(\d+)\s*@\s*\$?([\d.]+)", orders_text):
            side = m.group(1)
            qty = int(m.group(2))
            price = float(m.group(3))
            cls = order_map[side]
            if qty > 0:
                orders.append(cls.at_price(market_id, price, qty))

        return (analysis, fair_value, orders, motivation)

    def _validate_orders(self, orders: list[OrderSpec], block: Block) -> list[OrderSpec]:
        """Clip orders to what's affordable/held, clamp prices, enforce concentration limit."""
        market_id = next(iter(self.market_ids))
        yes_nanos, _ = self.filter_markets(block)[market_id]
        yes_price = yes_nanos / NANOS_PER_DOLLAR
        no_price = 1 - yes_price

        yes_held = self.get_position(market_id, "YES")
        no_held = self.get_position(market_id, "NO")
        cash = self.current_balance
        portfolio_value = cash + yes_held * yes_price + no_held * no_price

        # Hard cap: no single buy order can exceed 25% of portfolio value
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
                # Concentration limit
                max_qty_by_conc = int(max_order_value / price) if price > 0 else 0
                qty = min(qty, max_qty_by_conc)
                # Cash limit
                cost = qty * price
                if cost > cash:
                    qty = int(cash / price) if price > 0 else 0
                cash -= qty * price

            if qty <= 0:
                continue

            cls = type(order)
            valid.append(cls.at_price(market_id, price, qty))

        return valid

    async def _call_llm_raw(self, prompt: str) -> tuple[str, float]:
        """Call LLM without pause/resume. Caller must handle pausing."""
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

    async def _call_llm(self, prompt: str) -> tuple[str, float]:
        """Pause clock + client, call LLM, resume. Returns (text, duration_s)."""
        await self.client.pause()
        self.clock.pause()
        try:
            return await self._call_llm_raw(prompt)
        finally:
            self.clock.resume()
            await self.client.resume()

    def should_rebalance(self, interval_hours: float = 4) -> bool:
        """Return True if the trader has positions and enough time has passed since first trade."""
        market_id = next(iter(self.market_ids))
        yes_pos = self.get_position(market_id, "YES")
        no_pos = self.get_position(market_id, "NO")
        if yes_pos == 0 and no_pos == 0:
            return False
        # First rebalance: 4h after first trade. Subsequent: 4h after last rebalance.
        anchor = self._last_rebalance_time
        if anchor is None:
            # Use time of first trade with orders as anchor
            for rec in self.trade_log:
                if rec.orders:
                    anchor = rec.sim_time
                    break
            if anchor is None:
                return False
        elapsed = (self.clock.now() - anchor).total_seconds() / 3600
        return elapsed >= interval_hours

    def _compute_cost_basis(self, market_id: int) -> tuple[float, float]:
        """Compute weighted average entry price for YES and NO from trade_log.

        Returns (yes_avg_cost, no_avg_cost).
        """
        yes_total_cost = 0.0
        yes_total_qty = 0
        no_total_cost = 0.0
        no_total_qty = 0
        for rec in self.trade_log:
            for order in rec.orders:
                price = order.limit_price_nanos / NANOS_PER_DOLLAR
                if isinstance(order, BuyYes):
                    yes_total_cost += price * order.quantity
                    yes_total_qty += order.quantity
                elif isinstance(order, BuyNo):
                    no_total_cost += price * order.quantity
                    no_total_qty += order.quantity
        yes_avg = yes_total_cost / yes_total_qty if yes_total_qty > 0 else 0.0
        no_avg = no_total_cost / no_total_qty if no_total_qty > 0 else 0.0
        return yes_avg, no_avg

    def _get_price_at_offset(self, hours_ago: float) -> float | None:
        """Get YES price from price_history approximately hours_ago sim hours back."""
        if not self.price_history:
            return None
        target = self.clock.now() - timedelta(hours=hours_ago)
        best = None
        best_delta = float("inf")
        for snap in self.price_history:
            delta = abs((snap.sim_time - target).total_seconds())
            if delta < best_delta:
                best_delta = delta
                best = snap.yes_price
        return best

    def _build_rebalance_prompt(self, block: Block) -> str:
        """Construct the rebalance-specific prompt."""
        market_id = next(iter(self.market_ids))
        yes_nanos, _ = self.filter_markets(block)[market_id]
        yes_price = yes_nanos / NANOS_PER_DOLLAR
        no_price = 1 - yes_price

        price_4h = self._get_price_at_offset(4)
        price_8h = self._get_price_at_offset(8)
        price_4h_str = f"${price_4h:.2f}" if price_4h is not None else "n/a"
        price_8h_str = f"${price_8h:.2f}" if price_8h is not None else "n/a"

        balance = self.current_balance
        yes_pos = self.get_position(market_id, "YES")
        no_pos = self.get_position(market_id, "NO")
        portfolio_value = balance + yes_pos * yes_price + no_pos * no_price
        cash_pct = (balance / portfolio_value * 100) if portfolio_value > 0 else 100

        yes_avg_cost, no_avg_cost = self._compute_cost_basis(market_id)
        yes_pnl = (yes_price - yes_avg_cost) * yes_pos if yes_pos > 0 else 0.0
        no_pnl = (no_price - no_avg_cost) * no_pos if no_pos > 0 else 0.0
        starting_balance = 2000.0
        total_pnl = portfolio_value - starting_balance

        # Recent trade motivations (last 5)
        recent_trades_lines = []
        for rec in self.trade_log[-5:]:
            if rec.motivation:
                prefix = "[REBALANCE] " if rec.motivation.startswith("[REBALANCE]") else ""
                order_desc = ", ".join(_describe_order(o) for o in rec.orders) or "HOLD"
                recent_trades_lines.append(
                    f"- [{rec.sim_time:%H:%M}] {order_desc} | {rec.motivation}"
                )
        recent_trades = "\n".join(recent_trades_lines) if recent_trades_lines else "No trades yet."

        # Recent headlines since last rebalance
        headline_lines = []
        cutoff = self._last_rebalance_time or self.clock.now() - timedelta(hours=4)
        for art in self.articles:
            if art.timestamp > cutoff and self.clock.is_past(art.timestamp):
                headline_lines.append(f"- {art.source}: {art.title}")
        recent_headlines = "\n".join(headline_lines[-10:]) if headline_lines else "No new headlines."

        # Available actions — sell only
        actions = []
        if yes_pos > 0:
            actions.append(f"- SELL_YES <qty> @ <price>: sell up to {yes_pos} YES shares (currently ${yes_price:.2f}/share)")
        if no_pos > 0:
            actions.append(f"- SELL_NO <qty> @ <price>: sell up to {no_pos} NO shares (currently ${no_price:.2f}/share)")
        actions.append("- HOLD: keep current positions")
        actions_block = "\n".join(actions)

        return f"""{SYSTEM_PROMPT}

{self.persona}

PORTFOLIO REVIEW — Periodic rebalancing check.

Market: "{self.market_question}"
Current YES price: ${yes_price:.2f} | NO price: ${no_price:.2f}
Price 4h ago: {price_4h_str} | Price 8h ago: {price_8h_str}

Reminder: This is a Frequent Batch Auction — your limit price is the worst
price you'd accept, not what you'll pay. FBA guarantees you get the clearing
price, not your limit. For SELL_YES: set limit between your FAIR_VALUE and
the current YES price (lower = more likely to fill). For SELL_NO: set limit
between (1 - FAIR_VALUE) and the current NO price.

Your portfolio:
- ${balance:.2f} cash ({cash_pct:.0f}% of portfolio)
- {yes_pos} YES shares (avg cost: ${yes_avg_cost:.2f}, now ${yes_price:.2f} each) → unrealized P&L: ${yes_pnl:+.2f}
- {no_pos} NO shares (avg cost: ${no_avg_cost:.2f}, now ${no_price:.2f} each) → unrealized P&L: ${no_pnl:+.2f}
- Portfolio value: ${portfolio_value:.2f} (started at $2,000 → total P&L: ${total_pnl:+.2f})

Your recent trades:
{recent_trades}

Recent headlines since your last review:
{recent_headlines}

Available actions (SELL or HOLD only — no buying during rebalance):
{actions_block}

Review your positions. Profits are only real when locked in. Holding a winning
position through a reversal means you never had the profit. Consider:
- Has the market already priced in your thesis? If price ≈ what you expected,
  your edge is gone — sell some or all.
- Has counter-evidence appeared? Cut losers early.
- Are you overexposed? Reducing a large position to free cash for later
  opportunities is smart risk management.

ANALYSIS: [1-2 sentences: should you take profit, cut losses, reduce exposure,
or is holding still justified? Why?]
FAIR_VALUE: [Your probability that YES happens, 0.01-0.99. NOT your confidence in your position — the actual probability of the event. Low FV = event unlikely = NO is valuable.]
ORDERS: [HOLD | SELL_YES <qty> @ <price> | SELL_NO <qty> @ <price>]
MOTIVATION: [1 sentence thesis]"""

    async def rebalance(self, block: Block) -> list[OrderSpec]:
        """Run a rebalance check. Caller must handle clock/client pausing."""
        market_id = next(iter(self.market_ids))
        yes_pos = self.get_position(market_id, "YES")
        no_pos = self.get_position(market_id, "NO")
        if yes_pos == 0 and no_pos == 0:
            return []

        prompt = self._build_rebalance_prompt(block)

        try:
            raw_text, llm_duration_s = await self._call_llm_raw(prompt)
            log.info(
                "[%s] Rebalance LLM response (%.1fs):\n%s",
                self.name, llm_duration_s, raw_text,
            )
        except Exception as e:
            log.warning("[%s] Rebalance LLM call failed: %s", self.name, e)
            print(f"  [{self.name}] REBALANCE FAILED (API error: {e})", flush=True)
            return []

        parsed = self._parse_orders(raw_text)
        if parsed is None:
            print(f"  [{self.name}] REBALANCE FAILED (parse error)", flush=True)
            self._last_rebalance_time = self.clock.now()
            return []

        analysis, fair_value, orders, motivation = parsed

        # Filter out any BUY orders — rebalance is sell-only
        orders = [o for o in orders if isinstance(o, (SellYes, SellNo))]
        orders = self._validate_orders(orders, block)

        # Deduct pending sells from local positions to prevent double-selling
        # when on_block fires before these orders fill.
        for order in orders:
            if isinstance(order, SellYes):
                key = (market_id, "YES")
                self.positions[key] = self.positions.get(key, 0) - order.quantity
            elif isinstance(order, SellNo):
                key = (market_id, "NO")
                self.positions[key] = self.positions.get(key, 0) - order.quantity

        self._last_rebalance_time = self.clock.now()

        self.trade_log.append(TradeRecord(
            articles=[],
            analysis=analysis,
            fair_value=fair_value,
            orders=orders,
            motivation=f"[REBALANCE] {motivation}",
            raw_llm_response=raw_text,
            llm_duration_s=llm_duration_s,
            block_height=block.height,
            sim_time=self.clock.now(),
            balance=self.current_balance,
            yes_pos=self.get_position(market_id, "YES"),
            no_pos=self.get_position(market_id, "NO"),
        ))

        if orders:
            order_desc = ", ".join(_describe_order(o) for o in orders)
            print(f"  [{self.name}] REBALANCE FV={fair_value:.2f} ({llm_duration_s:.1f}s) -> {order_desc}", flush=True)
        else:
            print(f"  [{self.name}] REBALANCE FV={fair_value:.2f} ({llm_duration_s:.1f}s) -> HOLD", flush=True)

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

        # Skip first block: observe the market price before trading
        if not self._observed_first_block:
            self._observed_first_block = True
            return []

        arrived = self._drain_arrived_articles()
        if not arrived:
            return []

        sim_t = self.clock.now().strftime("%H:%M")
        titles = "; ".join(f'"{a.title[:50]}"' for a in arrived)
        print(f"  [{self.name}] block {block.height} ({sim_t}): {len(arrived)} article(s)", flush=True)
        print(f"    -> LLM: {titles}...", end="", flush=True)

        prompt = self._build_prompt(arrived, block)

        try:
            raw_text, llm_duration_s = await self._call_llm(prompt)
            log.info(
                "[%s] LLM response for %d articles (%.1fs):\n%s",
                self.name, len(arrived), llm_duration_s, raw_text,
            )
        except Exception as e:
            log.warning("[%s] LLM call failed: %s", self.name, e)
            print(f" FAILED (API error: {e})", flush=True)
            self.trade_log.append(TradeRecord(
                articles=arrived,
                analysis="",
                fair_value=0.0,
                orders=[],
                motivation=f"API error: {e}",
                raw_llm_response="",
                llm_duration_s=0.0,
                block_height=block.height,
                sim_time=self.clock.now(),
                balance=self.current_balance,
                yes_pos=self.get_position(market_id, "YES"),
                no_pos=self.get_position(market_id, "NO"),
            ))
            return []

        parsed = self._parse_orders(raw_text)
        if parsed is None:
            print(f" FAILED (parse error)", flush=True)
            self.trade_log.append(TradeRecord(
                articles=arrived,
                analysis="",
                fair_value=0.0,
                orders=[],
                motivation="parse error",
                raw_llm_response=raw_text,
                llm_duration_s=llm_duration_s,
                block_height=block.height,
                sim_time=self.clock.now(),
                balance=self.current_balance,
                yes_pos=self.get_position(market_id, "YES"),
                no_pos=self.get_position(market_id, "NO"),
            ))
            return []

        analysis, fair_value, orders, motivation = parsed
        orders = self._validate_orders(orders, block)

        # Reject directionally inconsistent orders (not applied to rebalancing)
        yes_nanos, _ = self.filter_markets(block)[market_id]
        cur_yes = yes_nanos / NANOS_PER_DOLLAR
        consistent = []
        for o in orders:
            if isinstance(o, (BuyYes, SellNo)) and fair_value < cur_yes:
                log.info("[%s] Rejected bullish order (FV=%.2f < mkt=%.2f): %s",
                         self.name, fair_value, cur_yes, _describe_order(o))
                continue
            if isinstance(o, (BuyNo, SellYes)) and fair_value > cur_yes:
                log.info("[%s] Rejected bearish order (FV=%.2f > mkt=%.2f): %s",
                         self.name, fair_value, cur_yes, _describe_order(o))
                continue
            consistent.append(o)
        orders = consistent

        self.trade_log.append(TradeRecord(
            articles=arrived,
            analysis=analysis,
            fair_value=fair_value,
            orders=orders,
            motivation=motivation,
            raw_llm_response=raw_text,
            llm_duration_s=llm_duration_s,
            block_height=block.height,
            sim_time=self.clock.now(),
            balance=self.current_balance,
            yes_pos=self.get_position(market_id, "YES"),
            no_pos=self.get_position(market_id, "NO"),
        ))

        if orders:
            order_desc = ", ".join(_describe_order(o) for o in orders)
            print(f" FV={fair_value:.2f} ({llm_duration_s:.1f}s) -> {order_desc}", flush=True)
        else:
            print(f" FV={fair_value:.2f} ({llm_duration_s:.1f}s) -> HOLD", flush=True)

        log.info(
            "[%s] FV=%.2f -> %d orders | %s",
            self.name, fair_value, len(orders), motivation,
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
