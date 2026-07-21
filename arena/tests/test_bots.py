"""Tests for trading bots."""

from unittest.mock import AsyncMock, Mock

import pytest

from bots.base import BaseAgent
from bots.informed import FixedProbabilityModel
from sybil_client.client import SybilClientError
from sybil_client.types import (
    Account,
    AccountFill,
    Block,
    BlockStreamBlockEvent,
    BlockStreamReplayCompleteEvent,
    BuyNo,
    BuyYes,
    OrderAdmissionPolicy,
    Position,
    SellYes,
)


class TestProbabilityModel:
    """Test probability model implementations."""

    def test_fixed_probability_model(self):
        model = FixedProbabilityModel({0: 0.6, 1: 0.3})

        assert model.get_probability(0) == 0.6
        assert model.get_probability(1) == 0.3
        assert model.get_probability(2) is None  # Unknown market

    def test_empty_model(self):
        model = FixedProbabilityModel({})
        assert model.get_probability(0) is None


class TestBlockParsing:
    """Test block data parsing for bot logic."""

    def test_clearing_prices_iteration(self):
        block = Block(
            height=1,
            parent_hash="",
            state_root="",
            fills=[],
            clearing_prices={
                0: (500_000_000, 500_000_000),
                1: (600_000_000, 400_000_000),
            },
            total_welfare=0,
            total_volume=0,
            orders_filled=0,
        )

        prices = list(block.clearing_prices.items())
        assert len(prices) == 2
        assert (0, (500_000_000, 500_000_000)) in prices
        assert (1, (600_000_000, 400_000_000)) in prices


class TestOrderGeneration:
    """Test that bots generate valid orders."""

    def test_buy_yes_order_structure(self):
        order = BuyYes(market_id=0, limit_price_nanos=550_000_000, quantity=10)

        assert order.market_id == 0
        assert order.limit_price_nanos == 550_000_000
        assert order.quantity == 10

    def test_buy_no_order_structure(self):
        order = BuyNo(market_id=1, limit_price_nanos=450_000_000, quantity=5)

        assert order.market_id == 1
        assert order.limit_price_nanos == 450_000_000
        assert order.quantity == 5

    def test_order_at_price_helper(self):
        order = BuyYes.at_price(market_id=0, price=0.55, quantity=10)

        # Should convert 0.55 to 550_000_000 nanos
        assert order.limit_price_nanos == 550_000_000


class TestEdgeCalculation:
    """Test edge calculation for informed trading."""

    def test_positive_edge(self):
        # Market price = 0.50, model says 0.60
        market_prob = 0.50
        model_prob = 0.60
        edge = model_prob - market_prob

        assert abs(edge - 0.10) < 1e-9  # 10% positive edge -> buy YES

    def test_negative_edge(self):
        # Market price = 0.60, model says 0.40
        market_prob = 0.60
        model_prob = 0.40
        edge = model_prob - market_prob

        assert abs(edge - (-0.20)) < 1e-9  # 20% negative edge -> buy NO

    def test_no_edge(self):
        # Market matches model
        market_prob = 0.55
        model_prob = 0.55
        edge = model_prob - market_prob

        assert edge == 0.0  # No edge -> no trade


class TestPriceConversion:
    """Test price format conversions."""

    def test_nanos_to_probability(self):
        # 500M nanos = $0.50 = 50% probability
        yes_nanos = 500_000_000
        prob = yes_nanos / 1_000_000_000
        assert prob == 0.5

    def test_probability_to_nanos(self):
        # 60% probability = $0.60 = 600M nanos
        prob = 0.60
        nanos = int(prob * 1_000_000_000)
        assert nanos == 600_000_000

    def test_complementary_prices(self):
        # YES + NO should equal $1 (1B nanos)
        yes_nanos = 650_000_000
        no_nanos = 350_000_000
        assert yes_nanos + no_nanos == 1_000_000_000


class _DummyAgent(BaseAgent):
    async def on_block(self, block: Block):
        return []


class _FailsOnceAgent(BaseAgent):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.on_block_calls = 0

    async def on_block(self, block: Block):
        self.on_block_calls += 1
        if self.on_block_calls == 1:
            raise ValueError("bad block")
        self.stop()
        return []


class _StreamingClient:
    def __init__(self, blocks: list[Block], accepted: list[bool] | None = None):
        self.blocks = blocks
        self.accepted = list(accepted or [])
        self.submit_calls = 0

    def stream_block_events(self):
        async def _stream():
            for block in self.blocks:
                yield BlockStreamBlockEvent(block=block, replayed=False)

        return _stream()

    async def get_account(self, account_id: int):
        return Account(id=account_id, balance_nanos=50_000_000_000, positions=[])

    async def get_account_fills(
        self,
        account_id: int,
        limit: int,
        after: str | None = None,
    ):
        return []

    async def get_pending_orders(self, account_id: int):
        return []

    async def submit_orders(self, account_id: int, orders, **kwargs):
        self.submit_calls += 1
        return self.accepted.pop(0) if self.accepted else True


class _OrderAgent(BaseAgent):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.on_block_calls = 0

    async def on_block(self, block: Block):
        self.on_block_calls += 1
        return [BuyYes.at_price(0, 0.55, 1)]


class _HookFailsAgent(_OrderAgent):
    async def on_orders_submitted(self, block: Block, orders) -> None:
        raise RuntimeError("observer failed")


@pytest.mark.anyio
async def test_base_agent_update_state_fetches_all_fill_pages():
    client = AsyncMock()
    client.get_account.return_value = Account(id=7, balance_nanos=50_000_000_000, positions=[])
    first_page = [
        AccountFill(
            cursor=f"1.{i}",
            order_id=i,
            fill_qty=1,
            fill_price_nanos=500_000_000,
            block_height=1,
            timestamp_ms=i,
        )
        for i in range(1, 101)
    ]
    client.get_account_fills.side_effect = [
        first_page,
        [
            AccountFill(
                cursor="2.101",
                order_id=101,
                fill_qty=1,
                fill_price_nanos=520_000_000,
                block_height=2,
                timestamp_ms=101,
            ),
        ],
    ]
    agent = _DummyAgent(client=client, account_id=7, name="Dummy")

    block = Block(
        height=1,
        parent_hash="",
        state_root="",
        fills=[],
        clearing_prices={},
        total_welfare=0,
        total_volume=0,
        orders_filled=0,
    )
    await agent._update_state(block)

    assert agent._last_fill_cursor == "2.101"
    assert len(agent._fill_history) == 101
    assert client.get_account_fills.await_count == 2
    client.get_account_fills.assert_any_await(7, limit=100, after="0.0")
    client.get_account_fills.assert_any_await(7, limit=100, after="1.100")


@pytest.mark.anyio
async def test_base_agent_cursor_tailing_handles_new_fills_between_polls():
    client = AsyncMock()
    client.get_account.return_value = Account(id=7, balance_nanos=50_000_000_000, positions=[])
    client.get_account_fills.side_effect = [
        [
            AccountFill(
                cursor="1.10",
                order_id=10,
                fill_qty=1,
                fill_price_nanos=500_000_000,
                block_height=1,
                timestamp_ms=1,
            )
        ],
        [
            AccountFill(
                cursor="2.11",
                order_id=11,
                fill_qty=1,
                fill_price_nanos=510_000_000,
                block_height=2,
                timestamp_ms=2,
            )
        ],
    ]
    agent = _DummyAgent(client=client, account_id=7, name="Dummy")
    block = Block(
        height=1,
        parent_hash="",
        state_root="",
        fills=[],
        clearing_prices={},
        total_welfare=0,
        total_volume=0,
        orders_filled=0,
    )

    await agent._update_state(block)
    await agent._update_state(block)

    assert [fill.order_id for fill in agent._fill_history] == [10, 11]
    assert agent._last_fill_cursor == "2.11"
    client.get_account_fills.assert_any_await(7, limit=100, after="0.0")
    client.get_account_fills.assert_any_await(7, limit=100, after="1.10")


@pytest.mark.anyio
async def test_base_agent_can_bound_live_observation_tails_without_losing_totals():
    client = AsyncMock()
    client.get_account.side_effect = [
        Account(id=7, balance_nanos=50_000_000_000, positions=[]),
        Account(id=7, balance_nanos=49_000_000_000, positions=[]),
        Account(id=7, balance_nanos=48_000_000_000, positions=[]),
    ]
    client.get_account_fills.side_effect = [
        [
            AccountFill(
                cursor=f"{height}.{height}",
                order_id=height,
                fill_qty=1,
                fill_price_nanos=500_000_000,
                block_height=height,
                timestamp_ms=height,
            )
        ]
        for height in range(1, 4)
    ]
    agent = _DummyAgent(client=client, account_id=7, name="Dummy")
    agent.bound_observation_history(2)

    for height in range(1, 4):
        block = Block(
            height=height,
            parent_hash="",
            state_root="",
            fills=[],
            clearing_prices={},
            total_welfare=0,
            total_volume=0,
            orders_filled=0,
        )
        await agent._update_state(block)

    assert list(agent.balance_history) == [49.0, 48.0]
    assert [fill.order_id for fill in agent._fill_history] == [2, 3]
    assert agent.total_fills_observed == 3
    assert agent.pnl == -2.0


@pytest.mark.anyio
async def test_base_agent_initializes_live_tail_without_replaying_historical_fills():
    client = AsyncMock()
    client.get_account_fills.return_value = [
        AccountFill(
            cursor="9.42",
            order_id=42,
            fill_qty=3,
            fill_price_nanos=500_000_000,
            block_height=9,
            timestamp_ms=9,
        )
    ]
    client.get_account.return_value = Account(
        id=7,
        balance_nanos=49_000_000_000,
        positions=[Position(market_id=3, outcome="YES", quantity=4)],
    )
    agent = _DummyAgent(client=client, account_id=7, name="Dummy")
    agent.bound_observation_history(2)

    await agent.initialize_live_observation_tail()

    assert agent._last_fill_cursor == "9.42"
    assert agent.positions == {(3, "YES"): 4}
    assert list(agent.balance_history) == [49.0]
    assert list(agent._fill_history) == []
    assert agent.total_fills_observed == 0
    client.get_account_fills.assert_awaited_once_with(7, limit=1)


@pytest.mark.anyio
async def test_base_agent_cursor_tailing_continues_after_retention_trims_tail():
    client = AsyncMock()
    client.get_account.return_value = Account(id=7, balance_nanos=50_000_000_000, positions=[])
    client.get_account_fills.side_effect = [
        [
            AccountFill(
                cursor="5.50",
                order_id=50,
                fill_qty=1,
                fill_price_nanos=500_000_000,
                block_height=5,
                timestamp_ms=5,
            ),
            AccountFill(
                cursor="6.51",
                order_id=51,
                fill_qty=1,
                fill_price_nanos=510_000_000,
                block_height=6,
                timestamp_ms=6,
            ),
        ],
    ]
    agent = _DummyAgent(client=client, account_id=7, name="Dummy")
    agent._last_fill_cursor = "1.10"
    block = Block(
        height=6,
        parent_hash="",
        state_root="",
        fills=[],
        clearing_prices={},
        total_welfare=0,
        total_volume=0,
        orders_filled=0,
    )

    await agent._update_state(block)

    assert [fill.order_id for fill in agent._fill_history] == [50, 51]
    assert agent._last_fill_cursor == "6.51"
    client.get_account_fills.assert_any_await(7, limit=100, after="1.10")


@pytest.mark.anyio
async def test_base_agent_reconciles_expired_fill_cursor_then_resumes_tailing(caplog):
    client = AsyncMock()
    client.get_account.side_effect = [
        Account(id=7, balance_nanos=50_000_000_000, positions=[]),
        Account(
            id=7,
            balance_nanos=49_000_000_000,
            positions=[Position(market_id=3, outcome="YES", quantity=2)],
        ),
        Account(
            id=7,
            balance_nanos=48_000_000_000,
            positions=[Position(market_id=3, outcome="YES", quantity=3)],
        ),
    ]
    retained_tail = AccountFill(
        cursor="6.51",
        order_id=51,
        fill_qty=1,
        fill_price_nanos=510_000_000,
        block_height=6,
        timestamp_ms=6,
    )
    new_fill = AccountFill(
        cursor="7.52",
        order_id=52,
        fill_qty=1,
        fill_price_nanos=520_000_000,
        block_height=7,
        timestamp_ms=7,
    )
    client.get_account_fills.side_effect = [
        SybilClientError(410, "cursor gap"),
        [retained_tail],
        [new_fill],
    ]
    agent = _DummyAgent(client=client, account_id=7, name="Dummy")
    block = Block(
        height=7,
        parent_hash="",
        state_root="",
        fills=[],
        clearing_prices={},
        total_welfare=0,
        total_volume=0,
        orders_filled=0,
    )

    with caplog.at_level("WARNING"):
        await agent._update_state(block)

    assert agent._last_fill_cursor == "6.51"
    assert agent.positions == {(3, "YES"): 2}
    assert agent.balance_history == [49.0]
    assert agent._fill_history == []
    assert "reconciled from canonical account state" in caplog.text
    client.get_account_fills.assert_any_await(7, limit=1)

    await agent._update_state(block)

    assert agent._last_fill_cursor == "7.52"
    assert agent.positions == {(3, "YES"): 3}
    assert [fill.order_id for fill in agent._fill_history] == [52]
    client.get_account_fills.assert_any_await(7, limit=100, after="6.51")


@pytest.mark.anyio
async def test_base_agent_reconciles_pruned_empty_fill_history_at_chain_tip():
    client = AsyncMock()
    client.get_account.return_value = Account(
        id=7,
        balance_nanos=50_000_000_000,
        positions=[],
    )
    client.get_account_fills.side_effect = [
        SybilClientError(410, "cursor gap"),
        [],
    ]
    client.health.return_value = {"height": 99}
    agent = _DummyAgent(client=client, account_id=7, name="Dummy")
    block = Block(
        height=1,
        parent_hash="",
        state_root="",
        fills=[],
        clearing_prices={},
        total_welfare=0,
        total_volume=0,
        orders_filled=0,
    )

    await agent._update_state(block)

    assert agent._last_fill_cursor == f"99.{2**64 - 1}"
    client.health.assert_awaited_once_with()


@pytest.mark.anyio
async def test_base_agent_on_block_exception_continues_loop(caplog):
    blocks = [
        Block(
            height=1,
            parent_hash="",
            state_root="",
            fills=[],
            clearing_prices={},
            total_welfare=0,
            total_volume=0,
            orders_filled=0,
        ),
        Block(
            height=2,
            parent_hash="",
            state_root="",
            fills=[],
            clearing_prices={},
            total_welfare=0,
            total_volume=0,
            orders_filled=0,
        ),
    ]
    agent = _FailsOnceAgent(client=_StreamingClient(blocks), account_id=7, name="Flaky")

    await agent.run()

    assert agent.on_block_calls == 2
    assert agent.on_block_error_count == 1
    assert "name=Flaky" in caplog.text
    assert "block_height=1" in caplog.text


@pytest.mark.anyio
async def test_base_agent_observes_replay_without_running_strategy():
    replayed = Block(
        height=5,
        parent_hash="",
        state_root="",
        fills=[],
        clearing_prices={},
        total_welfare=0,
        total_volume=0,
        orders_filled=0,
    )
    live = Block(
        height=6,
        parent_hash="",
        state_root="",
        fills=[],
        clearing_prices={},
        total_welfare=0,
        total_volume=0,
        orders_filled=0,
    )

    class ReplayClient(_StreamingClient):
        def stream_block_events(self):
            async def _stream():
                yield BlockStreamBlockEvent(block=replayed, replayed=True)
                yield BlockStreamReplayCompleteEvent(up_to_height=5)
                yield BlockStreamBlockEvent(block=live, replayed=False)

            return _stream()

    agent = _FailsOnceAgent(client=ReplayClient([]), account_id=7, name="ReplaySafe")
    await agent.run()

    # _FailsOnceAgent fails on its first strategy call. If replay had invoked
    # the strategy, the live block would have been a second call.
    assert agent.on_block_calls == 1
    assert len(agent.balance_history) == 2


@pytest.mark.anyio
async def test_base_agent_skips_strategy_when_canonical_refresh_fails():
    blocks = [
        Block(
            height=height,
            parent_hash="",
            state_root="",
            fills=[],
            clearing_prices={},
            total_welfare=0,
            total_volume=0,
            orders_filled=0,
        )
        for height in (1, 2)
    ]

    class Client(_StreamingClient):
        async def get_account(self, account_id: int):
            if not hasattr(self, "_refresh_failed"):
                self._refresh_failed = True
                raise ConnectionError("account API unavailable")
            return await super().get_account(account_id)

    client = Client(blocks)
    agent = _OrderAgent(client=client, account_id=7)

    await agent.run()

    assert agent.on_block_calls == 1
    assert client.submit_calls == 1
    assert agent.block_log[0][0] == 2


@pytest.mark.anyio
async def test_base_agent_fails_closed_when_pending_status_is_unknown():
    blocks = [
        Block(
            height=height,
            parent_hash="",
            state_root="",
            fills=[],
            clearing_prices={},
            total_welfare=0,
            total_volume=0,
            orders_filled=0,
        )
        for height in (1, 2)
    ]

    class Client(_StreamingClient):
        async def get_pending_orders(self, account_id: int):
            if not hasattr(self, "_pending_failed"):
                self._pending_failed = True
                raise ConnectionError("orders API unavailable")
            return []

    client = Client(blocks)
    agent = _OrderAgent(client=client, account_id=7)

    await agent.run()

    assert agent.on_block_calls == 1
    assert client.submit_calls == 1
    assert agent.block_log[0][0] == 2


@pytest.mark.anyio
async def test_base_agent_counts_only_accepted_blocks_toward_max_blocks():
    blocks = [
        Block(
            height=height,
            parent_hash="",
            state_root="",
            fills=[],
            clearing_prices={},
            total_welfare=0,
            total_volume=0,
            orders_filled=0,
        )
        for height in (1, 2, 3)
    ]
    client = _StreamingClient(blocks, accepted=[False, True, True])
    agent = _OrderAgent(client=client, account_id=7, max_blocks=1)

    await agent.run()

    assert agent.on_block_calls == 2
    assert client.submit_calls == 2
    assert agent.total_orders_submitted == 1
    assert [height for height, _ in agent.block_log] == [2]


@pytest.mark.anyio
async def test_base_agent_keeps_acceptance_when_post_submission_hook_fails(caplog):
    block = Block(
        height=1,
        parent_hash="",
        state_root="",
        fills=[],
        clearing_prices={},
        total_welfare=0,
        total_volume=0,
        orders_filled=0,
    )
    client = _StreamingClient([block])
    agent = _HookFailsAgent(client=client, account_id=7, max_blocks=1)

    await agent.run()

    assert agent.total_orders_submitted == 1
    assert len(agent.block_log) == 1
    assert "Post-submission hook failed after API acceptance" in caplog.text


def test_order_admission_policy_keeps_exact_minimum_and_suppresses_dust():
    agent = _DummyAgent(client=AsyncMock(), account_id=7, name="Policy")
    agent.order_admission_policy = OrderAdmissionPolicy(
        min_order_notional_nanos=1_000_000,
        share_scale=1_000,
    )
    exact_minimum = BuyYes(
        market_id=1,
        limit_price_nanos=1_000_000,
        quantity=1,
    )
    low_price_buy = BuyYes(
        market_id=2,
        limit_price_nanos=999_999,
        quantity=1,
    )
    tiny_remaining_sell = SellYes(
        market_id=3,
        limit_price_nanos=500_000_000,
        quantity=0.001,
    )

    accepted, suppressed = agent.apply_order_admission_policy(
        [low_price_buy, exact_minimum, tiny_remaining_sell]
    )

    assert accepted == [exact_minimum]
    assert suppressed == [low_price_buy, tiny_remaining_sell]
    assert agent.orders_suppressed_count == 2


def test_order_admission_policy_does_not_enlarge_an_unaffordable_order():
    metrics = Mock()
    agent = _DummyAgent(client=AsyncMock(), account_id=7, name="Conservative")
    agent.order_admission_policy = OrderAdmissionPolicy(
        min_order_notional_nanos=1_000_000,
        share_scale=1_000,
    )
    agent.metrics = metrics
    agent.balance_history = [0.0004]
    proposed = BuyYes(
        market_id=1,
        limit_price_nanos=500_000,
        quantity=1,
    )

    accepted, suppressed = agent.apply_order_admission_policy([proposed])

    assert accepted == []
    assert suppressed == [proposed]
    assert proposed.quantity == 1
    metrics.record_order_suppressed.assert_called_once_with(
        "Conservative",
        "below_min_notional",
        1,
    )


def test_order_admission_policy_exempts_flash_liquidity_bundles():
    agent = _DummyAgent(client=AsyncMock(), account_id=7)
    agent.order_admission_policy = OrderAdmissionPolicy(
        min_order_notional_nanos=1_000_000,
        share_scale=1_000,
    )
    agent.mm_budget_nanos = 10_000_000
    proposed = BuyYes(
        market_id=1,
        limit_price_nanos=1,
        quantity=0.001,
    )

    accepted, suppressed = agent.apply_order_admission_policy([proposed])

    assert accepted == [proposed]
    assert suppressed == []
