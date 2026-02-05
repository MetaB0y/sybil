#!/usr/bin/env python3
"""Competition runner script.

This module provides utilities for setting up, running, and analyzing
AI agent trading competitions on Sybil prediction markets.
"""

import asyncio
from dataclasses import dataclass, field
from datetime import datetime
from typing import Any, Callable

from rich.console import Console
from rich.live import Live
from rich.table import Table

from bots.base import BaseAgent
from sybil_client import SybilClient

console = Console()


@dataclass
class BotConfig:
    """Configuration for a bot in the competition."""

    bot_class: type[BaseAgent]
    name: str
    kwargs: dict[str, Any] = field(default_factory=dict)


@dataclass
class CompetitionConfig:
    """Configuration for a competition."""

    name: str
    initial_balance: float = 100.0  # dollars
    duration_seconds: int = 60
    markets: list[str] = field(default_factory=list)
    resolution_payouts: dict[str, float] = field(default_factory=dict)  # market_name -> payout (0-1)


@dataclass
class BotResult:
    """Result for a single bot."""

    name: str
    account_id: int
    initial_balance: float
    final_balance: float
    positions: dict[tuple[int, str], int]
    trade_count: int

    @property
    def pnl(self) -> float:
        return self.final_balance - self.initial_balance

    @property
    def pnl_pct(self) -> float:
        if self.initial_balance == 0:
            return 0.0
        return (self.pnl / self.initial_balance) * 100


@dataclass
class CompetitionResult:
    """Result of a competition."""

    config: CompetitionConfig
    start_time: datetime
    end_time: datetime
    bot_results: list[BotResult]
    market_ids: dict[str, int]  # market_name -> market_id

    @property
    def duration_seconds(self) -> float:
        return (self.end_time - self.start_time).total_seconds()

    def leaderboard(self) -> list[BotResult]:
        """Return bot results sorted by PnL (descending)."""
        return sorted(self.bot_results, key=lambda r: r.pnl, reverse=True)


async def setup_competition(
    client: SybilClient,
    config: CompetitionConfig,
    bot_configs: list[BotConfig],
) -> tuple[list[BaseAgent], dict[str, int]]:
    """Set up a competition with accounts and markets.

    Args:
        client: SybilClient instance
        config: Competition configuration
        bot_configs: List of bot configurations

    Returns:
        Tuple of (list of initialized bots, market_name -> market_id mapping)
    """
    console.print(f"[bold blue]Setting up competition: {config.name}[/bold blue]\n")

    # Create markets first (so we have the IDs to pass to bots)
    console.print("[bold]Creating markets...[/bold]")
    market_ids = {}
    for market_name in config.markets:
        market = await client.create_market(market_name)
        market_ids[market_name] = market.id
        console.print(f"  Market #{market.id}: {market_name}")

    # List of market IDs for bots to trade
    allowed_market_ids = list(market_ids.values())

    # Create accounts
    console.print("\n[bold]Creating accounts...[/bold]")
    bots = []
    for bot_config in bot_configs:
        account = await client.create_account(
            initial_balance_nanos=int(config.initial_balance * 1_000_000_000)
        )
        bot = bot_config.bot_class(
            client=client,
            account_id=account.id,
            name=bot_config.name,
            market_ids=allowed_market_ids,  # Only trade on competition markets
            **bot_config.kwargs,
        )
        bots.append(bot)
        console.print(f"  {bot.name}: Account #{account.id} with ${config.initial_balance:.2f}")

    return bots, market_ids


async def run_competition(
    client: SybilClient,
    bots: list[BaseAgent],
    config: CompetitionConfig,
    market_ids: dict[str, int],
    show_live: bool = True,
) -> CompetitionResult:
    """Run a competition.

    Args:
        client: SybilClient instance
        bots: List of initialized bots
        config: Competition configuration
        market_ids: Market name to ID mapping
        show_live: Whether to show live updates

    Returns:
        Competition result
    """
    start_time = datetime.now()

    console.print(f"\n[bold]Running competition for {config.duration_seconds} seconds...[/bold]")

    # Start all bots
    tasks = [asyncio.create_task(bot.run()) for bot in bots]

    if show_live:
        # Show live leaderboard updates
        with Live(console=console, refresh_per_second=1) as live:
            for elapsed in range(config.duration_seconds):
                await asyncio.sleep(1)

                # Build live table
                table = Table(title=f"Live Standings ({elapsed + 1}/{config.duration_seconds}s)")
                table.add_column("Bot", style="cyan")
                table.add_column("Balance", justify="right")
                table.add_column("PnL", justify="right")

                for bot in bots:
                    if bot.balance_history:
                        balance = bot.balance_history[-1]
                        pnl = balance - config.initial_balance
                        pnl_style = "green" if pnl >= 0 else "red"
                        table.add_row(
                            bot.name,
                            f"${balance:.2f}",
                            f"[{pnl_style}]${pnl:+.2f}[/{pnl_style}]",
                        )
                    else:
                        table.add_row(bot.name, "...", "...")

                live.update(table)
    else:
        await asyncio.sleep(config.duration_seconds)

    # Stop bots
    for bot in bots:
        bot.stop()

    # Cancel tasks
    for task in tasks:
        task.cancel()
    await asyncio.gather(*tasks, return_exceptions=True)

    end_time = datetime.now()

    # Resolve markets
    console.print("\n[bold]Resolving markets...[/bold]")
    for market_name, payout in config.resolution_payouts.items():
        if market_name in market_ids:
            market_id = market_ids[market_name]
            payout_nanos = int(payout * 1_000_000_000)
            await client.resolve_market(market_id, payout_nanos)
            console.print(f"  Market #{market_id}: resolved at {payout*100:.0f}% YES")

    # Collect results
    bot_results = []
    for bot in bots:
        account = await client.get_account(bot.account_id)
        bot_results.append(
            BotResult(
                name=bot.name,
                account_id=bot.account_id,
                initial_balance=config.initial_balance,
                final_balance=account.balance_dollars,
                positions={(p.market_id, p.outcome): p.quantity for p in account.positions},
                trade_count=len(bot.balance_history),
            )
        )

    return CompetitionResult(
        config=config,
        start_time=start_time,
        end_time=end_time,
        bot_results=bot_results,
        market_ids=market_ids,
    )


def print_leaderboard(result: CompetitionResult) -> None:
    """Print the final competition leaderboard."""
    console.print("\n[bold blue]═══════════════════════════════════════════════════════════[/bold blue]")
    console.print(f"[bold blue]  {result.config.name.upper()} - FINAL RESULTS[/bold blue]")
    console.print("[bold blue]═══════════════════════════════════════════════════════════[/bold blue]\n")

    table = Table(title="Leaderboard")
    table.add_column("Rank", style="cyan", justify="center")
    table.add_column("Bot", style="green")
    table.add_column("Final Balance", justify="right")
    table.add_column("PnL", justify="right")
    table.add_column("PnL %", justify="right")
    table.add_column("Trades", justify="right")

    for i, bot in enumerate(result.leaderboard(), 1):
        pnl_style = "green" if bot.pnl >= 0 else "red"
        rank_style = "bold yellow" if i == 1 else ("bold" if i <= 3 else "")

        table.add_row(
            f"[{rank_style}]{i}[/{rank_style}]" if rank_style else str(i),
            bot.name,
            f"${bot.final_balance:.2f}",
            f"[{pnl_style}]${bot.pnl:+.2f}[/{pnl_style}]",
            f"[{pnl_style}]{bot.pnl_pct:+.1f}%[/{pnl_style}]",
            str(bot.trade_count),
        )

    console.print(table)

    # Winner announcement
    winner = result.leaderboard()[0]
    console.print(f"\n[bold yellow]🏆 Winner: {winner.name} with ${winner.pnl:+.2f} PnL[/bold yellow]")

    # Stats
    console.print(f"\nCompetition duration: {result.duration_seconds:.1f} seconds")
    console.print(f"Markets traded: {len(result.market_ids)}")


async def run_full_competition(
    base_url: str,
    config: CompetitionConfig,
    bot_configs: list[BotConfig],
    show_live: bool = True,
) -> CompetitionResult:
    """Run a complete competition from setup to results.

    Args:
        base_url: Sybil API base URL
        config: Competition configuration
        bot_configs: List of bot configurations
        show_live: Whether to show live updates

    Returns:
        Competition result
    """
    async with SybilClient(base_url) as client:
        # Verify server is healthy
        health = await client.health()
        console.print(f"[green]Connected to Sybil at block {health.get('height', 0)}[/green]\n")

        # Setup
        bots, market_ids = await setup_competition(client, config, bot_configs)

        # Run
        result = await run_competition(client, bots, config, market_ids, show_live)

        # Print results
        print_leaderboard(result)

        return result
