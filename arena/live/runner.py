"""Live trading bot orchestrator.

Usage:
    cd arena && uv run python -m live.runner --api-key $OPENROUTER_API_KEY
    cd arena && uv run python -m live.runner --api-key $KEY --max-markets 10 --personas news_trader contrarian
"""

import argparse
import asyncio
import logging
import signal
import sys
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

from bots.random_trader import RandomTrader
from sybil_client import SybilClient
from sybil_client.types import NANOS_PER_DOLLAR

from .db import DecisionDB
from .news_feed import NewsFeed
from .personas import PERSONAS
from .trader import LiveLlmTrader

log = logging.getLogger(__name__)


@dataclass
class LiveConfig:
    sybil_url: str = "http://172.104.31.54:3000"
    api_key: str = ""
    model_name: str = "minimax/minimax-m2.7"
    initial_balance: float = 500.0
    max_markets: int = 20
    news_poll_interval: int = 300
    min_llm_interval: float = 60.0
    noise_count: int = 5
    noise_balance: float = 50.0
    db_path: str = ""
    personas: list[str] = field(default_factory=lambda: list(PERSONAS.keys()))
    market_ids: list[int] | None = None  # Manual market selection (overrides auto)
    mapping_path: str | None = None  # Path to polymarket_mapping.json


def select_markets(markets, max_n: int = 20):
    """Pick diverse Polymarket-mirrored markets for trading.

    Avoids picking many markets from the same group (e.g. 18 NBA MVP candidates).
    Prefers standalone markets and picks at most 2 per group prefix.
    """
    active = [
        m for m in markets
        if "polymarket" in m.tags
        and m.status.lower() == "active"
    ]

    # Separate standalone markets from group sub-markets (name contains ":")
    standalone = [m for m in active if ":" not in m.name]
    grouped = [m for m in active if ":" in m.name]

    selected = []

    # Add standalone markets first (these are typically more interesting)
    standalone.sort(key=lambda m: (-m.volume_nanos, m.id))
    selected.extend(standalone)

    # From grouped markets, pick at most 2 per group prefix
    from collections import defaultdict
    groups = defaultdict(list)
    for m in grouped:
        prefix = m.name.split(":")[0].strip()
        groups[prefix].append(m)

    for prefix in sorted(groups, key=lambda p: -len(groups[p])):
        # Sort within group by yes_price closeness to 0.5 (most uncertain = most interesting)
        members = groups[prefix]
        members.sort(key=lambda m: abs(m.yes_price - 0.5))
        selected.extend(members[:2])

    return selected[:max_n]


async def snapshot_portfolios(traders, db: DecisionDB, interval_s: float = 300):
    """Periodically log portfolio snapshots for all traders."""
    while True:
        await asyncio.sleep(interval_s)
        for trader in traders:
            try:
                positions = {}
                for (mid, outcome), qty in trader.positions.items():
                    if qty != 0:
                        positions.setdefault(str(mid), {})[outcome] = qty
                balance = trader.current_balance
                # Simple portfolio value (balance + positions at last known prices)
                pv = balance
                for mid_str, pos in positions.items():
                    mid = int(mid_str)
                    history = getattr(trader, "price_history", {}).get(mid, [])
                    if history:
                        yes_p = history[-1].yes_price
                        pv += pos.get("YES", 0) * yes_p + pos.get("NO", 0) * (1 - yes_p)
                # Count total fills for this trader
                total_fills = sum(
                    len(recs) for recs in getattr(trader, "trade_log", {}).values()
                    if recs
                )
                db.log_snapshot(
                    trader_name=trader.name,
                    balance=balance,
                    portfolio_value=pv,
                    pnl=pv - 500.0,  # starting balance
                    positions=positions,
                    total_fills=total_fills,
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


async def run_live(config: LiveConfig):
    """Main entry point for live trading."""
    # Resolve DB path
    db_path = config.db_path or str(Path(__file__).parent / "decisions.db")
    db = DecisionDB(db_path)
    log.info("Decision DB: %s", db_path)

    async with SybilClient(config.sybil_url) as client:
        # 1. Discover markets
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
            active = select_markets(all_markets, config.max_markets)

        if not active:
            log.error("No suitable markets found!")
            return

        log.info("Selected %d markets for trading:", len(active))
        for m in active:
            log.info("  [%d] %s (YES=%.2f, vol=$%.0f)", m.id, m.name[:60], m.yes_price, m.volume_dollars)

        markets_info = {m.id: m for m in active}
        market_ids = [m.id for m in active]

        # 2. Create accounts
        traders = []
        for persona_key in config.personas:
            if persona_key not in PERSONAS:
                log.warning("Unknown persona: %s, skipping", persona_key)
                continue
            persona = PERSONAS[persona_key]
            account = await client.create_account(int(config.initial_balance * NANOS_PER_DOLLAR))
            log.info("Created account %d for %s ($%.0f)", account.id, persona["name"], config.initial_balance)

            trader = LiveLlmTrader(
                client=client,
                account_id=account.id,
                news_feed=None,  # set below after feed creation
                api_key=config.api_key,
                persona=persona["persona"],
                model_name=config.model_name,
                market_ids=market_ids,
                markets_info=markets_info,
                db=db,
                min_llm_interval_s=config.min_llm_interval,
                name=persona["name"],
            )
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
            noise_traders.append(noise)
        log.info("Created %d noise traders", len(noise_traders))

        # 4. Create news feed (with LLM gate using cheap model)
        feed = NewsFeed(active, api_key=config.api_key, poll_interval_s=config.news_poll_interval,
                        mapping_path=config.mapping_path)

        # Wire feed into traders
        for trader in traders:
            trader.news_feed = feed

        # 5. Run everything
        log.info("Starting live trading with %d LLM traders + %d noise traders on %d markets",
                 len(traders), len(noise_traders), len(active))

        tasks = [
            asyncio.create_task(feed.run(), name="news_feed"),
            asyncio.create_task(snapshot_portfolios(traders, db), name="snapshots"),
            asyncio.create_task(log_articles_loop(feed, db), name="article_logger"),
        ]
        for t in traders:
            tasks.append(asyncio.create_task(t.run(), name=f"trader:{t.name}"))
        for n in noise_traders:
            tasks.append(asyncio.create_task(n.run(), name=f"noise:{n.name}"))

        # Graceful shutdown
        stop_event = asyncio.Event()

        def _signal_handler():
            log.info("Shutdown requested")
            stop_event.set()
            for t in traders:
                t.stop()
            for n in noise_traders:
                n.stop()

        loop = asyncio.get_event_loop()
        for sig in (signal.SIGINT, signal.SIGTERM):
            loop.add_signal_handler(sig, _signal_handler)

        log.info("All systems running. Press Ctrl+C to stop.")

        # Wait for shutdown signal
        await stop_event.wait()
        log.info("Stopping all tasks...")

        # Give traders a moment to finish current block processing
        await asyncio.sleep(3)

        # Cancel remaining tasks
        for task in tasks:
            task.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)

        db.close()
        log.info("Shutdown complete.")


def main():
    parser = argparse.ArgumentParser(description="Live AI trading bots")
    parser.add_argument("--sybil-url", default="http://172.104.31.54:3000")
    parser.add_argument("--api-key", required=True, help="OpenRouter API key")
    parser.add_argument("--model", default="minimax/minimax-m2.7")
    parser.add_argument("--max-markets", type=int, default=20)
    parser.add_argument("--balance", type=float, default=500.0, help="Initial balance per trader")
    parser.add_argument("--noise-count", type=int, default=5)
    parser.add_argument("--news-interval", type=int, default=300, help="RSS poll interval (seconds)")
    parser.add_argument("--min-llm-interval", type=float, default=60.0, help="Min seconds between LLM calls")
    parser.add_argument("--db-path", default="", help="SQLite DB path")
    parser.add_argument("--personas", nargs="+", default=list(PERSONAS.keys()),
                        help="Persona keys to use")
    parser.add_argument("--market-ids", nargs="+", type=int, default=None,
                        help="Manually specify market IDs to trade (overrides --max-markets)")
    parser.add_argument("--mapping-path", default=None,
                        help="Path to polymarket_mapping.json for reference prices")
    parser.add_argument("--log-level", default="INFO")
    args = parser.parse_args()

    logging.basicConfig(
        level=getattr(logging, args.log_level.upper()),
        format="%(asctime)s %(name)-20s %(levelname)-5s %(message)s",
        datefmt="%H:%M:%S",
    )

    config = LiveConfig(
        sybil_url=args.sybil_url,
        api_key=args.api_key,
        model_name=args.model,
        initial_balance=args.balance,
        max_markets=args.max_markets,
        noise_count=args.noise_count,
        news_poll_interval=args.news_interval,
        min_llm_interval=args.min_llm_interval,
        db_path=args.db_path,
        personas=args.personas,
        market_ids=args.market_ids,
        mapping_path=args.mapping_path,
    )

    asyncio.run(run_live(config))


if __name__ == "__main__":
    main()
