"""Tests for sybil_client."""

from sybil_client import BuyNo, BuyYes, SellNo, SellYes
from sybil_client.types import (
    NANOS_PER_DOLLAR,
    SHARE_SCALE,
    Account,
    Block,
    Fill,
    Market,
    Position,
    shares_to_quantity_units,
)


class TestTypes:
    """Test type conversions and properties."""

    def test_account_balance_dollars(self):
        account = Account(id=1, balance_nanos=50_000_000_000, positions=[])
        assert account.balance_dollars == 50.0

    def test_account_position(self):
        positions = [
            Position(market_id=0, outcome="YES", quantity=10),
            Position(market_id=0, outcome="NO", quantity=5),
            Position(market_id=1, outcome="YES", quantity=3),
        ]
        account = Account(id=1, balance_nanos=0, positions=positions)

        assert account.position(0, "YES") == 10
        assert account.position(0, "NO") == 5
        assert account.position(1, "YES") == 3
        assert account.position(1, "NO") == 0  # Not present
        assert account.position(2, "YES") == 0  # Market not present

    def test_market_prices(self):
        market = Market(
            id=0,
            name="Test",
            yes_price_nanos=600_000_000,
            no_price_nanos=400_000_000,
            status="active",
        )
        assert market.yes_price == 0.6
        assert market.no_price == 0.4

    def test_market_reference_price(self):
        market = Market(
            id=0,
            name="Test",
            yes_price_nanos=0,
            no_price_nanos=0,
            status="active",
            reference_price_nanos=125_000_000,
        )
        assert market.reference_price == 0.125

    def test_fill_price(self):
        fill = Fill(order_id=1, fill_qty=10, fill_price_nanos=550_000_000)
        assert fill.fill_price == 0.55

    def test_block_price_for(self):
        block = Block(
            height=1,
            parent_hash="abc",
            state_root="def",
            fills=[],
            clearing_prices={0: (600_000_000, 400_000_000)},
            total_welfare=0,
            total_volume=0,
            orders_filled=0,
        )
        assert block.price_for(0) == (0.6, 0.4)
        assert block.price_for(1) is None


class TestOrderSpecs:
    """Test order specification helpers."""

    def test_buy_yes_at_price(self):
        order = BuyYes.at_price(market_id=0, price=0.55, quantity=10)
        assert order.market_id == 0
        assert order.limit_price_nanos == 550_000_000
        assert order.quantity == 10

    def test_buy_no_at_price(self):
        order = BuyNo.at_price(market_id=1, price=0.40, quantity=5)
        assert order.market_id == 1
        assert order.limit_price_nanos == 400_000_000
        assert order.quantity == 5

    def test_sell_yes_at_price(self):
        order = SellYes.at_price(market_id=0, price=0.60, quantity=8)
        assert order.market_id == 0
        assert order.limit_price_nanos == 600_000_000
        assert order.quantity == 8

    def test_sell_no_at_price(self):
        order = SellNo.at_price(market_id=2, price=0.35, quantity=3)
        assert order.market_id == 2
        assert order.limit_price_nanos == 350_000_000
        assert order.quantity == 3

    def test_price_clamping(self):
        # Prices should be in valid range after conversion
        order = BuyYes.at_price(market_id=0, price=0.999, quantity=1)
        assert order.limit_price_nanos == 999_000_000

        order = BuyYes.at_price(market_id=0, price=0.001, quantity=1)
        assert order.limit_price_nanos == 1_000_000


class TestNanosConversion:
    """Test nanos/dollars conversions."""

    def test_nanos_per_dollar(self):
        assert NANOS_PER_DOLLAR == 1_000_000_000

    def test_share_scale(self):
        assert SHARE_SCALE == 1_000
        assert shares_to_quantity_units(1) == 1_000
        assert shares_to_quantity_units(0.001) == 1

    def test_account_balance_conversion(self):
        # $100
        account = Account(id=1, balance_nanos=100 * NANOS_PER_DOLLAR, positions=[])
        assert account.balance_dollars == 100.0

        # $0.50
        account = Account(id=1, balance_nanos=NANOS_PER_DOLLAR // 2, positions=[])
        assert account.balance_dollars == 0.5


def test_submit_orders_can_set_ioc_time_in_force(monkeypatch):
    import asyncio

    from sybil_client import SybilClient

    captured = {}
    client = SybilClient("http://example.invalid")

    async def fake_request(method, path, **kwargs):
        captured["method"] = method
        captured["path"] = path
        captured["json"] = kwargs["json"]
        return {"accepted": True}

    monkeypatch.setattr(client, "_request", fake_request)

    accepted = asyncio.run(
        client.submit_orders(
            42,
            [BuyYes.at_price(market_id=7, price=0.55, quantity=3)],
            time_in_force="IOC",
        )
    )

    assert accepted is True
    assert captured["method"] == "POST"
    assert captured["path"] == "/v1/orders"
    assert captured["json"]["account_id"] == 42
    assert captured["json"]["time_in_force"] == "IOC"
    assert "expires_at_block" not in captured["json"]
    assert captured["json"]["orders"] == [
        {
            "type": "BuyYes",
            "market_id": 7,
            "limit_price_nanos": 550_000_000,
            "quantity": 3_000,
        }
    ]


class _FakeStreamCM:
    """Async context manager mimicking httpx's client.stream(...)."""

    def __init__(self, lines, raise_exc=None):
        self._lines = lines
        self._raise_exc = raise_exc

    async def __aenter__(self):
        if self._raise_exc is not None:
            raise self._raise_exc
        return self

    async def __aexit__(self, *args):
        return False

    async def aiter_lines(self):
        for line in self._lines:
            yield line


class _FakeStreamingHttp:
    def __init__(self):
        self.calls = 0

    def stream(self, method, path):
        import httpx

        self.calls += 1
        if self.calls == 1:
            # First connection drops before yielding anything.
            return _FakeStreamCM([], raise_exc=httpx.ConnectError("boom"))
        return _FakeStreamCM(['data: {"height": 5}', 'data: {"height": 6}'])


async def test_stream_blocks_reconnects_after_drop():
    # AR-3: a dropped SSE connection must be retried with backoff instead of
    # ending the stream (which would tear down the consuming bot).
    from sybil_client import SybilClient

    client = SybilClient("http://example.invalid")
    client._client = _FakeStreamingHttp()
    client._stream_reconnect_base_s = 0.0
    client._stream_reconnect_max_s = 0.0

    heights = []
    gen = client.stream_blocks()
    try:
        async for block in gen:
            heights.append(block.height)
            if len(heights) >= 2:
                break
    finally:
        await gen.aclose()

    assert heights == [5, 6]
    assert client._client.calls == 2  # reconnected once after the drop
