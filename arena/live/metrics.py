"""Prometheus metrics owned by the live arena process (SYB-211).

This module covers *only* the observability that the arena process alone can
produce: news-feed poll health, market-selection sizing, and LLM call volume.

The per-trader portfolio series (``sybil_bot_*``) are deliberately NOT
exported here. sybil-api already owns that pipeline: ``crates/sybil-api/src/
arena.rs`` reads the arena SQLite DB (``portfolio_snapshots``) on every scrape
and publishes ``sybil_bot_db_available``, ``sybil_bot_total_orders``, and
``sybil_bot_total_fills``. Re-exporting those from the arena would create
duplicate series on a second scrape target with subtly different values, so we
leave them to the API and only fill the arena-only gaps.

Fail-open (SYB-185): every hook swallows its own exceptions. A metrics failure
must never take down a feed poll or a trader loop. prometheus_client's counter
and gauge mutations are already exception-safe in normal use; the guards are
belt-and-suspenders for the unexpected.
"""

from __future__ import annotations

import logging
import time

from prometheus_client import CollectorRegistry, Counter, Gauge, start_http_server

log = logging.getLogger(__name__)


class ArenaMetrics:
    """Arena-owned Prometheus metrics with fail-open hooks.

    Each instance uses its own ``CollectorRegistry`` so multiple instances
    (e.g. across tests) never collide on the process-global default registry,
    and so the exporter serves exactly this arena's series.
    """

    def __init__(self, registry: CollectorRegistry | None = None):
        self.registry = registry or CollectorRegistry()

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
            "Remaining USD LLM budget per arena agent.",
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

    # -- Hooks (all fail-open) -------------------------------------------- #

    def set_market_selection(self, selected_markets: int, reference_markets: int) -> None:
        try:
            self.selected_markets.set(selected_markets)
            self.selected_reference_markets.set(reference_markets)
        except Exception:  # pragma: no cover - defensive
            log.debug("set_market_selection metrics update failed", exc_info=True)

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
