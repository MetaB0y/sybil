"""Base agent class for trading bots."""

from abc import ABC, abstractmethod

from sybil_client import Account, Block, OrderSpec, SybilClient


class BaseAgent(ABC):
    """Abstract base class for trading bots."""

    def __init__(
        self,
        client: SybilClient,
        account_id: int,
        name: str | None = None,
        market_ids: list[int] | None = None,
    ):
        self.client = client
        self.account_id = account_id
        self.name = name or self.__class__.__name__
        self.market_ids = set(market_ids) if market_ids else None  # None means all markets
        self.positions: dict[tuple[int, str], int] = {}
        self.balance_history: list[float] = []
        self._running = False

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
        try:
            async for block in self.client.stream_blocks():
                if not self._running:
                    break

                # Update our state
                await self._update_state(block)

                # Get orders from strategy
                orders = await self.on_block(block)

                # Submit orders if any
                if orders:
                    try:
                        await self.client.submit_orders(self.account_id, orders)
                    except Exception as e:
                        print(f"[{self.name}] Order submission failed: {e}")

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

            # Check for our fills
            for fill in block.fills:
                # Note: We'd need order tracking to know which fills are ours
                await self.on_fill(fill.order_id, fill.fill_qty, fill.fill_price)

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
        """Get clearing prices filtered to this bot's allowed markets."""
        if self.market_ids is None:
            return block.clearing_prices
        return {
            mid: prices
            for mid, prices in block.clearing_prices.items()
            if mid in self.market_ids
        }
