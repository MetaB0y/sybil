"""Base agent class for trading bots."""

from abc import ABC, abstractmethod

from sybil_client import Block, OrderSpec, SybilClient


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
        # Order tracking for observability
        self.last_orders: list[OrderSpec] = []
        self.total_orders_submitted: int = 0
        # Per-block order log: (block_height, orders_submitted)
        self.block_log: list[tuple[int, list[OrderSpec]]] = []
        # Per-block stats from the sequencer (welfare, volume, fills)
        self.block_stats: dict[int, tuple[int, int, int]] = {}  # height -> (welfare, volume, fills)
        # Fill tracking via get_account_fills()
        self._last_fill_count: int = 0
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

    async def run(self) -> None:
        """Main loop - stream blocks and react."""
        self._running = True
        blocks_traded = 0
        try:
            async for block in self.client.stream_blocks():
                if not self._running:
                    break

                # Update our state
                await self._update_state(block)

                # Record block-level stats (welfare, volume, fills)
                self.block_stats[block.height] = (
                    block.total_welfare, block.total_volume, block.orders_filled,
                )

                # Get orders from strategy
                orders = await self.on_block(block)

                # Log and submit orders
                self.block_log.append((block.height, orders))
                if orders:
                    self.last_orders = orders
                    self.total_orders_submitted += len(orders)
                    try:
                        await self.client.submit_orders(
                            self.account_id, orders,
                            mm_budget_nanos=self.mm_budget_nanos,
                        )
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
            self.positions = {
                (pos.market_id, pos.outcome): pos.quantity for pos in account.positions
            }
            self.balance_history.append(account.balance_dollars)

            # Fetch only this agent's new fills
            new_fills = await self.client.get_account_fills(
                self.account_id, limit=20, offset=self._last_fill_count
            )
            self._fill_history.extend(new_fills)
            for fill in new_fills:
                await self.on_fill(fill.order_id, fill.fill_qty, fill.fill_price)
            self._last_fill_count += len(new_fills)

        except Exception as e:
            print(f"[{self.name}] Failed to update state: {e}")

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
        return {
            mid: block.clearing_prices.get(mid, default_price)
            for mid in self.market_ids
        }
