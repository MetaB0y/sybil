"""Sybil API client."""

import asyncio
import json
import logging
import os
from collections.abc import AsyncIterator
from typing import Any

import httpx

from ._generated.models import (
    AccountFillResponse,
    AccountResponse,
    PortfolioResponse,
    PriceHistoryResponse,
)
from ._generated.types import Unset
from .types import (
    NANOS_PER_DOLLAR,
    Account,
    AccountFill,
    Block,
    BuyNo,
    BuyYes,
    Fill,
    Market,
    OrderSpec,
    PendingOrder,
    Portfolio,
    Position,
    PositionDelta,
    PositionValue,
    PricePoint,
    SellNo,
    SellYes,
    TimeInForce,
    quantity_units_to_shares,
    shares_to_quantity_units,
)

log = logging.getLogger(__name__)


def _opt(value: Any) -> Any:
    """Unwrap a generated-model optional attribute: the ``UNSET`` sentinel -> ``None``.

    The vendored ``sybil_client._generated`` package (regenerated from the API's
    OpenAPI spec via ``just arena-sdk-regen``) represents absent optional fields
    with an ``Unset`` sentinel rather than ``None``. This collapses that back to
    ``None`` so callers can apply their own defaults.
    """
    return None if isinstance(value, Unset) else value


class SybilClientError(Exception):
    """Error from Sybil API."""

    def __init__(self, status_code: int, message: str):
        self.status_code = status_code
        self.message = message
        super().__init__(f"HTTP {status_code}: {message}")


class SybilClient:
    """Async client for Sybil API."""

    def __init__(self, base_url: str = "http://localhost:3000", service_token: str | None = None):
        self.base_url = base_url.rstrip("/")
        self.service_token = service_token or os.environ.get("SYBIL_SERVICE_TOKEN", "")
        self._client: httpx.AsyncClient | None = None
        # Reconnect backoff bounds for stream_blocks (AR-3). Tests override.
        self._stream_reconnect_base_s: float = 1.0
        self._stream_reconnect_max_s: float = 30.0

    async def __aenter__(self) -> "SybilClient":
        self._client = httpx.AsyncClient(base_url=self.base_url, timeout=30.0)
        return self

    async def __aexit__(self, *args: Any) -> None:
        if self._client:
            await self._client.aclose()

    @property
    def client(self) -> httpx.AsyncClient:
        if self._client is None:
            raise RuntimeError("Client not initialized. Use 'async with SybilClient():'")
        return self._client

    async def _request(self, method: str, path: str, **kwargs: Any) -> Any:
        if self.service_token:
            headers = dict(kwargs.pop("headers", {}) or {})
            headers.setdefault("authorization", f"Bearer {self.service_token}")
            kwargs["headers"] = headers
        response = await self.client.request(method, path, **kwargs)
        if response.status_code >= 400:
            raise SybilClientError(response.status_code, response.text)
        return response.json()

    # === Health ===

    async def health(self) -> dict[str, Any]:
        """Check server health."""
        return await self._request("GET", "/v1/health")

    async def state_root(self) -> str:
        """Get current state root hash."""
        data = await self._request("GET", "/v1/state-root")
        return data["state_root"]

    # === Accounts ===

    async def create_account(self, initial_balance_nanos: int = 0) -> Account:
        """Create a new account (dev mode only)."""
        data = await self._request(
            "POST", "/v1/accounts", json={"initial_balance_nanos": initial_balance_nanos}
        )
        return self._parse_account(data)

    async def get_account(self, account_id: int) -> Account:
        """Get account details."""
        data = await self._request("GET", f"/v1/accounts/{account_id}")
        return self._parse_account(data)

    async def fund_account(self, account_id: int, amount_nanos: int) -> Account:
        """Add funds to account (dev mode only)."""
        data = await self._request(
            "POST", f"/v1/accounts/{account_id}/fund", json={"amount_nanos": amount_nanos}
        )
        return self._parse_account(data)

    async def get_portfolio(self, account_id: int) -> Portfolio:
        """Get portfolio summary with valued positions and PnL."""
        data = await self._request("GET", f"/v1/accounts/{account_id}/portfolio")
        model = PortfolioResponse.from_dict(data)
        positions = [
            PositionValue(
                market_id=p.market_id,
                outcome=p.outcome,
                quantity=quantity_units_to_shares(p.quantity),
                current_price_nanos=p.current_price_nanos,
                value_nanos=p.value_nanos,
            )
            for p in model.positions
        ]
        return Portfolio(
            account_id=model.account_id,
            balance_nanos=model.balance_nanos,
            total_deposited_nanos=model.total_deposited_nanos,
            positions=positions,
            total_position_value_nanos=model.total_position_value_nanos,
            portfolio_value_nanos=model.portfolio_value_nanos,
            pnl_nanos=model.pnl_nanos,
        )

    async def get_account_fills(
        self,
        account_id: int,
        market_id: int | None = None,
        limit: int = 100,
        offset: int | None = None,
        after: str | None = None,
    ) -> list[AccountFill]:
        """Get fill history for an account."""
        params: dict[str, Any] = {"limit": limit}
        if after is not None:
            params["after"] = after
        elif offset is not None:
            params["offset"] = offset
        if market_id is not None:
            params["market_id"] = market_id
        data = await self._request(
            "GET", f"/v1/accounts/{account_id}/fills", params=params
        )
        if data.get("cursor_gap"):
            raise SybilClientError(
                410,
                "fill cursor predates retained history; reconcile from canonical portfolio state",
            )
        fills: list[AccountFill] = []
        for item in data["fills"]:
            f = AccountFillResponse.from_dict(item)
            cursor = f.additional_properties.get(
                "cursor",
                item.get("cursor", f"{f.block_height}.{f.order_id}"),
            )
            fills.append(
                AccountFill(
                    cursor=str(cursor),
                    order_id=f.order_id,
                    fill_qty=quantity_units_to_shares(f.fill_qty),
                    fill_price_nanos=f.fill_price_nanos,
                    block_height=f.block_height,
                    timestamp_ms=f.timestamp_ms,
                    position_deltas=[
                        PositionDelta(
                            market_id=d.market_id,
                            outcome=d.outcome,
                            delta=quantity_units_to_shares(d.delta),
                        )
                        for d in f.position_deltas
                    ],
                )
            )
        return fills

    async def get_pending_orders(self, account_id: int) -> list[PendingOrder]:
        """Get pending orders for an account."""
        # Hand-written: the generated ``PendingOrderResponse`` marks
        # ``expires_at_block`` as required, but the server omits it for
        # non-GTD orders. Keep the lenient ``.get`` so GTC/IOC orders parse.
        data = await self._request("GET", f"/v1/accounts/{account_id}/orders")
        return [
            PendingOrder(
                order_id=o["order_id"],
                account_id=o["account_id"],
                market_id=o["market_id"],
                side=o["side"],
                limit_price_nanos=o["limit_price_nanos"],
                remaining_quantity=quantity_units_to_shares(o["remaining_quantity"]),
                created_at_block=o["created_at_block"],
                expires_at_block=o.get("expires_at_block"),
                original_quantity=quantity_units_to_shares(
                    o.get("original_quantity", o["remaining_quantity"])
                ),
            )
            for o in data
        ]

    def _parse_account(self, data: dict[str, Any]) -> Account:
        model = AccountResponse.from_dict(data)
        positions = [
            Position(p.market_id, p.outcome, quantity_units_to_shares(p.quantity))
            for p in (_opt(model.positions) or [])
        ]
        return Account(model.account_id, model.balance_nanos, positions)

    # === Markets ===

    async def list_markets(self) -> list[Market]:
        """List all markets."""
        data = await self._request("GET", "/v1/markets")
        return [self._parse_market(m) for m in data]

    async def get_market(self, market_id: int) -> Market:
        """Get market details."""
        data = await self._request("GET", f"/v1/markets/{market_id}")
        return self._parse_market(data)

    async def create_market(
        self,
        name: str,
        *,
        description: str | None = None,
        category: str | None = None,
        tags: list[str] | None = None,
        resolution_criteria: str | None = None,
        expiry_timestamp_ms: int | None = None,
    ) -> Market:
        """Create a new market (dev mode only)."""
        payload: dict[str, Any] = {"name": name}
        if description is not None:
            payload["description"] = description
        if category is not None:
            payload["category"] = category
        if tags is not None:
            payload["tags"] = tags
        if resolution_criteria is not None:
            payload["resolution_criteria"] = resolution_criteria
        if expiry_timestamp_ms is not None:
            payload["expiry_timestamp_ms"] = expiry_timestamp_ms
        data = await self._request("POST", "/v1/markets", json=payload)
        return self._parse_market(data)

    async def get_prices(self) -> dict[int, tuple[int, int]]:
        """Get clearing prices for all markets."""
        data = await self._request("GET", "/v1/markets/prices")
        # Response is wrapped: {"prices": {"0": {...}, "1": {...}}}
        prices_map = data.get("prices", data) if isinstance(data, dict) else data
        return {
            int(market_id): (p["yes_price_nanos"], p["no_price_nanos"])
            for market_id, p in prices_map.items()
        }

    async def get_price_history(
        self,
        market_id: int,
        from_ms: int | None = None,
        to_ms: int | None = None,
    ) -> list[PricePoint]:
        """Get price history for a market."""
        params: dict[str, Any] = {}
        if from_ms is not None:
            params["from_ms"] = from_ms
        if to_ms is not None:
            params["to_ms"] = to_ms
        data = await self._request(
            "GET", f"/v1/markets/{market_id}/prices/history", params=params
        )
        model = PriceHistoryResponse.from_dict(data)
        return [
            PricePoint(
                height=p.height,
                timestamp_ms=p.timestamp_ms,
                yes_price_nanos=p.yes_price_nanos,
                no_price_nanos=p.no_price_nanos,
                volume_nanos=p.volume_nanos,
            )
            for p in model.points
        ]

    async def search_markets(
        self,
        *,
        q: str | None = None,
        tags: list[str] | None = None,
        category: str | None = None,
        status: str | None = None,
        min_volume: int | None = None,
        sort: str | None = None,
        limit: int | None = None,
        offset: int | None = None,
    ) -> list[Market]:
        """Search markets by various criteria."""
        params: dict[str, Any] = {}
        if q is not None:
            params["q"] = q
        if tags is not None:
            params["tags"] = ",".join(tags)
        if category is not None:
            params["category"] = category
        if status is not None:
            params["status"] = status
        if min_volume is not None:
            params["min_volume"] = min_volume
        if sort is not None:
            params["sort"] = sort
        if limit is not None:
            params["limit"] = limit
        if offset is not None:
            params["offset"] = offset
        data = await self._request("GET", "/v1/markets/search", params=params)
        return [self._parse_market(m) for m in data]

    async def resolve_market(self, market_id: int, payout_nanos: int) -> None:
        """Resolve a market (dev mode only)."""
        await self._request(
            "POST", f"/v1/markets/{market_id}/resolve", json={"payout_nanos": payout_nanos}
        )

    def _parse_market(self, data: dict[str, Any]) -> Market:
        # Hand-written: this parser is shared between the full ``MarketResponse``
        # (list/get/search) and the partial ``CreateMarketResponse`` (create,
        # which carries only market_id + name). Delegating to
        # ``MarketResponse.from_dict`` would reject the create-market shape on its
        # required ``status`` field, so we keep the lenient ``.get`` defaults.
        return Market(
            id=data["market_id"],
            name=data["name"],
            yes_price_nanos=data.get("yes_price_nanos") or 0,
            no_price_nanos=data.get("no_price_nanos") or 0,
            status=data.get("status", "Active"),
            reference_price_nanos=data.get("reference_price_nanos"),
            reference_price_expires_at_ms=data.get("reference_price_expires_at_ms"),
            polymarket_condition_id=data.get("polymarket_condition_id"),
            description=data.get("description", ""),
            category=data.get("category", ""),
            tags=data.get("tags", []),
            resolution_criteria=data.get("resolution_criteria", ""),
            expiry_timestamp_ms=data.get("expiry_timestamp_ms", 0),
            created_at_ms=data.get("created_at_ms", 0),
            volume_nanos=data.get("volume_nanos", 0),
        )

    # === Orders ===

    async def submit_orders(
        self,
        account_id: int,
        orders: list[OrderSpec],
        mm_budget_nanos: int | None = None,
        time_in_force: TimeInForce | None = None,
        expires_at_block: int | None = None,
    ) -> bool:
        """Submit orders for an account.

        Args:
            account_id: Account submitting the orders.
            orders: List of order specifications.
            mm_budget_nanos: If set, treat as market maker orders with flash
                liquidity. The value is the MM's total capital budget in nanos.
                MM orders skip per-order balance validation; the solver enforces
                the portfolio-level budget constraint at clearing time.
            time_in_force: Optional order lifetime policy ("GTC", "IOC", or "GTD").
                If omitted, the API default is used.
            expires_at_block: Required by the API for GTD submissions.
        """
        order_specs = [self._order_to_json(o) for o in orders]
        payload: dict[str, Any] = {"account_id": account_id, "orders": order_specs}
        if mm_budget_nanos is not None:
            payload["mm_budget_nanos"] = mm_budget_nanos
        if time_in_force is not None:
            payload["time_in_force"] = time_in_force
        if expires_at_block is not None:
            payload["expires_at_block"] = expires_at_block
        data = await self._request("POST", "/v1/orders", json=payload)
        return data.get("accepted", False)

    async def buy_yes(
        self, account_id: int, market_id: int, price: float, quantity: int | float
    ) -> bool:
        """Submit a buy YES order."""
        return await self.submit_orders(
            account_id, [BuyYes.at_price(market_id, price, quantity)]
        )

    async def buy_no(
        self, account_id: int, market_id: int, price: float, quantity: int | float
    ) -> bool:
        """Submit a buy NO order."""
        return await self.submit_orders(
            account_id, [BuyNo.at_price(market_id, price, quantity)]
        )

    async def sell_yes(
        self, account_id: int, market_id: int, price: float, quantity: int | float
    ) -> bool:
        """Submit a sell YES order."""
        return await self.submit_orders(
            account_id, [SellYes.at_price(market_id, price, quantity)]
        )

    async def sell_no(
        self, account_id: int, market_id: int, price: float, quantity: int | float
    ) -> bool:
        """Submit a sell NO order."""
        return await self.submit_orders(
            account_id, [SellNo.at_price(market_id, price, quantity)]
        )

    def _order_to_json(self, order: OrderSpec) -> dict[str, Any]:
        if isinstance(order, BuyYes):
            return {
                "type": "BuyYes",
                "market_id": order.market_id,
                "limit_price_nanos": order.limit_price_nanos,
                "quantity": shares_to_quantity_units(order.quantity),
            }
        elif isinstance(order, BuyNo):
            return {
                "type": "BuyNo",
                "market_id": order.market_id,
                "limit_price_nanos": order.limit_price_nanos,
                "quantity": shares_to_quantity_units(order.quantity),
            }
        elif isinstance(order, SellYes):
            return {
                "type": "SellYes",
                "market_id": order.market_id,
                "limit_price_nanos": order.limit_price_nanos,
                "quantity": shares_to_quantity_units(order.quantity),
            }
        elif isinstance(order, SellNo):
            return {
                "type": "SellNo",
                "market_id": order.market_id,
                "limit_price_nanos": order.limit_price_nanos,
                "quantity": shares_to_quantity_units(order.quantity),
            }
        else:
            raise ValueError(f"Unknown order type: {type(order)}")

    # === Simulation Control ===

    async def pause(self) -> None:
        """Pause server block production."""
        await self._request("POST", "/v1/simulation/pause")

    async def resume(self) -> None:
        """Resume server block production."""
        await self._request("POST", "/v1/simulation/resume")

    # === Blocks ===

    async def get_latest_block(self) -> Block:
        """Get the latest block."""
        data = await self._request("GET", "/v1/blocks/latest")
        return self._parse_block(data)

    async def get_block(self, height: int) -> Block:
        """Get block at specific height."""
        data = await self._request("GET", f"/v1/blocks/{height}")
        return self._parse_block(data)

    async def stream_blocks(self) -> AsyncIterator[Block]:
        """Stream new blocks via SSE, reconnecting with backoff on drops.

        AR-3: a transient SSE disconnect (or a clean server-side close) used to
        end the iterator, which tore down the consuming bot. Instead we
        transparently reconnect with exponential backoff so callers see one
        uninterrupted block stream; the backoff resets after any block is
        delivered. ``CancelledError`` (shutdown) still propagates immediately.
        """
        backoff = self._stream_reconnect_base_s
        while True:
            try:
                async with self.client.stream("GET", "/v1/blocks/stream") as response:
                    async for line in response.aiter_lines():
                        if line.startswith("data:"):
                            data = json.loads(line[5:].strip())
                            yield self._parse_block(data)
                            backoff = self._stream_reconnect_base_s
            except asyncio.CancelledError:
                raise
            except Exception as e:
                log.warning("Block stream error (%s); reconnecting in %.1fs", e, backoff)
            else:
                log.info("Block stream closed by server; reconnecting in %.1fs", backoff)

            await asyncio.sleep(backoff)
            backoff = min(backoff * 2, self._stream_reconnect_max_s)

    def _parse_block(self, data: dict[str, Any]) -> Block:
        fills = [
            Fill(
                f["order_id"],
                quantity_units_to_shares(f["fill_qty"]),
                f["fill_price_nanos"],
            )
            for f in data.get("fills", [])
        ]
        # clearing_prices_nanos format: {"market_id": [yes_nanos, no_nanos]}
        prices = {}
        for k, v in data.get("clearing_prices_nanos", {}).items():
            if isinstance(v, list) and len(v) >= 2:
                prices[int(k)] = (v[0], v[1])
            elif isinstance(v, dict):
                prices[int(k)] = (v.get("yes_price_nanos", 0), v.get("no_price_nanos", 0))
        return Block(
            height=data["height"],
            parent_hash=data.get("parent_hash", ""),
            state_root=data.get("state_root", ""),
            fills=fills,
            clearing_prices=prices,
            total_welfare=data.get("total_welfare_nanos", data.get("total_welfare", 0)),
            total_volume=data.get("total_volume_nanos", data.get("total_volume", 0)),
            orders_filled=data.get("orders_filled", 0),
        )

    # === Utilities ===

    @staticmethod
    def dollars_to_nanos(dollars: float) -> int:
        return int(dollars * NANOS_PER_DOLLAR)

    @staticmethod
    def nanos_to_dollars(nanos: int) -> float:
        return nanos / NANOS_PER_DOLLAR
