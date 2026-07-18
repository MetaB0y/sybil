"""Prometheus metrics owned by the live arena process (SYB-211).

This includes the per-trader portfolio series derived from Arena's private
SQLite database. Keeping the collector beside the writer prevents Rust/API
services from depending on Python-owned storage or mounting its volume.

Fail-open (SYB-185): every hook swallows its own exceptions. A metrics failure
must never take down a feed poll or a trader loop. prometheus_client's counter
and gauge mutations are already exception-safe in normal use; the guards are
belt-and-suspenders for the unexpected.
"""

from __future__ import annotations

import logging
import sqlite3
import time
from pathlib import Path

from prometheus_client import CollectorRegistry, Counter, Gauge, start_http_server
from prometheus_client.core import GaugeMetricFamily

log = logging.getLogger(__name__)


class BotPortfolioCollector:
    """Scrape-time bot totals from the latest Arena portfolio snapshots."""

    def __init__(self, db_path: str):
        self.db_path = db_path

    def collect(self):
        available = GaugeMetricFamily(
            "sybil_bot_db_available", "Whether the Arena decision database is readable."
        )
        traders = GaugeMetricFamily(
            "sybil_bot_traders_total", "Traders with a durable Arena portfolio snapshot."
        )
        fills = GaugeMetricFamily(
            "sybil_bot_total_fills",
            "Latest cumulative fill count reported by each Arena trader.",
            labels=["trader"],
        )
        orders = GaugeMetricFamily(
            "sybil_bot_total_orders",
            "Latest cumulative order count reported by each Arena trader.",
            labels=["trader"],
        )
        rows = []
        try:
            if not self.db_path or not Path(self.db_path).exists():
                raise FileNotFoundError(self.db_path)
            uri = Path(self.db_path).resolve().as_uri() + "?mode=ro"
            with sqlite3.connect(uri, uri=True, timeout=0.75) as conn:
                columns = {
                    str(row[1])
                    for row in conn.execute("PRAGMA table_info(portfolio_snapshots)").fetchall()
                }
                if not {"total_fills", "total_orders"}.issubset(columns):
                    raise sqlite3.OperationalError("portfolio snapshot totals are unavailable")
                rows = conn.execute(
                    "SELECT p.trader_name, p.total_fills, p.total_orders "
                    "FROM portfolio_snapshots p JOIN ("
                    "SELECT trader_name, MAX(id) AS id FROM portfolio_snapshots "
                    "GROUP BY trader_name) latest "
                    "ON p.trader_name = latest.trader_name AND p.id = latest.id"
                ).fetchall()
            available.add_metric([], 1)
        except (OSError, sqlite3.Error):
            available.add_metric([], 0)
            log.debug("Arena portfolio metrics unavailable", exc_info=True)
        traders.add_metric([], len(rows))
        for trader, total_fills, total_orders in rows:
            fills.add_metric([str(trader)], total_fills or 0)
            orders.add_metric([str(trader)], total_orders or 0)
        yield available
        yield traders
        yield fills
        yield orders


class ArenaMetrics:
    """Arena-owned Prometheus metrics with fail-open hooks.

    Each instance uses its own ``CollectorRegistry`` so multiple instances
    (e.g. across tests) never collide on the process-global default registry,
    and so the exporter serves exactly this arena's series.
    """

    def __init__(
        self,
        registry: CollectorRegistry | None = None,
        *,
        db_path: str | None = None,
    ):
        self.registry = registry or CollectorRegistry()
        if db_path is not None:
            self.registry.register(BotPortfolioCollector(db_path))

        # -- Market selection (arena-only: which markets the runner picked) --
        self.selected_markets = Gauge(
            "sybil_arena_selected_markets",
            "Markets selected for live arena trading.",
            registry=self.registry,
        )
        self.selected_reference_markets = Gauge(
            "sybil_arena_selected_reference_markets",
            "Selected arena markets that have an external reference price.",
            registry=self.registry,
        )
        self.synthetic_markets = Gauge(
            "sybil_arena_synthetic_markets",
            "Active public markets covered by Arena synthetic traders.",
            registry=self.registry,
        )
        # -- News feed poll health (arena-only: feed internals) --
        self.news_feed_poll_in_progress = Gauge(
            "sybil_news_feed_poll_in_progress",
            "1 while the arena news feed is mid-poll, else 0.",
            registry=self.registry,
        )
        self.news_feed_last_candidates = Gauge(
            "sybil_news_feed_last_candidates",
            "Candidate headlines seen in the latest news poll.",
            registry=self.registry,
        )
        self.news_feed_last_relevant_articles = Gauge(
            "sybil_news_feed_last_relevant_articles",
            "Relevant articles delivered in the latest news poll.",
            registry=self.registry,
        )
        self.news_feed_latest_poll_start = Gauge(
            "sybil_news_feed_latest_poll_start_timestamp_seconds",
            "Unix time the latest arena news poll started (0 = never).",
            registry=self.registry,
        )
        self.news_feed_latest_poll_success = Gauge(
            "sybil_news_feed_latest_poll_success_timestamp_seconds",
            "Unix time the latest arena news poll succeeded (0 = never).",
            registry=self.registry,
        )
        self.news_feed_polls = Counter(
            "sybil_news_feed_polls",
            "Arena news feed poll cycles started.",
            registry=self.registry,
        )
        self.news_feed_poll_errors = Counter(
            "sybil_news_feed_poll_errors",
            "Arena news feed poll cycles that raised.",
            registry=self.registry,
        )
        self.news_feed_relevant_articles = Counter(
            "sybil_news_feed_relevant_articles",
            "Relevant articles delivered by the arena news feed.",
            registry=self.registry,
        )

        # -- LLM call volume (arena-only: SYB-193 per-call gated trader loop) --
        self.llm_calls = Counter(
            "sybil_arena_llm_calls",
            "LLM analysis calls issued by arena traders.",
            ["trader"],
            registry=self.registry,
        )
        # -- LLM cost accounting (SYB-64: USD spend + budget state per agent) --
        self.llm_cost_usd = Counter(
            "sybil_arena_llm_cost_usd",
            "Cumulative USD LLM cost incurred by arena agents.",
            ["trader"],
            registry=self.registry,
        )
        self.llm_budget_remaining_usd = Gauge(
            "sybil_arena_llm_budget_remaining_usd",
            "Remaining local Arena experiment budget per agent; not provider credit.",
            ["trader"],
            registry=self.registry,
        )
        self.llm_paused = Gauge(
            "sybil_arena_llm_paused",
            "1 while an arena agent is paused for exhausting its LLM budget, else 0.",
            ["trader"],
            registry=self.registry,
        )
        self.analyst_parse_fallbacks = Counter(
            "sybil_arena_analyst_parse_fallbacks",
            "Structured analyst response fields that fell back to conservative defaults.",
            ["trader", "field"],
            registry=self.registry,
        )
        self.llm_provider_failures = Counter(
            "sybil_arena_llm_provider_failures",
            "OpenAI-compatible provider failures classified by caller and kind.",
            ["component", "kind"],
            registry=self.registry,
        )
        self.llm_provider_degraded = Gauge(
            "sybil_arena_llm_provider_degraded",
            "1 when the caller's latest provider attempt failed, else 0.",
            ["component"],
            registry=self.registry,
        )
        self.llm_provider_last_success = Gauge(
            "sybil_arena_llm_provider_last_success_timestamp_seconds",
            "Unix time of the caller's latest successful provider response (0 = never).",
            ["component"],
            registry=self.registry,
        )
        self.llm_provider_last_failure = Gauge(
            "sybil_arena_llm_provider_last_failure_timestamp_seconds",
            "Unix time of the caller's latest failed provider response (0 = never).",
            ["component"],
            registry=self.registry,
        )
        self.llm_provider_backoff_until = Gauge(
            "sybil_arena_llm_provider_backoff_until_timestamp_seconds",
            "Unix time before which the caller suppresses provider attempts (0 = no backoff).",
            ["component"],
            registry=self.registry,
        )
        self.orders_suppressed = Counter(
            "sybil_arena_orders_suppressed",
            "Arena-generated orders suppressed before API submission.",
            ["trader", "reason"],
            registry=self.registry,
        )

    # -- Hooks (all fail-open) -------------------------------------------- #

    def set_market_selection(self, selected_markets: int, reference_markets: int) -> None:
        try:
            self.selected_markets.set(selected_markets)
            self.selected_reference_markets.set(reference_markets)
        except Exception:  # pragma: no cover - defensive
            log.debug("set_market_selection metrics update failed", exc_info=True)

    def set_synthetic_market_selection(self, selected_markets: int) -> None:
        try:
            self.synthetic_markets.set(selected_markets)
        except Exception:  # pragma: no cover - defensive
            log.debug("set_synthetic_market_selection metrics update failed", exc_info=True)

    def record_news_poll_start(self) -> None:
        try:
            self.news_feed_polls.inc()
            self.news_feed_poll_in_progress.set(1)
            self.news_feed_latest_poll_start.set(time.time())
        except Exception:  # pragma: no cover - defensive
            log.debug("record_news_poll_start metrics update failed", exc_info=True)

    def record_news_poll_success(self, candidates: int, relevant_articles: int) -> None:
        try:
            self.news_feed_poll_in_progress.set(0)
            self.news_feed_latest_poll_success.set(time.time())
            self.news_feed_last_candidates.set(candidates)
            self.news_feed_last_relevant_articles.set(relevant_articles)
            if relevant_articles:
                self.news_feed_relevant_articles.inc(relevant_articles)
        except Exception:  # pragma: no cover - defensive
            log.debug("record_news_poll_success metrics update failed", exc_info=True)

    def record_news_poll_error(self) -> None:
        try:
            self.news_feed_poll_in_progress.set(0)
            self.news_feed_poll_errors.inc()
        except Exception:  # pragma: no cover - defensive
            log.debug("record_news_poll_error metrics update failed", exc_info=True)

    def record_llm_call(self, trader_name: str) -> None:
        try:
            self.llm_calls.labels(trader=trader_name).inc()
        except Exception:  # pragma: no cover - defensive
            log.debug("record_llm_call metrics update failed", exc_info=True)

    def record_llm_cost(
        self, trader_name: str, usd_cost: float, budget_remaining: float | None
    ) -> None:
        try:
            if usd_cost > 0:
                self.llm_cost_usd.labels(trader=trader_name).inc(usd_cost)
            # Only publish the remaining-budget gauge for budgeted agents; an
            # unlimited agent (budget None) has no meaningful "remaining".
            if budget_remaining is not None:
                self.llm_budget_remaining_usd.labels(trader=trader_name).set(budget_remaining)
        except Exception:  # pragma: no cover - defensive
            log.debug("record_llm_cost metrics update failed", exc_info=True)

    def set_llm_paused(self, trader_name: str, paused: bool) -> None:
        try:
            self.llm_paused.labels(trader=trader_name).set(1 if paused else 0)
        except Exception:  # pragma: no cover - defensive
            log.debug("set_llm_paused metrics update failed", exc_info=True)

    def record_analyst_parse_fallback(self, trader_name: str, field: str) -> None:
        try:
            self.analyst_parse_fallbacks.labels(trader=trader_name, field=field).inc()
        except Exception:  # pragma: no cover - defensive
            log.debug("record_analyst_parse_fallback metrics update failed", exc_info=True)

    def record_llm_provider_failure(
        self,
        component: str,
        kind: str,
        backoff_seconds: float,
    ) -> None:
        try:
            now = time.time()
            self.llm_provider_failures.labels(component=component, kind=kind).inc()
            self.llm_provider_degraded.labels(component=component).set(1)
            self.llm_provider_last_failure.labels(component=component).set(now)
            self.llm_provider_backoff_until.labels(component=component).set(
                now + backoff_seconds if backoff_seconds > 0 else 0
            )
        except Exception:  # pragma: no cover - defensive
            log.debug("record_llm_provider_failure metrics update failed", exc_info=True)

    def record_llm_provider_success(self, component: str) -> None:
        try:
            self.llm_provider_degraded.labels(component=component).set(0)
            self.llm_provider_last_success.labels(component=component).set(time.time())
            self.llm_provider_backoff_until.labels(component=component).set(0)
        except Exception:  # pragma: no cover - defensive
            log.debug("record_llm_provider_success metrics update failed", exc_info=True)

    def record_order_suppressed(self, trader_name: str, reason: str, count: int = 1) -> None:
        try:
            self.orders_suppressed.labels(trader=trader_name, reason=reason).inc(count)
        except Exception:  # pragma: no cover - defensive
            log.debug("record_order_suppressed metrics update failed", exc_info=True)


def start_metrics_server(
    metrics: ArenaMetrics,
    port: int,
    host: str = "0.0.0.0",
):
    """Start the Prometheus exporter, or no-op when ``port <= 0``.

    Returns the ``(server, thread)`` pair from ``start_http_server`` when a
    server is started, else ``None``. Any bind/start failure is swallowed so a
    metrics problem can never stop the arena from trading.
    """
    if port <= 0:
        return None
    try:
        return start_http_server(port, addr=host, registry=metrics.registry)
    except Exception:
        log.warning("Failed to start arena metrics server on %s:%d", host, port, exc_info=True)
        return None
