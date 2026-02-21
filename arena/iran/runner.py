"""Iran strike market simulation runner.

Usage:
    cd arena && uv run python -m iran.runner
    cd arena && uv run python -m iran.runner --compression 120 --noise-count 10
"""

import argparse
import asyncio
from bisect import bisect_left
import json
import logging
import os

from dotenv import load_dotenv

load_dotenv()
from dataclasses import asdict, dataclass
from datetime import datetime, timedelta
from pathlib import Path

from bots.market_maker import BalancedMarketMaker
from bots.random_trader import RandomTrader
from sybil_client import BuyNo, BuyYes, SybilClient
from sybil_client.types import NANOS_PER_DOLLAR, PricePoint

from .clock import SimulatedClock
from .news_trader import IranNewsTrader, load_articles

log = logging.getLogger(__name__)


@dataclass
class SimulationConfig:
    base_url: str = "http://localhost:3001"
    compression_ratio: float = 300.0
    mm_balance: float = 20_000.0
    mm_risk_fraction: float = 0.30
    mm_seed_qty: int = 10_000
    initial_price: float = 0.12
    noise_count: int = 20
    noise_balance: float = 20.0
    trader_balance: float = 1_000.0
    phase1_path: str = "iran/tmp/jan2_phase1_results.json"
    texts_path: str = "iran/tmp/article_texts.json"
    api_key: str = ""
    model_name: str = "moonshotai/kimi-k2"


async def run_simulation(config: SimulationConfig) -> None:
    async with SybilClient(config.base_url) as client:
        # 1. Create market
        market = await client.create_market(
            "Will US strike Iran by March 31?",
            description="Resolves YES if US military strikes Iran before 2026-03-31",
            category="geopolitics",
        )
        print(f"Created market {market.id}: {market.name}")

        # 2. Load articles
        articles = load_articles(config.phase1_path, config.texts_path)
        print(f"Loaded {len(articles)} articles (phase1=YES with full text)")
        if not articles:
            print("ERROR: No articles loaded. Check paths.")
            return
        for i, art in enumerate(articles):
            print(f"  [{i+1}] {art.timestamp:%H:%M} {art.source}: {art.title[:70]}")

        # 3. Create MM account + seed trade
        mm_acct = await client.create_account(int(config.mm_balance * NANOS_PER_DOLLAR))
        await client.submit_orders(mm_acct.id, [
            BuyYes.at_price(market.id, config.initial_price, config.mm_seed_qty),
            BuyNo.at_price(market.id, 1 - config.initial_price, config.mm_seed_qty),
        ])
        print(f"MM account {mm_acct.id}: seeded {config.mm_seed_qty} shares @ {config.initial_price:.2f}")

        # 4. Wait for seed trade to clear
        async for block in client.stream_blocks():
            print(f"Seed trade cleared in block {block.height}")
            break

        # 5. Create clock
        clock = SimulatedClock(
            sim_start=articles[0].timestamp,
            compression_ratio=config.compression_ratio,
        )

        # 6. Create MM bot
        mm = BalancedMarketMaker(
            client, mm_acct.id,
            risk_fraction=config.mm_risk_fraction,
            name="MM",
            market_ids=[market.id],
        )

        # 7. Create noise traders
        noise_bots = []
        for i in range(config.noise_count):
            acct = await client.create_account(int(config.noise_balance * NANOS_PER_DOLLAR))
            bot = RandomTrader(
                client, acct.id,
                trade_probability=0.5,
                seed=i,
                name=f"Noise-{i}",
                market_ids=[market.id],
            )
            noise_bots.append(bot)
        print(f"Created {config.noise_count} noise traders @ ${config.noise_balance} each")

        # 8. Create American Trader
        api_key = config.api_key or os.environ.get("OPENROUTER_API_KEY", "")
        if not api_key:
            print("WARNING: No OPENROUTER_API_KEY set. LLM calls will fail.")
        trader_acct = await client.create_account(int(config.trader_balance * NANOS_PER_DOLLAR))
        trader = IranNewsTrader(
            client, trader_acct.id, articles, clock,
            api_key=api_key,
            model_name=config.model_name,
            name="AmericanTrader",
            market_ids=[market.id],
        )
        print(f"AmericanTrader account {trader_acct.id}: ${config.trader_balance}")

        # 9. Start clock + all bots
        clock.start()
        all_bots = [mm, *noise_bots, trader]
        tasks = [asyncio.create_task(bot.run()) for bot in all_bots]

        sim_span = articles[-1].timestamp - articles[0].timestamp
        real_span = sim_span.total_seconds() / config.compression_ratio
        print(
            f"\nSimulation started: {len(all_bots)} bots"
            f"\n  Sim time: {articles[0].timestamp:%H:%M} → {articles[-1].timestamp:%H:%M}"
            f" ({sim_span}) + 30min buffer"
            f"\n  Real time: ~{real_span + 30*60/config.compression_ratio:.0f}s"
            f" (compression={config.compression_ratio}x)"
        )

        # 10. Wait for sim end
        sim_end = articles[-1].timestamp + timedelta(minutes=30)
        await clock.sleep_until(sim_end)

        # 11. Stop all bots
        print("\nStopping bots...")
        for bot in all_bots:
            bot.stop()
        await asyncio.gather(*tasks, return_exceptions=True)

        # 12. Collect results, print, and save
        await save_and_print_results(client, config, all_bots, trader, market.id)


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
    all_bots, mm, noise_bots, trader, price_history: list[PricePoint],
    trader_fills: list | None = None,
    mm_fills: list | None = None,
    sim_start: datetime | None = None,
    compression_ratio: float = 300.0,
) -> list[dict]:
    """Join per-bot block_logs with server price history into per-block records."""
    from .news_trader import _describe_order

    # 1. Collect all block heights seen by any bot
    all_heights: set[int] = set()
    for bot in all_bots:
        for height, _ in bot.block_log:
            all_heights.add(height)

    # 2. Index price history by block height
    price_by_height = {pt.height: pt for pt in price_history}

    # 3. Index trader LLM data by order_block (when orders actually reach the server)
    #    Also build a trigger→order_block mapping for reindexing block_log.
    llm_by_block: dict[int, dict] = {}
    trigger_to_order_block: dict[int, int] = {}
    for rec in trader.trade_log:
        if rec.block_height >= 0:
            order_block = rec.block_height + round(rec.llm_duration_s)
            trigger_to_order_block[rec.block_height] = order_block
            all_heights.add(order_block)  # ensure order block has a record
            llm_by_block[order_block] = {
                "article_title": rec.article.title,
                "article_source": rec.article.source,
                "probability": rec.probability,
                "conviction": rec.conviction,
                "motivation": rec.motivation,
                "llm_response": rec.llm_response,
                "llm_duration_s": rec.llm_duration_s,
                "trigger_block": rec.block_height,
                "order_block_height": order_block,
            }

    # 4. Index fills by block height (trader + MM)
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

    trader_fills_by_height = _index_fills(trader_fills, "Trader")
    mm_fills_by_height = _index_fills(mm_fills, "MM")

    # 5. Pre-index bot orders by block height
    mm_by_height: dict[int, list] = {}
    for h, orders in mm.block_log:
        mm_by_height.setdefault(h, []).extend(orders)

    noise_by_height: dict[int, list] = {}
    for nb in noise_bots:
        for h, orders in nb.block_log:
            noise_by_height.setdefault(h, []).extend(orders)

    # Reindex trader orders by order_block (estimated server arrival)
    # block_log records at trigger block; shift to order_block using LLM duration.
    trader_by_height: dict[int, list] = {}
    for h, orders in trader.block_log:
        dest = trigger_to_order_block.get(h, h)
        trader_by_height.setdefault(dest, []).extend(orders)

    # 5a. Compute sim_time from block height, accounting for clock pauses during LLM calls.
    # The SimulatedClock pauses while the LLM is thinking, but the server keeps producing
    # blocks. So blocks during an LLM call don't advance sim_time. We subtract those
    # "pause blocks" to get an accurate sim_time mapping.
    sim_time_by_height: dict[int, str] = {}
    if sim_start and all_heights:
        first_height = min(all_heights)

        # Build set of block heights where the sim clock was paused (during LLM calls)
        pause_blocks: set[int] = set()
        for rec in trader.trade_log:
            if rec.block_height >= 0 and rec.llm_duration_s > 0:
                pause_len = round(rec.llm_duration_s)
                for b in range(rec.block_height, rec.block_height + pause_len):
                    pause_blocks.add(b)

        # Pre-compute cumulative pause count for efficient lookup
        sorted_pauses = sorted(pause_blocks)

        for h in all_heights:
            paused_before = bisect_left(sorted_pauses, h)
            effective_offset = (h - first_height) - paused_before
            st = sim_start + timedelta(seconds=effective_offset * compression_ratio)
            sim_time_by_height[h] = st.isoformat()

    # 5b. Build records
    records = []
    for height in sorted(all_heights):
        pt = price_by_height.get(height)
        mm_orders = mm_by_height.get(height, [])
        noise_orders = noise_by_height.get(height, [])
        trader_orders = trader_by_height.get(height, [])

        rec = {
            "height": height,
            "timestamp_ms": pt.timestamp_ms if pt else None,
            "sim_time": sim_time_by_height.get(height),
            "yes_price": pt.yes_price_nanos / NANOS_PER_DOLLAR if pt else None,
            "volume_nanos": pt.volume_nanos if pt else 0,
            "mm_orders": [_describe_order(o) for o in mm_orders],
            "noise_orders": [_describe_order(o) for o in noise_orders],
            "noise_order_count": len(noise_orders),
            "trader_orders": [_describe_order(o) for o in trader_orders],
            "trader_fills": trader_fills_by_height.get(height, []),
            "mm_fills": mm_fills_by_height.get(height, []),
            "trader_llm": llm_by_block.get(height),
        }
        records.append(rec)

    # 5c. Compute active trader orders with TTL=3 carry-over
    # Trader orders persist for 3 blocks; fills reduce remaining qty.
    active_orders: list[dict] = []  # {qty, submitted_block}
    for rec in records:
        h = rec["height"]
        # Expire orders past TTL
        active_orders = [o for o in active_orders if h - o["submitted_block"] < 3]
        # Subtract fills
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
        # Add new trader orders
        for o_str in rec["trader_orders"]:
            parts = o_str.split()
            if len(parts) >= 2:
                try:
                    qty = int(parts[1])
                except ValueError:
                    qty = 0
                active_orders.append({"qty": qty, "submitted_block": h})
        rec["active_trader_orders"] = len(active_orders)

    return records


async def save_and_print_results(client, config, all_bots, trader, market_id):
    mm = all_bots[0]  # first bot is always the MM
    noise_bots = all_bots[1:-1]  # middle bots are noise

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

    # Trade log
    print(f"\n--- AmericanTrader Trade Log ({len(trader.trade_log)} articles) ---")
    for i, rec in enumerate(trader.trade_log, 1):
        order_desc = ", ".join(rec.to_dict()["orders"]) or "no trade"
        print(
            f"  [{i}] {rec.sim_time:%H:%M} P={rec.probability:.2f} "
            f"{rec.conviction:<6} | {order_desc}"
        )
        print(f"       {rec.article.source}: {rec.article.title[:65]}")
        if rec.motivation:
            print(f"       → {rec.motivation[:80]}")

    # Fetch fills for fill tracking
    trader_fills = await _fetch_all_fills(client, trader.account_id)
    mm_fills = await _fetch_all_fills(client, mm.account_id)

    # Build per-block records
    price_history = await client.get_price_history(market_id)
    sim_start = trader.articles[0].timestamp if trader.articles else None
    block_records = build_block_records(
        all_bots, mm, noise_bots, trader, price_history, trader_fills,
        mm_fills=mm_fills,
        sim_start=sim_start, compression_ratio=config.compression_ratio,
    )

    # Block summary
    print(f"\n--- Block Log ({len(block_records)} blocks) ---")
    for rec in block_records:
        price_str = f"YES={rec['yes_price']:.2f}" if rec["yes_price"] is not None else "YES=???"
        mm_n = len(rec["mm_orders"])
        noise_n = rec["noise_order_count"]
        trader_n = len(rec["trader_orders"])
        line = f"  Block {rec['height']:>3}: {price_str}  MM:{mm_n}  Noise:{noise_n}  Trader:{trader_n}"
        if rec["trader_llm"]:
            llm = rec["trader_llm"]
            line += f"  ← LLM P={llm['probability']:.2f} {llm['conviction']}"
        print(line)

    # Save to file
    runs_dir = Path("iran/runs")
    runs_dir.mkdir(parents=True, exist_ok=True)
    run_ts = datetime.now().strftime("%Y%m%d_%H%M%S")
    run_path = runs_dir / f"{run_ts}.json"

    run_data = {
        "meta": {
            "timestamp": datetime.now().isoformat(),
            "config": asdict(config),
        },
        "blocks": block_records,
        "trade_log": [
            {**rec.to_dict(), "order_block_height": rec.block_height + round(rec.llm_duration_s)}
            for rec in trader.trade_log
        ],
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
    parser.add_argument("--mm-risk-fraction", type=float, default=0.30,
                        help="Fraction of portfolio value to use as risk budget (default: 0.30)")
    parser.add_argument("--initial-price", type=float, default=0.12)
    parser.add_argument("--model", default="moonshotai/kimi-k2")
    parser.add_argument("--api-key", default="")
    parser.add_argument("--phase1", default="iran/tmp/jan2_phase1_results.json")
    parser.add_argument("--texts", default="iran/tmp/article_texts.json")
    parser.add_argument("-v", "--verbose", action="store_true")
    args = parser.parse_args()

    logging.basicConfig(
        level=logging.DEBUG if args.verbose else logging.INFO,
        format="%(asctime)s %(name)s %(levelname)s %(message)s",
    )

    config = SimulationConfig(
        base_url=args.base_url,
        compression_ratio=args.compression,
        noise_count=args.noise_count,
        noise_balance=args.noise_balance,
        trader_balance=args.trader_balance,
        mm_risk_fraction=args.mm_risk_fraction,
        initial_price=args.initial_price,
        model_name=args.model,
        api_key=args.api_key,
        phase1_path=args.phase1,
        texts_path=args.texts,
    )

    print("Iran Strike Market Simulation")
    print(f"  Server: {config.base_url}")
    print(f"  Model: {config.model_name}")
    print(f"  Compression: {config.compression_ratio}x")
    print(f"  Noise traders: {config.noise_count} @ ${config.noise_balance}")
    print(f"  MM risk fraction: {config.mm_risk_fraction:.0%}")
    print(f"  Trader balance: ${config.trader_balance}")
    print()

    asyncio.run(run_simulation(config))


if __name__ == "__main__":
    main()
