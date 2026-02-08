"""LLM-powered sports analyst trading bot for backtesting."""

import asyncio
import json
import logging
import re

from sybil_client import Block, BuyNo, BuyYes, OrderSpec

from backtest.agent import BacktestAgent
from backtest.dataset import NewsItem

logger = logging.getLogger(__name__)

NANOS_PER_DOLLAR = 1_000_000_000

DEFAULT_SYSTEM_PROMPT = (
    "You are an NBA analyst estimating real-time win probabilities.\n"
    "Given live game updates, estimate the probability that the HOME team wins.\n"
    "Consider: current score, quarter, momentum, injuries, matchups.\n"
    "Respond with ONLY a JSON object mapping market keys to probabilities.\n"
    "Example: {\"market_0\": 0.65, \"market_1\": 0.30}\n"
    "Use the full probability range. A 20-point lead in Q4 should be ~0.95."
)

CONTRARIAN_SYSTEM_PROMPT = (
    "You are a contrarian NBA analyst estimating real-time win probabilities.\n"
    "You tend to believe the market overreacts to recent news. A star injury doesn't\n"
    "doom a team - others step up. A big lead can evaporate. Recent momentum is noise.\n"
    "Given live game updates, estimate the probability that the HOME team wins.\n"
    "Respond with ONLY a JSON object mapping market keys to probabilities.\n"
    "Example: {\"market_0\": 0.65, \"market_1\": 0.30}\n"
    "Use the full probability range but lean against the crowd."
)


def _build_prompt(
    event_news: dict[str, list[str]],
    event_info: dict[str, dict],
    market_prices: dict[str, float],
) -> str:
    """Build the user prompt with accumulated news for all events.

    Args:
        event_news: event_id -> list of formatted news lines (most recent first)
        event_info: event_id -> {"home_team": ..., "away_team": ..., "market_key": ...}
        market_prices: market_key -> current market probability
    """
    lines = ["=== NBA Games in Progress ===\n"]

    for event_id, info in sorted(event_info.items(), key=lambda x: x[1]["market_key"]):
        market_key = info["market_key"]
        home = info["home_team"]
        away = info["away_team"]
        price = market_prices.get(market_key, 0.5)

        lines.append(f"[{market_key}] {home} (HOME) vs {away}")

        news_lines = event_news.get(event_id, [])
        if news_lines:
            lines.append("  Live updates (most recent first):")
            for nl in news_lines:
                lines.append(f"  - {nl}")
        else:
            lines.append("  No updates yet.")

        lines.append(f"  Current market price: {price * 100:.1f}% HOME wins")
        lines.append("")

    return "\n".join(lines)


def _parse_llm_response(text: str, expected_keys: list[str]) -> dict[str, float] | None:
    """Parse LLM response JSON, handling markdown fences and edge cases.

    Returns None on parse failure.
    """
    # Strip markdown code fences if present
    cleaned = text.strip()
    cleaned = re.sub(r"^```(?:json)?\s*", "", cleaned)
    cleaned = re.sub(r"\s*```$", "", cleaned)
    cleaned = cleaned.strip()

    try:
        data = json.loads(cleaned)
    except json.JSONDecodeError:
        return None

    if not isinstance(data, dict):
        return None

    result = {}
    for key in expected_keys:
        val = data.get(key)
        if val is not None:
            try:
                prob = float(val)
                result[key] = max(0.01, min(0.99, prob))
            except (ValueError, TypeError):
                continue

    return result if result else None


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
        edge_threshold: float = 0.04,
        order_size: int = 8,
        max_position: int = 50,
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
        self.order_size = order_size
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
        self._needs_llm_update = False
        # LLM client (lazy init)
        self._llm_client = None
        # Pending LLM task
        self._llm_task: asyncio.Task | None = None

        # Build reverse map: market_id -> market_key
        self._market_id_to_key: dict[int, str] = {}
        if event_market_map:
            for idx, (event_id, market_id) in enumerate(
                sorted(event_market_map.items(), key=lambda x: x[1])
            ):
                key = f"market_{idx}"
                self._market_id_to_key[market_id] = key
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
        else:
            raise ValueError(f"Unknown provider: {self.provider}")

        return self._llm_client

    def _format_news_line(self, news: NewsItem) -> str:
        """Format a news item as a concise line for the LLM prompt."""
        meta = news.metadata
        if news.source == "in_game":
            quarter = meta.get("quarter", "?")
            home_score = meta.get("home_score", "?")
            away_score = meta.get("away_score", "?")
            if meta.get("final"):
                return f"[FINAL] {home_score} - {away_score}"
            return f"[Q{quarter} END] {home_score} - {away_score}"
        elif news.source == "injury":
            player = meta.get("player", "Unknown")
            status = meta.get("status", meta.get("severity", "unknown"))
            return f"[INJURY] {player} {status}"
        elif news.source == "lineup":
            return f"[LINEUP] {news.content[:80]}"
        else:
            return f"[{news.source.upper()}] {news.headline[:80]}"

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

        self._needs_llm_update = True

    async def _call_llm(self, prompt: str) -> str | None:
        """Call the LLM API with timeout. Returns response text or None."""
        try:
            llm_client = self._get_llm_client()

            if self.provider == "anthropic":
                response = await asyncio.wait_for(
                    llm_client.messages.create(
                        model=self.model_name,
                        max_tokens=200,
                        system=self.system_prompt,
                        messages=[{"role": "user", "content": prompt}],
                    ),
                    timeout=10.0,
                )
                return response.content[0].text

            elif self.provider == "openai":
                response = await asyncio.wait_for(
                    llm_client.chat.completions.create(
                        model=self.model_name,
                        max_tokens=200,
                        messages=[
                            {"role": "system", "content": self.system_prompt},
                            {"role": "user", "content": prompt},
                        ],
                    ),
                    timeout=10.0,
                )
                return response.choices[0].message.content

        except asyncio.TimeoutError:
            logger.warning("[%s] LLM call timed out", self.name)
        except Exception as e:
            logger.warning("[%s] LLM call failed: %s", self.name, e)

        return None

    async def _update_probabilities(self, market_prices: dict[str, float]) -> None:
        """Call LLM and update cached probabilities."""
        prompt = _build_prompt(self._event_news, self._event_info, market_prices)
        expected_keys = [info["market_key"] for info in self._event_info.values()]

        response_text = await self._call_llm(prompt)
        if response_text is None:
            return

        parsed = _parse_llm_response(response_text, expected_keys)
        if parsed:
            self._cached_probs.update(parsed)
            logger.info("[%s] LLM probs updated: %s", self.name, parsed)
        else:
            logger.warning("[%s] Failed to parse LLM response: %s", self.name, response_text)

    async def on_block(self, block: Block) -> list[OrderSpec]:
        """Trade based on LLM probability estimates vs market prices."""
        self._blocks_since_last_call += 1

        # Collect current market prices as market_key -> probability
        market_prices: dict[str, float] = {}
        for market_id, (yes_nanos, _) in self.filter_markets(block).items():
            key = self._market_id_to_key.get(market_id)
            if key:
                market_prices[key] = yes_nanos / NANOS_PER_DOLLAR

        # Maybe call LLM (rate-limited, non-blocking)
        if (
            self._needs_llm_update
            and self._blocks_since_last_call >= self.min_blocks_between_calls
            and (self._llm_task is None or self._llm_task.done())
        ):
            self._needs_llm_update = False
            self._blocks_since_last_call = 0
            # Fire and forget - result updates _cached_probs
            self._llm_task = asyncio.create_task(self._update_probabilities(market_prices))

        # Check if pending LLM task completed
        if self._llm_task and self._llm_task.done():
            # Retrieve any exception to avoid "Task exception was never retrieved"
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
                if yes_pos < self.max_position:
                    bid_price = min(0.95, market_prob + 0.02)
                    orders.append(BuyYes.at_price(market_id, bid_price, self.order_size))
            elif edge < -self.edge_threshold:
                if no_pos < self.max_position:
                    no_price = 1 - market_prob
                    bid_price = min(0.95, no_price + 0.02)
                    orders.append(BuyNo.at_price(market_id, bid_price, self.order_size))

        return orders
