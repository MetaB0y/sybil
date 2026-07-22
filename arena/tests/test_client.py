"""Tests for sybil_client."""

import json

import pytest

from sybil_client import (
    BlockStreamBlockEvent,
    BlockStreamReplayCompleteEvent,
    BuyNo,
    BuyYes,
    SellNo,
    SellYes,
)
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

    def test_market_polymarket_condition_id(self):
        market = Market(
            id=0,
            name="Test",
            yes_price_nanos=0,
            no_price_nanos=0,
            status="active",
            polymarket_condition_id="0xabc",
        )
        assert market.polymarket_condition_id == "0xabc"

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
            "limit_price_nanos": "550000000",
            "quantity": 3_000,
        }
    ]


def test_submit_signed_mm_bundle_preserves_signed_integer_fields(monkeypatch):
    import asyncio

    from sybil_client import SybilClient

    captured = {}
    client = SybilClient("http://example.invalid")

    async def fake_request(method, path, **kwargs):
        captured["method"] = method
        captured["path"] = path
        captured["json"] = kwargs["json"]
        return {"accepted": True, "order_ids": [91, 92]}

    monkeypatch.setattr(client, "_request", fake_request)
    order_ids = asyncio.run(
        client.submit_signed_mm_bundle(
            account_id=42,
            bundle_id_hex="11" * 32,
            orders=[
                BuyYes.at_price(market_id=7, price=0.51, quantity=3),
                SellNo.at_price(market_id=8, price=0.49, quantity=4),
            ],
            expires_at_block=12,
            mm_budget_nanos=3_000_000_000,
            nonce=17,
            signer_pubkey_hex="02" + "22" * 32,
            signature_hex="33" * 64,
        )
    )

    assert order_ids == [91, 92]
    assert captured["method"] == "POST"
    assert captured["path"] == "/v1/orders/mm-bundles/signed"
    assert captured["json"]["mm_budget_nanos"] == "3000000000"
    assert captured["json"]["expires_at_block"] == 12
    assert captured["json"]["orders"][0]["quantity"] == 3_000
    assert captured["json"]["orders"][1]["quantity"] == 4_000


def test_fill_cursor_gap_requires_reconciliation(monkeypatch):
    import asyncio
    import pytest

    from sybil_client.client import SybilClient, SybilClientError

    client = SybilClient("http://example.invalid")

    async def fake_request(method, path, **kwargs):
        return {"fills": [], "cursor_gap": True}

    monkeypatch.setattr(client, "_request", fake_request)
    with pytest.raises(SybilClientError) as error:
        asyncio.run(client.get_account_fills(42, after="1.7"))
    assert error.value.status_code == 410


class _FakeWebSocket:
    """Async context manager and iterator matching ``websockets.connect``."""

    def __init__(self, messages=(), raise_exc=None):
        self._messages = iter(messages)
        self._raise_exc = raise_exc

    async def __aenter__(self):
        if self._raise_exc is not None:
            raise self._raise_exc
        return self

    async def __aexit__(self, *args):
        return False

    def __aiter__(self):
        return self

    async def __anext__(self):
        try:
            return next(self._messages)
        except StopIteration:
            raise StopAsyncIteration from None


class _FakeConnect:
    def __init__(self, connections):
        self.connections = iter(connections)
        self.urls = []

    def __call__(self, url, **kwargs):
        self.urls.append(url)
        return next(self.connections)


async def test_stream_blocks_reconnects_from_next_height(monkeypatch):
    from sybil_client import SybilClient
    import sybil_client.client as client_module

    client = SybilClient("http://example.invalid")
    client._stream_reconnect_base_s = 0.0
    client._stream_reconnect_max_s = 0.0

    def envelope(height):
        return json.dumps({"v": 2, "type": "block", "data": {"height": height}})

    fake_connect = _FakeConnect(
        [
            _FakeWebSocket([envelope(5)]),
            _FakeWebSocket([envelope(5), envelope(6)]),
        ]
    )
    monkeypatch.setattr(client_module, "connect", fake_connect)

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
    assert fake_connect.urls == [
        "ws://example.invalid/v2/blocks/ws",
        "ws://example.invalid/v2/blocks/ws?from_block=6",
    ]


async def test_stream_block_events_marks_replay_until_boundary(monkeypatch):
    from sybil_client import SybilClient
    import sybil_client.client as client_module

    def envelope(height):
        return json.dumps({"v": 2, "type": "block", "data": {"height": height}})

    messages = [
        json.dumps({"v": 3, "type": "future_message", "data": "ignored"}),
        envelope(5),
        json.dumps({"v": 2, "type": "replay_complete", "up_to_height": 5}),
        json.dumps({"v": 2, "type": "heartbeat", "at": 123}),
        envelope(6),
    ]
    monkeypatch.setattr(
        client_module,
        "connect",
        _FakeConnect([_FakeWebSocket(messages)]),
    )

    events = []
    stream = SybilClient("http://example.invalid").stream_block_events(from_block=5)
    try:
        async for event in stream:
            events.append(event)
            if len(events) == 3:
                break
    finally:
        await stream.aclose()

    assert isinstance(events[0], BlockStreamBlockEvent)
    assert events[0].block.height == 5
    assert events[0].replayed is True
    assert events[1] == BlockStreamReplayCompleteEvent(up_to_height=5)
    assert isinstance(events[2], BlockStreamBlockEvent)
    assert events[2].block.height == 6
    assert events[2].replayed is False


async def test_stream_blocks_surfaces_retention_gap(monkeypatch):
    from sybil_client import BlockStreamGapError, SybilClient
    import sybil_client.client as client_module

    message = json.dumps(
        {
            "v": 2,
            "type": "retention_gap",
            "requested_height": 5,
            "retention_min_height": 10,
            "head_height": 20,
        }
    )
    monkeypatch.setattr(client_module, "connect", _FakeConnect([_FakeWebSocket([message])]))

    with pytest.raises(BlockStreamGapError, match="retention_min=10"):
        await anext(SybilClient("https://example.invalid").stream_blocks(from_block=5))


def test_parse_market_preserves_polymarket_condition_id():
    from sybil_client import SybilClient

    client = SybilClient("http://example.invalid")

    market = client._parse_market(
        {
            "market_id": 9,
            "name": "Mirror",
            "status": "active",
            "reference_price_nanos": 610_000_000,
            "polymarket_condition_id": "0xcondition",
        }
    )

    assert market.reference_price == 0.61
    assert market.polymarket_condition_id == "0xcondition"


async def test_list_market_groups_decodes_core_membership(monkeypatch):
    from sybil_client import SybilClient

    client = SybilClient("http://example.invalid")

    async def fake_request(method, path, **kwargs):
        assert method == "GET"
        assert path == "/v1/markets/groups"
        return [{"group_id": 7, "name": "Winner", "market_ids": [11, 12, 13]}]

    monkeypatch.setattr(client, "_request", fake_request)
    groups = await client.list_market_groups()

    assert [(group.id, group.name, group.market_ids) for group in groups] == [
        (7, "Winner", (11, 12, 13))
    ]
