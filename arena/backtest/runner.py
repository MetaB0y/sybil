"""Backtest runner orchestration."""

import asyncio
from dataclasses import dataclass, field
from datetime import datetime
from typing import Any

from rich.console import Console, Group
from rich.live import Live
from rich.panel import Panel
from rich.table import Table

from sybil_client import SybilClient

from .agent import BacktestAgent, BacktestAgentConfig
from .clock import SimulatedClock
from .dataset import Dataset, Event
from .news import NewsScheduler

from bots.strategy_agent import format_news_line

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
    _latest_prices: dict[int, tuple[int, int]] = field(default_factory=dict, init=False)
    _market_display_names: dict[int, str] = field(default_factory=dict, init=False)
    _last_block: Any = field(default=None, init=False)

    async def _setup_markets(self) -> None:
        """Create markets for all events in the dataset."""
        console.print("[bold]Creating markets for events...[/bold]")

        for event in self.dataset.events:
            market_name = event.moneyline_market_name
            market = await self._client.create_market(market_name)
            self._market_ids[event.event_id] = market.id
            self._market_display_names[market.id] = f"{event.home_team} vs {event.away_team}"
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

    async def _resolve_remaining_markets(self) -> None:
        """Resolve any markets that weren't resolved by the scheduler."""
        for event in self.dataset.events:
            market_id = self._market_ids.get(event.event_id)
            if market_id is None:
                continue
            try:
                await self._resolve_event(event)
            except Exception:
                pass  # Already resolved

    def _build_news_panel(self) -> Panel:
        """Build the NEWS panel showing recent news items."""
        recent = self._news_scheduler.recent_news
        if not recent:
            content = "[dim]Waiting for news...[/dim]"
        else:
            lines = []
            for news in recent:
                ts = news.timestamp.strftime("%H:%M")
                # Show which game this news is about
                game_tag = ""
                if news.event_id:
                    market_id = self._market_ids.get(news.event_id)
                    if market_id is not None:
                        display = self._market_display_names.get(market_id, "")
                        # Shorten: "Boston Celtics vs Detroit Pistons" -> "BOS vs DET"
                        parts = display.split(" vs ")
                        if len(parts) == 2:
                            short = " vs ".join(p.split()[-1][:3].upper() for p in parts)
                            game_tag = f"[dim]{short}[/dim] "
                formatted = format_news_line(news)
                lines.append(f" {ts}  {game_tag}{formatted}")
            content = "\n".join(lines)
        return Panel(content, title="NEWS", border_style="cyan", height=min(12, 4 + len(self._news_scheduler.recent_news)))

    def _build_markets_panel(self) -> Panel:
        """Build the MARKETS & AI ESTIMATES panel."""
        # Identify agents that have beliefs (directional bots, not MMs)
        estimate_agents = [a for a in self._agents if a.beliefs]

        table = Table(box=None, pad_edge=False, show_header=True, expand=True)
        table.add_column("Game", style="cyan", ratio=3)
        table.add_column("Price", justify="center", ratio=1)
        for agent in estimate_agents:
            table.add_column(agent.name, justify="center", ratio=1)

        for market_id, display_name in sorted(self._market_display_names.items()):
            prices = self._latest_prices.get(market_id)
            if prices is None:
                price_str = "[dim]--[/dim]"
                market_prob = 0.5
            else:
                market_prob = prices[0] / NANOS_PER_DOLLAR
                price_str = f"{market_prob * 100:.0f}%"

            row = [display_name, price_str]

            for agent in estimate_agents:
                belief = agent.beliefs.get(market_id)
                if belief is None:
                    row.append("[dim]--[/dim]")
                    continue
                est = belief.probability
                edge = est - market_prob
                est_pct = f"{est * 100:.0f}%"
                if edge > 0.03:
                    row.append(f"[green]{est_pct}^[/green]")
                elif edge < -0.03:
                    row.append(f"[red]{est_pct}v[/red]")
                else:
                    row.append(f"[dim]{est_pct}[/dim]")

            table.add_row(*row)

        return Panel(table, title="MARKETS & AI ESTIMATES", border_style="yellow")

    def _build_leaderboard_panel(self) -> Panel:
        """Build the LEADERBOARD panel."""
        # Sort agents by current balance
        ranked = sorted(
            self._agents,
            key=lambda a: a.balance_history[-1] if a.balance_history else 0,
            reverse=True,
        )

        table = Table(box=None, pad_edge=False, show_header=True, expand=True)
        table.add_column("#", justify="center", width=3)
        table.add_column("Agent", ratio=2)
        table.add_column("Balance", justify="right", ratio=1)
        table.add_column("PnL", justify="right", ratio=1)
        table.add_column("Pos", justify="right", width=4)

        for i, agent in enumerate(ranked, 1):
            if agent.balance_history:
                balance = agent.balance_history[-1]
                pnl = balance - self.initial_balance
                pnl_style = "green" if pnl >= 0 else "red"
                pos_count = sum(
                    1 for (_, outcome), qty in agent.positions.items()
                    if outcome == "YES" and qty != 0
                ) + sum(
                    1 for (_, outcome), qty in agent.positions.items()
                    if outcome == "NO" and qty != 0
                )
                rank_style = "bold yellow" if i == 1 else ("bold" if i <= 3 else "")
                rank_str = f"[{rank_style}]{i}[/{rank_style}]" if rank_style else str(i)
                table.add_row(
                    rank_str,
                    agent.name,
                    f"${balance:.2f}",
                    f"[{pnl_style}]${pnl:+.2f}[/{pnl_style}]",
                    str(pos_count),
                )
            else:
                table.add_row(str(i), agent.name, "...", "...", "...")

        return Panel(table, title="LEADERBOARD", border_style="green")

    def _build_orders_panel(self) -> Panel:
        """Build the ORDERS panel showing what each bot submitted last block."""
        from sybil_client import BuyYes, BuyNo, SellYes, SellNo

        lines = []
        for agent in self._agents:
            orders = agent.last_orders
            if not orders:
                lines.append(f" [dim]{agent.name}: no orders[/dim]")
                continue

            # Summarize orders by type
            buy_yes = [o for o in orders if isinstance(o, BuyYes)]
            buy_no = [o for o in orders if isinstance(o, BuyNo)]
            sell_yes = [o for o in orders if isinstance(o, SellYes)]
            sell_no = [o for o in orders if isinstance(o, SellNo)]

            parts = []
            if buy_yes:
                prices = [o.limit_price_nanos / NANOS_PER_DOLLAR for o in buy_yes]
                qty = sum(o.quantity for o in buy_yes)
                parts.append(f"[green]BY {qty}@{min(prices):.2f}-{max(prices):.2f}[/green]")
            if buy_no:
                prices = [o.limit_price_nanos / NANOS_PER_DOLLAR for o in buy_no]
                qty = sum(o.quantity for o in buy_no)
                parts.append(f"[red]BN {qty}@{min(prices):.2f}-{max(prices):.2f}[/red]")
            if sell_yes:
                parts.append(f"SY {len(sell_yes)}")
            if sell_no:
                parts.append(f"SN {len(sell_no)}")

            lines.append(f" {agent.name}: {' | '.join(parts)}  ({agent.total_orders_submitted} total)")

        # Show last block fills if available
        if self._last_block:
            b = self._last_block
            lines.append("")
            vol = b.total_volume / NANOS_PER_DOLLAR
            vol_str = f", vol=${vol:.2f}" if vol > 0 else ""
            lines.append(f" [bold]Block #{b.height}[/bold]: {b.orders_filled} fills{vol_str}")
            our_market_ids = set(self._market_ids.values())
            price_parts = []
            for market_id in sorted(our_market_ids):
                prices = b.clearing_prices.get(market_id)
                if prices is None:
                    continue
                yes_p, _no_p = prices
                yes_pct = yes_p / NANOS_PER_DOLLAR * 100
                if abs(yes_pct - 50) < 0.5:
                    continue  # Skip markets at default 50%
                name = self._market_display_names.get(market_id, f"M{market_id}")
                parts = name.split(" vs ")
                short = " vs ".join(p.split()[-1][:3].upper() for p in parts) if len(parts) == 2 else name
                price_parts.append(f"{short}={yes_pct:.0f}%")
            if price_parts:
                lines.append(f"   {', '.join(price_parts)}")

        content = "\n".join(lines) if lines else "[dim]No orders yet[/dim]"
        return Panel(content, title="ORDERS & LAST BATCH", border_style="magenta")

    def _build_thoughts_panel(self) -> Panel:
        """Build the AI THOUGHTS panel showing LLM reasoning."""
        lines = []
        for agent in self._agents:
            reasoning = getattr(agent, "last_reasoning", "")
            if reasoning:
                lines.append(f" [bold]{agent.name}[/bold]: {reasoning}")
        content = "\n".join(lines) if lines else "[dim]Waiting for LLM responses...[/dim]"
        return Panel(content, title="AI THOUGHTS", border_style="blue")

    def _build_display(self) -> Group:
        """Build the full 5-panel display."""
        sim_time = self._clock.now()
        elapsed_hrs = self._clock.elapsed_sim_time().total_seconds() / 3600
        news_count = self._news_scheduler.delivered_count
        total_news = len(self.dataset.news)

        footer = (
            f" Sim: {sim_time.strftime('%H:%M')} | "
            f"{elapsed_hrs:.1f} hrs | "
            f"News: {news_count}/{total_news}"
        )

        return Group(
            self._build_news_panel(),
            self._build_markets_panel(),
            self._build_orders_panel(),
            self._build_thoughts_panel(),
            self._build_leaderboard_panel(),
            footer,
        )

    async def _track_blocks(self) -> None:
        """Background task to track blocks from SSE for display."""
        try:
            our_market_ids = set(self._market_ids.values())
            async for block in self._client.stream_blocks():
                self._last_block = block
                # Update prices from block data (only our markets)
                for market_id, prices in block.clearing_prices.items():
                    if market_id in our_market_ids:
                        self._latest_prices[market_id] = prices
        except (asyncio.CancelledError, Exception):
            pass

    async def _display_live_status(self, update_interval: float = 1.0) -> None:
        """Display live 4-panel status during backtest."""
        block_task = asyncio.create_task(self._track_blocks())
        try:
            with Live(console=console, refresh_per_second=1) as live:
                while any(not t.done() for t in self._agent_tasks):
                    live.update(self._build_display())
                    await asyncio.sleep(update_interval)
        finally:
            block_task.cancel()

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

            # Wait for resolution scheduler to finish (don't cancel it)
            if not resolution_task.done():
                console.print("[bold]Waiting for market resolution...[/bold]")
                try:
                    await asyncio.wait_for(resolution_task, timeout=5.0)
                except (asyncio.TimeoutError, asyncio.CancelledError):
                    pass

            # Safety net: resolve any markets that weren't resolved
            await self._resolve_remaining_markets()

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
