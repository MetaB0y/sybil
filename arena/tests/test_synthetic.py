from sybil_client.types import Block, BuyNo, BuyYes, Market, SellNo, SellYes

from live.synthetic import (
    CrossingNoiseStrategy,
    FastReferenceStrategy,
    NativeNoiseStrategy,
    SyntheticStrategyConfig,
    is_mirror_market,
)


def _block(price: float, market_id: int = 1) -> Block:
    yes_nanos = int(price * 1_000_000_000)
    return Block(
        height=1,
        parent_hash="",
        state_root="",
        fills=[],
        clearing_prices={market_id: (yes_nanos, 1_000_000_000 - yes_nanos)},
        total_welfare=0,
        total_volume=0,
        orders_filled=0,
    )


def _market(
    market_id: int = 1,
    *,
    ref: float | None = None,
    condition_id: str | None = None,
    tags: list[str] | None = None,
) -> Market:
    return Market(
        id=market_id,
        name=f"Market {market_id}",
        yes_price_nanos=500_000_000,
        no_price_nanos=500_000_000,
        status="active",
        reference_price_nanos=int(ref * 1_000_000_000) if ref is not None else None,
        polymarket_condition_id=condition_id,
        tags=tags or [],
    )


def _apply_full_fill(positions: dict[tuple[int, str], float], orders) -> None:
    for order in orders:
        if isinstance(order, BuyYes):
            positions[(order.market_id, "YES")] = (
                positions.get((order.market_id, "YES"), 0) + order.quantity
            )
        elif isinstance(order, BuyNo):
            positions[(order.market_id, "NO")] = (
                positions.get((order.market_id, "NO"), 0) + order.quantity
            )
        elif isinstance(order, SellYes):
            positions[(order.market_id, "YES")] = max(
                0, positions.get((order.market_id, "YES"), 0) - order.quantity
            )
        elif isinstance(order, SellNo):
            positions[(order.market_id, "NO")] = max(
                0, positions.get((order.market_id, "NO"), 0) - order.quantity
            )


def test_fast_reference_trader_moves_toward_reference_price():
    strategy = FastReferenceStrategy(
        SyntheticStrategyConfig(
            max_inventory=100,
            quote_width=0.0,
            notional_budget=20.0,
            random_seed=7,
            randomization_range=0.0,
        )
    )

    orders = strategy.generate_orders(
        _block(0.50),
        {1: _market(ref=0.62)},
        {},
        cash=100.0,
    )

    assert len(orders) == 1
    assert isinstance(orders[0], BuyYes)
    assert orders[0].market_id == 1
    assert orders[0].limit_price_nanos == 620_000_000


def test_fast_reference_inventory_stays_bounded_over_repeated_fills():
    config = SyntheticStrategyConfig(
        max_inventory=10,
        quote_width=0.0,
        notional_budget=100.0,
        random_seed=1,
        randomization_range=0.0,
    )
    strategy = FastReferenceStrategy(config)
    positions: dict[tuple[int, str], float] = {}
    markets = {1: _market(ref=0.80)}

    for _ in range(50):
        orders = strategy.generate_orders(_block(0.50), markets, positions, cash=1_000.0)
        _apply_full_fill(positions, orders)
        assert positions.get((1, "YES"), 0) <= config.max_inventory
        assert positions.get((1, "NO"), 0) <= config.max_inventory


def test_fast_reference_unwinds_when_already_over_inventory_limit():
    strategy = FastReferenceStrategy(
        SyntheticStrategyConfig(
            max_inventory=10,
            quote_width=0.0,
            notional_budget=100.0,
            random_seed=1,
            randomization_range=0.0,
        )
    )

    orders = strategy.generate_orders(
        _block(0.50),
        {1: _market(ref=0.80)},
        {(1, "YES"): 15},
        cash=1_000.0,
    )

    assert len(orders) == 1
    assert isinstance(orders[0], SellYes)
    assert orders[0].quantity == 5


def test_native_noise_uses_sub_two_percent_price_jitter_even_if_config_is_wider():
    strategy = NativeNoiseStrategy(
        SyntheticStrategyConfig(
            max_inventory=100,
            quote_width=0.0,
            notional_budget=20.0,
            random_seed=3,
            randomization_range=0.50,
        )
    )

    orders = strategy.generate_orders(
        _block(0.50),
        {1: _market()},
        {},
        cash=100.0,
    )

    assert len(orders) == 1
    order = orders[0]
    if isinstance(order, BuyYes):
        target_yes = order.limit_price_nanos / 1_000_000_000
    else:
        assert isinstance(order, BuyNo)
        target_yes = 1.0 - order.limit_price_nanos / 1_000_000_000
    assert abs(target_yes - 0.50) <= 0.020000001


def test_native_noise_inventory_stays_bounded_over_repeated_fills():
    config = SyntheticStrategyConfig(
        max_inventory=8,
        quote_width=0.0,
        notional_budget=100.0,
        random_seed=5,
        randomization_range=0.02,
    )
    strategy = NativeNoiseStrategy(config)
    positions: dict[tuple[int, str], float] = {}
    markets = {1: _market()}

    for _ in range(100):
        orders = strategy.generate_orders(_block(0.50), markets, positions, cash=1_000.0)
        _apply_full_fill(positions, orders)
        assert positions.get((1, "YES"), 0) <= config.max_inventory
        assert positions.get((1, "NO"), 0) <= config.max_inventory


def test_per_market_enablement_is_honored():
    strategy = FastReferenceStrategy(
        SyntheticStrategyConfig(
            max_inventory=100,
            quote_width=0.0,
            notional_budget=20.0,
            random_seed=9,
            randomization_range=0.0,
            enabled_market_ids=frozenset({2}),
        )
    )
    block = _block(0.50)
    block.clearing_prices[2] = block.clearing_prices[1]

    orders = strategy.generate_orders(
        block,
        {1: _market(1, ref=0.70), 2: _market(2, ref=0.70)},
        {},
        cash=100.0,
    )

    assert len(orders) == 1
    assert orders[0].market_id == 2


def test_native_noise_skips_mirror_markets_without_live_reference_price():
    strategy = NativeNoiseStrategy(
        SyntheticStrategyConfig(
            max_inventory=100,
            quote_width=0.0,
            notional_budget=20.0,
            random_seed=11,
            randomization_range=0.02,
        )
    )
    block = _block(0.50)
    block.clearing_prices[2] = block.clearing_prices[1]

    orders = strategy.generate_orders(
        block,
        {
            1: _market(1, condition_id="0xcondition"),
            2: _market(2),
        },
        {},
        cash=100.0,
    )

    assert is_mirror_market(_market(1, condition_id="0xcondition"))
    assert len(orders) == 1
    assert orders[0].market_id == 2


def test_seeded_strategies_are_deterministic():
    config = SyntheticStrategyConfig(
        max_inventory=100,
        quote_width=0.0,
        notional_budget=20.0,
        random_seed=123,
        randomization_range=0.02,
    )
    markets = {1: _market(), 2: _market(2)}
    block = _block(0.50)
    block.clearing_prices[2] = block.clearing_prices[1]

    first = NativeNoiseStrategy(config).generate_orders(block, markets, {}, cash=100.0)
    second = NativeNoiseStrategy(config).generate_orders(block, markets, {}, cash=100.0)

    assert first == second


def test_sparse_crossing_noise_never_builds_a_same_account_complete_group():
    markets = {market_id: _market(market_id) for market_id in range(1, 5)}
    block = _block(0.50)
    block.clearing_prices.update({market_id: (500_000_000, 500_000_000) for market_id in markets})
    members = frozenset(markets)
    strategy = CrossingNoiseStrategy(
        SyntheticStrategyConfig(
            max_inventory=100,
            notional_budget=20.0,
            random_seed=10_000,
            randomization_range=0.02,
            crossing_markets_per_block=4,
        ),
        group_members_by_market={market_id: members for market_id in members},
    )

    orders = strategy.generate_orders(block, markets, {}, cash=100_000.0)

    assert len(orders) == 4
    assert len({order.market_id for order in orders}) == 4
    assert all(isinstance(order, (BuyYes, BuyNo)) for order in orders)
    assert sum(isinstance(order, BuyYes) for order in orders) < len(members)


def test_crossing_noise_randomness_is_keyed_by_block_height():
    markets = {market_id: _market(market_id) for market_id in range(1, 20)}
    block = _block(0.50)
    strategy = CrossingNoiseStrategy(
        SyntheticStrategyConfig(random_seed=123, crossing_markets_per_block=4)
    )

    first = strategy.generate_orders(block, markets, {}, cash=100_000.0)
    replay = strategy.generate_orders(block, markets, {}, cash=100_000.0)
    next_block = _block(0.50)
    next_block.height = block.height + 1
    following = strategy.generate_orders(next_block, markets, {}, cash=100_000.0)

    assert replay == first
    assert following != first


def test_fifteen_sparse_noise_streams_cover_about_quarter_catalog():
    markets = {market_id: _market(market_id) for market_id in range(1, 207)}
    block = _block(0.50)
    block.clearing_prices.update({market_id: (500_000_000, 500_000_000) for market_id in markets})
    touched = set()
    for actor_index in range(15):
        strategy = CrossingNoiseStrategy(
            SyntheticStrategyConfig(
                random_seed=10_000 + actor_index,
                crossing_markets_per_block=4,
            )
        )
        touched.update(
            order.market_id
            for order in strategy.generate_orders(block, markets, {}, cash=100_000.0)
        )

    assert 40 <= len(touched) <= 60


def test_sparse_noise_can_unwind_inventory_with_one_order_per_market():
    strategy = CrossingNoiseStrategy(
        SyntheticStrategyConfig(
            max_inventory=0,
            notional_budget=20.0,
            random_seed=3,
            crossing_markets_per_block=1,
        )
    )

    orders = strategy.generate_orders(
        _block(0.50),
        {1: _market()},
        {(1, "YES"): 10, (1, "NO"): 10},
        cash=0,
    )

    assert len(orders) == 1
    assert isinstance(orders[0], (SellYes, SellNo))
