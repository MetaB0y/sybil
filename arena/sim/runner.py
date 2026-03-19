"""Generic simulation runner for news-reactive LLM trading.

Usage:
    cd arena && uv run python -m sim.runner --market iran
    cd arena && uv run python -m sim.runner --market iran --compression 120 --noise-count 10
"""

import argparse
import asyncio
import logging
import os
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

from dotenv import load_dotenv

load_dotenv()

from bots.market_maker import BalancedMarketMaker, FastAnchorMM
from bots.random_trader import RandomTrader
from sybil_client import BuyNo, BuyYes, SybilClient
from sybil_client.types import NANOS_PER_DOLLAR

from .clock import SimulatedClock
from .llm_trader import LlmTrader, load_articles
from .rebalancer import run_rebalancer
from .results import save_and_print_results

log = logging.getLogger(__name__)


def _default_model(trader_specs: list | None) -> str:
    """Derive default model from trader specs, or fall back to a hardcoded default."""
    if trader_specs:
        for s in trader_specs:
            if s.model:
                return s.model
    return "google/gemini-3.1-flash-lite-preview"


def _load_market_config(market_name: str):
    """Load a MarketConfig by name."""
    import importlib
    mod = importlib.import_module(f"markets.{market_name}")
    return mod.get_config()


def _lookup_polymarket_price(prices_file: Path, date_str: str) -> float | None:
    """Look up the Polymarket YES price at the start of a given date (YYYYMMDD)."""
    import json
    if not prices_file.exists():
        return None
    try:
        with open(prices_file) as f:
            prices = json.load(f)
        # Format: [{"timestamp": "2026-01-26T00:00:36+00:00", "yes_price": 0.585}, ...]
        target = f"{date_str[:4]}-{date_str[4:6]}-{date_str[6:8]}"
        for entry in prices:
            if entry["timestamp"].startswith(target):
                return entry["yes_price"]
    except Exception:
        pass
    return None


def _resolve_phase1_path(market_config, bot_key: str, date: str | None = None) -> str:
    """Resolve the phase1 results path for a bot key.

    Returns a template path with {date} placeholder when date is None
    and --dates will be used, or a concrete path when date is given.
    """
    phase1_key = market_config.personas.get(bot_key, {}).get("phase1_bot", bot_key)
    phase1_dir = market_config.phase1_dir
    if date:
        return str(phase1_dir / f"{phase1_key}_{date}_phase1_results.json")
    # Return a template — the per-day loop will substitute {date}
    return str(phase1_dir / f"{phase1_key}_{{date}}_phase1_results.json")


@dataclass
class TraderSpec:
    """Specification for a single LLM trader."""
    name: str
    bot_key: str
    persona: str
    phase1_path: str
    model: str | None = None


@dataclass
class SimulationConfig:
    base_url: str = "http://localhost:3001"
    compression_ratio: float = 300.0
    block_interval_s: float = 2.0
    mm_balance: float = 50_000.0
    initial_price: float = 0.12
    noise_count: int = 10
    noise_balance: float = 50.0
    trader_balance: float = 2_000.0
    api_key: str = ""
    model_name: str = "google/gemini-3.1-flash-lite-preview"
    sim_start_hour: str = "00:00"
    sim_end_hour: str = "23:59"
    trader_specs: list[TraderSpec] | None = None
    dates: list[str] | None = None
    rebalance_interval: float = 4.0
    # Market-specific fields
    market_question: str = ""
    market_description: str = ""
    market_category: str = ""
    context: str = ""
    phase1_dir: Path | None = None
    runs_dir: Path | None = None
    mm_per_side: float = 500.0  # max $ deployed per side per block
    mm_max_blocks: int | None = None  # None = unlimited, 1 = single initial trade
    mm_strategy: str = "balanced"  # "balanced" or "fast-anchor"


async def run_simulation(config: SimulationConfig) -> None:
    async with SybilClient(config.base_url) as client:
        # === ONE-TIME SETUP ===

        market = await client.create_market(
            config.market_question or "Prediction Market",
            description=config.market_description or "",
            category=config.market_category or "",
        )
        print(f"Created market {market.id}: {market.name}")

        mm_acct = await client.create_account(int(config.mm_balance * NANOS_PER_DOLLAR))
        # Seed trade: slight surplus ($1.02 total) guarantees positive welfare
        # so the solver always matches it, even alone in a batch.
        seed_yes = config.initial_price + 0.01
        seed_no = (1 - config.initial_price) + 0.01
        await client.submit_orders(mm_acct.id, [
            BuyYes.at_price(market.id, seed_yes, 1),
            BuyNo.at_price(market.id, seed_no, 1),
        ])
        print(f"MM account {mm_acct.id}: seed price @ {config.initial_price:.2f} (waiting for fill...)")

        # Wait until the seed trade actually clears (market appears in clearing_prices)
        async for block in client.stream_blocks():
            if market.id in block.clearing_prices:
                yes_nanos, _ = block.clearing_prices[market.id]
                print(f"Seed trade cleared in block {block.height} @ YES={yes_nanos / NANOS_PER_DOLLAR:.4f}")
                break

        api_key = config.api_key or os.environ.get("OPENROUTER_API_KEY", "")
        if not api_key:
            print("WARNING: No OPENROUTER_API_KEY set. LLM calls will fail.")

        specs = config.trader_specs or []

        trader_accounts: dict[str, int] = {}
        for spec in specs:
            acct = await client.create_account(int(config.trader_balance * NANOS_PER_DOLLAR))
            trader_accounts[spec.name] = acct.id
            print(f"{spec.name} account {acct.id}: ${config.trader_balance}")

        noise_accounts = []
        for i in range(config.noise_count):
            acct = await client.create_account(int(config.noise_balance * NANOS_PER_DOLLAR))
            noise_accounts.append(acct)

        trader_state: dict[str, dict] = {}
        run_id = datetime.now().strftime("%Y%m%d_%H%M%S")

        dates = config.dates
        if not dates:
            dates = [None]

        runs_dir = config.runs_dir or Path("runs")

        # === PER-DAY LOOP ===
        for day_idx, date_str in enumerate(dates):
            day_label = date_str or "single"
            print(f"\n{'='*70}")
            print(f"DAY {day_idx + 1}/{len(dates)}: {day_label}")
            print(f"{'='*70}")

            spec_articles: dict[str, list] = {}
            for spec in specs:
                if date_str and "{date}" in spec.phase1_path:
                    path = spec.phase1_path.replace("{date}", date_str)
                    if not Path(path).exists():
                        # Fall back: try without date
                        path = spec.phase1_path.replace("_{date}", "")
                elif date_str:
                    path = spec.phase1_path
                else:
                    path = spec.phase1_path
                arts = load_articles(path)
                if not arts:
                    print(f"  WARNING: No articles for {spec.name} from {path}, skipping")
                else:
                    spec_articles[spec.name] = arts
                    print(f"  {spec.name}: {len(arts)} articles from {path}")

            if not spec_articles:
                print(f"  ERROR: No articles for day {day_label}. Skipping.")
                continue

            first_articles = next(iter(spec_articles.values()))
            article_date = first_articles[0].timestamp.date()
            h, m = (int(x) for x in config.sim_start_hour.split(":"))
            sim_start = datetime(article_date.year, article_date.month, article_date.day, h, m)
            h_end, m_end = (int(x) for x in config.sim_end_hour.split(":"))
            sim_end = datetime(article_date.year, article_date.month, article_date.day, h_end, m_end)

            clock = SimulatedClock(
                sim_start=sim_start,
                compression_ratio=config.compression_ratio,
            )

            if config.mm_strategy == "fast-anchor":
                mm = FastAnchorMM(
                    client, mm_acct.id,
                    budget_dollars=config.mm_balance,
                    max_per_side_dollars=config.mm_per_side,
                    name="MM",
                    market_ids=[market.id],
                    max_blocks=config.mm_max_blocks,
                )
            else:
                mm = BalancedMarketMaker(
                    client, mm_acct.id,
                    budget_dollars=config.mm_balance,
                    max_per_side_dollars=config.mm_per_side,
                    name="MM",
                    market_ids=[market.id],
                    max_blocks=config.mm_max_blocks,
                )

            noise_bots = []
            for i, acct in enumerate(noise_accounts):
                bot = RandomTrader(
                    client, acct.id,
                    trade_probability=0.5,
                    seed=day_idx * 1000 + i,
                    name=f"Noise-{i}",
                    market_ids=[market.id],
                )
                noise_bots.append(bot)
            if day_idx == 0:
                print(f"  Created {config.noise_count} noise traders @ ${config.noise_balance} each")
            else:
                print(f"  Reusing {config.noise_count} noise traders")

            traders = []
            for spec in specs:
                if spec.name not in spec_articles:
                    continue
                # Only include articles within the sim window
                window_articles = [
                    a for a in spec_articles[spec.name]
                    if sim_start <= a.timestamp <= sim_end
                ]
                if not window_articles:
                    print(f"  {spec.name}: 0 articles in {config.sim_start_hour}–{config.sim_end_hour}, skipping")
                    continue
                t = LlmTrader(
                    client, trader_accounts[spec.name],
                    window_articles, clock,
                    api_key=api_key,
                    persona=spec.persona,
                    market_question=config.market_question,
                    context=config.context,
                    model_name=spec.model or config.model_name,
                    name=spec.name,
                    market_ids=[market.id],
                )
                if spec.name in trader_state:
                    t.restore_state(trader_state[spec.name])
                traders.append(t)

            if not traders:
                print(f"  ERROR: No traders created for day {day_label}. Skipping.")
                continue

            # Record starting block so results only include this day's blocks
            day_start_block = (await client.get_latest_block()).height

            clock.start()
            all_bots = [mm, *noise_bots, *traders]
            tasks = [asyncio.create_task(bot.run()) for bot in all_bots]

            rebalance_task = None
            if config.rebalance_interval > 0 and traders:
                rebalance_task = asyncio.create_task(
                    run_rebalancer(traders, clock, client, config.rebalance_interval)
                )

            sim_span = sim_end - sim_start
            real_span = sim_span.total_seconds() / config.compression_ratio
            print(
                f"\n  Simulation started: {len(all_bots)} bots"
                f"\n    Sim time: {sim_start:%H:%M} → {sim_end:%H:%M}"
                f" ({sim_span})"
                f"\n    Real time: ~{real_span:.0f}s"
                f" (compression={config.compression_ratio}x)"
            )

            # Block heartbeat monitor
            async def _monitor():
                last_block = 0
                while True:
                    await asyncio.sleep(15)
                    try:
                        blk = await client.get_latest_block()
                        sim_t = clock.now().strftime("%H:%M")
                        if blk.height > last_block:
                            price = blk.clearing_prices.get(market.id, (0, 0))[0] / NANOS_PER_DOLLAR if blk.clearing_prices else 0
                            print(f"\n  ── block {blk.height} | sim {sim_t} | YES={price:.4f} | fills={blk.orders_filled} ──", flush=True)
                            last_block = blk.height
                        else:
                            print(f"\n  ── heartbeat | sim {sim_t} | block {blk.height} (paused — LLM in flight) ──", flush=True)
                    except Exception:
                        pass

            monitor_task = asyncio.create_task(_monitor())
            await clock.sleep_until(sim_end)
            monitor_task.cancel()
            if rebalance_task is not None:
                rebalance_task.cancel()

            print(f"\n  Stopping bots (day {day_label})...")
            for bot in all_bots:
                bot.stop()
            await asyncio.gather(*tasks, return_exceptions=True)

            # Wait for pending orders to settle (TTL up to 5 blocks × 2s)
            await asyncio.sleep(12)

            for t in traders:
                trader_state[t.name] = t.snapshot_state()

            await save_and_print_results(
                client, config, all_bots, traders, market.id,
                runs_dir=runs_dir,
                day_label=date_str,
                run_id=run_id,
                min_block=day_start_block,
            )


def main():
    parser = argparse.ArgumentParser(description="News-reactive LLM simulation runner")
    parser.add_argument("--market", required=True, help="Market name (e.g. iran)")
    parser.add_argument("--base-url", default="http://localhost:3001")
    parser.add_argument("--compression", type=float, default=300.0,
                        help="Time compression ratio (default: 300)")
    parser.add_argument("--noise-count", type=int, default=10)
    parser.add_argument("--noise-balance", type=float, default=50.0)
    parser.add_argument("--trader-balance", type=float, default=2000.0)
    parser.add_argument("--initial-price", type=float, default=None)
    parser.add_argument("--model", default=None,
                        help="Default LLM model (overrides market config per-persona models)")
    parser.add_argument("--api-key", default="")
    parser.add_argument("--traders", nargs="+", metavar="BOT_KEY",
                        help="Bot keys from market personas")
    parser.add_argument("--date", default=None,
                        help="Article date YYYYMMDD for phase1 file lookup (default: most recent)")
    parser.add_argument("--dates", nargs="+", metavar="YYYYMMDD",
                        help="Multiple dates for multi-day simulation")
    parser.add_argument("--sim-start", default="00:00", help="Sim start HH:MM (default: 00:00)")
    parser.add_argument("--sim-end", default="23:59", help="Sim end HH:MM (default: 23:59)")
    parser.add_argument("--rebalance-interval", type=float, default=4,
                        help="Rebalance interval in sim hours (0=disabled, default: 4)")
    parser.add_argument("--mm-max-blocks", type=int, default=None,
                        help="Stop MM after N blocks with trades (default: unlimited, 1=seed only)")
    parser.add_argument("--mm", default="balanced", choices=["balanced", "fast-anchor"],
                        help="MM strategy: balanced (slow anchor) or fast-anchor (default: balanced)")
    parser.add_argument("-v", "--verbose", action="store_true")
    args = parser.parse_args()

    logging.basicConfig(
        level=logging.DEBUG if args.verbose else logging.WARNING,
        format="%(asctime)s %(name)s %(levelname)s %(message)s",
    )
    # Only show sim-level logs at INFO unless verbose
    if not args.verbose:
        logging.getLogger("sim").setLevel(logging.INFO)

    # Load market config
    market_config = _load_market_config(args.market)
    initial_price = args.initial_price if args.initial_price is not None else market_config.initial_price

    # Auto-lookup Polymarket price for the first sim date if available
    first_date = (args.dates or [args.date])[0] if (args.dates or args.date) else None
    if args.initial_price is None and first_date and market_config.polymarket_prices_file:
        poly_price = _lookup_polymarket_price(market_config.polymarket_prices_file, first_date)
        if poly_price is not None:
            initial_price = poly_price
            print(f"  Initial price from Polymarket: {initial_price:.4f} (date {first_date})")

    # Build trader specs from --traders arg or defaults
    trader_specs = None
    if args.traders:
        trader_specs = []
        tradeable = {k: v for k, v in market_config.personas.items() if "persona" in v}
        for bot_key in args.traders:
            if bot_key not in tradeable:
                available = ", ".join(tradeable.keys())
                print(f"Unknown or non-tradeable bot: {bot_key}. Available: {available}")
                return
            bot_cfg = tradeable[bot_key]
            trader_specs.append(TraderSpec(
                name=bot_cfg["name"],
                bot_key=bot_key,
                persona=market_config.build_persona(bot_cfg),
                phase1_path=_resolve_phase1_path(market_config, bot_key, args.date),
                model=bot_cfg.get("model"),
            ))
    else:
        # Default: all tradeable personas that are enabled
        tradeable = {k: v for k, v in market_config.personas.items()
                     if "persona" in v and v.get("enabled", True)}
        keys = list(tradeable.keys())
        trader_specs = []
        for bot_key in keys:
            bot_cfg = tradeable[bot_key]
            trader_specs.append(TraderSpec(
                name=bot_cfg["name"],
                bot_key=bot_key,
                persona=market_config.build_persona(bot_cfg),
                phase1_path=_resolve_phase1_path(market_config, bot_key, args.date),
                model=bot_cfg.get("model"),
            ))

    # Resolve dates
    if args.dates:
        dates = args.dates
    elif args.date:
        dates = [args.date]
    else:
        dates = None

    config = SimulationConfig(
        base_url=args.base_url,
        compression_ratio=args.compression,
        noise_count=args.noise_count,
        noise_balance=args.noise_balance,
        trader_balance=args.trader_balance,
        initial_price=initial_price,
        model_name=args.model or _default_model(trader_specs),
        api_key=args.api_key,
        sim_start_hour=args.sim_start,
        sim_end_hour=args.sim_end,
        trader_specs=trader_specs,
        dates=dates,
        rebalance_interval=args.rebalance_interval,
        market_question=market_config.question,
        market_description=market_config.description,
        market_category=market_config.category,
        context=market_config.context,
        runs_dir=market_config.runs_dir,
        mm_max_blocks=args.mm_max_blocks,
        mm_strategy=args.mm,
    )

    print(f"{market_config.question}")
    print(f"  Server: {config.base_url}")
    print(f"  Default model: {config.model_name}")
    print(f"  MM strategy: {config.mm_strategy}")
    print(f"  Compression: {config.compression_ratio}x")
    print(f"  Noise traders: {config.noise_count} @ ${config.noise_balance}")
    print(f"  Trader balance: ${config.trader_balance}")
    if trader_specs:
        for s in trader_specs:
            model = (s.model or config.model_name).split("/")[-1]
            print(f"  {s.name}: {model}")
    if dates:
        print(f"  Dates: {', '.join(dates)}")
    print()

    asyncio.run(run_simulation(config))


if __name__ == "__main__":
    main()
