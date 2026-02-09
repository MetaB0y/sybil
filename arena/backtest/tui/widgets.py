"""TUI widgets for the Arena backtest dashboard."""

from __future__ import annotations

from typing import TYPE_CHECKING

from textual.message import Message
from textual.widgets import DataTable, RichLog, Static

if TYPE_CHECKING:
    from backtest.agent import BacktestAgent
    from backtest.runner import BacktestRunner

NANOS_PER_DOLLAR = 1_000_000_000


def _position_value(agent: BacktestAgent, prices: dict[int, tuple[int, int]]) -> float:
    """Mark-to-market value of an agent's positions in dollars.

    YES shares valued at yes_price, NO shares at no_price (= 1 - yes_price).
    """
    total = 0.0
    for (market_id, outcome), qty in agent.positions.items():
        if qty == 0:
            continue
        market_prices = prices.get(market_id)
        if market_prices is None:
            continue
        yes_nanos, no_nanos = market_prices
        if outcome == "YES":
            total += qty * yes_nanos / NANOS_PER_DOLLAR
        else:
            total += qty * no_nanos / NANOS_PER_DOLLAR
    return total


def _market_short_name(display_name: str) -> str:
    parts = display_name.split(" vs ")
    if len(parts) == 2:
        return " vs ".join(p.split()[-1][:3].upper() for p in parts)
    return display_name[:10]


class StatusBar(Static):
    """Top-level status: sim time, elapsed, news count, block height."""

    def refresh_data(self, runner: BacktestRunner) -> None:
        clock = runner._clock
        ns = runner._news_scheduler
        if clock is None or ns is None:
            return
        sim_time = clock.now().strftime("%H:%M")
        elapsed_hrs = clock.elapsed_sim_time().total_seconds() / 3600
        news_count = ns.delivered_count
        total_news = len(runner.dataset.news)
        block_height = runner._last_block.height if runner._last_block else 0
        self.update(
            f" Sim {sim_time} | {elapsed_hrs:.1f} hrs | "
            f"News: {news_count}/{total_news} | Blk #{block_height}"
        )


class MarketsPanel(DataTable):
    """Market rows with agent estimates, clickable."""

    class MarketSelected(Message):
        def __init__(self, market_id: int) -> None:
            super().__init__()
            self.market_id = market_id

    def __init__(self, **kwargs) -> None:
        super().__init__(**kwargs)
        self._market_row_keys: dict[int, str] = {}  # market_id -> row key for click handling
        self._prev_agent_names: list[str] | None = None  # None = never built

    def on_mount(self) -> None:
        self.cursor_type = "row"

    def on_data_table_row_selected(self, event: DataTable.RowSelected) -> None:
        for mid, rk in self._market_row_keys.items():
            if rk == event.row_key.value:
                self.post_message(self.MarketSelected(mid))
                break

    def refresh_data(self, runner: BacktestRunner) -> None:
        estimate_agents = [a for a in runner._agents if a.beliefs]
        agent_names = [a.name for a in estimate_agents]

        # Rebuild columns when the set of agents with beliefs changes
        if agent_names != self._prev_agent_names:
            self.clear(columns=True)
            self._market_row_keys.clear()
            self.add_column("Game", key="game")
            self.add_column("Price", key="price")
            for name in agent_names:
                self.add_column(name, key=name)
            self._prev_agent_names = agent_names

        for market_id, display_name in sorted(runner._market_display_names.items()):
            prices = runner._latest_prices.get(market_id)
            if prices is None:
                price_str = "--"
                market_prob = 0.5
            else:
                market_prob = prices[0] / NANOS_PER_DOLLAR
                price_str = f"{market_prob * 100:.0f}%"

            row = [display_name, price_str]
            for agent in estimate_agents:
                belief = agent.beliefs.get(market_id)
                if belief is None:
                    row.append("--")
                    continue
                est = belief.probability
                edge = est - market_prob
                est_pct = f"{est * 100:.0f}%"
                if edge > 0.03:
                    row.append(f"{est_pct}^")
                elif edge < -0.03:
                    row.append(f"{est_pct}v")
                else:
                    row.append(est_pct)

            rk = f"market-{market_id}"
            if rk in self._market_row_keys.values():
                col_keys = ["game", "price"] + agent_names
                for ci, val in enumerate(row):
                    self.update_cell(rk, col_keys[ci], val)
            else:
                self.add_row(*row, key=rk)
                self._market_row_keys[market_id] = rk


class DetailArea(Static):
    """Context-dependent detail: market or agent info."""

    def __init__(self, content="", **kwargs) -> None:
        super().__init__(content, **kwargs)
        self._selected_market: int | None = None
        self._selected_agent: str | None = None

    def show_market(self, market_id: int, runner: BacktestRunner) -> None:
        self._selected_market = market_id
        self._selected_agent = None
        self._render_market(market_id, runner)

    def show_agent(self, agent_name: str, runner: BacktestRunner) -> None:
        self._selected_agent = agent_name
        self._selected_market = None
        self._render_agent(agent_name, runner)

    def clear_selection(self) -> None:
        self._selected_market = None
        self._selected_agent = None
        self.update("[dim]Select a market or agent for details[/dim]")

    def refresh_data(self, runner: BacktestRunner) -> None:
        if self._selected_market is not None:
            self._render_market(self._selected_market, runner)
        elif self._selected_agent is not None:
            self._render_agent(self._selected_agent, runner)

    def _render_market(self, market_id: int, runner: BacktestRunner) -> None:
        name = runner._market_display_names.get(market_id, f"Market {market_id}")
        prices = runner._latest_prices.get(market_id)

        lines = [f"[bold]{name}[/bold]  (id={market_id})"]
        if prices:
            yes_p = prices[0] / NANOS_PER_DOLLAR * 100
            lines.append(f"Price: YES {yes_p:.1f}%  NO {100 - yes_p:.1f}%")
        else:
            lines.append("Price: --")

        # Positions per agent
        lines.append("")
        lines.append("[bold]Positions:[/bold]")
        for agent in runner._agents:
            yes_qty = agent.positions.get((market_id, "YES"), 0)
            no_qty = agent.positions.get((market_id, "NO"), 0)
            if yes_qty or no_qty:
                lines.append(f"  {agent.name}: YES={yes_qty} NO={no_qty}")

        # Beliefs
        lines.append("")
        lines.append("[bold]Agent Estimates:[/bold]")
        for agent in runner._agents:
            belief = agent.beliefs.get(market_id)
            if belief:
                lines.append(
                    f"  {agent.name}: {belief.probability * 100:.1f}% "
                    f"(conf={belief.confidence:.2f})"
                )

        self.update("\n".join(lines))

    def _render_agent(self, agent_name: str, runner: BacktestRunner) -> None:
        agent: BacktestAgent | None = None
        for a in runner._agents:
            if a.name == agent_name:
                agent = a
                break
        if agent is None:
            self.update(f"[red]Agent '{agent_name}' not found[/red]")
            return

        bal = agent.balance_history[-1] if agent.balance_history else 0.0
        pos_val = _position_value(agent, runner._latest_prices)
        total = bal + pos_val
        pnl = total - runner.initial_balance

        lines = [
            f"[bold]{agent.name}[/bold]",
            f"Cash: ${bal:.2f}  Positions: ${pos_val:.2f}  Total: ${total:.2f}",
            f"PnL: ${pnl:+.2f} ({pnl / runner.initial_balance * 100:+.1f}%)",
            f"Orders submitted: {agent.total_orders_submitted}",
        ]

        # Reasoning
        reasoning = getattr(agent, "last_reasoning", "")
        if reasoning:
            lines.append("")
            lines.append("[bold]Last Reasoning:[/bold]")
            lines.append(reasoning[:500])

        # Positions
        pos_items = [
            (k, v) for k, v in agent.positions.items() if v != 0
        ]
        if pos_items:
            lines.append("")
            lines.append("[bold]Positions:[/bold]")
            for (mid, outcome), qty in sorted(pos_items):
                short = _market_short_name(
                    runner._market_display_names.get(mid, f"M{mid}")
                )
                lines.append(f"  {short} {outcome}: {qty}")

        # Beliefs
        if agent.beliefs:
            lines.append("")
            lines.append("[bold]Beliefs:[/bold]")
            for mid, belief in sorted(agent.beliefs.items()):
                short = _market_short_name(
                    runner._market_display_names.get(mid, f"M{mid}")
                )
                lines.append(f"  {short}: {belief.probability * 100:.1f}%")

        self.update("\n".join(lines))


class NewsPanel(RichLog):
    """Scrollable news feed."""

    def __init__(self, **kwargs) -> None:
        super().__init__(**kwargs)
        self._last_count = 0

    def refresh_data(self, runner: BacktestRunner) -> None:
        ns = runner._news_scheduler
        if ns is None:
            return
        current = ns.delivered_count
        if current <= self._last_count:
            return
        # Append only new items (recent_news is newest-first, up to 10)
        recent = ns.recent_news
        new_count = current - self._last_count
        # recent is newest-first, take up to new_count from the front
        new_items = list(reversed(recent[:new_count]))
        for news in new_items:
            ts = news.timestamp.strftime("%H:%M")
            game_tag = ""
            if news.event_id:
                mid = runner._market_ids.get(news.event_id)
                if mid is not None:
                    display = runner._market_display_names.get(mid, "")
                    game_tag = f"[{_market_short_name(display)}] "
            from bots.strategy_agent import format_news_line

            self.write(f"{ts}  {game_tag}{format_news_line(news)}")
        self._last_count = current


class ThoughtsPanel(RichLog):
    """Scrollable LLM reasoning history + errors."""

    def __init__(self, **kwargs) -> None:
        super().__init__(markup=True, **kwargs)
        self._seen: dict[str, str] = {}
        self._seen_errors: dict[str, str] = {}

    def add_log_line(self, line: str) -> None:
        """Add a line from a logging handler."""
        self.write(line)

    def refresh_data(self, runner: BacktestRunner) -> None:
        clock = runner._clock
        if clock is None:
            return
        sim_time = clock.now().strftime("%H:%M")
        for agent in runner._agents:
            # Show reasoning
            reasoning = getattr(agent, "last_reasoning", "")
            if reasoning and reasoning != self._seen.get(agent.name):
                self._seen[agent.name] = reasoning
                text = reasoning[:200] + "..." if len(reasoning) > 200 else reasoning
                # Escape any Rich markup in the reasoning text itself
                from rich.markup import escape
                self.write(f"{sim_time} [bold]{agent.name}[/bold]: {escape(text)}")
            # Show errors
            last_error = getattr(agent, "last_error", "")
            if last_error and last_error != self._seen_errors.get(agent.name):
                self._seen_errors[agent.name] = last_error
                from rich.markup import escape
                self.write(f"{sim_time} [red][bold]{agent.name}[/bold] ERROR: {escape(last_error)}[/red]")


class OrdersPanel(Static):
    """Per-agent orders + last block info."""

    def refresh_data(self, runner: BacktestRunner) -> None:
        from sybil_client import BuyNo, BuyYes, SellNo, SellYes

        lines: list[str] = []

        # Block summary first
        if runner._last_block:
            b = runner._last_block
            vol = b.total_volume / NANOS_PER_DOLLAR
            vol_str = f" vol=${vol:.2f}" if vol > 0 else ""
            lines.append(f"[bold]Blk #{b.height}[/bold] {b.orders_filled} fills{vol_str}")

            our_market_ids = set(runner._market_ids.values())
            price_parts = []
            for market_id in sorted(our_market_ids):
                prices = b.clearing_prices.get(market_id)
                if prices is None:
                    continue
                yes_pct = prices[0] / NANOS_PER_DOLLAR * 100
                if abs(yes_pct - 50) < 0.5:
                    continue
                short = _market_short_name(
                    runner._market_display_names.get(market_id, f"M{market_id}")
                )
                price_parts.append(f"{short}={yes_pct:.0f}%")
            if price_parts:
                lines.append(f"  {', '.join(price_parts)}")
            lines.append("")

        for agent in runner._agents:
            orders = agent.last_orders
            if not orders:
                lines.append(f"[dim]{agent.name}[/dim] —")
                continue

            is_mm = "MM" in (agent.name or "")
            if is_mm:
                buy_yes = [o for o in orders if isinstance(o, BuyYes)]
                buy_no = [o for o in orders if isinstance(o, BuyNo)]
                parts = []
                if buy_yes:
                    parts.append(f"[green]BY {sum(o.quantity for o in buy_yes)}[/green]")
                if buy_no:
                    parts.append(f"[red]BN {sum(o.quantity for o in buy_no)}[/red]")
                lines.append(
                    f"[bold]{agent.name}[/bold] {' '.join(parts)} "
                    f"[dim]({agent.total_orders_submitted} tot)[/dim]"
                )
            else:
                lines.append(f"[bold]{agent.name}[/bold]")
                by_market: dict[int, list] = {}
                for o in orders:
                    by_market.setdefault(o.market_id, []).append(o)
                for mid, morders in sorted(by_market.items()):
                    short = _market_short_name(
                        runner._market_display_names.get(mid, f"M{mid}")
                    )
                    for o in morders:
                        price = o.limit_price_nanos / NANOS_PER_DOLLAR
                        if isinstance(o, BuyYes):
                            lines.append(f"  [green]BY[/green] {short} {o.quantity}@{price:.0%}")
                        elif isinstance(o, BuyNo):
                            lines.append(f"  [red]BN[/red] {short} {o.quantity}@{price:.0%}")
                        elif isinstance(o, SellYes):
                            lines.append(f"  [yellow]SY[/yellow] {short} {o.quantity}@{price:.0%}")
                        elif isinstance(o, SellNo):
                            lines.append(f"  [yellow]SN[/yellow] {short} {o.quantity}@{price:.0%}")

        self.update("\n".join(lines) if lines else "[dim]No orders yet[/dim]")


class LeaderboardPanel(DataTable):
    """Ranked agents, clickable for detail."""

    class AgentSelected(Message):
        def __init__(self, agent_name: str) -> None:
            super().__init__()
            self.agent_name = agent_name

    def __init__(self, **kwargs) -> None:
        super().__init__(**kwargs)
        self._columns_built = False

    def on_mount(self) -> None:
        self.cursor_type = "row"

    def on_data_table_row_selected(self, event: DataTable.RowSelected) -> None:
        # Row key is the agent name
        self.post_message(self.AgentSelected(event.row_key.value))

    def refresh_data(self, runner: BacktestRunner) -> None:
        if not self._columns_built:
            self.add_column("#", key="rank")
            self.add_column("Agent", key="agent")
            self.add_column("Cash", key="cash")
            self.add_column("Pos$", key="pos_value")
            self.add_column("Total", key="total")
            self.add_column("PnL", key="pnl")
            self._columns_built = True

        prices = runner._latest_prices

        # Compute total value (cash + positions) for sorting
        def _total(agent):
            if not agent.balance_history:
                return 0.0
            cash = agent.balance_history[-1]
            return cash + _position_value(agent, prices)

        ranked = sorted(runner._agents, key=_total, reverse=True)

        self.clear()
        for i, agent in enumerate(ranked, 1):
            if agent.balance_history:
                cash = agent.balance_history[-1]
                pos_val = _position_value(agent, prices)
                total = cash + pos_val
                pnl = total - runner.initial_balance
                self.add_row(
                    str(i),
                    agent.name,
                    f"${cash:.0f}",
                    f"${pos_val:.0f}",
                    f"${total:.0f}",
                    f"${pnl:+.0f}",
                    key=agent.name,
                )
            else:
                self.add_row(str(i), agent.name, *["..."] * 4, key=agent.name)
