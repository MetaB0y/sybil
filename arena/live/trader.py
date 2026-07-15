"""Live sizing trader — the LLM-free consumer half of the arena (SYB-210).

Architecture (post analysis/sizing split):
- A per-persona ``PersonaAnalyst`` (live/analyst.py) runs the analysis LLM and
  publishes ``FairValueUpdate``s onto a ``FairValueBus``.
- This trader is now a pure *sizer*: it drains fair-value updates, records the
  first sizing application of each update, and runs mechanical position
  management between updates. Both the Kelly and Flat arms of one persona
  subscribe to the same persona stream, so their fair-value inputs are
  provably identical.

The class name ``LiveLlmTrader`` is retained (it is the DB ``trader_name`` and
the sybil-api reader / runner wiring depend on it); it no longer calls an LLM.
"""

import logging
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Callable

from bots.base import BaseAgent
from sybil_client import Block, OrderSpec
from sybil_client.types import NANOS_PER_DOLLAR, Market

from .db import DecisionDB
from .fair_value_bus import FairValueBus, FairValueSubscription, FairValueUpdate
from .news_feed import LiveArticle, NewsFeed
from .pricing import market_price, observed_market_prices
from .strategy import (
    RESOLVED_HIGH,
    RESOLVED_LOW,
    FairValueFreshnessConfig,
    FreshFairValue,
    KellyStrategy,
    SizingStrategy,
    effective_fair_value,
    position_orders,
)

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
    analysis_batch_id: str
    analysis_reference_price: float | None
    articles: list[LiveArticle]
    analysis: str
    fair_value: float
    raw_fair_value: float | None
    effective_fair_value: float | None
    fair_value_age_s: float | None
    confidence: float | None
    restate: str
    countercase: str
    rejection_reason: str | None
    orders: list[OrderSpec]
    motivation: str
    raw_llm_response: str
    llm_duration_s: float
    block_height: int
    timestamp: datetime
    balance: float
    yes_pos: int
    no_pos: int


@dataclass
class FairValueDecisionContext:
    raw_fair_value: float | None
    effective_fair_value: float | None
    age_s: float | None
    freshness_factor: float
    confidence: float | None
    restate: str
    countercase: str


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
        fair_value_ttl_s: float = FairValueFreshnessConfig.ttl_s,
        fair_value_half_life_s: float = FairValueFreshnessConfig.half_life_s,
        fair_value_hard_expiry_s: float = FairValueFreshnessConfig.hard_expiry_s,
        now_fn: Callable[[], datetime] | None = None,
        monotonic_fn: Callable[[], float] | None = None,
    ):
        super().__init__(client, account_id, name or "LiveLlmTrader", market_ids)
        # The feed is kept only for its Polymarket price cache; the sizer no
        # longer subscribes to news (analysis moved to PersonaAnalyst, SYB-210).
        self.news_feed = news_feed
        self.strategy = strategy or KellyStrategy()
        self.markets_info = markets_info or {}
        self.db = db
        self.fv_freshness_config = FairValueFreshnessConfig(
            ttl_s=fair_value_ttl_s,
            half_life_s=fair_value_half_life_s,
            hard_expiry_s=fair_value_hard_expiry_s,
        )
        self._now = now_fn or (lambda: datetime.now(timezone.utc))
        self._monotonic = monotonic_fn or time.monotonic

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
        self.fair_value_timestamps: dict[int, datetime] = {}
        self.fair_value_confidences: dict[int, float | None] = {}
        self.fair_value_restates: dict[int, str] = {}
        self.fair_value_countercases: dict[int, str] = {}
        self._latest_rebalance_context: dict[int, FairValueDecisionContext] = {}
        self._latest_rejection_reasons: dict[int, str | None] = {}
        self._latest_updates: dict[int, FairValueUpdate] = {}

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
        raw_fair_value: float | None = None,
        effective_fair_value: float | None = None,
        fair_value_age_s: float | None = None,
        confidence: float | None = None,
        restate: str = "",
        countercase: str = "",
        rejection_reason: str | None = None,
        analysis_batch_id: str = "",
        analysis_reference_price: float | None = None,
    ) -> None:
        if orders and rejection_reason is not None:
            raise ValueError("submitted decisions cannot have a rejection_reason")
        if not orders and not rejection_reason:
            raise ValueError("no-order decisions require a rejection_reason")
        yes_pos = self.get_position(market_id, "YES")
        no_pos = self.get_position(market_id, "NO")
        record = TradeRecord(
            market_id=market_id,
            analysis_batch_id=analysis_batch_id,
            analysis_reference_price=analysis_reference_price,
            articles=articles or [],
            analysis=analysis,
            fair_value=fair_value,
            raw_fair_value=raw_fair_value,
            effective_fair_value=effective_fair_value,
            fair_value_age_s=fair_value_age_s,
            confidence=confidence,
            restate=restate,
            countercase=countercase,
            rejection_reason=rejection_reason,
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
            market = self.markets_info.get(market_id)
            market_category = getattr(market, "category", "")
            if not isinstance(market_category, str):
                market_category = ""
            market_tags = getattr(market, "tags", [])
            if not isinstance(market_tags, (list, tuple, set)):
                market_tags = []
            article_urls = [
                {"title": a.title, "url": a.url, "source": a.source} for a in (articles or [])
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
                raw_fair_value=raw_fair_value,
                effective_fair_value=effective_fair_value,
                fair_value_age_s=fair_value_age_s,
                confidence=confidence,
                restate=restate,
                countercase=countercase,
                rejection_reason=rejection_reason,
                market_category=market_category,
                market_tags=[str(tag) for tag in market_tags],
                analysis_batch_id=analysis_batch_id,
                analysis_reference_price=analysis_reference_price,
            )

    # -- Price helpers --

    def _get_market_price(self, market_id: int, block: Block) -> float:
        return market_price(self.news_feed, market_id, block)

    def _observed_market_prices(self, block: Block) -> dict[int, tuple[int, int]]:
        """Prices worth recording for live trading, without synthetic 50/50s."""
        return observed_market_prices(self.news_feed, self.market_ids, block)

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

    # -- Fair-value freshness --

    def _store_fair_value_update(self, update: FairValueUpdate, observed_at: datetime) -> None:
        self.fair_values[update.market_id] = update.fair_value
        self.fair_value_timestamps[update.market_id] = update.ts or observed_at
        self.fair_value_confidences[update.market_id] = update.confidence
        self.fair_value_restates[update.market_id] = update.restate
        self.fair_value_countercases[update.market_id] = update.countercase

    def _forget_fair_value(self, market_id: int) -> None:
        self.fair_values.pop(market_id, None)
        self.fair_value_timestamps.pop(market_id, None)
        self.fair_value_confidences.pop(market_id, None)
        self.fair_value_restates.pop(market_id, None)
        self.fair_value_countercases.pop(market_id, None)
        self._latest_updates.pop(market_id, None)

    def _fresh_fair_value(
        self,
        market_id: int,
        market_price: float,
        observed_at: datetime,
    ) -> FreshFairValue | None:
        raw_fv = self.fair_values.get(market_id)
        if raw_fv is None:
            return None
        ts = self.fair_value_timestamps.get(market_id, observed_at)
        if ts.tzinfo is None:
            ts = ts.replace(tzinfo=timezone.utc)
        age_s = max(0.0, (observed_at - ts).total_seconds())
        return effective_fair_value(
            raw_fv,
            market_price,
            age_s,
            self.fv_freshness_config,
        )

    def _decision_context(
        self,
        market_id: int,
        fresh_fv: FreshFairValue | None,
    ) -> FairValueDecisionContext:
        return FairValueDecisionContext(
            raw_fair_value=fresh_fv.raw_fair_value if fresh_fv else None,
            effective_fair_value=fresh_fv.effective_fair_value if fresh_fv else None,
            age_s=fresh_fv.age_s if fresh_fv else None,
            freshness_factor=fresh_fv.freshness_factor if fresh_fv else 0.0,
            confidence=self.fair_value_confidences.get(market_id),
            restate=self.fair_value_restates.get(market_id, ""),
            countercase=self.fair_value_countercases.get(market_id, ""),
        )

    # -- Position management --

    def _rebalance_all(self, block: Block, now: datetime) -> list[OrderSpec]:
        """Rebalance all positions using the sizing strategy."""
        pv = self._portfolio_value(block)
        cash = self.current_balance
        min_cash = MIN_CASH_FRAC * pv

        all_orders: list[OrderSpec] = []
        self._latest_rebalance_context = {}
        self._latest_rejection_reasons = {}

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
            fresh_fv = self._fresh_fair_value(market_id, market_price, now)
            context = self._decision_context(market_id, fresh_fv)
            self._latest_rebalance_context[market_id] = context
            fv = fresh_fv.effective_fair_value if fresh_fv else None

            market = self.markets_info.get(market_id)
            is_resolved = (
                str(getattr(market, "status", "")).lower() == "resolved"
                or market_price >= RESOLVED_HIGH
                or market_price <= RESOLVED_LOW
            )

            if fv is None or is_resolved:
                # No fair value or drawdown → exit positions
                from sybil_client import SellNo, SellYes

                market_orders: list[OrderSpec] = []
                if current_yes > 0:
                    market_orders.append(SellYes.at_price(market_id, market_price, current_yes))
                if current_no > 0:
                    market_orders.append(SellNo.at_price(market_id, 1 - market_price, current_no))
                all_orders.extend(market_orders)
                self._latest_rejection_reasons[market_id] = (
                    None if market_orders else "resolved" if is_resolved else "fv_expired"
                )
                if not market_orders:
                    self._forget_fair_value(market_id)
                continue

            target_yes, target_no = self.strategy.target(
                fv,
                market_price,
                pv,
                current_yes,
                current_no,
                market_id=market_id,
                confidence=context.confidence,
                freshness_factor=context.freshness_factor,
            )

            available_cash = max(0, cash - min_cash)
            orders = position_orders(
                market_id,
                target_yes,
                target_no,
                current_yes,
                current_no,
                fv,
                market_price,
                available_cash,
            )

            rejection_reason = None
            if not orders:
                min_edge = float(getattr(self.strategy, "min_edge", 0.0))
                if abs(fv - market_price) < min_edge:
                    rejection_reason = "below_min_edge"
                elif (target_yes, target_no) != (current_yes, current_no):
                    rejection_reason = "insufficient_cash"
                else:
                    rejection_reason = "hold_position"
            self._latest_rejection_reasons[market_id] = rejection_reason

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
        fresh_updates: dict[int, FairValueUpdate] = {}
        prices = self._observed_market_prices(block)
        now = self._now()

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
                self._store_fair_value_update(update, now)
                self._latest_updates[market_id] = update
                fresh_updates[market_id] = update
                update_ts = update.ts or now
                if update_ts.tzinfo is None:
                    update_ts = update_ts.replace(tzinfo=timezone.utc)
                update_age_s = max(0.0, (now - update_ts).total_seconds())
                log.info(
                    "[%s] %s: FV %.2f->%.2f (market=%.2f, age=%.0fs, conf=%s) | %s",
                    self.name,
                    market_name[:30],
                    old_fv or 0,
                    update.fair_value,
                    ref_price,
                    update_age_s,
                    f"{update.confidence:.2f}" if update.confidence is not None else "n/a",
                    update.motivation,
                )
        # Position management via strategy
        elapsed_rebal = self._monotonic() - self._last_rebalance
        if (
            fresh_updates
            or elapsed_rebal >= self.strategy.rebalance_interval_s
            or self._last_rebalance == 0
        ):
            rebalance_orders = self._rebalance_all(block, now)
            if rebalance_orders:
                order_desc = ", ".join(_describe_order(o) for o in rebalance_orders)
                log.info("[%s] %s rebalance: %s", self.name, self.strategy.name, order_desc)
            orders_by_market: dict[int, list[OrderSpec]] = {}
            for order in rebalance_orders:
                orders_by_market.setdefault(order.market_id, []).append(order)
            for market_id, context in self._latest_rebalance_context.items():
                # A durable decision is one application of fresh analyst
                # evidence. Timer-only position management is operational
                # behavior, not another forecast observation.
                update = fresh_updates.get(market_id)
                if update is None:
                    continue
                market_orders = orders_by_market.get(market_id, [])
                market = self.markets_info.get(market_id)
                market_name = market.name if market else str(market_id)
                current_price = self._get_market_price(market_id, block)
                effective_fv = context.effective_fair_value
                raw_fv = context.raw_fair_value
                fair_value = effective_fv if effective_fv is not None else raw_fv
                fair_value = fair_value if fair_value is not None else current_price
                if market_orders:
                    motivation = f"{self.strategy.name} rebalance to target position"
                else:
                    motivation = (
                        f"{self.strategy.name} rejected: "
                        f"{self._latest_rejection_reasons[market_id]}"
                    )
                entry = {
                    "market_id": market_id,
                    "market_name": market_name,
                    "fair_value": fair_value,
                    "raw_fair_value": raw_fv,
                    "effective_fair_value": effective_fv,
                    "fair_value_age_s": context.age_s if context else None,
                    "confidence": context.confidence if context else None,
                    "restate": context.restate if context else "",
                    "countercase": context.countercase if context else "",
                    "orders": market_orders,
                    "motivation": motivation,
                    "analysis": update.analysis,
                    "articles": update.articles,
                    "market_price": current_price,
                    "block_height": block.height,
                    "timestamp": now,
                    "rejection_reason": self._latest_rejection_reasons[market_id],
                    "analysis_batch_id": update.analysis_batch_id,
                    "analysis_reference_price": update.analysis_reference_price,
                }
                self._record_trade(
                    market_id=entry["market_id"],
                    market_name=entry["market_name"],
                    fair_value=entry["fair_value"],
                    orders=entry["orders"],
                    motivation=entry["motivation"],
                    analysis=entry["analysis"],
                    raw_llm_response="",
                    llm_duration_s=0.0,
                    market_price=entry["market_price"],
                    block_height=entry["block_height"],
                    timestamp=entry["timestamp"],
                    articles=entry["articles"],
                    raw_fair_value=entry["raw_fair_value"],
                    effective_fair_value=entry["effective_fair_value"],
                    fair_value_age_s=entry["fair_value_age_s"],
                    confidence=entry["confidence"],
                    restate=entry["restate"],
                    countercase=entry["countercase"],
                    rejection_reason=entry["rejection_reason"],
                    analysis_batch_id=entry["analysis_batch_id"],
                    analysis_reference_price=entry["analysis_reference_price"],
                )
            all_orders.extend(rebalance_orders)
            self._last_rebalance = self._monotonic()

        return all_orders
