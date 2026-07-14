"""Tests for the generated OpenAPI SDK and its thin ergonomic layer.

Complements ``test_client.py`` (which covers the hand-written dataclasses and
order helpers) by pinning:

  * the vendored ``sybil_client._generated`` package imports cleanly, and
  * the thin layer's decode path round-trips through the generated ``attrs``
    models with correct share-unit / nanodollar conversions.
"""

import asyncio
import importlib

from sybil_client import Portfolio, SybilClient
from sybil_client.types import NANOS_PER_DOLLAR, SHARE_SCALE, quantity_units_to_shares


class TestGeneratedPackageImports:
    """The vendored generated client must import without error."""

    def test_client_classes_import(self):
        from sybil_client._generated import AuthenticatedClient, Client

        assert Client is not None
        assert AuthenticatedClient is not None

    def test_models_and_types_import(self):
        models = importlib.import_module("sybil_client._generated.models")
        exported = [n for n in dir(models) if not n.startswith("_")]
        # A healthy generation exports a model per request/response schema.
        assert len(exported) > 50
        # Sentinels used by the thin layer to detect absent optional fields.
        from sybil_client._generated.types import UNSET, Unset

        assert isinstance(UNSET, Unset)

    def test_route_module_imports(self):
        # One representative operation module from the generated API package.
        # The utoipa document currently emits no operation tags, so
        # openapi-python-client places routes in its standard `default` group.
        list_markets = importlib.import_module(
            "sybil_client._generated.api.default.list_markets"
        )
        assert hasattr(list_markets, "sync_detailed") or hasattr(list_markets, "asyncio_detailed")


class TestGeneratedModelRoundTrip:
    """Generated attrs models must survive from_dict -> to_dict unchanged."""

    def test_position_value_round_trip(self):
        from sybil_client._generated.models.position_value_response import (
            PositionValueResponse,
        )

        payload = {
            "market_id": 7,
            "outcome": "YES",
            "quantity": 3_000,  # share-units
            "current_price_nanos": 550_000_000,
            "value_nanos": 1_650_000_000,
        }
        model = PositionValueResponse.from_dict(payload)
        assert model.market_id == 7
        assert model.to_dict() == payload


def _portfolio_payload() -> dict:
    return {
        "account_id": 42,
        "balance_nanos": 100 * NANOS_PER_DOLLAR,
        "available_balance_nanos": 100 * NANOS_PER_DOLLAR,
        "reserved_balance_nanos": 0,
        "total_deposited_nanos": 100 * NANOS_PER_DOLLAR,
        "positions": [
            {
                "market_id": 7,
                "outcome": "YES",
                "quantity": 3_000,  # 3.0 shares in share-units
                "current_price_nanos": 550_000_000,
                "value_nanos": 1_650_000_000,
            }
        ],
        "total_position_value_nanos": 1_650_000_000,
        "portfolio_value_nanos": 101_650_000_000,
        "pnl_nanos": 1_650_000_000,
    }


def test_get_portfolio_decodes_via_generated_model(monkeypatch):
    """Mocked round-trip: the thin layer decodes an API response through the
    generated ``PortfolioResponse`` model and applies unit conversions."""
    client = SybilClient("http://example.invalid")

    async def fake_request(method, path, **kwargs):
        assert method == "GET"
        assert path == "/v1/accounts/42/portfolio"
        return _portfolio_payload()

    monkeypatch.setattr(client, "_request", fake_request)

    portfolio = asyncio.run(client.get_portfolio(42))

    assert isinstance(portfolio, Portfolio)
    assert portfolio.account_id == 42
    # nanodollar -> display dollars
    assert portfolio.balance_dollars == 100.0
    assert portfolio.pnl_dollars == 1.65
    # share-units -> shares at the boundary
    assert len(portfolio.positions) == 1
    assert portfolio.positions[0].quantity == quantity_units_to_shares(3_000) == 3.0
    assert portfolio.positions[0].market_id == 7


class TestUnitConversionSymmetry:
    def test_share_units_symmetry(self):
        assert SHARE_SCALE == 1_000
        for shares in (1, 3, 10, 250):
            units = shares * SHARE_SCALE
            assert quantity_units_to_shares(units) == float(shares)

    def test_nanodollar_helpers(self):
        assert SybilClient.dollars_to_nanos(1.0) == NANOS_PER_DOLLAR
        assert SybilClient.nanos_to_dollars(NANOS_PER_DOLLAR) == 1.0
        assert SybilClient.nanos_to_dollars(SybilClient.dollars_to_nanos(0.55)) == 0.55
