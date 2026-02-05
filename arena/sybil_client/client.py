"""Sybil API client."""

import json
from collections.abc import AsyncIterator
from typing import Any

import httpx

from .types import (
    NANOS_PER_DOLLAR,
    Account,
    Block,
    BuyNo,
    BuyYes,
    Fill,
    Market,
    OrderSpec,
    Position,
    SellNo,
    SellYes,
)


class SybilClientError(Exception):
    """Error from Sybil API."""

    def __init__(self, status_code: int, message: str):
        self.status_code = status_code
        self.message = message
        super().__init__(f"HTTP {status_code}: {message}")


class SybilClient:
    """Async client for Sybil API."""

    def __init__(self, base_url: str = "http://localhost:3000"):
        self.base_url = base_url.rstrip("/")
        self._client: httpx.AsyncClient | None = None

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

    async def _request(self, method: str, path: str, **kwargs: Any) -> dict[str, Any]:
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

    def _parse_account(self, data: dict[str, Any]) -> Account:
        positions = [
            Position(p["market_id"], p["outcome"], p["quantity"]) for p in data.get("positions", [])
        ]
        return Account(data["account_id"], data["balance_nanos"], positions)

    # === Markets ===

    async def list_markets(self) -> list[Market]:
        """List all markets."""
        data = await self._request("GET", "/v1/markets")
        return [self._parse_market(m) for m in data]

    async def get_market(self, market_id: int) -> Market:
        """Get market details."""
        data = await self._request("GET", f"/v1/markets/{market_id}")
        return self._parse_market(data)

    async def create_market(self, name: str) -> Market:
        """Create a new market (dev mode only)."""
        data = await self._request("POST", "/v1/markets", json={"name": name})
        return self._parse_market(data)

    async def get_prices(self) -> dict[int, tuple[int, int]]:
        """Get clearing prices for all markets."""
        data = await self._request("GET", "/v1/markets/prices")
        return {
            int(market_id): (p["yes_price_nanos"], p["no_price_nanos"])
            for market_id, p in data.items()
        }

    async def resolve_market(self, market_id: int, payout_nanos: int) -> None:
        """Resolve a market (dev mode only)."""
        await self._request(
            "POST", f"/v1/markets/{market_id}/resolve", json={"payout_nanos": payout_nanos}
        )

    def _parse_market(self, data: dict[str, Any]) -> Market:
        return Market(
            data["market_id"],
            data["name"],
            data.get("yes_price_nanos", 0),
            data.get("no_price_nanos", 0),
            data.get("status", "Active"),
        )

    # === Orders ===

    async def submit_orders(self, account_id: int, orders: list[OrderSpec]) -> bool:
        """Submit orders for an account."""
        order_specs = [self._order_to_json(o) for o in orders]
        data = await self._request(
            "POST", "/v1/orders", json={"account_id": account_id, "orders": order_specs}
        )
        return data.get("accepted", False)

    async def buy_yes(
        self, account_id: int, market_id: int, price: float, quantity: int
    ) -> bool:
        """Submit a buy YES order."""
        return await self.submit_orders(
            account_id, [BuyYes.at_price(market_id, price, quantity)]
        )

    async def buy_no(
        self, account_id: int, market_id: int, price: float, quantity: int
    ) -> bool:
        """Submit a buy NO order."""
        return await self.submit_orders(
            account_id, [BuyNo.at_price(market_id, price, quantity)]
        )

    async def sell_yes(
        self, account_id: int, market_id: int, price: float, quantity: int
    ) -> bool:
        """Submit a sell YES order."""
        return await self.submit_orders(
            account_id, [SellYes.at_price(market_id, price, quantity)]
        )

    async def sell_no(
        self, account_id: int, market_id: int, price: float, quantity: int
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
                "quantity": order.quantity,
            }
        elif isinstance(order, BuyNo):
            return {
                "type": "BuyNo",
                "market_id": order.market_id,
                "limit_price_nanos": order.limit_price_nanos,
                "quantity": order.quantity,
            }
        elif isinstance(order, SellYes):
            return {
                "type": "SellYes",
                "market_id": order.market_id,
                "limit_price_nanos": order.limit_price_nanos,
                "quantity": order.quantity,
            }
        elif isinstance(order, SellNo):
            return {
                "type": "SellNo",
                "market_id": order.market_id,
                "limit_price_nanos": order.limit_price_nanos,
                "quantity": order.quantity,
            }
        else:
            raise ValueError(f"Unknown order type: {type(order)}")

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
        """Stream new blocks via SSE."""
        async with self.client.stream("GET", "/v1/blocks/stream") as response:
            async for line in response.aiter_lines():
                if line.startswith("data:"):
                    data = json.loads(line[5:].strip())
                    yield self._parse_block(data)

    def _parse_block(self, data: dict[str, Any]) -> Block:
        fills = [
            Fill(f["order_id"], f["fill_qty"], f["fill_price_nanos"])
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
