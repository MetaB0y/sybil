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

from bots.market_maker import AnchorMarketMaker
from bots.random_trader import RandomTrader
from sybil_client import BuyNo, BuyYes, SybilClient
from sybil_client.types import NANOS_PER_DOLLAR

from .clock import SimulatedClock
from .news_trader import NewsTrader, load_articles
from .results import save_and_print_results

log = logging.getLogger(__name__)


def _load_market_config(market_name: str):
    """Load a MarketConfig by name."""
    import importlib
    mod = importlib.import_module(f"markets.{market_name}")
    return mod.get_config()


def _resolve_phase1_path(market_config, bot_key: str, date: str | None = None) -> str:
    """Resolve the phase1 results path for a bot key."""
    phase1_key = market_config.personas.get(bot_key, {}).get("phase1_bot", bot_key)
    phase1_dir = market_config.phase1_dir
    if date:
        return str(phase1_dir / f"{phase1_key}_{date}_phase1_results.json")
    candidates = sorted(phase1_dir.glob(f"{phase1_key}_*_phase1_results.json"))
    if candidates:
        return str(candidates[-1])
    return str(phase1_dir / f"{phase1_key}_phase1_results.json")


@dataclass
class TraderSpec:
    """Specification for a single LLM trader."""
    name: str
    bot_key: str
    persona: str
    phase1_path: str
    strategy: dict | None = None


@dataclass
class SimulationConfig:
    base_url: str = "http://localhost:3001"
    compression_ratio: float = 300.0
    mm_balance: float = 20_000.0
    mm_seed_qty: int = 10_000
    initial_price: float = 0.12
    noise_count: int = 20
    noise_balance: float = 20.0
    trader_balance: float = 1_000.0
    api_key: str = ""
    model_name: str = "moonshotai/kimi-k2"
    sim_start_hour: str = "00:00"
    sim_end_hour: str = "23:59"
    trader_specs: list[TraderSpec] | None = None
    dates: list[str] | None = None
    # Market-specific fields
    market_question: str = ""
    market_description: str = ""
    market_category: str = ""
    context: str = ""
    analysis_question: str = ""
    phase1_dir: Path | None = None
    runs_dir: Path | None = None


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
        await client.submit_orders(mm_acct.id, [
            BuyYes.at_price(market.id, config.initial_price, config.mm_seed_qty),
            BuyNo.at_price(market.id, 1 - config.initial_price, config.mm_seed_qty),
        ])
        print(f"MM account {mm_acct.id}: seeded {config.mm_seed_qty} shares @ {config.initial_price:.2f}")

        async for block in client.stream_blocks():
            print(f"Seed trade cleared in block {block.height}")
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
                if date_str:
                    path = spec.phase1_path.replace("_phase1_results.json", f"_{date_str}_phase1_results.json")
                    # Try date-specific path first, fall back to original
                    if not Path(path).exists():
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

            mm = AnchorMarketMaker(
                client, mm_acct.id,
                budget_dollars=config.mm_balance,
                name="MM",
                market_ids=[market.id],
            )

            noise_bots = []
            for i in range(config.noise_count):
                acct = await client.create_account(int(config.noise_balance * NANOS_PER_DOLLAR))
                bot = RandomTrader(
                    client, acct.id,
                    trade_probability=0.5,
                    seed=day_idx * 1000 + i,
                    name=f"Noise-{i}",
                    market_ids=[market.id],
                )
                noise_bots.append(bot)
            print(f"  Created {config.noise_count} noise traders @ ${config.noise_balance} each")

            traders = []
            for spec in specs:
                if spec.name not in spec_articles:
                    continue
                t = NewsTrader(
                    client, trader_accounts[spec.name],
                    spec_articles[spec.name], clock,
                    api_key=api_key,
                    persona=spec.persona,
                    analysis_question=config.analysis_question,
                    model_name=config.model_name,
                    name=spec.name,
                    market_ids=[market.id],
                    strategy=spec.strategy,
                )
                if spec.name in trader_state:
                    t.restore_state(trader_state[spec.name])
                traders.append(t)

            if not traders:
                print(f"  ERROR: No traders created for day {day_label}. Skipping.")
                continue

            clock.start()
            all_bots = [mm, *noise_bots, *traders]
            tasks = [asyncio.create_task(bot.run()) for bot in all_bots]

            sim_span = sim_end - sim_start
            real_span = sim_span.total_seconds() / config.compression_ratio
            print(
                f"\n  Simulation started: {len(all_bots)} bots"
                f"\n    Sim time: {sim_start:%H:%M} → {sim_end:%H:%M}"
                f" ({sim_span})"
                f"\n    Real time: ~{real_span:.0f}s"
                f" (compression={config.compression_ratio}x)"
            )

            await clock.sleep_until(sim_end)

            print(f"\n  Stopping bots (day {day_label})...")
            for bot in all_bots:
                bot.stop()
            await asyncio.gather(*tasks, return_exceptions=True)

            for t in traders:
                trader_state[t.name] = t.snapshot_state()

            await save_and_print_results(
                client, config, all_bots, traders, market.id,
                runs_dir=runs_dir,
                day_label=date_str,
                run_id=run_id,
            )


def main():
    parser = argparse.ArgumentParser(description="News-reactive LLM simulation runner")
    parser.add_argument("--market", required=True, help="Market name (e.g. iran)")
    parser.add_argument("--base-url", default="http://localhost:3001")
    parser.add_argument("--compression", type=float, default=300.0,
                        help="Time compression ratio (default: 300)")
    parser.add_argument("--noise-count", type=int, default=20)
    parser.add_argument("--noise-balance", type=float, default=20.0)
    parser.add_argument("--trader-balance", type=float, default=1000.0)
    parser.add_argument("--initial-price", type=float, default=None)
    parser.add_argument("--model", default="moonshotai/kimi-k2")
    parser.add_argument("--api-key", default="")
    parser.add_argument("--traders", nargs="+", metavar="BOT_KEY",
                        help="Bot keys from market personas")
    parser.add_argument("--date", default=None,
                        help="Article date YYYYMMDD for phase1 file lookup (default: most recent)")
    parser.add_argument("--dates", nargs="+", metavar="YYYYMMDD",
                        help="Multiple dates for multi-day simulation")
    parser.add_argument("--sim-start", default="00:00", help="Sim start HH:MM (default: 00:00)")
    parser.add_argument("--sim-end", default="23:59", help="Sim end HH:MM (default: 23:59)")
    parser.add_argument("-v", "--verbose", action="store_true")
    args = parser.parse_args()

    logging.basicConfig(
        level=logging.DEBUG if args.verbose else logging.INFO,
        format="%(asctime)s %(name)s %(levelname)s %(message)s",
    )

    # Load market config
    market_config = _load_market_config(args.market)
    initial_price = args.initial_price if args.initial_price is not None else market_config.initial_price

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
                strategy=bot_cfg.get("strategy"),
            ))
    else:
        # Default: first two tradeable personas
        tradeable = {k: v for k, v in market_config.personas.items() if "persona" in v}
        keys = list(tradeable.keys())[:2]
        trader_specs = []
        for bot_key in keys:
            bot_cfg = tradeable[bot_key]
            trader_specs.append(TraderSpec(
                name=bot_cfg["name"],
                bot_key=bot_key,
                persona=market_config.build_persona(bot_cfg),
                phase1_path=_resolve_phase1_path(market_config, bot_key, args.date),
                strategy=bot_cfg.get("strategy"),
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
        model_name=args.model,
        api_key=args.api_key,
        sim_start_hour=args.sim_start,
        sim_end_hour=args.sim_end,
        trader_specs=trader_specs,
        dates=dates,
        market_question=market_config.question,
        market_description=market_config.description,
        market_category=market_config.category,
        context=market_config.context,
        analysis_question=market_config.analysis_question,
        runs_dir=market_config.runs_dir,
    )

    print(f"{market_config.question}")
    print(f"  Server: {config.base_url}")
    print(f"  Model: {config.model_name}")
    print(f"  Compression: {config.compression_ratio}x")
    print(f"  Noise traders: {config.noise_count} @ ${config.noise_balance}")
    print(f"  Trader balance: ${config.trader_balance}")
    if trader_specs:
        print(f"  Traders: {', '.join(s.name for s in trader_specs)}")
    if dates:
        print(f"  Dates: {', '.join(dates)}")
    print()

    asyncio.run(run_simulation(config))


if __name__ == "__main__":
    main()
