import json

import pytest

from sybil_client import Block, Market, Portfolio, PositionValue, SybilClient

from live.noise_coordinator import (
    AGGRESSIVE_ORDER_PROBABILITY,
    BASE_SELECTION_PROBABILITY,
    MAX_ORDER_DOLLARS,
    MIN_ORDER_DOLLARS,
    NOISE_ACTOR_COUNT,
    NoiseActorCredential,
    NoiseCoordinator,
    _lite_deviation,
    _load_credentials,
)


def _coordinator() -> NoiseCoordinator:
    client = SybilClient("http://localhost")
    actors = tuple(
        NoiseActorCredential(f"noise-{index}", index + 2, f"{index:032x}")
        for index in range(NOISE_ACTOR_COUNT)
    )
    coordinator = NoiseCoordinator(client, actors)
    coordinator._state_key = "test:3"
    coordinator._generation_start_height = 1
    return coordinator


def _market(market_id: int, *, native: bool = False) -> Market:
    return Market(
        id=market_id,
        name=f"market-{market_id}",
        yes_price_nanos=500_000_000,
        no_price_nanos=500_000_000,
        status="active",
        actor_min_yes_nanos=300_000_000 if native else None,
        actor_max_yes_nanos=700_000_000 if native else None,
        actor_seed_yes_nanos=450_000_000 if native else None,
    )


def _block() -> Block:
    return Block(7, "parent", "root", [], {}, 0, 0, 0)


def _portfolio(*positions: PositionValue) -> Portfolio:
    return Portfolio(
        account_id=2,
        balance_nanos=20_000_000_000_000,
        total_deposited_nanos=20_000_000_000_000,
        positions=list(positions),
        total_position_value_nanos=sum(p.value_nanos for p in positions),
        portfolio_value_nanos=20_000_000_000_000,
        pnl_nanos=0,
    )


def test_selection_is_deterministic_sparse_and_actor_specific():
    coordinator = _coordinator()
    universe = tuple(range(1, 207))
    first = coordinator._select_markets(3, 8, universe)
    second = coordinator._select_markets(3, 8, universe)

    assert first == second
    assert all(len(markets) <= 32 for markets in first)
    assert len(set(first)) > 1
    assert sum(len(markets) for markets in first) < len(universe)


def test_statistical_aggregate_coverage_converges_to_twenty_five_percent():
    coordinator = _coordinator()
    universe = tuple(range(1, 207))
    covered = 0
    selected_orders = 0
    # 500 full-universe blocks = 103,000 market-block observations.
    samples = 500
    for height in range(2, samples + 2):
        selected = coordinator._select_markets(3, height, universe)
        selected_orders += sum(len(markets) for markets in selected)
        touched = set().union(*map(set, selected))
        covered += len(touched)
        for market_id in touched:
            coordinator._last_accepted_height[market_id] = height
    coverage = covered / (samples * len(universe))
    selected_per_actor_block = selected_orders / (samples * NOISE_ACTOR_COUNT)
    assert 0.235 <= coverage <= 0.265
    assert 3.4 <= selected_per_actor_block <= 4.4
    assert abs(BASE_SELECTION_PROBABILITY - 0.01899) < 0.0001


def test_actor_specific_group_holes_do_not_lock_the_cohort_direction():
    coordinator = _coordinator()
    members = (1, 2, 3, 4, 5)
    holes = []
    for actor in coordinator.actors:
        candidates = coordinator._action_candidates(
            actor,
            _market(1),
            3,
            8,
            {},
            members,
            members[hash(actor.principal_id) % len(members)],
        )
        holes.append(candidates[0])
    # Actor personality/direction lanes are independent even before the pure
    # group-safety override is applied by payload construction.
    assert len(set(holes)) >= 2
    drawn_holes = {
        members[
            __import__("live.noise_coordinator", fromlist=["_draw"])._draw(
                coordinator.seed, 3, 8, actor.principal_id, members[0], "group-hole"
            )
            % len(members)
        ]
        for actor in coordinator.actors
    }
    assert len(drawn_holes) >= 3


def test_inventory_bias_increases_selling_and_caps_quantity():
    coordinator = _coordinator()
    actor = coordinator.actors[0]
    market = _market(1)
    sell_without_inventory = 0
    sell_with_inventory = 0
    held = PositionValue(1, "YES", 2.0, 500_000_000, 2_000_000_000_000)
    for height in range(1, 200):
        empty = coordinator._order_for_market(actor, market, 3, height, {}, None, None)
        full = coordinator._order_for_market(
            actor,
            market,
            3,
            height,
            {(1, "YES"): (held.quantity, held.value_nanos / 1e9)},
            None,
            None,
        )
        sell_without_inventory += int(empty is not None and empty["type"].startswith("Sell"))
        sell_with_inventory += int(full is not None and full["type"].startswith("Sell"))
        if full is not None and full["type"] == "SellYes":
            assert full["quantity"] <= 2_000
    assert sell_without_inventory == 0
    assert sell_with_inventory > 150


def test_order_notional_is_randomized_inside_configured_range_with_size_personalities():
    coordinator = _coordinator()
    samples_by_actor = {
        actor.principal_id: [
            coordinator._order_notional(actor, 1, 3, height) for height in range(1, 2_001)
        ]
        for actor in coordinator.actors
    }
    realized = [value for samples in samples_by_actor.values() for value in samples]

    assert all(MIN_ORDER_DOLLARS <= value <= MAX_ORDER_DOLLARS for value in realized)
    assert min(realized) < MIN_ORDER_DOLLARS + 0.1
    assert max(realized) > MAX_ORDER_DOLLARS - 0.1

    smallest = min(coordinator.actors, key=lambda a: coordinator.personalities[a.principal_id].size)
    largest = max(coordinator.actors, key=lambda a: coordinator.personalities[a.principal_id].size)
    assert (
        sum(samples_by_actor[smallest.principal_id]) / 2_000
        < sum(samples_by_actor[largest.principal_id]) / 2_000
    )


def test_prices_randomize_within_lite_envelope_without_mm_quote_knowledge():
    coordinator = _coordinator()
    actor = coordinator.actors[0]
    market = _market(1)
    prices = [
        coordinator._price_for_action(actor, market, 3, height, "buy", "YES", 0.5, 0.02, 0.98)
        for height in range(1, 2_001)
    ]
    assert all(price is not None for price in prices)
    realized = [price for price in prices if price is not None]
    envelope = _lite_deviation(0.5)
    assert envelope == 0.04
    assert all(0.0 < abs(price - 0.5) <= envelope for price in realized)
    aggressive_share = sum(price > 0.5 for price in realized) / len(realized)
    assert abs(aggressive_share - AGGRESSIVE_ORDER_PROBABILITY) < 0.03
    assert len(set(realized)) > 1_900


def test_native_yes_and_no_prices_respect_complementary_actor_ranges():
    coordinator = _coordinator()
    actor = coordinator.actors[0]
    market = _market(1, native=True)
    for outcome in ("YES", "NO"):
        price = coordinator._price_for_action(actor, market, 3, 8, "buy", outcome, 0.45, 0.30, 0.70)
        assert price is not None
        assert 0.30 <= price <= 0.70


def test_sparse_payload_serializes_only_selected_markets_and_group_safe_buys():
    coordinator = _coordinator()
    markets = {mid: _market(mid) for mid in (1, 2, 3, 4)}
    members = {mid: (1, 2, 3) for mid in (1, 2, 3)}
    payload = coordinator._build_payload(
        coordinator.actors[0],
        _portfolio(),
        markets,
        members,
        3,
        8,
        (1, 2, 3),
    )
    assert [row["market_id"] for row in payload["market_intents"]] == [1, 2, 3]
    buy_types = [
        row["orders"][0]["type"]
        for row in payload["market_intents"]
        if row["orders"] and row["orders"][0]["type"].startswith("Buy")
    ]
    assert buy_types.count("BuyNo") <= 1


def test_credentials_accept_shared_api_file_and_require_exactly_fifteen_noise():
    payload = {
        "actors": [
            {"principal_id": "mm", "account_id": 1, "token": "m" * 32, "role": "market_maker"},
            *[
                {
                    "principal_id": f"noise-{i}",
                    "account_id": 2 + i,
                    "token": f"{i:032x}",
                    "role": "noise",
                }
                for i in range(NOISE_ACTOR_COUNT)
            ],
        ]
    }
    assert len(_load_credentials(json.dumps(payload))) == NOISE_ACTOR_COUNT
    payload["actors"].pop()
    try:
        _load_credentials(json.dumps(payload))
    except ValueError as error:
        assert "fifteen" in str(error)
    else:
        raise AssertionError("legacy 1+14 credential file was accepted")


def test_anti_starvation_rises_gradually_without_forcing_selection():
    coordinator = _coordinator()
    actor = coordinator.actors[0]
    coordinator._last_accepted_height[1] = 1
    base = coordinator._selection_probability(actor, 1, 9)
    aged = coordinator._selection_probability(actor, 1, 20)
    capped = coordinator._selection_probability(actor, 1, 10_000)
    assert aged > base
    assert capped == 0.06
    assert capped < 1.0


@pytest.mark.asyncio
async def test_runtime_metadata_is_cached_by_generation_and_failed_epoch_retries_once():
    coordinator = _coordinator()

    class FakeClient:
        def __init__(self):
            self.universe_calls = 0
            self.market_calls = 0
            self.group_calls = 0
            self.submit_attempts: dict[str, int] = {}

        async def actor_universe(self, token):
            self.universe_calls += 1
            actor = next(actor for actor in coordinator.actors if actor.token == token)
            return {
                "actor_ready": True,
                "actor_role": "noise",
                "account_id": actor.account_id,
                "generation": 3,
                "policy_digest_hex": "abc",
                "market_ids": [1],
            }

        async def list_markets(self):
            self.market_calls += 1
            market = _market(1)
            market.yes_price_nanos = 400_000_000 + self.market_calls * 100_000_000
            market.no_price_nanos = 1_000_000_000 - market.yes_price_nanos
            return [market]

        async def list_market_groups(self):
            self.group_calls += 1
            return []

        async def get_portfolio(self, _account_id):
            return _portfolio()

        async def submit_actor_epoch(self, token, _payload):
            attempt = self.submit_attempts.get(token, 0) + 1
            self.submit_attempts[token] = attempt
            if token == coordinator.actors[1].token and attempt == 1:
                raise RuntimeError("transient")
            return {"accepted": True}

    client = FakeClient()
    coordinator.client = client
    for agent in coordinator.snapshot_agents:
        object.__setattr__(agent, "client", client)

    await coordinator.submit_for_block(_block())
    next_block = _block()
    next_block.height = 8
    await coordinator.submit_for_block(next_block)

    # The first block validates all bindings; unchanged blocks probe only the
    # lead credential and reuse immutable group metadata. The bulk market read
    # remains per-block because it carries the changing committed marks.
    assert client.universe_calls == NOISE_ACTOR_COUNT + 1
    assert client.market_calls == 2
    assert client.group_calls == 1
    assert client.submit_attempts[coordinator.actors[1].token] == 3
    assert coordinator._market_by_id[1].yes_price_nanos == 600_000_000
