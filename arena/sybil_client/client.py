"""Sybil API client."""

import asyncio
import json
import logging
import os
from collections.abc import AsyncIterator
from typing import Any

import httpx
from websockets.asyncio.client import connect

from ._generated.models import (
    AccountFillResponse,
    AccountResponse,
    PortfolioResponse,
    PriceHistoryResponse,
)
from ._generated.types import Unset
from .types import (
    NANOS_PER_DOLLAR,
    SHARE_SCALE,
    Account,
    AccountFill,
    Block,
    BlockStreamBlockEvent,
    BlockStreamEvent,
    BlockStreamReplayCompleteEvent,
    BuyNo,
    BuyYes,
    Fill,
    Market,
    MarketGroup,
    OrderAdmissionPolicy,
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


def _nanos_int(value: Any) -> int:
    """Parse an exact decimal-string nanos value, with legacy integer support."""
    if isinstance(value, bool) or not isinstance(value, (str, int)):
        raise TypeError(f"expected decimal-string nanos, got {value!r}")
    return int(value)


def _optional_nanos_int(value: Any) -> int | None:
    value = _opt(value)
    return None if value is None else _nanos_int(value)


class SybilClientError(Exception):
    """Error from Sybil API."""

    def __init__(self, status_code: int, message: str):
        self.status_code = status_code
        self.message = message
        super().__init__(f"HTTP {status_code}: {message}")


class BlockStreamGapError(SybilClientError):
    """The requested block-stream history is no longer retained."""


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

    async def get_order_admission_policy(self) -> OrderAdmissionPolicy:
        """Fetch public constraints used to construct admissible orders."""
        data = await self._request("GET", "/v1/orders/policy")
        policy = OrderAdmissionPolicy(
            min_order_notional_nanos=_nanos_int(data["min_order_notional_nanos"]),
            share_scale=int(data["share_scale"]),
        )
        if policy.share_scale != SHARE_SCALE:
            raise ValueError(
                "server/client share scale mismatch: "
                f"server={policy.share_scale} client={SHARE_SCALE}"
            )
        if policy.min_order_notional_nanos < 0:
            raise ValueError("minimum order notional cannot be negative")
        return policy

    # === Accounts ===

    async def create_account(self, initial_balance_nanos: int = 0) -> Account:
        """Create a new account (dev mode only)."""
        data = await self._request(
            "POST", "/v1/accounts", json={"initial_balance_nanos": str(initial_balance_nanos)}
        )
        return self._parse_account(data)

    async def get_account(self, account_id: int) -> Account:
        """Get account details."""
        data = await self._request("GET", f"/v1/accounts/{account_id}")
        return self._parse_account(data)

    async def fund_account(self, account_id: int, amount_nanos: int) -> Account:
        """Add funds to account (dev mode only)."""
        data = await self._request(
            "POST", f"/v1/accounts/{account_id}/fund", json={"amount_nanos": str(amount_nanos)}
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
                current_price_nanos=_nanos_int(p.current_price_nanos),
                value_nanos=_nanos_int(p.value_nanos),
            )
            for p in model.positions
        ]
        return Portfolio(
            account_id=model.account_id,
            balance_nanos=_nanos_int(model.balance_nanos),
            total_deposited_nanos=_nanos_int(model.total_deposited_nanos),
            positions=positions,
            total_position_value_nanos=_nanos_int(model.total_position_value_nanos),
            portfolio_value_nanos=_nanos_int(model.portfolio_value_nanos),
            pnl_nanos=_nanos_int(model.pnl_nanos),
        )

    async def get_account_fills(
        self,
        account_id: int,
        market_id: int | None = None,
        limit: int = 100,
        after: str | None = None,
    ) -> list[AccountFill]:
        """Get fill history for an account."""
        params: dict[str, Any] = {"limit": limit}
        if after is not None:
            params["after"] = after
        if market_id is not None:
            params["market_id"] = market_id
        data = await self._request("GET", f"/v1/accounts/{account_id}/fills", params=params)
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
                    fill_price_nanos=_nanos_int(f.fill_price_nanos),
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
        data = await self._request("GET", f"/v1/accounts/{account_id}/orders")
        return [
            PendingOrder(
                order_id=o["order_id"],
                account_id=o["account_id"],
                market_id=o["market_id"],
                side=o["side"],
                limit_price_nanos=_nanos_int(o["limit_price_nanos"]),
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
        return Account(model.account_id, _nanos_int(model.balance_nanos), positions)

    # === Markets ===

    async def list_markets(self) -> list[Market]:
        """List all markets."""
        data = await self._request("GET", "/v1/markets")
        return [self._parse_market(m) for m in data]

    async def list_market_groups(self) -> list[MarketGroup]:
        """List core mutually-exclusive market groups."""
        data = await self._request("GET", "/v1/markets/groups")
        return [
            MarketGroup(
                id=int(group["group_id"]),
                name=str(group["name"]),
                market_ids=tuple(int(market_id) for market_id in group["market_ids"]),
            )
            for group in data
        ]

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
            int(market_id): (
                _nanos_int(p["yes_price_nanos"]),
                _nanos_int(p["no_price_nanos"]),
            )
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
        data = await self._request("GET", f"/v1/markets/{market_id}/prices/history", params=params)
        model = PriceHistoryResponse.from_dict(data)
        return [
            PricePoint(
                height=p.height,
                timestamp_ms=p.timestamp_ms,
                yes_price_nanos=_nanos_int(p.yes_price_nanos),
                no_price_nanos=_nanos_int(p.no_price_nanos),
                volume_nanos=_nanos_int(p.volume_nanos),
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
        min_volume_nanos: int | None = None,
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
        if min_volume_nanos is not None:
            params["min_volume_nanos"] = str(min_volume_nanos)
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
            "POST", f"/v1/markets/{market_id}/resolve", json={"payout_nanos": str(payout_nanos)}
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
            yes_price_nanos=_nanos_int(data.get("yes_price_nanos") or 0),
            no_price_nanos=_nanos_int(data.get("no_price_nanos") or 0),
            status=data.get("status", "Active"),
            reference_price_nanos=_optional_nanos_int(data.get("reference_price_nanos")),
            reference_price_expires_at_ms=data.get("reference_price_expires_at_ms"),
            polymarket_condition_id=data.get("polymarket_condition_id"),
            description=data.get("description", ""),
            category=data.get("category", ""),
            tags=data.get("tags", []),
            resolution_criteria=data.get("resolution_criteria", ""),
            expiry_timestamp_ms=data.get("expiry_timestamp_ms", 0),
            created_at_ms=data.get("created_at_ms", 0),
            volume_nanos=_nanos_int(data.get("volume_nanos", 0)),
            closed=data.get("closed", False),
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
            payload["mm_budget_nanos"] = str(mm_budget_nanos)
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
        return await self.submit_orders(account_id, [BuyYes.at_price(market_id, price, quantity)])

    async def buy_no(
        self, account_id: int, market_id: int, price: float, quantity: int | float
    ) -> bool:
        """Submit a buy NO order."""
        return await self.submit_orders(account_id, [BuyNo.at_price(market_id, price, quantity)])

    async def sell_yes(
        self, account_id: int, market_id: int, price: float, quantity: int | float
    ) -> bool:
        """Submit a sell YES order."""
        return await self.submit_orders(account_id, [SellYes.at_price(market_id, price, quantity)])

    async def sell_no(
        self, account_id: int, market_id: int, price: float, quantity: int | float
    ) -> bool:
        """Submit a sell NO order."""
        return await self.submit_orders(account_id, [SellNo.at_price(market_id, price, quantity)])

    def _order_to_json(self, order: OrderSpec) -> dict[str, Any]:
        if isinstance(order, BuyYes):
            return {
                "type": "BuyYes",
                "market_id": order.market_id,
                "limit_price_nanos": str(order.limit_price_nanos),
                "quantity": shares_to_quantity_units(order.quantity),
            }
        elif isinstance(order, BuyNo):
            return {
                "type": "BuyNo",
                "market_id": order.market_id,
                "limit_price_nanos": str(order.limit_price_nanos),
                "quantity": shares_to_quantity_units(order.quantity),
            }
        elif isinstance(order, SellYes):
            return {
                "type": "SellYes",
                "market_id": order.market_id,
                "limit_price_nanos": str(order.limit_price_nanos),
                "quantity": shares_to_quantity_units(order.quantity),
            }
        elif isinstance(order, SellNo):
            return {
                "type": "SellNo",
                "market_id": order.market_id,
                "limit_price_nanos": str(order.limit_price_nanos),
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

    async def stream_block_events(
        self, from_block: int | None = None
    ) -> AsyncIterator[BlockStreamEvent]:
        """Stream blocks while preserving the replay-to-live boundary.

        After delivering a block, reconnects request the next height so a
        transient disconnect cannot silently skip blocks. A retention gap is
        terminal because the caller must perform a cold resync before it can
        safely continue. Replayed blocks are explicitly marked so side-effecting
        consumers can rebuild state without submitting historical work.
        """
        backoff = self._stream_reconnect_base_s
        last_seen_height = from_block - 1 if from_block is not None else None
        ws_base_url = self.base_url.replace("https://", "wss://", 1).replace("http://", "ws://", 1)
        while True:
            next_height = last_seen_height + 1 if last_seen_height is not None else None
            url = f"{ws_base_url}/v2/blocks/ws"
            if next_height is not None:
                url = f"{url}?from_block={next_height}"
            try:
                replaying = next_height is not None
                async with connect(
                    url,
                    ping_interval=20,
                    ping_timeout=20,
                    close_timeout=5,
                ) as websocket:
                    async for raw_message in websocket:
                        if isinstance(raw_message, bytes):
                            raw_message = raw_message.decode("utf-8")
                        message = json.loads(raw_message)
                        if message.get("v") != 2:
                            log.warning("Ignoring unsupported block stream version: %r", message)
                            continue

                        message_type = message.get("type")
                        if message_type == "block":
                            block = self._parse_block(message["data"])
                            if last_seen_height is not None and block.height <= last_seen_height:
                                continue
                            last_seen_height = block.height
                            backoff = self._stream_reconnect_base_s
                            yield BlockStreamBlockEvent(block=block, replayed=replaying)
                        elif message_type == "replay_complete":
                            replaying = False
                            yield BlockStreamReplayCompleteEvent(
                                up_to_height=int(message["up_to_height"])
                            )
                        elif message_type == "lagged":
                            server_height = message.get("last_sent_height")
                            if server_height is not None:
                                last_seen_height = max(last_seen_height or 0, int(server_height))
                            log.warning(
                                "Block stream lagged by %s messages; resuming after height %s",
                                message.get("skipped"),
                                last_seen_height,
                            )
                            break
                        elif message_type == "retention_gap":
                            raise BlockStreamGapError(
                                410,
                                "block replay starts before retained history "
                                f"(requested={message.get('requested_height')}, "
                                f"retention_min={message.get('retention_min_height')}, "
                                f"head={message.get('head_height')})",
                            )
            except asyncio.CancelledError:
                raise
            except BlockStreamGapError:
                raise
            except Exception as e:
                log.warning("Block stream error (%s); reconnecting in %.1fs", e, backoff)
            else:
                log.info("Block stream closed by server; reconnecting in %.1fs", backoff)

            await asyncio.sleep(backoff)
            backoff = min(backoff * 2, self._stream_reconnect_max_s)

    async def stream_blocks(self, from_block: int | None = None) -> AsyncIterator[Block]:
        """Stream all committed blocks, including blocks replayed after reconnect."""
        async for event in self.stream_block_events(from_block=from_block):
            if isinstance(event, BlockStreamBlockEvent):
                yield event.block

    async def stream_live_blocks(self, from_block: int | None = None) -> AsyncIterator[Block]:
        """Stream only blocks known to be live, never replayed history."""
        async for event in self.stream_block_events(from_block=from_block):
            if isinstance(event, BlockStreamBlockEvent) and not event.replayed:
                yield event.block

    def _parse_block(self, data: dict[str, Any]) -> Block:
        fills = [
            Fill(
                f["order_id"],
                quantity_units_to_shares(f["fill_qty"]),
                _nanos_int(f["fill_price_nanos"]),
            )
            for f in data.get("fills", [])
        ]
        # clearing_prices_nanos format: {"market_id": [yes_nanos, no_nanos]}
        prices = {}
        for k, v in data.get("clearing_prices_nanos", {}).items():
            if isinstance(v, list) and len(v) >= 2:
                prices[int(k)] = (_nanos_int(v[0]), _nanos_int(v[1]))
            elif isinstance(v, dict):
                prices[int(k)] = (
                    _nanos_int(v.get("yes_price_nanos", 0)),
                    _nanos_int(v.get("no_price_nanos", 0)),
                )
        return Block(
            height=data["height"],
            parent_hash=data.get("parent_hash", ""),
            state_root=data.get("state_root", ""),
            fills=fills,
            clearing_prices=prices,
            total_welfare=_nanos_int(data.get("total_welfare_nanos", data.get("total_welfare", 0))),
            total_volume=_nanos_int(data.get("total_volume_nanos", data.get("total_volume", 0))),
            orders_filled=data.get("orders_filled", 0),
        )

    # === Utilities ===

    @staticmethod
    def dollars_to_nanos(dollars: float) -> int:
        return int(dollars * NANOS_PER_DOLLAR)

    @staticmethod
    def nanos_to_dollars(nanos: int) -> float:
        return nanos / NANOS_PER_DOLLAR
