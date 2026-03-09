"""Simulation results: block record construction and saving."""

import json
import logging
from dataclasses import asdict
from datetime import datetime, timedelta
from pathlib import Path

from sybil_client.types import NANOS_PER_DOLLAR, PricePoint

log = logging.getLogger(__name__)


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
    """Join per-bot block_logs with server price history into per-block records."""
    from .news_trader import _describe_order

    if trader_fills_map is None:
        trader_fills_map = {}

    all_heights: set[int] = set()
    for bot in all_bots:
        for height, _ in bot.block_log:
            all_heights.add(height)

    price_by_height = {pt.height: pt for pt in price_history}

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

    all_trader_fills_by_height: dict[int, list[dict]] = {}
    for tname, fills in trader_fills_map.items():
        for h, entries in _index_fills(fills, tname).items():
            all_trader_fills_by_height.setdefault(h, []).extend(entries)
    mm_fills_by_height = _index_fills(mm_fills, "MM")
    noise_fills_by_height = _index_fills(noise_fills, "Noise")

    mm_by_height: dict[int, list] = {}
    for h, orders in mm.block_log:
        mm_by_height.setdefault(h + 1, []).extend(orders)

    noise_by_height: dict[int, list] = {}
    for nb in noise_bots:
        for h, orders in nb.block_log:
            noise_by_height.setdefault(h + 1, []).extend(orders)

    trader_orders_by_height: dict[int, list[tuple[str, list]]] = {}
    for t in traders:
        for h, orders in t.block_log:
            trader_orders_by_height.setdefault(h + 1, []).append((t.name, orders))

    sim_time_by_height: dict[int, str] = {}
    if sim_start and all_heights:
        first_height = min(all_heights)
        for h in all_heights:
            offset = (h - first_height) * compression_ratio
            st = sim_start + timedelta(seconds=offset)
            sim_time_by_height[h] = st.isoformat()

    records = []
    for height in sorted(all_heights):
        pt = price_by_height.get(height)
        mm_orders = mm_by_height.get(height, [])
        noise_orders = noise_by_height.get(height, [])

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
            "trader_orders": [e["order"] for e in all_trader_orders],
            "trader_orders_detail": all_trader_orders,
            "trader_fills": all_trader_fills_by_height.get(height, []),
            "mm_fills": mm_fills_by_height.get(height, []),
            "noise_fills": noise_fills_by_height.get(height, []),
            "trader_llm": llm_by_block.get(height, []),
        }
        records.append(rec)

    # Compute active trader orders with TTL=3 carry-over
    active_orders: list[dict] = []
    for rec in records:
        h = rec["height"]
        active_orders = [o for o in active_orders if h - o["submitted_block"] < 3]
        for o_str in rec["trader_orders"]:
            parts = o_str.split()
            if len(parts) >= 2:
                try:
                    qty = int(parts[1])
                except ValueError:
                    qty = 0
                active_orders.append({"qty": qty, "submitted_block": h})
        rec["active_trader_orders"] = len(active_orders)
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


async def save_and_print_results(
    client, config, all_bots, traders: list, market_id,
    runs_dir: Path,
    day_label=None, run_id=None,
):
    mm = all_bots[0]
    num_traders = len(traders)
    noise_bots = all_bots[1:-num_traders]

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

    # Trade logs
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

    # Fetch fills
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

    # Enrich with welfare/volume/fills
    block_stats = mm.block_stats
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
    run_path.write_text(json.dumps(run_data, indent=2, default=str))
    print(f"\nResults saved to {run_path}")
