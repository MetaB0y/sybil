"""Tests for trading bots."""

from unittest.mock import AsyncMock

import pytest

from bots.base import BaseAgent
from bots.informed import FixedProbabilityModel
from sybil_client.types import Account, AccountFill, Block, BuyNo, BuyYes


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
    def __init__(self, blocks: list[Block]):
        self.blocks = blocks

    def stream_blocks(self):
        async def _stream():
            for block in self.blocks:
                yield block

        return _stream()

    async def get_account(self, account_id: int):
        return Account(id=account_id, balance_nanos=50_000_000_000, positions=[])

    async def get_account_fills(self, account_id: int, limit: int, offset: int):
        return []

    async def get_pending_orders(self, account_id: int):
        return []


@pytest.mark.anyio
async def test_base_agent_update_state_fetches_all_fill_pages():
    client = AsyncMock()
    client.get_account.return_value = Account(id=7, balance_nanos=50_000_000_000, positions=[])
    first_page = [
        AccountFill(
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

    assert agent._last_fill_count == 101
    assert len(agent._fill_history) == 101
    assert client.get_account_fills.await_count == 2


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
