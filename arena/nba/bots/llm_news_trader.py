"""LLM-powered sports analyst trading bot for backtesting."""

import asyncio
import json
import logging
import re

from sybil_client import Block, BuyNo, BuyYes, OrderSpec

from backtest.agent import BacktestAgent
from backtest.dataset import NewsItem
from bots.strategy_agent import format_news_line

logger = logging.getLogger(__name__)

NANOS_PER_DOLLAR = 1_000_000_000

DEFAULT_SYSTEM_PROMPT = (
    "You are an NBA analyst estimating real-time win probabilities.\n"
    "Given live game updates, estimate the probability that the HOME team wins.\n"
    "Consider: current score, quarter, momentum, injuries, matchups.\n"
    "First output a JSON object mapping market keys to probabilities.\n"
    "Example: {\"market_0\": 0.65, \"market_1\": 0.30}\n"
    "Then write 1-2 sentences of reasoning explaining your key factors.\n"
    "Use the full probability range. A 20-point lead in Q4 should be ~0.95."
)

CONTRARIAN_SYSTEM_PROMPT = (
    "You are a contrarian NBA analyst estimating real-time win probabilities.\n"
    "You tend to believe the market overreacts to recent news. A star injury doesn't\n"
    "doom a team - others step up. A big lead can evaporate. Recent momentum is noise.\n"
    "Given live game updates, estimate the probability that the HOME team wins.\n"
    "First output a JSON object mapping market keys to probabilities.\n"
    "Example: {\"market_0\": 0.65, \"market_1\": 0.30}\n"
    "Then write 1-2 sentences of reasoning. Use the full range but lean against the crowd."
)


def _build_prompt(
    event_news: dict[str, list[str]],
    event_info: dict[str, dict],
    market_prices: dict[str, float],
    positions: dict[str, int] | None = None,
    balance: float | None = None,
) -> str:
    """Build the user prompt with accumulated news for all events.

    Args:
        event_news: event_id -> list of formatted news lines (most recent first)
        event_info: event_id -> {"home_team": ..., "away_team": ..., "market_key": ...}
        market_prices: market_key -> current market probability
        positions: market_key -> net position (positive=long YES, negative=long NO)
        balance: current cash balance in dollars
    """
    lines = ["=== NBA Games in Progress ===\n"]

    if balance is not None:
        lines.append(f"Your cash balance: ${balance:.2f}")
        lines.append("")

    for event_id, info in sorted(event_info.items(), key=lambda x: x[1]["market_key"]):
        market_key = info["market_key"]
        home = info["home_team"]
        away = info["away_team"]
        price = market_prices.get(market_key, 0.5)

        lines.append(f"[{market_key}] {home} (HOME) vs {away}")

        news_lines = event_news.get(event_id, [])
        if news_lines:
            lines.append("  Live updates (most recent first):")
            for nl in news_lines[-15:]:  # Cap to avoid huge prompts
                lines.append(f"  - {nl}")
        else:
            lines.append("  No updates yet.")

        lines.append(f"  Current market price: {price * 100:.1f}% HOME wins")

        if positions:
            pos = positions.get(market_key, 0)
            if pos > 0:
                lines.append(f"  Your position: long {pos} YES shares")
            elif pos < 0:
                lines.append(f"  Your position: long {-pos} NO shares")
            else:
                lines.append(f"  Your position: flat")

        lines.append("")

    return "\n".join(lines)


def _extract_reasoning(text: str) -> str:
    """Extract reasoning text after the JSON block."""
    start = text.find("{")
    if start == -1:
        return text.strip()
    depth = 0
    for i in range(start, len(text)):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                remainder = text[i + 1:].strip()
                # Strip markdown code fences
                remainder = re.sub(r"^```\w*\s*", "", remainder)
                remainder = re.sub(r"\s*```$", "", remainder)
                # Strip markdown bold markers
                remainder = remainder.replace("**", "")
                # Strip leading "Reasoning:" or similar headers
                remainder = re.sub(r"^Reasoning[^:]*:\s*", "", remainder, flags=re.IGNORECASE)
                remainder = remainder.strip()
                return remainder[:300] if remainder else ""
    return ""


def _parse_llm_response(text: str, expected_keys: list[str]) -> dict[str, float] | None:
    """Parse LLM response JSON, handling markdown fences and edge cases.

    Returns None on parse failure.
    """
    # Extract the first JSON object from the response.
    # Models may add commentary before/after the JSON.
    cleaned = text.strip()

    # Try to find a JSON object by matching braces
    start = cleaned.find("{")
    if start == -1:
        return None
    depth = 0
    end = start
    for i in range(start, len(cleaned)):
        if cleaned[i] == "{":
            depth += 1
        elif cleaned[i] == "}":
            depth -= 1
            if depth == 0:
                end = i + 1
                break
    if depth != 0:
        return None

    try:
        data = json.loads(cleaned[start:end])
    except json.JSONDecodeError:
        return None

    if not isinstance(data, dict):
        return None

    # Try exact key match first
    result = {}
    for key in expected_keys:
        val = data.get(key)
        if val is not None:
            try:
                prob = float(val)
                result[key] = max(0.01, min(0.99, prob))
            except (ValueError, TypeError):
                continue

    if result:
        return result

    # Fallback: match by index — if response has numeric values, map them
    # to expected keys in order. Handles models that use different key names.
    float_vals = []
    for v in data.values():
        try:
            prob = float(v)
            if 0 <= prob <= 1:
                float_vals.append(max(0.01, min(0.99, prob)))
        except (ValueError, TypeError):
            continue

    if float_vals and len(float_vals) <= len(expected_keys):
        for key, prob in zip(expected_keys, float_vals):
            result[key] = prob
        return result

    return None


class LLMNewsTrader(BacktestAgent):
    """Processes game news via LLM to estimate win probabilities and trade.

    Accumulates news per event, periodically sends to an LLM (Anthropic or OpenAI)
    for probability estimates, then trades when edge exceeds threshold.
    """

    def __init__(
        self,
        client,
        account_id: int,
        clock,
        name: str | None = None,
        market_ids: list[int] | None = None,
        event_market_map: dict[str, int] | None = None,
        provider: str = "anthropic",
        model_name: str = "claude-sonnet-4-5-20250929",
        api_key: str = "",
        system_prompt: str | None = None,
        edge_threshold: float = 0.03,
        order_size: int = 15,
        max_position: int = 80,
        min_blocks_between_calls: int = 5,
    ):
        super().__init__(
            client=client,
            account_id=account_id,
            clock=clock,
            name=name,
            market_ids=market_ids,
            event_market_map=event_market_map,
        )
        self.provider = provider
        self.model_name = model_name
        self.api_key = api_key
        self.system_prompt = system_prompt or DEFAULT_SYSTEM_PROMPT
        self.edge_threshold = edge_threshold
        self.base_order_size = order_size
        self.max_position = max_position
        self.min_blocks_between_calls = min_blocks_between_calls

        # Per-event accumulated news (event_id -> list of formatted strings, newest first)
        self._event_news: dict[str, list[str]] = {}
        # Per-event info (event_id -> {"home_team", "away_team", "market_key"})
        self._event_info: dict[str, dict] = {}
        # Cached probabilities from last LLM call (market_key -> probability)
        self._cached_probs: dict[str, float] = {}
        # Rate limiting
        self._blocks_since_last_call = 0
        self._pending_news_count = 0  # news items since last LLM call
        # LLM client (lazy init)
        self._llm_client = None
        # Pending LLM task
        self._llm_task: asyncio.Task | None = None
        # Last LLM reasoning (for display)
        self.last_reasoning: str = ""
        # Last error (surfaced in TUI thoughts panel)
        self.last_error: str = ""

        # Build reverse maps: market_id <-> market_key
        self._market_id_to_key: dict[int, str] = {}
        self._key_to_market_id: dict[str, int] = {}
        if event_market_map:
            for idx, (event_id, market_id) in enumerate(
                sorted(event_market_map.items(), key=lambda x: x[1])
            ):
                key = f"market_{idx}"
                self._market_id_to_key[market_id] = key
                self._key_to_market_id[key] = market_id
                self._event_info[event_id] = {
                    "home_team": "",
                    "away_team": "",
                    "market_key": key,
                }

    def _get_llm_client(self):
        """Lazily initialize the LLM API client."""
        if self._llm_client is not None:
            return self._llm_client

        if self.provider == "anthropic":
            import anthropic

            self._llm_client = anthropic.AsyncAnthropic(api_key=self.api_key)
        elif self.provider == "openai":
            import openai

            self._llm_client = openai.AsyncOpenAI(api_key=self.api_key)
        elif self.provider == "openrouter":
            import openai

            self._llm_client = openai.AsyncOpenAI(
                api_key=self.api_key,
                base_url="https://openrouter.ai/api/v1",
            )
        else:
            raise ValueError(f"Unknown provider: {self.provider}")

        return self._llm_client

    def _format_news_line(self, news: NewsItem) -> str:
        """Format a news item as a concise line for the LLM prompt."""
        return format_news_line(news)

    async def on_news(self, news: NewsItem) -> None:
        """Accumulate news and flag for LLM update."""
        if news.event_id is None:
            return

        # Update event info from metadata
        info = self._event_info.get(news.event_id)
        if info is not None:
            if "home_team" in news.metadata:
                info["home_team"] = news.metadata["home_team"]
            if "away_team" in news.metadata:
                info["away_team"] = news.metadata["away_team"]

        # Also try to extract team names from the headline for score updates
        if info is not None and not info["home_team"]:
            market_id = self.event_market_map.get(news.event_id)
            if market_id is not None:
                # Will be filled from lineup news
                pass

        # Accumulate news (prepend = most recent first)
        formatted = self._format_news_line(news)
        if news.event_id not in self._event_news:
            self._event_news[news.event_id] = []
        self._event_news[news.event_id].insert(0, formatted)

        self._pending_news_count += 1

    async def _call_llm(self, prompt: str) -> str | None:
        """Call the LLM API with timeout. Returns response text or None."""
        try:
            llm_client = self._get_llm_client()

            if self.provider == "anthropic":
                response = await asyncio.wait_for(
                    llm_client.messages.create(
                        model=self.model_name,
                        max_tokens=400,
                        system=self.system_prompt,
                        messages=[
                            {"role": "user", "content": prompt},
                        ],
                    ),
                    timeout=25.0,
                )
                self.last_error = ""
                return response.content[0].text

            elif self.provider in ("openai", "openrouter"):
                kwargs = dict(
                    model=self.model_name,
                    max_tokens=400,
                    messages=[
                        {"role": "system", "content": self.system_prompt},
                        {"role": "user", "content": prompt},
                    ],
                )
                # Only native OpenAI supports forced JSON mode reliably
                if self.provider == "openai":
                    kwargs["response_format"] = {"type": "json_object"}
                response = await asyncio.wait_for(
                    llm_client.chat.completions.create(**kwargs),
                    timeout=30.0,
                )
                self.last_error = ""
                return response.choices[0].message.content

        except asyncio.TimeoutError:
            self.last_error = f"LLM call timed out ({self.model_name})"
            logger.warning("[%s] LLM call timed out", self.name)
        except Exception as e:
            self.last_error = f"LLM call failed: {e}"
            logger.warning("[%s] LLM call failed: %s", self.name, e)

        return None

    def _get_positions_by_key(self) -> dict[str, int]:
        """Get net positions keyed by market_key."""
        positions = {}
        for market_id, key in self._market_id_to_key.items():
            yes = self.get_position(market_id, "YES")
            no = self.get_position(market_id, "NO")
            positions[key] = yes - no
        return positions

    async def _update_probabilities(self, market_prices: dict[str, float]) -> None:
        """Call LLM and update cached probabilities."""
        balance = self.balance_history[-1] if self.balance_history else None
        prompt = _build_prompt(
            self._event_news, self._event_info, market_prices,
            positions=self._get_positions_by_key(),
            balance=balance,
        )
        expected_keys = [info["market_key"] for info in self._event_info.values()]

        response_text = await self._call_llm(prompt)
        if response_text is None:
            return

        # Extract reasoning (text after the JSON block)
        self.last_reasoning = _extract_reasoning(response_text)

        parsed = _parse_llm_response(response_text, expected_keys)
        if parsed:
            self._cached_probs.update(parsed)
            # Sync to beliefs so the display can read them
            for key, prob in parsed.items():
                market_id = self._key_to_market_id.get(key)
                if market_id is not None:
                    self.update_belief(market_id, prob)
            logger.info("[%s] LLM probs updated: %s", self.name, parsed)
        else:
            logger.warning("[%s] Failed to parse LLM response: %s", self.name, response_text)

    def _compute_order_size(self, edge: float, current_pos: int) -> int:
        """Scale order size with edge magnitude, reduce near position limit."""
        abs_edge = abs(edge)

        # Scale with edge: 3% edge -> base/3, 10% -> base, 30%+ -> base*2
        edge_mult = min(2.0, abs_edge / 0.10)
        size = max(1, int(self.base_order_size * edge_mult))

        # Reduce as position grows toward limit
        remaining = self.max_position - current_pos
        if remaining <= 0:
            return 0
        if remaining < size:
            size = remaining

        return size

    async def on_block(self, block: Block) -> list[OrderSpec]:
        """Trade based on LLM probability estimates vs market prices."""
        self._blocks_since_last_call += 1

        # Collect current market prices as market_key -> probability
        market_prices: dict[str, float] = {}
        for market_id, (yes_nanos, _) in self.filter_markets(block).items():
            key = self._market_id_to_key.get(market_id)
            if key:
                market_prices[key] = yes_nanos / NANOS_PER_DOLLAR

        # Call LLM when we have new news and enough time has passed.
        # Call sooner if lots of news accumulated (urgency).
        min_wait = max(1, self.min_blocks_between_calls - self._pending_news_count)
        should_call = (
            self._pending_news_count > 0
            and self._blocks_since_last_call >= min_wait
            and (self._llm_task is None or self._llm_task.done())
        )
        if should_call:
            self._pending_news_count = 0
            self._blocks_since_last_call = 0
            self._llm_task = asyncio.create_task(self._update_probabilities(market_prices))

        # Check if pending LLM task completed
        if self._llm_task and self._llm_task.done():
            try:
                self._llm_task.result()
            except Exception:
                pass
            self._llm_task = None

        # Trade based on cached probabilities
        orders = []
        for market_id, (yes_nanos, _) in self.filter_markets(block).items():
            key = self._market_id_to_key.get(market_id)
            if key is None or key not in self._cached_probs:
                continue

            market_prob = yes_nanos / NANOS_PER_DOLLAR
            llm_prob = self._cached_probs[key]
            edge = llm_prob - market_prob

            yes_pos = self.get_position(market_id, "YES")
            no_pos = self.get_position(market_id, "NO")

            if edge > self.edge_threshold:
                size = self._compute_order_size(edge, yes_pos)
                if size > 0:
                    bid_price = min(0.95, market_prob + edge * 0.5)
                    orders.append(BuyYes.at_price(market_id, bid_price, size))
            elif edge < -self.edge_threshold:
                size = self._compute_order_size(edge, no_pos)
                if size > 0:
                    no_price = 1 - market_prob
                    no_edge = -edge
                    bid_price = min(0.95, no_price + no_edge * 0.5)
                    orders.append(BuyNo.at_price(market_id, bid_price, size))

        return orders
