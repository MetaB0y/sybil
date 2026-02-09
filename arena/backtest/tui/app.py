"""Textual TUI app for Arena backtest."""

from __future__ import annotations

from typing import TYPE_CHECKING

from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.widgets import Footer

from .widgets import (
    DetailArea,
    LeaderboardPanel,
    MarketsPanel,
    NewsPanel,
    OrdersPanel,
    StatusBar,
    ThoughtsPanel,
)

if TYPE_CHECKING:
    from backtest.runner import BacktestRunner


class SybilTUI(App):
    """Interactive TUI for the Arena backtest dashboard."""

    TITLE = "Sybil Arena"
    CSS_PATH = "styles.tcss"

    BINDINGS = [
        Binding("q", "quit", "Quit"),
        Binding("m", "focus_markets", "Markets"),
        Binding("l", "focus_leaderboard", "Leaderboard"),
        Binding("n", "focus_news", "News"),
        Binding("t", "focus_thoughts", "Thoughts"),
        Binding("o", "focus_orders", "Orders"),
        Binding("d", "focus_detail", "Detail"),
        Binding("escape", "clear_detail", "Back"),
        Binding("c", "copy_panel", "Copy"),
    ]

    def __init__(self, runner: BacktestRunner, real_duration: float, **kwargs) -> None:
        super().__init__(**kwargs)
        self.runner = runner
        self.real_duration = real_duration

    def compose(self) -> ComposeResult:
        yield StatusBar(id="status-bar")
        yield MarketsPanel(id="markets-panel")
        yield DetailArea("[dim]Select a market or agent for details[/dim]", id="detail-area")
        yield NewsPanel(id="news-panel")
        yield ThoughtsPanel(id="thoughts-panel")
        yield OrdersPanel(id="orders-panel")
        yield LeaderboardPanel(id="leaderboard-panel")
        yield Footer()

    def on_mount(self) -> None:
        # Set border titles
        self.query_one("#markets-panel").border_title = "MARKETS"
        self.query_one("#detail-area").border_title = "DETAIL"
        self.query_one("#news-panel").border_title = "NEWS"
        self.query_one("#thoughts-panel").border_title = "THOUGHTS"
        self.query_one("#orders-panel").border_title = "ORDERS"
        self.query_one("#leaderboard-panel").border_title = "LEADERBOARD"

        # Route Python logging into the thoughts panel
        import logging

        thoughts: ThoughtsPanel = self.query_one("#thoughts-panel")

        class _TUIHandler(logging.Handler):
            def emit(self, record: logging.LogRecord) -> None:
                msg = self.format(record)
                try:
                    thoughts.add_log_line(msg)
                except Exception:
                    pass

        handler = _TUIHandler()
        handler.setFormatter(logging.Formatter("%(name)s: %(message)s"))
        logging.getLogger("bots").addHandler(handler)
        logging.getLogger("bots").setLevel(logging.INFO)

        # Start 1Hz refresh timer
        self.set_interval(1.0, self._refresh_all)

        # Start the simulation end worker
        self.run_worker(self._wait_for_end(), thread=False)

    async def _wait_for_end(self) -> None:
        """Wait for simulation duration, then stop agents and exit."""
        import asyncio

        await asyncio.sleep(self.real_duration)

        # Stop agents
        for agent in self.runner._agents:
            agent.stop()

        # Small grace period for cleanup
        await asyncio.sleep(1.0)
        self.exit()

    def _refresh_all(self) -> None:
        status_bar: StatusBar = self.query_one("#status-bar")
        markets: MarketsPanel = self.query_one("#markets-panel")
        detail: DetailArea = self.query_one("#detail-area")
        news: NewsPanel = self.query_one("#news-panel")
        thoughts: ThoughtsPanel = self.query_one("#thoughts-panel")
        orders: OrdersPanel = self.query_one("#orders-panel")
        leaderboard: LeaderboardPanel = self.query_one("#leaderboard-panel")

        status_bar.refresh_data(self.runner)
        markets.refresh_data(self.runner)
        detail.refresh_data(self.runner)
        news.refresh_data(self.runner)
        thoughts.refresh_data(self.runner)
        orders.refresh_data(self.runner)
        leaderboard.refresh_data(self.runner)

    # --- Message handlers ---

    def on_markets_panel_market_selected(self, event: MarketsPanel.MarketSelected) -> None:
        detail: DetailArea = self.query_one("#detail-area")
        detail.show_market(event.market_id, self.runner)

    def on_leaderboard_panel_agent_selected(self, event: LeaderboardPanel.AgentSelected) -> None:
        detail: DetailArea = self.query_one("#detail-area")
        detail.show_agent(event.agent_name, self.runner)

    # --- Key actions ---

    def action_focus_markets(self) -> None:
        self.query_one("#markets-panel").focus()

    def action_focus_leaderboard(self) -> None:
        self.query_one("#leaderboard-panel").focus()

    def action_focus_news(self) -> None:
        self.query_one("#news-panel").focus()

    def action_focus_thoughts(self) -> None:
        self.query_one("#thoughts-panel").focus()

    def action_focus_orders(self) -> None:
        self.query_one("#orders-panel").focus()

    def action_focus_detail(self) -> None:
        self.query_one("#detail-area").focus()

    def action_clear_detail(self) -> None:
        detail: DetailArea = self.query_one("#detail-area")
        detail.clear_selection()

    def action_copy_panel(self) -> None:
        """Copy the focused panel's text content to clipboard."""
        import subprocess

        focused = self.focused
        if focused is None:
            self.notify("No panel focused", severity="warning")
            return

        text = self._extract_text(focused)
        if not text:
            self.notify("Nothing to copy", severity="warning")
            return

        try:
            subprocess.run(
                ["pbcopy"], input=text.encode(), check=True, timeout=2,
            )
            panel_id = focused.id or focused.__class__.__name__
            self.notify(f"Copied {panel_id} ({len(text)} chars)")
        except FileNotFoundError:
            # Not on macOS, try xclip
            try:
                subprocess.run(
                    ["xclip", "-selection", "clipboard"],
                    input=text.encode(), check=True, timeout=2,
                )
                self.notify(f"Copied ({len(text)} chars)")
            except Exception:
                self.notify("No clipboard tool found", severity="error")
        except Exception as e:
            self.notify(f"Copy failed: {e}", severity="error")

    def _extract_text(self, widget) -> str:
        """Extract plain text from a widget."""
        from rich.console import Console
        from rich.text import Text
        from textual.widgets import DataTable, RichLog, Static

        if isinstance(widget, Static):
            # Static stores content as a renderable
            content = widget.renderable
            if isinstance(content, str):
                # Strip Rich markup
                t = Text.from_markup(content)
                return t.plain
            return str(content)

        if isinstance(widget, RichLog):
            console = Console(file=None, force_terminal=False, no_color=True, width=200)
            lines = []
            for line_obj in widget.lines:
                with console.capture() as capture:
                    console.print(line_obj)
                lines.append(capture.get().rstrip())
            return "\n".join(lines)

        if isinstance(widget, DataTable):
            # Build a text table from columns + rows
            col_keys = list(widget.columns.keys())
            col_labels = [str(widget.columns[k].label) for k in col_keys]
            lines = ["\t".join(col_labels)]
            for row_key in widget.rows:
                cells = []
                for ck in col_keys:
                    val = widget.get_cell(row_key, ck)
                    cells.append(str(val))
                lines.append("\t".join(cells))
            return "\n".join(lines)

        return ""
