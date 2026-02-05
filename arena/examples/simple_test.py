#!/usr/bin/env python3
"""Simple test competition with synthetic markets.

Run this to verify the arena setup works:
1. Start sybil-api: cargo run -p sybil-api -- --dev-mode
2. Run this script: python examples/simple_test.py
"""

import asyncio

from rich.console import Console
from rich.table import Table

from bots import RandomTrader, SimpleMarketMaker
from sybil_client import SybilClient

console = Console()

INITIAL_BALANCE = 100  # $100 per bot
NUM_BOTS = 4
DURATION_SECONDS = 30


async def main():
    console.print("[bold blue]Sybil Arena - Simple Test Competition[/bold blue]\n")

    async with SybilClient("http://localhost:3001") as client:
        # Check server health
        try:
            health = await client.health()
            console.print(f"[green]Server healthy at block {health.get('height', 0)}[/green]")
        except Exception as e:
            console.print(f"[red]Failed to connect to server: {e}[/red]")
            console.print("Make sure sybil-api is running: cargo run -p sybil-api -- --dev-mode")
            return

        # Create accounts for bots
        console.print("\n[bold]Creating bot accounts...[/bold]")
        accounts = []
        for i in range(NUM_BOTS):
            account = await client.create_account(
                initial_balance_nanos=int(INITIAL_BALANCE * 1_000_000_000)
            )
            accounts.append(account)
            console.print(f"  Bot {i+1}: Account #{account.id} with ${account.balance_dollars:.2f}")

        # Create a test market
        console.print("\n[bold]Creating test market...[/bold]")
        market = await client.create_market("Test: Will this experiment succeed?")
        console.print(f"  Market #{market.id}: {market.name}")

        # Initialize bots
        console.print("\n[bold]Initializing bots...[/bold]")
        bots = [
            SimpleMarketMaker(client, accounts[0].id, spread_bps=100, name="MM-Tight"),
            SimpleMarketMaker(client, accounts[1].id, spread_bps=300, name="MM-Wide"),
            RandomTrader(client, accounts[2].id, trade_probability=0.5, seed=42, name="Random-1"),
            RandomTrader(client, accounts[3].id, trade_probability=0.3, seed=123, name="Random-2"),
        ]
        for bot in bots:
            console.print(f"  {bot.name} (Account #{bot.account_id})")

        # Run competition
        console.print(f"\n[bold]Running competition for {DURATION_SECONDS} seconds...[/bold]")
        tasks = [asyncio.create_task(bot.run()) for bot in bots]

        # Wait for duration
        await asyncio.sleep(DURATION_SECONDS)

        # Stop bots
        for bot in bots:
            bot.stop()

        # Cancel tasks and wait for cleanup
        for task in tasks:
            task.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)

        # Resolve market (50% YES for this test)
        console.print("\n[bold]Resolving market at 50%...[/bold]")
        await client.resolve_market(market.id, 500_000_000)

        # Collect results
        console.print("\n[bold]Final Results:[/bold]\n")
        results = []
        for bot in bots:
            account = await client.get_account(bot.account_id)
            pnl = account.balance_dollars - INITIAL_BALANCE
            results.append({
                "name": bot.name,
                "account_id": bot.account_id,
                "final_balance": account.balance_dollars,
                "pnl": pnl,
                "trades": len(bot.balance_history),
            })

        # Sort by PnL
        results.sort(key=lambda x: x["pnl"], reverse=True)

        # Print leaderboard
        table = Table(title="Competition Leaderboard")
        table.add_column("Rank", style="cyan")
        table.add_column("Bot", style="green")
        table.add_column("Final Balance", justify="right")
        table.add_column("PnL", justify="right")
        table.add_column("Updates", justify="right")

        for i, r in enumerate(results, 1):
            pnl_style = "green" if r["pnl"] >= 0 else "red"
            table.add_row(
                str(i),
                r["name"],
                f"${r['final_balance']:.2f}",
                f"[{pnl_style}]${r['pnl']:+.2f}[/{pnl_style}]",
                str(r["trades"]),
            )

        console.print(table)
        console.print("\n[bold blue]Competition complete![/bold blue]")


if __name__ == "__main__":
    asyncio.run(main())
