"""Live trading bot orchestrator.

Usage:
    cd arena && OPENROUTER_API_KEY=... uv run python -m live.runner
    cd arena && OPENROUTER_API_KEY=... uv run python -m live.runner --max-markets 10
"""

import argparse
import asyncio
import logging
import os
import signal
from dataclasses import dataclass, field
from pathlib import Path

from bots.random_trader import RandomTrader
from sybil_client import SybilClient
from sybil_client.types import NANOS_PER_DOLLAR, TimeInForce

from .analyst import PersonaAnalyst
from .db import DecisionDB
from .fair_value_bus import FairValueBus
from .market_selection import MarketProfile, select_markets
from .metrics import ArenaMetrics, start_metrics_server
from .news_feed import NewsFeed
from .personas import PERSONAS
from .strategy import FlatStrategy, KellyStrategy
from .trader import LiveLlmTrader

log = logging.getLogger(__name__)

# When --require-reference-prices is set, arena may start before the Polymarket
# mirror has published any reference prices. Rather than exit, poll for a
# reference-backed market set on this cadence until one appears.
MARKET_DISCOVERY_RETRY_SECONDS = 30


@dataclass
class LiveConfig:
    sybil_url: str = "http://172.104.31.54:3000"
    api_key: str = ""
    model_name: str = "deepseek/deepseek-v4-flash"
    initial_balance: float = 500.0
    max_markets: int = 0
    market_profile: MarketProfile = "all"
    require_reference_prices: bool = False
    order_time_in_force: TimeInForce = "IOC"
    news_poll_interval: int = 300
    min_llm_interval: float = 60.0
    # SYB-64: per-analyst LLM budget (USD). The analyst is a persona's sole LLM
    # caller and holds no trading account, so this is a separate pool from the
    # sizers' trading bankroll. Exhausting it pauses the persona's analyst.
    # None (or <=0 on the CLI) disables the budget (unlimited).
    llm_budget_usd: float | None = 5.0
    noise_count: int = 5
    noise_balance: float = 50.0
    db_path: str = ""
    metrics_host: str = "0.0.0.0"
    metrics_port: int = 0  # <=0 disables the exporter (default: off)
    personas: list[str] = field(default_factory=lambda: list(PERSONAS.keys()))
    market_ids: list[int] | None = None  # Manual market selection (overrides auto)
    mapping_path: str | None = None  # Path to polymarket_mapping.json


def _env_int(name: str, default: int) -> int:
    raw = os.environ.get(name, "").strip()
    if not raw:
        return default
    return int(raw)


def _env_market_profile(name: str, default: MarketProfile = "all") -> MarketProfile:
    raw = os.environ.get(name, "").strip()
    if not raw:
        return default
    if raw in ("all", "important-news"):
        return raw
    raise ValueError(f"{name} must be one of: all, important-news")


def _fallback_unfiltered_markets(markets, max_n: int = 0, require_reference_price: bool = False):
    """Return active mirrored markets without profile scoring or grouping."""
    def is_active_mirrored(market) -> bool:
        tags = {
            str(tag).strip().lower().replace("-", " ")
            for tag in getattr(market, "tags", [])
        }
        if "polymarket" not in tags:
            return False
        if str(getattr(market, "status", "")).lower() != "active":
            return False
        if require_reference_price:
            ref = getattr(market, "reference_price_nanos", None)
            if ref is None or ref <= 0:
                return False
        return True

    active = [
        m
        for m in markets
        if is_active_mirrored(m)
    ]
    active.sort(key=lambda m: (-getattr(m, "volume_nanos", 0), getattr(m, "id", 0)))
    if max_n <= 0:
        return active
    return active[:max_n]


def _select_markets_resilient(
    markets,
    max_n: int = 0,
    profile: MarketProfile = "all",
    require_reference_price: bool = False,
):
    try:
        return select_markets(
            markets, max_n, profile, require_reference_price=require_reference_price
        )
    except Exception as e:
        log.warning(
            "Market selection failed for profile=%s: %s; falling back to unfiltered markets",
            profile,
            e,
            exc_info=True,
        )
        return _fallback_unfiltered_markets(
            markets, max_n, require_reference_price=require_reference_price
        )


async def snapshot_portfolios(traders, db: DecisionDB, interval_s: float = 300):
    """Periodically log portfolio snapshots for all traders."""
    while True:
        await asyncio.sleep(interval_s)
        for trader in traders:
            try:
                portfolio = await trader.client.get_portfolio(trader.account_id)
                positions = {}
                for (mid, outcome), qty in trader.positions.items():
                    if qty != 0:
                        positions.setdefault(str(mid), {})[outcome] = qty
                balance = portfolio.balance_dollars
                pv = portfolio.portfolio_value_dollars
                total_fills = len(getattr(trader, "_fill_history", []))
                total_orders = getattr(trader, "total_orders_submitted", 0)
                db.log_snapshot(
                    trader_name=trader.name,
                    balance=balance,
                    portfolio_value=pv,
                    pnl=portfolio.pnl_dollars,
                    positions=positions,
                    total_fills=total_fills,
                    total_orders=total_orders,
                )
            except Exception as e:
                log.warning("Snapshot error for %s: %s", trader.name, e)


async def log_articles_loop(feed: NewsFeed, db: DecisionDB, interval_s: float = 30):
    """Periodically flush new articles from the feed into the DB."""
    while True:
        await asyncio.sleep(interval_s)
        articles = feed.drain_all_new()
        for article in articles:
            db.log_article(article)


async def supervise_bot(agent, stop_event: asyncio.Event, restart_delay_s: float = 5.0):
    """Run one bot with per-task restart supervision."""
    while not stop_event.is_set():
        try:
            await agent.run()
        except asyncio.CancelledError:
            raise
        except Exception:
            if stop_event.is_set():
                break
            log.exception("Bot task %s failed unexpectedly; restarting", agent.name)
        else:
            if stop_event.is_set():
                break
            log.error("Bot task %s exited unexpectedly; restarting", agent.name)

        if not stop_event.is_set():
            try:
                await asyncio.wait_for(stop_event.wait(), timeout=restart_delay_s)
            except TimeoutError:
                pass


async def _resolve_bot_account(
    client: SybilClient,
    db: DecisionDB,
    persona_key: str,
    strat_label: str,
    initial_balance_nanos: int,
    bot_name: str,
) -> int:
    """Reattach a (persona, strategy) bot to its persisted account, or mint one.

    AR-3: restarts must not abandon portfolios. We look up the persisted
    account id for this (persona, strategy) pair and reuse it when the account
    still exists on the server; otherwise we create a fresh account and persist
    the mapping so the next restart reattaches.
    """
    existing = db.get_bot_account_id(persona_key, strat_label)
    if existing is not None:
        try:
            await client.get_account(existing)
            log.info("Reattached %s to existing account %d", bot_name, existing)
            return existing
        except Exception as e:
            log.warning(
                "Persisted account %d for %s is unusable (%s); creating a new one",
                existing, bot_name, e,
            )

    account = await client.create_account(initial_balance_nanos)
    db.save_bot_account_id(persona_key, strat_label, account.id)
    log.info(
        "Created account %d for %s ($%.2f)",
        account.id, bot_name, initial_balance_nanos / NANOS_PER_DOLLAR,
    )
    return account.id


async def run_live(config: LiveConfig):
    """Main entry point for live trading."""
    # Resolve DB path
    db_path = config.db_path or str(Path(__file__).parent / "decisions.db")
    db = DecisionDB(db_path)
    log.info("Decision DB: %s", db_path)

    # Arena-owned metrics exporter (off by default; sybil-api owns sybil_bot_*).
    metrics = ArenaMetrics()
    metrics_server = start_metrics_server(metrics, config.metrics_port, config.metrics_host)
    if metrics_server is not None:
        log.info(
            "Arena metrics listening on %s:%d", config.metrics_host, config.metrics_port
        )

    async with SybilClient(config.sybil_url) as client:
        # 1. Discover markets. When reference prices are required, arena may
        # start before the Polymarket mirror has published any; retry instead of
        # exiting so a cold start self-heals once the mirror catches up.
        active = []
        while not active:
            all_markets = await client.list_markets()
            log.info("Total markets on server: %d", len(all_markets))

            if config.market_ids:
                # Manual market selection by ID
                market_by_id = {m.id: m for m in all_markets}
                active = []
                for mid in config.market_ids:
                    if mid in market_by_id:
                        active.append(market_by_id[mid])
                    else:
                        log.warning("Market ID %d not found on server, skipping", mid)
                log.info("Manual market selection: %d markets", len(active))
            else:
                active = _select_markets_resilient(
                    all_markets,
                    config.max_markets,
                    config.market_profile,
                    require_reference_price=config.require_reference_prices,
                )

            metrics.set_market_selection(
                len(active),
                sum(1 for m in active if (getattr(m, "reference_price_nanos", 0) or 0) > 0),
            )
            if active:
                break

            if not config.require_reference_prices:
                log.error("No suitable markets found!")
                return

            log.warning(
                "No reference-backed markets found for profile=%s; retrying in %ss",
                config.market_profile,
                MARKET_DISCOVERY_RETRY_SECONDS,
            )
            await asyncio.sleep(MARKET_DISCOVERY_RETRY_SECONDS)

        log.info(
            "Selected %d markets for trading with profile=%s:",
            len(active),
            config.market_profile,
        )
        for m in active:
            log.info(
                "  [%d] %s (YES=%.2f, vol=$%.0f)",
                m.id,
                m.name[:60],
                m.yes_price,
                m.volume_dollars,
            )

        markets_info = {m.id: m for m in active}
        market_ids = [m.id for m in active]

        # 2. Create accounts — each persona gets two bots (Kelly + Flat)
        strategies = [
            ("Kelly", KellyStrategy()),
            ("Flat", FlatStrategy()),
        ]

        # SYB-210: split analysis from sizing. Each persona gets ONE analyst
        # (the sole LLM caller) publishing onto a per-persona FairValueBus, and
        # TWO sizers (Kelly + Flat) subscribing to that same bus. Both sizing
        # arms therefore consume identical fair-value updates, and the analysis
        # LLM is called N times per batch instead of 2N.
        analysts = []
        traders = []
        for persona_key in config.personas:
            if persona_key not in PERSONAS:
                log.warning("Unknown persona: %s, skipping", persona_key)
                continue
            persona = PERSONAS[persona_key]

            bus = FairValueBus(persona_key=persona_key)
            analyst = PersonaAnalyst(
                client=client,
                news_feed=None,  # attached below after feed creation
                bus=bus,
                api_key=config.api_key,
                persona=persona["persona"],
                persona_key=persona_key,
                model_name=config.model_name,
                market_ids=market_ids,
                markets_info=markets_info,
                db=db,
                min_llm_interval_s=config.min_llm_interval,
                name=f"{persona['name']} (Analyst)",
                metrics=metrics,
                llm_budget_usd=config.llm_budget_usd,
            )
            analysts.append(analyst)

            for strat_label, strategy in strategies:
                bot_name = f"{persona['name']} ({strat_label})"
                account_id = await _resolve_bot_account(
                    client, db, persona_key, strat_label,
                    int(config.initial_balance * NANOS_PER_DOLLAR),
                    bot_name,
                )

                trader = LiveLlmTrader(
                    client=client,
                    account_id=account_id,
                    news_feed=None,  # set below after feed creation
                    strategy=strategy,
                    market_ids=market_ids,
                    markets_info=markets_info,
                    db=db,
                    name=bot_name,
                    fair_value_bus=bus,
                )
                trader.time_in_force = config.order_time_in_force
                traders.append(trader)

        # 3. Create noise traders
        noise_traders = []
        for i in range(config.noise_count):
            account = await client.create_account(int(config.noise_balance * NANOS_PER_DOLLAR))
            noise = RandomTrader(
                client=client,
                account_id=account.id,
                name=f"Noise-{i}",
                market_ids=market_ids,
                seed=i + 42,
            )
            noise.time_in_force = config.order_time_in_force
            noise_traders.append(noise)
        log.info("Created %d noise traders", len(noise_traders))

        # 4. Create news feed (with LLM gate using cheap model)
        feed = NewsFeed(active, api_key=config.api_key, poll_interval_s=config.news_poll_interval,
                        mapping_path=config.mapping_path, metrics=metrics)

        # Wire feed into analysts (news subscription) and sizers (price cache
        # only). Each analyst registers its own subscriber view of the feed so
        # every persona sees every article (SYB-192); the two sizers of a
        # persona no longer subscribe to news — they consume the analyst's
        # FairValueBus instead (SYB-210).
        for analyst in analysts:
            analyst.attach_feed_and_bus(feed, analyst.bus)
        for trader in traders:
            trader.attach_news_feed(feed)

        # 5. Run everything
        log.info(
            "Starting live trading with %d analysts + %d sizers + %d noise traders on %d markets",
            len(analysts), len(traders), len(noise_traders), len(active),
        )

        stop_event = asyncio.Event()
        tasks = [
            asyncio.create_task(feed.run(), name="news_feed"),
            asyncio.create_task(snapshot_portfolios(traders, db), name="snapshots"),
            asyncio.create_task(log_articles_loop(feed, db), name="article_logger"),
        ]
        for a in analysts:
            tasks.append(asyncio.create_task(supervise_bot(a, stop_event), name=f"analyst:{a.name}"))
        for t in traders:
            tasks.append(asyncio.create_task(supervise_bot(t, stop_event), name=f"trader:{t.name}"))
        for n in noise_traders:
            tasks.append(asyncio.create_task(supervise_bot(n, stop_event), name=f"noise:{n.name}"))

        # Graceful shutdown
        def _signal_handler():
            log.info("Shutdown requested")
            stop_event.set()
            for a in analysts:
                a.stop()
            for t in traders:
                t.stop()
            for n in noise_traders:
                n.stop()

        loop = asyncio.get_event_loop()
        for sig in (signal.SIGINT, signal.SIGTERM):
            loop.add_signal_handler(sig, _signal_handler)

        log.info("All systems running. Press Ctrl+C to stop.")

        stop_task = asyncio.create_task(stop_event.wait(), name="stop_signal")
        watched_tasks = [stop_task, *tasks]
        done, _ = await asyncio.wait(watched_tasks, return_when=asyncio.FIRST_COMPLETED)

        failure: BaseException | None = None
        if stop_task in done:
            log.info("Stopping all tasks...")
        else:
            stop_event.set()
            for task in done:
                if task is stop_task:
                    continue
                exc = task.exception()
                if exc is not None:
                    log.error("Task %s failed: %s", task.get_name(), exc)
                    failure = exc
                else:
                    log.error("Task %s exited unexpectedly", task.get_name())
                    failure = RuntimeError(f"Task {task.get_name()} exited unexpectedly")
            log.info("Stopping all tasks after worker failure...")
            for a in analysts:
                a.stop()
            for t in traders:
                t.stop()
            for n in noise_traders:
                n.stop()

        # Give traders a moment to finish current block processing
        await asyncio.sleep(3)

        # Cancel remaining tasks
        for task in watched_tasks:
            task.cancel()
        await asyncio.gather(*watched_tasks, return_exceptions=True)

        db.close()
        if metrics_server is not None:
            try:
                server, _thread = metrics_server
                server.shutdown()
            except Exception:
                log.debug("Metrics server shutdown failed", exc_info=True)
        log.info("Shutdown complete.")
        if failure is not None:
            raise failure


def main():
    parser = argparse.ArgumentParser(description="Live AI trading bots")
    parser.add_argument("--sybil-url", default="http://172.104.31.54:3000")
    parser.add_argument("--model", default="deepseek/deepseek-v4-flash")
    parser.add_argument(
        "--max-markets",
        type=int,
        default=None,
        help=(
            "Maximum markets for bots to trade. Defaults to ARENA_MAX_MARKETS or 0. "
            "For --market-profile=all, 0 means all suitable mirrored markets; focused "
            "profiles use their profile default."
        ),
    )
    parser.add_argument(
        "--market-profile",
        choices=["all", "important-news"],
        default=None,
        help=(
            "Market selection profile for automatic market discovery. Defaults to "
            "ARENA_MARKET_PROFILE or all."
        ),
    )
    parser.add_argument(
        "--require-reference-prices",
        action="store_true",
        help="Only auto-select markets with live external reference prices.",
    )
    parser.add_argument(
        "--order-time-in-force",
        choices=["GTC", "IOC", "GTD"],
        default="IOC",
        help="Time-in-force for live bot/noise orders. IOC avoids stale resting orders.",
    )
    parser.add_argument("--balance", type=float, default=500.0, help="Initial balance per trader")
    parser.add_argument("--noise-count", type=int, default=5)
    parser.add_argument(
        "--news-interval", type=int, default=300, help="RSS poll interval (seconds)"
    )
    parser.add_argument(
        "--min-llm-interval",
        type=float,
        default=60.0,
        help="Min seconds between LLM calls",
    )
    parser.add_argument(
        "--llm-budget-usd",
        type=float,
        default=5.0,
        help=(
            "Per-analyst LLM budget in USD (SYB-64). When an analyst's spend "
            "reaches this, it pauses (no more LLM calls / fair values). "
            "<=0 disables the budget (unlimited)."
        ),
    )
    parser.add_argument("--db-path", default="", help="SQLite DB path")
    parser.add_argument(
        "--metrics-port",
        type=int,
        default=0,
        help="Prometheus exporter port for arena metrics; <=0 (default) disables it.",
    )
    parser.add_argument(
        "--metrics-host",
        default="0.0.0.0",
        help="Bind host for the arena metrics exporter.",
    )
    parser.add_argument("--personas", nargs="+", default=list(PERSONAS.keys()),
                        help="Persona keys to use")
    parser.add_argument("--market-ids", nargs="+", type=int, default=None,
                        help="Manually specify market IDs to trade (overrides --max-markets)")
    parser.add_argument("--mapping-path", default=None,
                        help="Path to polymarket_mapping.json for reference prices")
    parser.add_argument("--log-level", default="INFO")
    args = parser.parse_args()
    try:
        max_markets = args.max_markets if args.max_markets is not None else _env_int(
            "ARENA_MAX_MARKETS", 0
        )
        market_profile = args.market_profile or _env_market_profile("ARENA_MARKET_PROFILE")
    except ValueError as e:
        parser.error(str(e))

    api_key = os.environ.get("OPENROUTER_API_KEY", "")
    if not api_key:
        parser.error(
            "OPENROUTER_API_KEY is required in the environment; do not pass it as a CLI argument"
        )

    logging.basicConfig(
        level=getattr(logging, args.log_level.upper()),
        format="%(asctime)s %(name)-20s %(levelname)-5s %(message)s",
        datefmt="%H:%M:%S",
    )
    logging.getLogger("httpx").setLevel(logging.WARNING)

    config = LiveConfig(
        sybil_url=args.sybil_url,
        api_key=api_key,
        model_name=args.model,
        initial_balance=args.balance,
        max_markets=max_markets,
        market_profile=market_profile,
        require_reference_prices=args.require_reference_prices,
        order_time_in_force=args.order_time_in_force,
        noise_count=args.noise_count,
        news_poll_interval=args.news_interval,
        min_llm_interval=args.min_llm_interval,
        llm_budget_usd=args.llm_budget_usd if args.llm_budget_usd > 0 else None,
        db_path=args.db_path,
        metrics_host=args.metrics_host,
        metrics_port=args.metrics_port,
        personas=args.personas,
        market_ids=args.market_ids,
        mapping_path=args.mapping_path,
    )

    asyncio.run(run_live(config))


if __name__ == "__main__":
    main()
