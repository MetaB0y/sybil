"""Sparse, deterministic target-block noise liquidity coordinator.

Fifteen durable role-bound accounts independently sample markets. Aggregate
coverage targets roughly 25% per block. Prices are randomized around the prior
committed Sybil mark, so fills emerge without seeing another actor's upcoming
orders. Every order is carried by an actor epoch and exists for one FBA block.
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import time
from dataclasses import dataclass
from hashlib import sha256
from pathlib import Path
from typing import Any

from sybil_client import SybilClient
from sybil_client.types import NANOS_PER_DOLLAR, Block, Market, shares_to_quantity_units

log = logging.getLogger(__name__)

NOISE_ACTOR_COUNT = 15
TARGET_AGGREGATE_COVERAGE = 0.25
BASE_SELECTION_PROBABILITY = 1.0 - (1.0 - TARGET_AGGREGATE_COVERAGE) ** (1.0 / NOISE_ACTOR_COUNT)
MAX_ORDERS_PER_ACTOR_EPOCH = 32
ANTI_STARVATION_START_BLOCKS = 8
ANTI_STARVATION_PROBABILITY_CAP = 0.06
AGGRESSIVE_ORDER_PROBABILITY = 0.60
LITE_PEAK_DEVIATION = 0.04
LITE_DEVIATION_EXPONENT = 1.3
MIN_ORDER_DOLLARS = 7.0
MAX_ORDER_DOLLARS = 150.0
INVENTORY_BIAS_DOLLARS = 800.0


@dataclass(frozen=True)
class NoiseActorCredential:
    principal_id: str
    account_id: int
    token: str


@dataclass(frozen=True)
class NoiseActorView:
    """Minimal shape consumed by the portfolio snapshot loop."""

    client: SybilClient
    account_id: int
    name: str
    participant_kind: str = "noise"


@dataclass(frozen=True)
class NoisePersonality:
    activity: float
    size: float
    direction_bias: float
    price_distance: float


def _load_credentials(raw: str) -> tuple[NoiseActorCredential, ...]:
    payload = raw.strip()
    if not payload:
        return ()
    if not payload.startswith("[") and not payload.startswith("{"):
        payload = Path(payload).read_text()
    decoded = json.loads(payload)
    rows = decoded.get("actors", []) if isinstance(decoded, dict) else decoded
    actors = tuple(
        NoiseActorCredential(
            principal_id=str(row["principal_id"]),
            account_id=int(row["account_id"]),
            token=str(row["token"]),
        )
        for row in rows
        if str(row.get("role", "noise")).lower() == "noise"
    )
    if len(actors) != NOISE_ACTOR_COUNT:
        raise ValueError("noise actor config must contain exactly fifteen noise credentials")
    if (
        len({a.principal_id for a in actors}) != NOISE_ACTOR_COUNT
        or len({a.account_id for a in actors}) != NOISE_ACTOR_COUNT
    ):
        raise ValueError("noise actor principals and accounts must be unique")
    if any(not a.principal_id or len(a.token) < 32 or a.account_id <= 0 for a in actors):
        raise ValueError("noise actor credentials contain an invalid principal, account, or token")
    return actors


def _draw(
    seed: str,
    generation: int,
    height: int,
    actor_id: str,
    market_id: int,
    lane: str,
) -> int:
    material = f"{seed}|{generation}|{height}|{actor_id}|{market_id}|{lane}".encode()
    return int.from_bytes(sha256(material).digest()[:8], "big")


def _unit(draw: int) -> float:
    return draw / float(2**64 - 1)


def _clamp(value: float, lower: float, upper: float) -> float:
    return min(max(value, lower), upper)


class NoiseCoordinator:
    def __init__(
        self,
        client: SybilClient,
        actors: tuple[NoiseActorCredential, ...],
        *,
        seed: str = "sybil-noise-v2.1",
        min_order_dollars: float = MIN_ORDER_DOLLARS,
        max_order_dollars: float = MAX_ORDER_DOLLARS,
        inventory_bias_dollars: float = INVENTORY_BIAS_DOLLARS,
        state_path: str | Path | None = None,
    ) -> None:
        if len(actors) != NOISE_ACTOR_COUNT:
            raise ValueError("NoiseCoordinator requires exactly fifteen actors")
        if min_order_dollars <= 0 or max_order_dollars < min_order_dollars:
            raise ValueError("invalid noise order range")
        if inventory_bias_dollars <= 0:
            raise ValueError("invalid noise inventory bias scale")
        self.client = client
        self.actors = actors
        self.seed = seed
        self.min_order_dollars = min_order_dollars
        self.max_order_dollars = max_order_dollars
        self.inventory_bias_dollars = inventory_bias_dollars
        self.state_path = Path(state_path) if state_path else None
        self.snapshot_agents = [
            NoiseActorView(client, actor.account_id, f"Noise-{index + 1}")
            for index, actor in enumerate(actors)
        ]
        raw_personalities = [self._raw_personality(actor) for actor in actors]
        mean_activity = sum(p.activity for p in raw_personalities) / len(raw_personalities)
        self.personalities = {
            actor.principal_id: NoisePersonality(
                activity=personality.activity / mean_activity,
                size=personality.size,
                direction_bias=personality.direction_bias,
                price_distance=personality.price_distance,
            )
            for actor, personality in zip(actors, raw_personalities, strict=True)
        }
        self._last_height = -1
        self._state_key = ""
        self._generation_start_height = 0
        self._last_accepted_height: dict[int, int] = {}
        self._metadata_key: tuple[str, tuple[int, ...]] | None = None
        self._market_by_id: dict[int, Market] = {}
        self._group_members: dict[int, tuple[int, ...]] = {}
        self._load_state()

    def _raw_personality(self, actor: NoiseActorCredential) -> NoisePersonality:
        def stable(lane: str) -> float:
            return _unit(_draw(self.seed, 0, 0, actor.principal_id, actor.account_id, lane))

        return NoisePersonality(
            activity=0.80 + 0.40 * stable("personality-activity"),
            size=0.65 + 0.70 * stable("personality-size"),
            direction_bias=-0.08 + 0.16 * stable("personality-direction"),
            price_distance=stable("personality-price-distance"),
        )

    @classmethod
    def from_env(cls, client: SybilClient) -> NoiseCoordinator | None:
        raw = os.environ.get("SYBIL_NOISE_ACTORS_JSON", "").strip()
        if not raw:
            return None
        state_path = os.environ.get("ARENA_NOISE_STATE_PATH", "").strip() or None
        return cls(
            client,
            _load_credentials(raw),
            seed=os.environ.get("ARENA_NOISE_SEED", "sybil-noise-v2.1"),
            min_order_dollars=float(
                os.environ.get("ARENA_NOISE_MIN_ORDER_DOLLARS", str(MIN_ORDER_DOLLARS))
            ),
            max_order_dollars=float(
                os.environ.get("ARENA_NOISE_MAX_ORDER_DOLLARS", str(MAX_ORDER_DOLLARS))
            ),
            inventory_bias_dollars=float(
                os.environ.get("ARENA_NOISE_INVENTORY_BIAS_DOLLARS", str(INVENTORY_BIAS_DOLLARS))
            ),
            state_path=state_path,
        )

    def _load_state(self) -> None:
        if self.state_path is None or not self.state_path.exists():
            return
        try:
            payload = json.loads(self.state_path.read_text())
            self._state_key = str(payload.get("state_key", ""))
            self._generation_start_height = int(payload.get("generation_start_height", 0))
            self._last_accepted_height = {
                int(market_id): int(height)
                for market_id, height in payload.get("last_accepted_height", {}).items()
            }
        except Exception:
            log.warning("noise anti-starvation state is unreadable; starting from base randomness")
            self._state_key = ""
            self._generation_start_height = 0
            self._last_accepted_height = {}

    def _save_state(self) -> None:
        if self.state_path is None:
            return
        try:
            self.state_path.parent.mkdir(parents=True, exist_ok=True)
            temporary = self.state_path.with_suffix(self.state_path.suffix + ".tmp")
            temporary.write_text(
                json.dumps(
                    {
                        "state_key": self._state_key,
                        "generation_start_height": self._generation_start_height,
                        "last_accepted_height": self._last_accepted_height,
                    },
                    sort_keys=True,
                )
            )
            temporary.replace(self.state_path)
        except Exception:
            log.exception("failed to persist noise anti-starvation state")

    async def run(self, stop_event: asyncio.Event) -> None:
        async for block in self.client.stream_blocks():
            if stop_event.is_set():
                return
            if block.height <= self._last_height:
                continue
            self._last_height = block.height
            try:
                await self.submit_for_block(block)
            except asyncio.CancelledError:
                raise
            except Exception:
                log.exception("noise actor epoch generation failed at block %d", block.height)

    async def submit_for_block(self, block: Block) -> None:
        first, markets, portfolios = await asyncio.gather(
            self.client.actor_universe(self.actors[0].token),
            self.client.list_markets(),
            asyncio.gather(*(self.client.get_portfolio(actor.account_id) for actor in self.actors)),
        )
        if not first.get("actor_ready"):
            log.warning("noise epochs paused: liquidity universe is not committed")
            return
        generation = int(first["generation"])
        universe_ids = tuple(int(mid) for mid in first["market_ids"])
        state_key = f"{first.get('policy_digest_hex', '')}:{generation}"
        metadata_key = (state_key, universe_ids)
        universe_set = set(universe_ids)
        market_by_id = {market.id: market for market in markets if market.id in universe_set}
        if set(market_by_id) != universe_set:
            raise RuntimeError("market listing and committed actor universe diverged")
        # Marks are live serving data, so refresh this one bulk snapshot every
        # block even while group membership and credential bindings are cached.
        self._market_by_id = market_by_id
        if metadata_key != self._metadata_key:
            remaining_rows, groups = await asyncio.gather(
                asyncio.gather(
                    *(self.client.actor_universe(actor.token) for actor in self.actors[1:])
                ),
                self.client.list_market_groups(),
            )
            universe_rows = (first, *remaining_rows)
            for actor, row in zip(self.actors, universe_rows, strict=True):
                if (
                    row.get("actor_role") != "noise"
                    or int(row.get("account_id", -1)) != actor.account_id
                    or int(row["generation"]) != generation
                    or tuple(int(mid) for mid in row["market_ids"]) != universe_ids
                ):
                    raise RuntimeError(
                        f"noise credential binding/universe mismatch: {actor.principal_id}"
                    )

            group_members: dict[int, tuple[int, ...]] = {}
            for group in groups:
                members = tuple(
                    sorted(
                        int(mid) for mid in group.get("market_ids", []) if int(mid) in universe_set
                    )
                )
                if len(members) < 2:
                    continue
                for market_id in members:
                    group_members[market_id] = members
            self._metadata_key = metadata_key
            self._group_members = group_members
        elif (
            first.get("actor_role") != "noise"
            or int(first.get("account_id", -1)) != self.actors[0].account_id
        ):
            raise RuntimeError(f"noise credential binding mismatch: {self.actors[0].principal_id}")

        target_height = block.height + 1
        if state_key != self._state_key:
            if self._state_key:
                log.warning("noise universe generation changed; resetting anti-starvation ages")
            self._state_key = state_key
            self._generation_start_height = target_height
            self._last_accepted_height = {}

        selected_by_actor = self._select_markets(generation, target_height, universe_ids)
        payloads = [
            self._build_payload(
                actor,
                portfolios[actor_index],
                self._market_by_id,
                self._group_members,
                generation,
                target_height,
                selected_by_actor[actor_index],
            )
            for actor_index, actor in enumerate(self.actors)
        ]
        results = await asyncio.gather(
            *(
                self.client.submit_actor_epoch(actor.token, payload)
                for actor, payload in zip(self.actors, payloads, strict=True)
            ),
            return_exceptions=True,
        )
        retry_indices = [
            index for index, result in enumerate(results) if isinstance(result, Exception)
        ]
        if retry_indices:
            retried = await asyncio.gather(
                *(
                    self.client.submit_actor_epoch(self.actors[index].token, payloads[index])
                    for index in retry_indices
                ),
                return_exceptions=True,
            )
            for index, result in zip(retry_indices, retried, strict=True):
                results[index] = result
        accepted_markets: set[int] = set()
        failures = []
        for payload, result in zip(payloads, results, strict=True):
            if isinstance(result, Exception):
                failures.append(result)
                continue
            accepted_markets.update(
                int(intent["market_id"])
                for intent in payload["market_intents"]
                if intent.get("orders")
            )
        for market_id in accepted_markets:
            self._last_accepted_height[market_id] = target_height
        self._save_state()
        if failures:
            log.warning(
                "noise epoch target=%d accepted=%d failed=%d errors=%s",
                target_height,
                len(results) - len(failures),
                len(failures),
                "; ".join(str(error) for error in failures),
            )
        log.info(
            "submitted sparse noise epochs target=%d generation=%d actors=%d orders=%d markets=%d",
            target_height,
            generation,
            len(results) - len(failures),
            sum(
                len(intent["orders"])
                for payload in payloads
                for intent in payload["market_intents"]
            ),
            len(accepted_markets),
        )

    def _selection_probability(
        self,
        actor: NoiseActorCredential,
        market_id: int,
        target_height: int,
    ) -> float:
        personality = self.personalities[actor.principal_id]
        last_height = self._last_accepted_height.get(
            market_id, self._generation_start_height or target_height
        )
        drought = max(0, target_height - last_height)
        starvation_multiplier = 1.0
        if drought > ANTI_STARVATION_START_BLOCKS:
            starvation_multiplier += 0.08 * (drought - ANTI_STARVATION_START_BLOCKS)
        return min(
            ANTI_STARVATION_PROBABILITY_CAP,
            BASE_SELECTION_PROBABILITY * personality.activity * starvation_multiplier,
        )

    def _select_markets(
        self,
        generation: int,
        target_height: int,
        universe_ids: tuple[int, ...],
    ) -> list[tuple[int, ...]]:
        selected = []
        for actor in self.actors:
            markets = [
                market_id
                for market_id in universe_ids
                if _unit(
                    _draw(
                        self.seed,
                        generation,
                        target_height,
                        actor.principal_id,
                        market_id,
                        "selection",
                    )
                )
                < self._selection_probability(actor, market_id, target_height)
            ]
            if len(markets) > MAX_ORDERS_PER_ACTOR_EPOCH:
                markets.sort(
                    key=lambda market_id: _draw(
                        self.seed,
                        generation,
                        target_height,
                        actor.principal_id,
                        market_id,
                        "selection-cap",
                    )
                )
                markets = markets[:MAX_ORDERS_PER_ACTOR_EPOCH]
            selected.append(tuple(sorted(markets)))
        return selected

    def _build_payload(
        self,
        actor: NoiseActorCredential,
        portfolio: Any,
        markets: dict[int, Market],
        group_members: dict[int, tuple[int, ...]],
        generation: int,
        target_height: int,
        selected_market_ids: tuple[int, ...],
    ) -> dict[str, Any]:
        positions = {
            (position.market_id, position.outcome): (
                float(position.quantity),
                max(0.0, float(position.value_nanos) / NANOS_PER_DOLLAR),
            )
            for position in portfolio.positions
        }
        intents = []
        group_holes: dict[tuple[int, ...], int] = {}
        for market_id in selected_market_ids:
            members = group_members.get(market_id)
            if members:
                group_holes.setdefault(
                    members,
                    members[
                        _draw(
                            self.seed,
                            generation,
                            target_height,
                            actor.principal_id,
                            members[0],
                            "group-hole",
                        )
                        % len(members)
                    ],
                )
            order = self._order_for_market(
                actor,
                markets[market_id],
                generation,
                target_height,
                positions,
                members,
                group_holes.get(members) if members else None,
            )
            if order is not None:
                intents.append({"market_id": market_id, "orders": [order]})
            else:
                intents.append(
                    {
                        "market_id": market_id,
                        "orders": [],
                        "skip_reason": "inventory_or_range_unavailable",
                    }
                )
        now_ms = time.time_ns() // 1_000_000
        return {
            "epoch_id": f"noise-{actor.principal_id}-{generation}-{target_height}",
            "target_height": target_height,
            "universe_generation": generation,
            "observed_at_ms": now_ms,
            "valid_until_ms": now_ms + 25_000,
            "market_intents": intents,
        }

    def _action_candidates(
        self,
        actor: NoiseActorCredential,
        market: Market,
        generation: int,
        target_height: int,
        positions: dict[tuple[int, str], tuple[float, float]],
        members: tuple[int, ...] | None,
        group_hole: int | None,
    ) -> list[tuple[str, str]]:
        personality = self.personalities[actor.principal_id]
        yes_qty, yes_value = positions.get((market.id, "YES"), (0.0, 0.0))
        no_qty, no_value = positions.get((market.id, "NO"), (0.0, 0.0))
        inventory_value = yes_value + no_value
        sell_probability = _clamp(
            0.10 + 0.80 * (inventory_value / self.inventory_bias_dollars),
            0.10,
            0.90,
        )
        sell_draw = _unit(
            _draw(
                self.seed,
                generation,
                target_height,
                actor.principal_id,
                market.id,
                "sell-action",
            )
        )
        candidates: list[tuple[str, str]] = []
        if sell_draw < sell_probability and (yes_qty > 0 or no_qty > 0):
            if yes_value == no_value:
                sell_yes_first = bool(
                    _draw(
                        self.seed,
                        generation,
                        target_height,
                        actor.principal_id,
                        market.id,
                        "sell-direction",
                    )
                    % 2
                )
            else:
                sell_yes_first = yes_value > no_value
            if sell_yes_first:
                candidates.extend([("sell", "YES"), ("sell", "NO")])
            else:
                candidates.extend([("sell", "NO"), ("sell", "YES")])

        if members:
            buy_outcome = "NO" if market.id == group_hole else "YES"
        else:
            inventory_skew = _clamp(
                (yes_value - no_value) / self.inventory_bias_dollars,
                -1.0,
                1.0,
            )
            p_yes = _clamp(0.5 + personality.direction_bias - 0.35 * inventory_skew, 0.08, 0.92)
            buy_outcome = (
                "YES"
                if _unit(
                    _draw(
                        self.seed,
                        generation,
                        target_height,
                        actor.principal_id,
                        market.id,
                        "buy-direction",
                    )
                )
                < p_yes
                else "NO"
            )
        candidates.append(("buy", buy_outcome))
        candidates.append(("buy", "NO" if buy_outcome == "YES" else "YES"))
        return candidates

    def _order_for_market(
        self,
        actor: NoiseActorCredential,
        market: Market,
        generation: int,
        target_height: int,
        positions: dict[tuple[int, str], tuple[float, float]],
        members: tuple[int, ...] | None,
        group_hole: int | None,
    ) -> dict[str, Any] | None:
        min_yes = (market.actor_min_yes_nanos or 20_000_000) / NANOS_PER_DOLLAR
        max_yes = (market.actor_max_yes_nanos or 980_000_000) / NANOS_PER_DOLLAR
        seed_mid = (market.actor_seed_yes_nanos or 500_000_000) / NANOS_PER_DOLLAR
        mid = market.yes_price if 0.0 < market.yes_price < 1.0 else seed_mid
        mid = _clamp(mid, min_yes, max_yes)
        notional = self._order_notional(actor, market.id, generation, target_height)
        candidates = self._action_candidates(
            actor,
            market,
            generation,
            target_height,
            positions,
            members,
            group_hole,
        )
        for action, outcome in candidates:
            # Group buys must preserve an actor-specific uncovered outcome.
            if members and action == "buy":
                if outcome == "NO" and market.id != group_hole:
                    continue
                if outcome == "YES" and market.id == group_hole:
                    continue
            held_qty = positions.get((market.id, outcome), (0.0, 0.0))[0]
            if action == "sell" and held_qty <= 0:
                continue
            price = self._price_for_action(
                actor,
                market,
                generation,
                target_height,
                action,
                outcome,
                mid,
                min_yes,
                max_yes,
            )
            if price is None:
                continue
            desired_shares = notional / max(price, 0.01)
            if action == "sell":
                desired_shares = min(desired_shares, held_qty)
            quantity = shares_to_quantity_units(desired_shares)
            if quantity <= 0:
                continue
            return {
                "type": f"{action.title()}{outcome.title()}",
                "market_id": market.id,
                "limit_price_nanos": round(price * NANOS_PER_DOLLAR),
                "quantity": quantity,
            }
        return None

    def _order_notional(
        self,
        actor: NoiseActorCredential,
        market_id: int,
        generation: int,
        target_height: int,
    ) -> float:
        """Draw bounded notional while preserving each actor's size personality."""
        personality = self.personalities[actor.principal_id]
        size_draw = _unit(
            _draw(
                self.seed,
                generation,
                target_height,
                actor.principal_id,
                market_id,
                "size",
            )
        )
        # Larger personalities put more probability mass near the upper bound;
        # every actor can still draw the full configured range.
        size_fraction = size_draw ** (1.0 / personality.size)
        return (
            self.min_order_dollars
            + (self.max_order_dollars - self.min_order_dollars) * size_fraction
        )

    def _price_for_action(
        self,
        actor: NoiseActorCredential,
        market: Market,
        generation: int,
        target_height: int,
        action: str,
        outcome: str,
        mid_yes: float,
        min_yes: float,
        max_yes: float,
    ) -> float | None:
        lower = min_yes if outcome == "YES" else 1.0 - max_yes
        upper = max_yes if outcome == "YES" else 1.0 - min_yes
        fair = mid_yes if outcome == "YES" else 1.0 - mid_yes
        aggressive = (
            _unit(
                _draw(
                    self.seed,
                    generation,
                    target_height,
                    actor.principal_id,
                    market.id,
                    f"aggressive-{action}-{outcome}",
                )
            )
            < AGGRESSIVE_ORDER_PROBABILITY
        )
        random_distance = _unit(
            _draw(
                self.seed,
                generation,
                target_height,
                actor.principal_id,
                market.id,
                f"price-distance-{action}-{outcome}",
            )
        )
        personality = self.personalities[actor.principal_id]
        distance_fraction = 0.15 + 0.85 * _clamp(
            0.75 * random_distance + 0.25 * personality.price_distance,
            0.0,
            1.0,
        )
        distance = _lite_deviation(fair) * distance_fraction
        adverse_sign = 1.0 if action == "buy" else -1.0
        price = fair + adverse_sign * distance * (1.0 if aggressive else -1.0)
        return _clamp(price, lower, upper)


def _lite_deviation(price: float) -> float:
    """Exact Lite-tax envelope, expressed in probability units."""
    if not 0.0 < price < 1.0:
        return 0.0
    return LITE_PEAK_DEVIATION * (4.0 * price * (1.0 - price)) ** LITE_DEVIATION_EXPONENT
