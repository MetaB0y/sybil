"""Iran strike market simulation runner.

Usage:
    cd arena && uv run python -m iran.runner
    cd arena && uv run python -m iran.runner --compression 120 --noise-count 10
"""

import argparse
import asyncio
import json
import logging
import os

from dotenv import load_dotenv

load_dotenv()
from dataclasses import asdict, dataclass
from datetime import datetime, timedelta
from pathlib import Path

from bots.market_maker import AnchorMarketMaker
from bots.random_trader import RandomTrader
from sybil_client import BuyNo, BuyYes, SybilClient
from sybil_client.types import NANOS_PER_DOLLAR, PricePoint

from .clock import SimulatedClock
from .news_explorer import BOT_PERSONAS
from .news_trader import IranNewsTrader, load_articles

log = logging.getLogger(__name__)

# ── Persona template ──

_CONTEXT = """\
Context:
USA-Iran tensions stem from long-standing issues like Iran's nuclear program and proxies, but escalated sharply after the June 2025 US strikes on Iranian nuclear sites during the Israel-Iran Twelve-Day War. They rose further in early January 2026 amid Iran's crackdown on anti-government protests, prompting President Trump to threaten military action and review strike options."""


def build_persona(bot_config: dict) -> str:
    """Build a full persona prompt from a BOT_PERSONAS entry."""
    p = bot_config["persona"]
    style_lines = "\n".join(f"- {s}" for s in p["style"])
    return f"""\
You are {p['identity']}.

You're trading on the market: "Will the United States carry out a military strike against Iran before March 31, 2026?"

{_CONTEXT}

Your analytical style:
{style_lines}"""


def _resolve_phase1_path(bot_key: str, date: str | None = None) -> str:
    """Resolve the phase1 results path for a bot key.

    If date is given, looks for iran/tmp/{phase1_key}_{date}_phase1_results.json.
    Otherwise, finds the most recent phase1 file for this bot.
    """
    phase1_key = BOT_PERSONAS.get(bot_key, {}).get("phase1_bot", bot_key)
    phase1_dir = Path("iran/tmp")
    if date:
        return str(phase1_dir / f"{phase1_key}_{date}_phase1_results.json")
    # Find most recent
    candidates = sorted(phase1_dir.glob(f"{phase1_key}_*_phase1_results.json"))
    if candidates:
        return str(candidates[-1])
    return str(phase1_dir / f"{phase1_key}_phase1_results.json")


@dataclass
class TraderSpec:
    """Specification for a single LLM trader."""
    name: str
    bot_key: str          # e.g. "american_believer", for phase1 path resolution
    persona: str
    phase1_path: str
    strategy: dict | None = None  # overrides for IranNewsTrader strategy params

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
    sim_start_hour: str = "00:00"  # HH:MM on the article date
    sim_end_hour: str = "23:59"
    trader_specs: list[TraderSpec] | None = None
    dates: list[str] | None = None  # e.g. ["20260101", "20260102", "20260103"]


async def run_simulation(config: SimulationConfig) -> None:
    async with SybilClient(config.base_url) as client:
        # === ONE-TIME SETUP ===

        # 1. Create market
        market = await client.create_market(
            "Will US strike Iran by March 31?",
            description="Resolves YES if US military strikes Iran before 2026-03-31",
            category="geopolitics",
        )
        print(f"Created market {market.id}: {market.name}")

        # 2. Create MM account + seed trade
        mm_acct = await client.create_account(int(config.mm_balance * NANOS_PER_DOLLAR))
        await client.submit_orders(mm_acct.id, [
            BuyYes.at_price(market.id, config.initial_price, config.mm_seed_qty),
            BuyNo.at_price(market.id, 1 - config.initial_price, config.mm_seed_qty),
        ])
        print(f"MM account {mm_acct.id}: seeded {config.mm_seed_qty} shares @ {config.initial_price:.2f}")

        # 3. Wait for seed trade to clear
        async for block in client.stream_blocks():
            print(f"Seed trade cleared in block {block.height}")
            break

        # 4. Resolve API key and trader specs
        api_key = config.api_key or os.environ.get("OPENROUTER_API_KEY", "")
        if not api_key:
            print("WARNING: No OPENROUTER_API_KEY set. LLM calls will fail.")

        if config.trader_specs:
            specs = config.trader_specs
        else:
            specs = [
                TraderSpec(
                    name="Believer",
                    bot_key="american_believer",
                    persona=build_persona(BOT_PERSONAS["american_believer"]),
                    phase1_path=_resolve_phase1_path("american_believer"),
                    strategy=BOT_PERSONAS["american_believer"].get("strategy"),
                ),
                TraderSpec(
                    name="Skeptic",
                    bot_key="american_skeptic",
                    persona=build_persona(BOT_PERSONAS["american_skeptic"]),
                    phase1_path=_resolve_phase1_path("american_skeptic"),
                    strategy=BOT_PERSONAS["american_skeptic"].get("strategy"),
                ),
            ]

        # 5. Create trader accounts ONCE (persist across days)
        trader_accounts: dict[str, int] = {}
        for spec in specs:
            acct = await client.create_account(int(config.trader_balance * NANOS_PER_DOLLAR))
            trader_accounts[spec.name] = acct.id
            print(f"{spec.name} account {acct.id}: ${config.trader_balance}")

        # Cross-day state
        trader_state: dict[str, dict] = {}  # name -> snapshot from IranNewsTrader.snapshot_state()

        # Run ID for grouping multi-day saves in dashboard
        run_id = datetime.now().strftime("%Y%m%d_%H%M%S")

        # Resolve date list
        dates = config.dates
        if not dates:
            # Fall back to single-day: use spec.phase1_path as-is
            dates = [None]

        # === PER-DAY LOOP ===
        for day_idx, date_str in enumerate(dates):
            day_label = date_str or "single"
            print(f"\n{'='*70}")
            print(f"DAY {day_idx + 1}/{len(dates)}: {day_label}")
            print(f"{'='*70}")

            # 1. Load articles for each trader
            spec_articles: dict[str, list] = {}
            for spec in specs:
                if date_str:
                    path = _resolve_phase1_path(spec.bot_key, date=date_str)
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

            # 2. New clock for this day
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

            # 3. New MM instance (same account)
            mm = AnchorMarketMaker(
                client, mm_acct.id,
                budget_dollars=config.mm_balance,
                name="MM",
                market_ids=[market.id],
            )

            # 4. New noise bots (fresh accounts each day)
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

            # 5. Create LLM traders (new instances, restore cross-day state)
            traders = []
            for spec in specs:
                if spec.name not in spec_articles:
                    continue
                t = IranNewsTrader(
                    client, trader_accounts[spec.name],
                    spec_articles[spec.name], clock,
                    api_key=api_key,
                    model_name=config.model_name,
                    name=spec.name,
                    market_ids=[market.id],
                    persona=spec.persona,
                    strategy=spec.strategy,
                )
                # Restore cross-day state
                if spec.name in trader_state:
                    t.restore_state(trader_state[spec.name])
                traders.append(t)

            if not traders:
                print(f"  ERROR: No traders created for day {day_label}. Skipping.")
                continue

            # 6. Start clock + all bots
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

            # 7. Wait for sim end
            await clock.sleep_until(sim_end)

            # 8. Stop all bots
            print(f"\n  Stopping bots (day {day_label})...")
            for bot in all_bots:
                bot.stop()
            await asyncio.gather(*tasks, return_exceptions=True)

            # 9. Save cross-day state
            for t in traders:
                trader_state[t.name] = t.snapshot_state()

            # 10. Save this day's results (incremental!)
            await save_and_print_results(
                client, config, all_bots, traders, market.id,
                day_label=date_str,
                run_id=run_id,
            )


async def _fetch_all_fills(client, account_id: int) -> list:
    """Fetch all fills for an account, paginating if needed."""
    all_fills = []
    offset = 0
    while True:
        batch = await client.get_account_fills(account_id, limit=100, offset=offset)
        all_fills.extend(batch)
        if len(batch) < 100:
            break
        offset += len(batch)
    return all_fills


def build_block_records(
    all_bots, mm, noise_bots, traders: list, price_history: list[PricePoint],
    trader_fills_map: dict[str, list] | None = None,
    mm_fills: list | None = None,
    noise_fills: list | None = None,
    sim_start: datetime | None = None,
    compression_ratio: float = 300.0,
) -> list[dict]:
    """Join per-bot block_logs with server price history into per-block records.

    traders: list of IranNewsTrader instances (each with .name, .trade_log, .block_log)
    trader_fills_map: {trader.name: [AccountFill, ...]}
    """
    from .news_trader import _describe_order

    if trader_fills_map is None:
        trader_fills_map = {}

    # 1. Collect all block heights seen by any bot
    all_heights: set[int] = set()
    for bot in all_bots:
        for height, _ in bot.block_log:
            all_heights.add(height)

    # 2. Index price history by block height
    price_by_height = {pt.height: pt for pt in price_history}

    # 3. Index trader LLM data by block height, per trader
    llm_by_block: dict[int, list[dict]] = {}
    for t in traders:
        for rec in t.trade_log:
            if rec.block_height >= 0:
                llm_by_block.setdefault(rec.block_height, []).append({
                    "trader": t.name,
                    "article_title": rec.article.title,
                    "article_source": rec.article.source,
                    "probability": rec.probability,
                    "conviction": rec.conviction,
                    "motivation": rec.motivation,
                    "llm_response": rec.llm_response,
                    "llm_duration_s": rec.llm_duration_s,
                })

    # 4. Index fills by block height
    def _index_fills(raw_fills: list | None, source: str) -> dict[int, list[dict]]:
        by_height: dict[int, list[dict]] = {}
        if raw_fills:
            for f in raw_fills:
                deltas = [
                    {"market_id": d.market_id, "outcome": d.outcome, "delta": d.delta}
                    for d in f.position_deltas
                ]
                by_height.setdefault(f.block_height, []).append({
                    "source": source,
                    "order_id": f.order_id,
                    "fill_qty": f.fill_qty,
                    "fill_price": f.fill_price_nanos / NANOS_PER_DOLLAR,
                    "position_deltas": deltas,
                })
        return by_height

    # Merge all trader fills into one index (tagged by source=trader.name)
    all_trader_fills_by_height: dict[int, list[dict]] = {}
    for tname, fills in trader_fills_map.items():
        for h, entries in _index_fills(fills, tname).items():
            all_trader_fills_by_height.setdefault(h, []).extend(entries)
    mm_fills_by_height = _index_fills(mm_fills, "MM")
    noise_fills_by_height = _index_fills(noise_fills, "Noise")

    # 5. Pre-index bot orders by block height.
    # Orders generated in on_block(N) go into the mempool and clear in block N+1,
    # so we shift by +1 to align orders with the block where they actually execute.
    mm_by_height: dict[int, list] = {}
    for h, orders in mm.block_log:
        mm_by_height.setdefault(h + 1, []).extend(orders)

    noise_by_height: dict[int, list] = {}
    for nb in noise_bots:
        for h, orders in nb.block_log:
            noise_by_height.setdefault(h + 1, []).extend(orders)

    # Per-trader order index
    trader_orders_by_height: dict[int, list[tuple[str, list]]] = {}
    for t in traders:
        for h, orders in t.block_log:
            trader_orders_by_height.setdefault(h + 1, []).append((t.name, orders))

    # 5a. Compute sim_time from block height (simple linear mapping since server
    # pauses during LLM calls, so every block = one tick of simulated time).
    sim_time_by_height: dict[int, str] = {}
    if sim_start and all_heights:
        first_height = min(all_heights)
        for h in all_heights:
            offset = (h - first_height) * compression_ratio
            st = sim_start + timedelta(seconds=offset)
            sim_time_by_height[h] = st.isoformat()

    # 5b. Build records
    records = []
    for height in sorted(all_heights):
        pt = price_by_height.get(height)
        mm_orders = mm_by_height.get(height, [])
        noise_orders = noise_by_height.get(height, [])

        # Flatten all trader orders for this block
        trader_entries = trader_orders_by_height.get(height, [])
        all_trader_orders = []
        for tname, orders in trader_entries:
            all_trader_orders.extend(
                {"trader": tname, "order": _describe_order(o)} for o in orders
            )

        rec = {
            "height": height,
            "timestamp_ms": pt.timestamp_ms if pt else None,
            "sim_time": sim_time_by_height.get(height),
            "yes_price": pt.yes_price_nanos / NANOS_PER_DOLLAR if pt else None,
            "volume_nanos": pt.volume_nanos if pt else 0,
            "mm_orders": [_describe_order(o) for o in mm_orders],
            "noise_orders": [_describe_order(o) for o in noise_orders],
            "noise_order_count": len(noise_orders),
            # Backward-compat: flat list of order strings
            "trader_orders": [e["order"] for e in all_trader_orders],
            # Per-trader detail
            "trader_orders_detail": all_trader_orders,
            "trader_fills": all_trader_fills_by_height.get(height, []),
            "mm_fills": mm_fills_by_height.get(height, []),
            "noise_fills": noise_fills_by_height.get(height, []),
            "trader_llm": llm_by_block.get(height, []),
        }
        records.append(rec)

    # 5c. Compute active trader orders with TTL=3 carry-over
    # Trader orders persist for 3 blocks; fills reduce remaining qty.
    active_orders: list[dict] = []  # {qty, submitted_block}
    for rec in records:
        h = rec["height"]
        # Expire orders past TTL
        active_orders = [o for o in active_orders if h - o["submitted_block"] < 3]
        # Add new trader orders BEFORE subtracting fills (same-block fills)
        for o_str in rec["trader_orders"]:
            parts = o_str.split()
            if len(parts) >= 2:
                try:
                    qty = int(parts[1])
                except ValueError:
                    qty = 0
                active_orders.append({"qty": qty, "submitted_block": h})
        # Count before fill subtraction (= what the solver saw)
        rec["active_trader_orders"] = len(active_orders)
        # Subtract fills (covers both carry-over and same-block orders)
        for f in rec["trader_fills"]:
            remaining = f["fill_qty"]
            for o in active_orders:
                if remaining <= 0:
                    break
                if o["qty"] > 0:
                    taken = min(o["qty"], remaining)
                    o["qty"] -= taken
                    remaining -= taken
        active_orders = [o for o in active_orders if o["qty"] > 0]

    return records


async def save_and_print_results(client, config, all_bots, traders: list, market_id, day_label=None, run_id=None):
    mm = all_bots[0]  # first bot is always the MM
    num_traders = len(traders)
    noise_bots = all_bots[1:-num_traders]  # middle bots are noise

    print("\n" + "=" * 70)
    print("SIMULATION RESULTS")
    print("=" * 70)

    # Leaderboard
    print("\n--- Leaderboard ---")
    print(f"{'Name':<20} {'Balance':>10} {'PosValue':>10} {'Total':>10} {'PnL':>10}")
    print("-" * 62)

    leaderboard = []
    for bot in all_bots:
        try:
            portfolio = await client.get_portfolio(bot.account_id)
            pos_val = portfolio.total_position_value_nanos / NANOS_PER_DOLLAR
            total = portfolio.portfolio_value_nanos / NANOS_PER_DOLLAR
            yes_qty = sum(p.quantity for p in portfolio.positions if p.outcome == "YES")
            no_qty = sum(p.quantity for p in portfolio.positions if p.outcome == "NO")
            leaderboard.append({
                "name": bot.name,
                "account_id": bot.account_id,
                "balance": portfolio.balance_dollars,
                "yes_shares": yes_qty,
                "no_shares": no_qty,
                "position_value": pos_val,
                "portfolio_value": total,
                "pnl": portfolio.pnl_dollars,
            })
        except Exception as e:
            log.warning("Failed to get portfolio for %s: %s", bot.name, e)

    leaderboard.sort(key=lambda r: r["pnl"], reverse=True)
    for r in leaderboard:
        print(
            f"{r['name']:<20} "
            f"${r['balance']:>9.2f} "
            f"${r['position_value']:>9.2f} "
            f"${r['portfolio_value']:>9.2f} "
            f"${r['pnl']:>+9.2f}"
        )

    # Trade logs — one per trader
    for t in traders:
        print(f"\n--- {t.name} Trade Log ({len(t.trade_log)} articles) ---")
        for i, rec in enumerate(t.trade_log, 1):
            order_desc = ", ".join(rec.to_dict()["orders"]) or "no trade"
            print(
                f"  [{i}] {rec.sim_time:%H:%M} P={rec.probability:.2f} "
                f"{rec.conviction:<6} | {order_desc}"
            )
            print(f"       {rec.article.source}: {rec.article.title[:65]}")
            if rec.motivation:
                print(f"       → {rec.motivation[:80]}")

    # Fetch fills for fill tracking
    trader_fills_map: dict[str, list] = {}
    for t in traders:
        trader_fills_map[t.name] = await _fetch_all_fills(client, t.account_id)
    mm_fills = await _fetch_all_fills(client, mm.account_id)
    noise_fills = []
    for nb in noise_bots:
        noise_fills.extend(await _fetch_all_fills(client, nb.account_id))

    # Build per-block records
    price_history = await client.get_price_history(market_id)
    article_date = traders[0].articles[0].timestamp.date() if traders and traders[0].articles else None
    if article_date:
        h, m = (int(x) for x in config.sim_start_hour.split(":"))
        rec_sim_start = datetime(article_date.year, article_date.month, article_date.day, h, m)
    else:
        rec_sim_start = None
    block_records = build_block_records(
        all_bots, mm, noise_bots, traders, price_history,
        trader_fills_map=trader_fills_map,
        mm_fills=mm_fills, noise_fills=noise_fills,
        sim_start=rec_sim_start, compression_ratio=config.compression_ratio,
    )

    # Enrich block records with welfare/volume/fills from bot's live block stats
    # (fetching via get_block() misses early blocks evicted from the ring buffer)
    block_stats = mm.block_stats  # all bots see the same blocks; use MM's copy
    for rec in block_records:
        stats = block_stats.get(rec["height"])
        if stats:
            rec["welfare_nanos"] = stats[0]
            rec["total_volume_nanos"] = stats[1]
            rec["orders_filled"] = stats[2]
        else:
            rec["welfare_nanos"] = 0
            rec["total_volume_nanos"] = 0
            rec["orders_filled"] = 0

    # Block summary
    print(f"\n--- Block Log ({len(block_records)} blocks) ---")
    for rec in block_records:
        price_str = f"YES={rec['yes_price']:.2f}" if rec["yes_price"] is not None else "YES=???"
        mm_n = len(rec["mm_orders"])
        noise_n = rec["noise_order_count"]
        trader_n = len(rec["trader_orders"])
        line = f"  Block {rec['height']:>3}: {price_str}  MM:{mm_n}  Noise:{noise_n}  Trader:{trader_n}"
        for llm in rec["trader_llm"]:
            tag = f"[{llm['trader']}]" if "trader" in llm else ""
            line += f"  ← {tag} P={llm['probability']:.2f} {llm['conviction']}"
        print(line)

    # Save to file
    runs_dir = Path("iran/runs")
    runs_dir.mkdir(parents=True, exist_ok=True)
    run_ts = datetime.now().strftime("%Y%m%d_%H%M%S")
    suffix = f"_day{day_label}" if day_label else ""
    run_path = runs_dir / f"{run_ts}{suffix}.json"

    run_data = {
        "meta": {
            "timestamp": datetime.now().isoformat(),
            "simulation_date": day_label,
            "run_id": run_id,
            "config": asdict(config),
        },
        "blocks": block_records,
        "trade_logs": {t.name: [rec.to_dict() for rec in t.trade_log] for t in traders},
        "leaderboard": leaderboard,
    }
    run_path.write_text(json.dumps(run_data, indent=2))
    print(f"\nResults saved to {run_path}")


def main():
    parser = argparse.ArgumentParser(description="Iran strike market simulation")
    parser.add_argument("--base-url", default="http://localhost:3001")
    parser.add_argument("--compression", type=float, default=300.0,
                        help="Time compression ratio (default: 300, i.e. 1 real sec = 5 sim min)")
    parser.add_argument("--noise-count", type=int, default=20)
    parser.add_argument("--noise-balance", type=float, default=20.0)
    parser.add_argument("--trader-balance", type=float, default=1000.0)
    parser.add_argument("--initial-price", type=float, default=0.12)
    parser.add_argument("--model", default="moonshotai/kimi-k2")
    parser.add_argument("--api-key", default="")
    parser.add_argument("--traders", nargs="+", metavar="BOT_KEY",
                        help="Bot keys from BOT_PERSONAS, e.g. israeli_trader american_believer")
    parser.add_argument("--date", default=None,
                        help="Article date YYYYMMDD for phase1 file lookup (default: most recent)")
    parser.add_argument("--dates", nargs="+", metavar="YYYYMMDD",
                        help="Multiple dates for multi-day simulation, e.g. --dates 20260101 20260102 20260103")
    parser.add_argument("--sim-start", default="00:00", help="Sim start HH:MM (default: 00:00)")
    parser.add_argument("--sim-end", default="23:59", help="Sim end HH:MM (default: 23:59)")
    parser.add_argument("-v", "--verbose", action="store_true")
    args = parser.parse_args()

    logging.basicConfig(
        level=logging.DEBUG if args.verbose else logging.INFO,
        format="%(asctime)s %(name)s %(levelname)s %(message)s",
    )

    # Build trader specs from --traders arg
    trader_specs = None
    if args.traders:
        trader_specs = []
        tradeable = {k: v for k, v in BOT_PERSONAS.items() if "persona" in v}
        for bot_key in args.traders:
            if bot_key not in tradeable:
                available = ", ".join(tradeable.keys())
                print(f"Unknown or non-tradeable bot: {bot_key}. Available: {available}")
                return
            bot_cfg = tradeable[bot_key]
            trader_specs.append(TraderSpec(
                name=bot_cfg["name"],
                bot_key=bot_key,
                persona=build_persona(bot_cfg),
                phase1_path=_resolve_phase1_path(bot_key, args.date),
                strategy=bot_cfg.get("strategy"),
            ))

    # Resolve dates: --dates takes priority over --date
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
        initial_price=args.initial_price,
        model_name=args.model,
        api_key=args.api_key,
        sim_start_hour=args.sim_start,
        sim_end_hour=args.sim_end,
        trader_specs=trader_specs,
        dates=dates,
    )

    print("Iran Strike Market Simulation")
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
