"""Base agent class for trading bots."""

import logging
from abc import ABC, abstractmethod

from sybil_client import Block, BlockStreamBlockEvent, OrderSpec, SybilClient
from sybil_client.client import SybilClientError
from sybil_client.types import TimeInForce

log = logging.getLogger(__name__)


class BaseAgent(ABC):
    """Abstract base class for trading bots."""

    def __init__(
        self,
        client: SybilClient,
        account_id: int,
        name: str | None = None,
        market_ids: list[int] | None = None,
        max_blocks: int | None = None,
    ):
        self.client = client
        self.account_id = account_id
        self.name = name or self.__class__.__name__
        self.market_ids = set(market_ids) if market_ids else None  # None means all markets
        self.max_blocks = max_blocks  # None = unlimited
        self.positions: dict[tuple[int, str], int] = {}
        self.balance_history: list[float] = []
        self._running = False
        # MM budget constraint (None = regular orders, set to enable flash liquidity)
        self.mm_budget_nanos: int | None = None
        self.time_in_force: TimeInForce | None = None
        self.expires_at_block: int | None = None
        # Order tracking for observability
        self.last_orders: list[OrderSpec] = []
        self.total_orders_submitted: int = 0
        self.on_block_error_count: int = 0
        # Per-block order log: (block_height, orders_submitted)
        self.block_log: list[tuple[int, list[OrderSpec]]] = []
        # Per-block stats from the sequencer (welfare, volume, fills)
        self.block_stats: dict[int, tuple[int, int, int]] = {}  # height -> (welfare, volume, fills)
        # Fill tracking via get_account_fills(after=cursor)
        self._last_fill_cursor: str = "0.0"
        self._fill_history: list = []  # list[AccountFill], available to subclasses

    @abstractmethod
    async def on_block(self, block: Block) -> list[OrderSpec]:
        """Called every block. Return orders to submit.

        Override this method to implement your trading logic.
        """
        pass

    async def on_market_created(self, market_id: int, name: str) -> None:
        """Called when a new market is created. Override if needed."""
        pass

    async def on_fill(self, order_id: int, fill_qty: int, fill_price: float) -> None:
        """Called when one of our orders is filled. Override if needed."""
        pass

    async def on_orders_submitted(self, block: Block, orders: list[OrderSpec]) -> None:
        """Called after a batch of orders is accepted by the API."""
        pass

    async def run(self) -> None:
        """Main loop - stream blocks and react."""
        self._running = True
        blocks_traded = 0
        try:
            async for event in self.client.stream_block_events():
                if not isinstance(event, BlockStreamBlockEvent):
                    continue
                block = event.block
                if not self._running:
                    break

                # Update our state
                await self._update_state(block)

                # Replays repair canonical observations after reconnect but
                # must never call a strategy or submit historical orders.
                if event.replayed:
                    continue

                if await self._has_pending_orders():
                    continue

                # Record block-level stats (welfare, volume, fills)
                self.block_stats[block.height] = (
                    block.total_welfare,
                    block.total_volume,
                    block.orders_filled,
                )

                # Get orders from strategy. Strategy errors are isolated to this
                # block so one bad model response or bot bug cannot kill the task.
                try:
                    orders = await self.on_block(block)
                except Exception:
                    self.on_block_error_count += 1
                    persona = getattr(self, "persona", self.name)
                    log.exception(
                        "Bot on_block failed; continuing: name=%s persona=%s block_height=%s "
                        "on_block_error_count=%d",
                        self.name,
                        persona,
                        block.height,
                        self.on_block_error_count,
                    )
                    continue

                # Log and submit orders
                self.block_log.append((block.height, orders))
                if orders:
                    try:
                        accepted = await self.client.submit_orders(
                            self.account_id,
                            orders,
                            mm_budget_nanos=self.mm_budget_nanos,
                            time_in_force=self.time_in_force,
                            expires_at_block=self.expires_at_block,
                        )
                        if accepted:
                            self.last_orders = orders
                            self.total_orders_submitted += len(orders)
                            await self.on_orders_submitted(block, orders)
                        else:
                            print(f"[{self.name}] Order submission was not accepted")
                    except Exception as e:
                        print(f"[{self.name}] Order submission failed: {e}")
                    blocks_traded += 1
                    if self.max_blocks is not None and blocks_traded >= self.max_blocks:
                        print(f"[{self.name}] Reached max_blocks={self.max_blocks}, stopping.")
                        break

        except Exception as e:
            print(f"[{self.name}] Error in run loop: {e}")
            raise

    def stop(self) -> None:
        """Stop the bot gracefully."""
        self._running = False

    async def _update_state(self, block: Block) -> None:
        """Update positions and balance from account state."""
        try:
            account = await self.client.get_account(self.account_id)
            self._apply_canonical_account(account)

            # Fetch all fills we haven't seen yet.
            page_size = 100
            while True:
                try:
                    new_fills = await self.client.get_account_fills(
                        self.account_id, limit=page_size, after=self._last_fill_cursor
                    )
                except SybilClientError as exc:
                    if exc.status_code != 410:
                        raise
                    await self._reconcile_fill_cursor()
                    break
                if not new_fills:
                    break
                self._fill_history.extend(new_fills)
                for fill in new_fills:
                    await self.on_fill(fill.order_id, fill.fill_qty, fill.fill_price)
                self._last_fill_cursor = new_fills[-1].cursor
                if len(new_fills) < page_size:
                    break

        except Exception as e:
            print(f"[{self.name}] Failed to update state: {e}")

    def _apply_canonical_account(self, account, *, replace_latest_balance: bool = False) -> None:
        """Replace strategy state from the canonical account snapshot."""
        self.positions = {(pos.market_id, pos.outcome): pos.quantity for pos in account.positions}
        if replace_latest_balance and self.balance_history:
            self.balance_history[-1] = account.balance_dollars
        else:
            self.balance_history.append(account.balance_dollars)

    async def _reconcile_fill_cursor(self) -> None:
        """Resume live fill tailing after retained history overtakes our cursor.

        Positions and cash come from the canonical account endpoint, so replaying
        an incomplete retained suffix would be misleading. Advance to the newest
        retained fill (or the current chain tip when no row remains), then refresh
        the canonical snapshot after choosing that boundary. Future fills continue
        normally from the new cursor.
        """
        latest = await self.client.get_account_fills(self.account_id, limit=1, offset=0)
        if latest:
            cursor = latest[0].cursor
        else:
            health = await self.client.health()
            cursor = f"{int(health['height'])}.{2**64 - 1}"

        account = await self.client.get_account(self.account_id)
        self._last_fill_cursor = cursor
        self._apply_canonical_account(account, replace_latest_balance=True)
        log.warning(
            "Fill history cursor expired; reconciled from canonical account state: "
            "name=%s account_id=%d resume_after=%s",
            self.name,
            self.account_id,
            cursor,
        )

    async def _has_pending_orders(self) -> bool:
        """Avoid stacking reservations from previously accepted orders."""
        get_pending = getattr(self.client, "get_pending_orders", None)
        if get_pending is None:
            return False
        try:
            return bool(await get_pending(self.account_id))
        except Exception as e:
            print(f"[{self.name}] Failed to check pending orders: {e}")
            return False

    @property
    def current_balance(self) -> float:
        """Get current balance in dollars."""
        return self.balance_history[-1] if self.balance_history else 0.0

    @property
    def pnl(self) -> float:
        """Get profit/loss from initial balance."""
        if len(self.balance_history) < 2:
            return 0.0
        return self.balance_history[-1] - self.balance_history[0]

    def get_position(self, market_id: int, outcome: str) -> int:
        """Get position quantity for a market outcome."""
        return self.positions.get((market_id, outcome), 0)

    def filter_markets(self, block: Block) -> dict[int, tuple[int, int]]:
        """Get clearing prices filtered to this bot's allowed markets.

        Markets that haven't traded yet default to 50/50 (the bot's prior).
        """
        default_price = (500_000_000, 500_000_000)
        if self.market_ids is None:
            return block.clearing_prices
        return {mid: block.clearing_prices.get(mid, default_price) for mid in self.market_ids}
