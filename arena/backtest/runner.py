"""Backtest runner orchestration."""

import asyncio
from dataclasses import dataclass, field
from datetime import datetime
from typing import Any

from rich.console import Console
from rich.live import Live
from rich.table import Table

from sybil_client import SybilClient

from .agent import BacktestAgent, BacktestAgentConfig
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

    # Runtime state
    _client: SybilClient | None = field(default=None, init=False)
    _clock: SimulatedClock | None = field(default=None, init=False)
    _news_scheduler: NewsScheduler | None = field(default=None, init=False)
    _agents: list[BacktestAgent] = field(default_factory=list, init=False)
    _market_ids: dict[str, int] = field(default_factory=dict, init=False)
    _agent_tasks: list[asyncio.Task] = field(default_factory=list, init=False)

    async def _setup_markets(self) -> None:
        """Create markets for all events in the dataset."""
        console.print("[bold]Creating markets for events...[/bold]")

        for event in self.dataset.events:
            market_name = event.moneyline_market_name
            market = await self._client.create_market(market_name)
            self._market_ids[event.event_id] = market.id
            console.print(f"  Market #{market.id}: {market_name}")

    async def _setup_agents(self) -> None:
        """Create accounts and initialize agents."""
        console.print("\n[bold]Creating agents...[/bold]")

        market_id_list = list(self._market_ids.values())

        for config in self.agent_configs:
            # Create account
            account = await self._client.create_account(
                initial_balance_nanos=int(self.initial_balance * NANOS_PER_DOLLAR)
            )

            # Create agent
            agent = config.agent_class(
                client=self._client,
                account_id=account.id,
                clock=self._clock,
                name=config.name,
                market_ids=market_id_list,
                event_market_map=self._market_ids,
                **config.kwargs,
            )

            # Subscribe to news
            queue = self._news_scheduler.subscribe()
            agent.set_news_queue(queue)

            self._agents.append(agent)
            console.print(
                f"  {agent.name}: Account #{account.id} with ${self.initial_balance:.2f}"
            )

    async def _resolve_event(self, event: Event) -> None:
        """Resolve a market when its event ends."""
        market_id = self._market_ids.get(event.event_id)
        if market_id is None:
            return

        # Determine payout based on outcome
        if event.actual_outcome == "home":
            payout = 1.0  # Home team won, YES pays out
        elif event.actual_outcome == "away":
            payout = 0.0  # Away team won, NO pays out
        else:
            payout = 0.5  # Draw, split

        payout_nanos = int(payout * NANOS_PER_DOLLAR)
        await self._client.resolve_market(market_id, payout_nanos)
        console.print(
            f"  Resolved market #{market_id}: {event.home_team} vs {event.away_team} "
            f"-> {'HOME' if payout == 1.0 else 'AWAY' if payout == 0.0 else 'DRAW'}"
        )

    async def _run_resolution_scheduler(self) -> None:
        """Background task to resolve markets at event end times."""
        # Sort events by end time
        events_by_end = sorted(self.dataset.events, key=lambda e: e.end_time)

        for event in events_by_end:
            # Wait until event end time
            await self._clock.sleep_until(event.end_time)

            # Resolve the market
            await self._resolve_event(event)

    async def _display_live_status(self, update_interval: float = 1.0) -> None:
        """Display live status during backtest."""
        with Live(console=console, refresh_per_second=1) as live:
            while any(not t.done() for t in self._agent_tasks):
                # Build status table
                table = Table(title="Backtest Status")
                table.add_column("Agent", style="cyan")
                table.add_column("Balance", justify="right")
                table.add_column("PnL", justify="right")
                table.add_column("Positions", justify="right")

                sim_time = self._clock.now()
                elapsed = self._clock.elapsed_sim_time()

                for agent in self._agents:
                    if agent.balance_history:
                        balance = agent.balance_history[-1]
                        pnl = balance - self.initial_balance
                        pnl_style = "green" if pnl >= 0 else "red"
                        pos_count = len(agent.positions)
                        table.add_row(
                            agent.name,
                            f"${balance:.2f}",
                            f"[{pnl_style}]${pnl:+.2f}[/{pnl_style}]",
                            str(pos_count),
                        )
                    else:
                        table.add_row(agent.name, "...", "...", "...")

                # Add time info
                table.caption = (
                    f"Sim time: {sim_time.strftime('%H:%M:%S')} | "
                    f"Elapsed: {elapsed.total_seconds()/60:.1f} sim minutes | "
                    f"News delivered: {self._news_scheduler.delivered_count}/{len(self.dataset.news)}"
                )

                live.update(table)
                await asyncio.sleep(update_interval)

    async def run(self, show_live: bool = True) -> BacktestResult:
        """Run the backtest.

        Args:
            show_live: Whether to display live status updates.

        Returns:
            BacktestResult with agent performance and statistics.
        """
        start_time = datetime.now()

        console.print(f"[bold blue]Starting backtest: {self.dataset.name}[/bold blue]")
        console.print(f"Time range: {self.dataset.time_range[0]} to {self.dataset.time_range[1]}")
        console.print(f"Events: {len(self.dataset.events)}, News items: {len(self.dataset.news)}")
        console.print(f"Compression ratio: {self.compression_ratio}x\n")

        async with SybilClient(self.base_url) as client:
            self._client = client

            # Verify server is healthy
            health = await client.health()
            console.print(f"[green]Connected to Sybil at block {health.get('height', 0)}[/green]\n")

            # Initialize clock
            self._clock = SimulatedClock(
                sim_start=self.dataset.time_range[0],
                compression_ratio=self.compression_ratio,
            )

            # Initialize news scheduler
            self._news_scheduler = NewsScheduler(
                clock=self._clock,
                news_items=self.dataset.news,
            )

            # Setup markets and agents
            await self._setup_markets()
            await self._setup_agents()

            # Start clock
            self._clock.start()

            # Start news delivery
            news_task = self._news_scheduler.start()

            # Start resolution scheduler
            resolution_task = asyncio.create_task(self._run_resolution_scheduler())

            # Start all agents
            console.print("\n[bold]Starting agents...[/bold]")
            self._agent_tasks = [asyncio.create_task(agent.run()) for agent in self._agents]

            # Calculate real duration
            real_duration = self._clock.sim_to_real_seconds(self.dataset.duration)
            console.print(
                f"Running for {real_duration:.1f} real seconds "
                f"({self.dataset.duration/3600:.1f} simulated hours)\n"
            )

            # Wait for completion with optional live display
            try:
                if show_live:
                    display_task = asyncio.create_task(self._display_live_status())
                    await asyncio.sleep(real_duration + 1)
                    display_task.cancel()
                else:
                    await asyncio.sleep(real_duration + 1)
            except asyncio.CancelledError:
                pass

            # Stop everything
            console.print("\n[bold]Stopping agents...[/bold]")
            for agent in self._agents:
                agent.stop()

            self._news_scheduler.stop()

            # Cancel tasks
            for task in self._agent_tasks + [news_task, resolution_task]:
                if not task.done():
                    task.cancel()

            await asyncio.gather(
                *self._agent_tasks,
                news_task,
                resolution_task,
                return_exceptions=True,
            )

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

            # Get resolution payouts
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

            # Print leaderboard
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

    # Winner announcement
    if result.agent_results:
        winner = result.leaderboard()[0]
        console.print(
            f"\n[bold yellow]Winner: {winner.name} with ${winner.pnl:+.2f} PnL[/bold yellow]"
        )

    # Stats
    console.print(f"\nReal duration: {result.duration_real_seconds:.1f} seconds")
    console.print(f"Simulated duration: {result.duration_sim_seconds/3600:.1f} hours")
    console.print(f"Events: {len(result.dataset.events)}")
    console.print(f"News items: {len(result.dataset.news)}")
