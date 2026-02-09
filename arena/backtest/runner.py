"""Backtest runner orchestration."""

import asyncio
from dataclasses import dataclass, field
from datetime import datetime
from typing import Any

from rich.console import Console
from rich.table import Table

from sybil_client import SybilClient

from .agent import BacktestAgentConfig
from .clock import SimulatedClock
from .dataset import Dataset, Event
from .news import NewsScheduler

console = Console()

NANOS_PER_DOLLAR = 1_000_000_000


@dataclass
class AgentResult:
    """Result for a single agent after backtest."""

    name: str
    account_id: int
    initial_balance: float
    final_balance: float
    positions: dict[tuple[int, str], int]
    news_processed: int

    @property
    def pnl(self) -> float:
        return self.final_balance - self.initial_balance

    @property
    def pnl_pct(self) -> float:
        if self.initial_balance == 0:
            return 0.0
        return (self.pnl / self.initial_balance) * 100


@dataclass
class BacktestResult:
    """Result of a backtest run."""

    dataset: Dataset
    start_time: datetime
    end_time: datetime
    agent_results: list[AgentResult]
    market_ids: dict[str, int]  # event_id -> market_id
    resolutions: dict[int, float]  # market_id -> payout

    @property
    def duration_real_seconds(self) -> float:
        return (self.end_time - self.start_time).total_seconds()

    @property
    def duration_sim_seconds(self) -> float:
        return self.dataset.duration

    def leaderboard(self) -> list[AgentResult]:
        """Return agent results sorted by PnL (descending)."""
        return sorted(self.agent_results, key=lambda r: r.pnl, reverse=True)


@dataclass
class BacktestRunner:
    """Orchestrates backtest execution.

    The runner:
    1. Loads the dataset
    2. Creates markets for all events
    3. Sets up accounts and bots
    4. Initializes clock and news scheduler
    5. Connects news queues to bots
    6. Runs the simulation with time compression
    7. Resolves markets when events end (simulated time)
    8. Collects results

    Usage:
        runner = BacktestRunner(
            base_url="http://localhost:3001",
            dataset=Dataset.load("datasets/nba_sample.json"),
            agent_configs=[
                BacktestAgentConfig(NewsTrader, "Bot1", {}),
            ],
            initial_balance=100.0,
            compression_ratio=60.0,
        )
        result = await runner.run()
    """

    base_url: str
    dataset: Dataset
    agent_configs: list[BacktestAgentConfig]
    initial_balance: float = 100.0
    compression_ratio: float = 60.0

    # Runtime state (read by TUI widgets)
    _client: SybilClient | None = field(default=None, init=False)
    _clock: SimulatedClock | None = field(default=None, init=False)
    _news_scheduler: NewsScheduler | None = field(default=None, init=False)
    _agents: list = field(default_factory=list, init=False)
    _market_ids: dict[str, int] = field(default_factory=dict, init=False)
    _agent_tasks: list[asyncio.Task] = field(default_factory=list, init=False)
    _latest_prices: dict[int, tuple[int, int]] = field(default_factory=dict, init=False)
    _market_display_names: dict[int, str] = field(default_factory=dict, init=False)
    _last_block: Any = field(default=None, init=False)

    async def _setup_markets(self) -> None:
        """Create markets for all events in the dataset."""
        print("Creating markets for events...")

        for event in self.dataset.events:
            market_name = event.moneyline_market_name
            market = await self._client.create_market(market_name)
            self._market_ids[event.event_id] = market.id
            self._market_display_names[market.id] = f"{event.home_team} vs {event.away_team}"
            print(f"  Market #{market.id}: {market_name}")

    async def _setup_agents(self) -> None:
        """Create accounts and initialize agents."""
        print("\nCreating agents...")

        market_id_list = list(self._market_ids.values())

        for config in self.agent_configs:
            account = await self._client.create_account(
                initial_balance_nanos=int(self.initial_balance * NANOS_PER_DOLLAR)
            )

            agent = config.agent_class(
                client=self._client,
                account_id=account.id,
                clock=self._clock,
                name=config.name,
                market_ids=market_id_list,
                event_market_map=self._market_ids,
                **config.kwargs,
            )

            queue = self._news_scheduler.subscribe()
            agent.set_news_queue(queue)

            self._agents.append(agent)
            print(f"  {agent.name}: Account #{account.id} with ${self.initial_balance:.2f}")

    async def _resolve_event(self, event: Event) -> None:
        """Resolve a market when its event ends."""
        market_id = self._market_ids.get(event.event_id)
        if market_id is None:
            return

        if event.actual_outcome == "home":
            payout = 1.0
        elif event.actual_outcome == "away":
            payout = 0.0
        else:
            payout = 0.5

        payout_nanos = int(payout * NANOS_PER_DOLLAR)
        await self._client.resolve_market(market_id, payout_nanos)
        outcome_str = "HOME" if payout == 1.0 else "AWAY" if payout == 0.0 else "DRAW"
        print(
            f"  Resolved market #{market_id}: "
            f"{event.home_team} vs {event.away_team} -> {outcome_str}"
        )

    async def _run_resolution_scheduler(self) -> None:
        """Background task to resolve markets at event end times."""
        events_by_end = sorted(self.dataset.events, key=lambda e: e.end_time)
        for event in events_by_end:
            await self._clock.sleep_until(event.end_time)
            await self._resolve_event(event)

    async def _resolve_remaining_markets(self) -> None:
        """Resolve any markets that weren't resolved by the scheduler."""
        for event in self.dataset.events:
            market_id = self._market_ids.get(event.event_id)
            if market_id is None:
                continue
            try:
                await self._resolve_event(event)
            except Exception:
                pass

    async def _track_blocks(self) -> None:
        """Background task to track blocks from SSE for display."""
        try:
            our_market_ids = set(self._market_ids.values())
            async for block in self._client.stream_blocks():
                self._last_block = block
                for market_id, prices in block.clearing_prices.items():
                    if market_id in our_market_ids:
                        self._latest_prices[market_id] = prices
        except (asyncio.CancelledError, Exception):
            pass

    async def run(self, show_live: bool = True) -> BacktestResult:
        """Run the backtest.

        Args:
            show_live: Whether to display TUI during simulation.

        Returns:
            BacktestResult with agent performance and statistics.
        """
        start_time = datetime.now()

        print(f"Starting backtest: {self.dataset.name}")
        print(f"Time range: {self.dataset.time_range[0]} to {self.dataset.time_range[1]}")
        print(f"Events: {len(self.dataset.events)}, News items: {len(self.dataset.news)}")
        print(f"Compression ratio: {self.compression_ratio}x\n")

        async with SybilClient(self.base_url) as client:
            self._client = client

            health = await client.health()
            print(f"Connected to Sybil at block {health.get('height', 0)}\n")

            self._clock = SimulatedClock(
                sim_start=self.dataset.time_range[0],
                compression_ratio=self.compression_ratio,
            )

            self._news_scheduler = NewsScheduler(
                clock=self._clock,
                news_items=self.dataset.news,
            )

            await self._setup_markets()
            await self._setup_agents()

            self._clock.start()

            news_task = self._news_scheduler.start()
            resolution_task = asyncio.create_task(self._run_resolution_scheduler())

            print("\nStarting agents...")
            self._agent_tasks = [asyncio.create_task(agent.run()) for agent in self._agents]

            # Start block tracker (used by TUI to read prices)
            block_task = asyncio.create_task(self._track_blocks())

            real_duration = self._clock.sim_to_real_seconds(self.dataset.duration)
            print(
                f"Running for {real_duration:.1f} real seconds "
                f"({self.dataset.duration/3600:.1f} simulated hours)\n"
            )

            try:
                if show_live:
                    from .tui import SybilTUI

                    app = SybilTUI(runner=self, real_duration=real_duration)
                    await app.run_async()
                else:
                    await asyncio.sleep(real_duration + 1)
            except asyncio.CancelledError:
                pass

            # Wait for resolution scheduler to finish
            if not resolution_task.done():
                print("Waiting for market resolution...")
                try:
                    await asyncio.wait_for(resolution_task, timeout=5.0)
                except (asyncio.TimeoutError, asyncio.CancelledError):
                    pass

            await self._resolve_remaining_markets()

            print("\nStopping agents...")
            for agent in self._agents:
                agent.stop()

            self._news_scheduler.stop()

            for task in self._agent_tasks + [news_task, resolution_task, block_task]:
                if not task.done():
                    task.cancel()

            await asyncio.gather(
                *self._agent_tasks,
                news_task,
                resolution_task,
                block_task,
                return_exceptions=True,
            )

            # Debug: check for remaining positions and total balance
            total_final_balance = 0
            total_initial = len(self._agents) * self.initial_balance
            our_market_ids = set(self._market_ids.values())
            for agent in self._agents:
                account = await client.get_account(agent.account_id)
                total_final_balance += account.balance_dollars
                if account.positions:
                    for p in account.positions:
                        tag = "OUR" if p.market_id in our_market_ids else "SEED"
                        print(
                            f"  REMAINING POS: {agent.name} market={p.market_id}({tag}) "
                            f"{p.outcome}={p.quantity}"
                        )
            print(f"\nBALANCE CHECK: initial=${total_initial:.2f} "
                  f"final=${total_final_balance:.2f} "
                  f"diff=${total_final_balance - total_initial:.2f}")

            # Collect results
            agent_results = []
            for agent in self._agents:
                account = await client.get_account(agent.account_id)
                agent_results.append(
                    AgentResult(
                        name=agent.name,
                        account_id=agent.account_id,
                        initial_balance=self.initial_balance,
                        final_balance=account.balance_dollars,
                        positions={
                            (p.market_id, p.outcome): p.quantity
                            for p in account.positions
                        },
                        news_processed=len(agent.beliefs),
                    )
                )

            resolutions = {}
            for event in self.dataset.events:
                market_id = self._market_ids.get(event.event_id)
                if market_id:
                    if event.actual_outcome == "home":
                        resolutions[market_id] = 1.0
                    elif event.actual_outcome == "away":
                        resolutions[market_id] = 0.0
                    else:
                        resolutions[market_id] = 0.5

            end_time = datetime.now()

            result = BacktestResult(
                dataset=self.dataset,
                start_time=start_time,
                end_time=end_time,
                agent_results=agent_results,
                market_ids=self._market_ids,
                resolutions=resolutions,
            )

            print_leaderboard(result)

            return result


def print_leaderboard(result: BacktestResult) -> None:
    """Print the final backtest leaderboard."""
    console.print(
        "\n[bold blue]═══════════════════════════════════════════════════════════[/bold blue]"
    )
    console.print(f"[bold blue]  {result.dataset.name.upper()} - BACKTEST RESULTS[/bold blue]")
    console.print(
        "[bold blue]═══════════════════════════════════════════════════════════[/bold blue]\n"
    )

    table = Table(title="Leaderboard")
    table.add_column("Rank", style="cyan", justify="center")
    table.add_column("Agent", style="green")
    table.add_column("Final Balance", justify="right")
    table.add_column("PnL", justify="right")
    table.add_column("PnL %", justify="right")

    for i, agent in enumerate(result.leaderboard(), 1):
        pnl_style = "green" if agent.pnl >= 0 else "red"
        rank_style = "bold yellow" if i == 1 else ("bold" if i <= 3 else "")

        table.add_row(
            f"[{rank_style}]{i}[/{rank_style}]" if rank_style else str(i),
            agent.name,
            f"${agent.final_balance:.2f}",
            f"[{pnl_style}]${agent.pnl:+.2f}[/{pnl_style}]",
            f"[{pnl_style}]{agent.pnl_pct:+.1f}%[/{pnl_style}]",
        )

    console.print(table)

    if result.agent_results:
        winner = result.leaderboard()[0]
        console.print(
            f"\n[bold yellow]Winner: {winner.name} with ${winner.pnl:+.2f} PnL[/bold yellow]"
        )

    console.print(f"\nReal duration: {result.duration_real_seconds:.1f} seconds")
    console.print(f"Simulated duration: {result.duration_sim_seconds/3600:.1f} hours")
    console.print(f"Events: {len(result.dataset.events)}")
    console.print(f"News items: {len(result.dataset.news)}")
