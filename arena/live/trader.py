"""Live sizing trader — the LLM-free consumer half of the arena (SYB-210).

Architecture (post analysis/sizing split):
- A per-persona ``PersonaAnalyst`` (live/analyst.py) runs the analysis LLM and
  publishes ``FairValueUpdate``s onto a ``FairValueBus``.
- This trader is now a pure *sizer*: it drains fair-value updates, records the
  decision, and runs mechanical sizing/rebalance (``_rebalance_all``, position
  orders). Both the Kelly and Flat arms of one persona subscribe to the same
  persona stream, so their fair-value inputs are provably identical.

The class name ``LiveLlmTrader`` is retained (it is the DB ``trader_name`` and
the sybil-api reader / runner wiring depend on it); it no longer calls an LLM.
"""

import logging
import time
from dataclasses import dataclass
from datetime import datetime, timezone

from bots.base import BaseAgent
from sybil_client import Block, OrderSpec
from sybil_client.types import NANOS_PER_DOLLAR, Market

from .db import DecisionDB
from .fair_value_bus import FairValueBus, FairValueSubscription
from .news_feed import LiveArticle, NewsFeed
from .pricing import market_price, observed_market_prices
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
# LiveLlmTrader (sizer)
# --------------------------------------------------------------------------- #
class LiveLlmTrader(BaseAgent):
    """Mechanical sizer driven by a persona's FairValueBus.

    Consumes ``FairValueUpdate``s (no LLM), maintains target positions via the
    sizing strategy, and rebalances on the strategy's cadence.
    """

    def __init__(
        self,
        client,
        account_id: int,
        news_feed: NewsFeed | None,
        strategy: SizingStrategy | None = None,
        market_ids: list[int] | None = None,
        markets_info: dict[int, Market] | None = None,
        db: DecisionDB | None = None,
        name: str | None = None,
        fair_value_bus: FairValueBus | None = None,
    ):
        super().__init__(client, account_id, name or "LiveLlmTrader", market_ids)
        # The feed is kept only for its Polymarket price cache; the sizer no
        # longer subscribes to news (analysis moved to PersonaAnalyst, SYB-210).
        self.news_feed = news_feed
        self.strategy = strategy or KellyStrategy()
        self.markets_info = markets_info or {}
        self.db = db

        # SYB-210: both sizing arms of a persona subscribe to the SAME bus, so
        # they drain identical FairValueUpdate objects (provably equal A/B inputs).
        self.fv_sub: FairValueSubscription | None = (
            fair_value_bus.subscribe(name=name) if fair_value_bus is not None else None
        )

        self._last_rebalance: float = 0.0
        self._observed_first_block = False

        # Per-market state
        self.price_history: dict[int, list[PriceSnapshot]] = {}
        self.trade_log: dict[int, list[TradeRecord]] = {}
        self.fair_values: dict[int, float] = {}
        self._pending_order_logs: list[dict] = []

    def attach_news_feed(self, feed: NewsFeed) -> None:
        """Wire in the shared feed (price cache only; no news subscription)."""
        self.news_feed = feed

    def subscribe_fair_values(self, bus: FairValueBus) -> None:
        """Register this sizer's own view of its persona's fair-value bus.

        Used by the runner, which constructs sizers before the bus exists.
        """
        self.fv_sub = bus.subscribe(name=self.name)

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

    # -- Price helpers --

    def _get_market_price(self, market_id: int, block: Block) -> float:
        return market_price(self.news_feed, self.markets_info, market_id, block)

    def _observed_market_prices(self, block: Block) -> dict[int, tuple[int, int]]:
        """Prices worth recording for live trading, without synthetic 50/50s."""
        return observed_market_prices(
            self.news_feed, self.markets_info, self.market_ids, block
        )

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

    def _clear_resolved_markets(self, block: Block) -> None:
        """Drop fair values for resolved markets so the sizer exits them.

        Pre-split, the trader cleared a resolved market's FV inside its own LLM
        loop (only when a fresh article arrived). Post-split the analyst clears
        its copy and stops publishing, so the sizer must notice resolution on
        its own. This is LLM-free and runs every block, which is strictly more
        robust; sizing math (_rebalance_all) is unchanged.
        """
        for market_id in list(self.fair_values.keys()):
            ref_price = self._get_market_price(market_id, block)
            if ref_price > 0 and (ref_price >= RESOLVED_HIGH or ref_price <= RESOLVED_LOW):
                market = self.markets_info.get(market_id)
                name = market.name[:30] if market else str(market_id)
                log.info("[%s] %s resolved (price=%.2f), clearing FV",
                         self.name, name, ref_price)
                del self.fair_values[market_id]

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

        # Drain fair-value updates published by this persona's analyst. Each
        # update is recorded per-sizer (matching the pre-split per-trader
        # decision row) so sybil-api's per-trader_name reader is unchanged.
        for market_id in list(self.market_ids or []):
            updates = await self.fv_sub.drain(market_id) if self.fv_sub else []
            if not updates:
                continue

            market = self.markets_info.get(market_id)
            market_name = market.name if market else str(market_id)
            ref_price = self._get_market_price(market_id, block)

            for update in updates:
                old_fv = self.fair_values.get(market_id)
                self.fair_values[market_id] = update.fair_value
                log.info("[%s] %s: FV %.2f->%.2f (market=%.2f) | %s",
                         self.name, market_name[:30], old_fv or 0,
                         update.fair_value, ref_price, update.motivation)
                self._record_trade(
                    market_id=market_id,
                    market_name=market_name,
                    fair_value=update.fair_value,
                    orders=[],
                    motivation=update.motivation,
                    analysis=update.analysis,
                    raw_llm_response="",
                    llm_duration_s=0.0,
                    market_price=ref_price,
                    block_height=update.block_height,
                    timestamp=now,
                    articles=update.articles,
                )

        # Exit any markets that have since resolved (LLM-free).
        self._clear_resolved_markets(block)

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
