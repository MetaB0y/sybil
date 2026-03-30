"""
Comprehensive MM P&L analysis for the latest Iran market simulation run.
"""
import json
import os
from collections import defaultdict

RUN_DIR = "markets/iran/runs"
RUN_FILES = [
    "20260316_153706_day20260101.json",
    "20260316_154938_day20260102.json",
    "20260316_155801_day20260103.json",
    "20260316_160847_day20260104.json",
    "20260316_162213_day20260105.json",
    "20260316_163041_day20260106.json",
]

def parse_mm_order(s):
    """Parse 'BuyYes 2083 @ $0.1200' or 'SellNo 100 @ $0.8500'"""
    parts = s.split()
    action = parts[0]  # BuyYes, BuyNo, SellYes, SellNo
    qty = int(parts[1])
    price = float(parts[3].replace("$", ""))
    side = "buy" if action.startswith("Buy") else "sell"
    outcome = "YES" if action.endswith("Yes") else "NO"
    return side, outcome, qty, price

def analyze():
    all_days = []

    # Cumulative tracking across days
    cum_yes_bought = 0
    cum_yes_sold = 0
    cum_no_bought = 0
    cum_no_sold = 0
    cum_yes_buy_cost = 0.0
    cum_yes_sell_revenue = 0.0
    cum_no_buy_cost = 0.0
    cum_no_sell_revenue = 0.0

    # Track all individual fills for adverse selection analysis
    all_fills_with_context = []

    # Track skip-buy blocks
    total_skip_buy_blocks = 0

    for fi, fname in enumerate(RUN_FILES):
        path = os.path.join(RUN_DIR, fname)
        with open(path) as f:
            data = json.load(f)

        day_label = fname.split("_day")[1].replace(".json", "")
        blocks = data["blocks"]
        leaderboard = data["leaderboard"]

        mm_lb = None
        for entry in leaderboard:
            if entry["name"] == "MM":
                mm_lb = entry
                break

        # Per-day fill tracking
        day_yes_bought_qty = 0
        day_yes_bought_cost = 0.0
        day_no_bought_qty = 0
        day_no_bought_cost = 0.0
        day_yes_sold_qty = 0
        day_yes_sold_revenue = 0.0
        day_no_sold_qty = 0
        day_no_sold_revenue = 0.0

        # Order posting tracking
        day_buy_orders_posted = 0
        day_sell_orders_posted = 0
        day_buy_yes_orders = 0
        day_buy_no_orders = 0
        day_sell_yes_orders = 0
        day_sell_no_orders = 0

        # Fill counts
        day_buy_fills = 0
        day_sell_fills = 0

        # Skip-buy detection: blocks where MM posted no buy orders
        skip_buy_blocks = 0
        blocks_with_mm_orders = 0

        for b in blocks:
            clearing_price = b["yes_price"]  # YES clearing price

            # Parse MM orders
            has_buy = False
            has_sell = False
            sell_prices = []
            buy_prices = []

            if b["mm_orders"]:
                blocks_with_mm_orders += 1
                for order_str in b["mm_orders"]:
                    side, outcome, qty, price = parse_mm_order(order_str)
                    if side == "buy":
                        has_buy = True
                        day_buy_orders_posted += 1
                        buy_prices.append(price)
                        if outcome == "YES":
                            day_buy_yes_orders += 1
                        else:
                            day_buy_no_orders += 1
                    else:
                        has_sell = True
                        day_sell_orders_posted += 1
                        sell_prices.append(price)
                        if outcome == "YES":
                            day_sell_yes_orders += 1
                        else:
                            day_sell_no_orders += 1

                if not has_buy and has_sell:
                    skip_buy_blocks += 1

            # Parse MM fills
            for fill in b["mm_fills"]:
                for pd in fill["position_deltas"]:
                    outcome = pd["outcome"]
                    delta = pd["delta"]
                    price = fill["fill_price"]
                    qty = abs(delta)

                    if delta > 0:  # Bought
                        if outcome == "YES":
                            day_yes_bought_qty += qty
                            day_yes_bought_cost += qty * price
                        else:
                            day_no_bought_qty += qty
                            day_no_bought_cost += qty * price
                        day_buy_fills += 1
                    else:  # Sold
                        if outcome == "YES":
                            day_yes_sold_qty += qty
                            day_yes_sold_revenue += qty * price
                        else:
                            day_no_sold_qty += qty
                            day_no_sold_revenue += qty * price
                        day_sell_fills += 1

                    # For adverse selection: did price move against MM after fill?
                    all_fills_with_context.append({
                        "day": day_label,
                        "block": b["height"],
                        "outcome": outcome,
                        "side": "buy" if delta > 0 else "sell",
                        "qty": qty,
                        "price": price,
                        "clearing_price": clearing_price,
                        "delta": delta,
                    })

        total_skip_buy_blocks += skip_buy_blocks

        cum_yes_bought += day_yes_bought_qty
        cum_yes_sold += day_yes_sold_qty
        cum_no_bought += day_no_bought_qty
        cum_no_sold += day_no_sold_qty
        cum_yes_buy_cost += day_yes_bought_cost
        cum_yes_sell_revenue += day_yes_sold_revenue
        cum_no_buy_cost += day_no_bought_cost
        cum_no_sell_revenue += day_no_sold_revenue

        all_days.append({
            "day": day_label,
            "mm_lb": mm_lb,
            "yes_bought": day_yes_bought_qty,
            "yes_bought_cost": day_yes_bought_cost,
            "no_bought": day_no_bought_qty,
            "no_bought_cost": day_no_bought_cost,
            "yes_sold": day_yes_sold_qty,
            "yes_sold_revenue": day_yes_sold_revenue,
            "no_sold": day_no_sold_qty,
            "no_sold_revenue": day_no_sold_revenue,
            "buy_orders_posted": day_buy_orders_posted,
            "sell_orders_posted": day_sell_orders_posted,
            "buy_fills": day_buy_fills,
            "sell_fills": day_sell_fills,
            "skip_buy_blocks": skip_buy_blocks,
            "blocks_with_mm_orders": blocks_with_mm_orders,
            "num_blocks": len(blocks),
            "buy_yes_orders": day_buy_yes_orders,
            "buy_no_orders": day_buy_no_orders,
            "sell_yes_orders": day_sell_yes_orders,
            "sell_no_orders": day_sell_no_orders,
        })

    # ============================================================
    # REPORT
    # ============================================================

    print("=" * 80)
    print("MM P&L ANALYSIS — Latest Iran Simulation Run (20260316_15*)")
    print("=" * 80)

    # 1. MM inventory at end of each day
    print("\n" + "=" * 80)
    print("1. MM INVENTORY & P&L AT END OF EACH DAY")
    print("=" * 80)
    print(f"{'Day':<12} {'Yes':>6} {'No':>6} {'Net(Y-N)':>8} {'Matched':>8} {'Balance':>12} {'PosVal':>10} {'Portfolio':>12} {'PnL':>10}")
    print("-" * 96)
    for d in all_days:
        lb = d["mm_lb"]
        yes = lb["yes_shares"]
        no = lb["no_shares"]
        matched = min(yes, no)
        net = yes - no
        print(f"{d['day']:<12} {yes:>6} {no:>6} {net:>+8} {matched:>8} {lb['balance']:>12.2f} {lb['position_value']:>10.2f} {lb['portfolio_value']:>12.2f} {lb['pnl']:>+10.2f}")

    # 2. Buy vs Sell activity per day
    print("\n" + "=" * 80)
    print("2. BUY vs SELL ACTIVITY PER DAY (shares filled)")
    print("=" * 80)
    print(f"{'Day':<12} {'YesBuy':>8} {'NoBuy':>8} {'TotBuy':>8} {'YesSell':>8} {'NoSell':>8} {'TotSell':>8} {'BuyFills':>9} {'SellFills':>10}")
    print("-" * 93)
    for d in all_days:
        tot_buy = d["yes_bought"] + d["no_bought"]
        tot_sell = d["yes_sold"] + d["no_sold"]
        print(f"{d['day']:<12} {d['yes_bought']:>8} {d['no_bought']:>8} {tot_buy:>8} {d['yes_sold']:>8} {d['no_sold']:>8} {tot_sell:>8} {d['buy_fills']:>9} {d['sell_fills']:>10}")

    tot_buy_all = cum_yes_bought + cum_no_bought
    tot_sell_all = cum_yes_sold + cum_no_sold
    print("-" * 93)
    print(f"{'TOTAL':<12} {cum_yes_bought:>8} {cum_no_bought:>8} {tot_buy_all:>8} {cum_yes_sold:>8} {cum_no_sold:>8} {tot_sell_all:>8}")
    print(f"\nBuy/Sell ratio: {tot_buy_all}/{tot_sell_all} = {tot_buy_all/max(tot_sell_all,1):.2f}x")

    # 2b. Orders posted
    print("\n" + "=" * 80)
    print("2b. ORDERS POSTED PER DAY")
    print("=" * 80)
    print(f"{'Day':<12} {'BuyY':>6} {'BuyN':>6} {'SellY':>6} {'SellN':>6} {'TotBuy':>8} {'TotSell':>8} {'Blocks':>7}")
    print("-" * 65)
    for d in all_days:
        print(f"{d['day']:<12} {d['buy_yes_orders']:>6} {d['buy_no_orders']:>6} {d['sell_yes_orders']:>6} {d['sell_no_orders']:>6} {d['buy_orders_posted']:>8} {d['sell_orders_posted']:>8} {d['num_blocks']:>7}")

    # 3. Sell order effectiveness
    print("\n" + "=" * 80)
    print("3. SELL ORDER EFFECTIVENESS — FILL PRICES vs CLEARING PRICES")
    print("=" * 80)

    sell_fills = [f for f in all_fills_with_context if f["side"] == "sell"]
    buy_fills = [f for f in all_fills_with_context if f["side"] == "buy"]

    # For sells: MM sells at fill_price. Clearing price tells us market level.
    # If MM sells YES at clearing_price, spread = 0.
    # If MM sells YES above clearing, that's good (selling high).
    yes_sells = [f for f in sell_fills if f["outcome"] == "YES"]
    no_sells = [f for f in sell_fills if f["outcome"] == "NO"]

    print(f"\nYES sell fills: {len(yes_sells)}, total qty: {sum(f['qty'] for f in yes_sells)}")
    if yes_sells:
        avg_sell_price = sum(f['price']*f['qty'] for f in yes_sells) / sum(f['qty'] for f in yes_sells)
        avg_clearing = sum(f['clearing_price']*f['qty'] for f in yes_sells) / sum(f['qty'] for f in yes_sells)
        print(f"  Avg sell price: ${avg_sell_price:.4f}, Avg clearing price: ${avg_clearing:.4f}")
        print(f"  Avg spread earned (sell - clearing): ${avg_sell_price - avg_clearing:+.4f}")

    print(f"\nNO sell fills: {len(no_sells)}, total qty: {sum(f['qty'] for f in no_sells)}")
    if no_sells:
        avg_sell_price = sum(f['price']*f['qty'] for f in no_sells) / sum(f['qty'] for f in no_sells)
        # For NO, clearing price on NO side is 1 - yes_clearing_price
        avg_no_clearing = sum((1-f['clearing_price'])*f['qty'] for f in no_sells) / sum(f['qty'] for f in no_sells)
        print(f"  Avg sell price: ${avg_sell_price:.4f}, Avg NO clearing price: ${avg_no_clearing:.4f}")
        print(f"  Avg spread earned (sell - clearing): ${avg_sell_price - avg_no_clearing:+.4f}")

    # 4. Matched pair accumulation
    print("\n" + "=" * 80)
    print("4. MATCHED PAIR ACCUMULATION (min(YES, NO) shares)")
    print("=" * 80)
    for d in all_days:
        lb = d["mm_lb"]
        matched = min(lb["yes_shares"], lb["no_shares"])
        capital_locked = matched  # $1 per matched pair
        print(f"  {d['day']}: YES={lb['yes_shares']}, NO={lb['no_shares']}, matched_pairs={matched}, capital_locked=${capital_locked}")

    final_lb = all_days[-1]["mm_lb"]
    final_matched = min(final_lb["yes_shares"], final_lb["no_shares"])
    print(f"\n  Final matched pairs: {final_matched} (locks ${final_matched} of capital)")
    print(f"  Net directional: {final_lb['yes_shares'] - final_lb['no_shares']:+d} YES shares")

    # 5. Adverse selection analysis
    print("\n" + "=" * 80)
    print("5. ADVERSE SELECTION ANALYSIS")
    print("=" * 80)

    # Look at price moves between consecutive blocks within each day
    # For each MM buy fill, check if the next block's price moved against us
    for fi, fname in enumerate(RUN_FILES):
        path = os.path.join(RUN_DIR, fname)
        with open(path) as f:
            data = json.load(f)

        day_label = fname.split("_day")[1].replace(".json", "")
        blocks = data["blocks"]

        # Build price series (skip None prices)
        prices = {b["height"]: b["yes_price"] for b in blocks if b["yes_price"] is not None}
        heights = sorted(prices.keys())

        adverse_buys = 0
        total_buys_checked = 0
        adverse_sells = 0
        total_sells_checked = 0

        for b in blocks:
            h = b["height"]
            # Find next block with a different price (looking ahead up to 5 blocks)
            future_prices = []
            for fh in heights:
                if fh > h and fh <= h + 10:
                    future_prices.append(prices[fh])

            if not future_prices or b["yes_price"] is None:
                continue

            next_price = future_prices[-1] if len(future_prices) >= 3 else future_prices[-1]

            for fill in b["mm_fills"]:
                for pd in fill["position_deltas"]:
                    if pd["delta"] > 0:  # MM bought
                        total_buys_checked += 1
                        if pd["outcome"] == "YES":
                            # Bought YES; adverse if price drops
                            if next_price < b["yes_price"]:
                                adverse_buys += 1
                        else:
                            # Bought NO; adverse if YES price rises (NO price drops)
                            if next_price > b["yes_price"]:
                                adverse_buys += 1
                    elif pd["delta"] < 0:  # MM sold
                        total_sells_checked += 1
                        if pd["outcome"] == "YES":
                            # Sold YES; adverse if price rises
                            if next_price > b["yes_price"]:
                                adverse_sells += 1
                        else:
                            # Sold NO; adverse if YES price drops (NO price rises)
                            if next_price < b["yes_price"]:
                                adverse_sells += 1

        buy_adv_rate = adverse_buys / max(total_buys_checked, 1) * 100
        sell_adv_rate = adverse_sells / max(total_sells_checked, 1) * 100
        print(f"  {day_label}: Buy adverse={adverse_buys}/{total_buys_checked} ({buy_adv_rate:.1f}%), Sell adverse={adverse_sells}/{total_sells_checked} ({sell_adv_rate:.1f}%)")

    # 6. Where does the $4K loss come from?
    print("\n" + "=" * 80)
    print("6. LOSS DECOMPOSITION")
    print("=" * 80)

    initial_balance = 50000.0
    final_balance = final_lb["balance"]
    final_yes = final_lb["yes_shares"]
    final_no = final_lb["no_shares"]
    final_pos_val = final_lb["position_value"]
    final_pnl = final_lb["pnl"]

    # Cash spent
    total_buy_cost = cum_yes_buy_cost + cum_no_buy_cost
    total_sell_revenue = cum_yes_sell_revenue + cum_no_sell_revenue
    net_cash_flow = total_sell_revenue - total_buy_cost

    print(f"\n  Starting balance:          ${initial_balance:>12.2f}")
    print(f"  Total buy cost:            ${total_buy_cost:>12.2f}  (YES: ${cum_yes_buy_cost:.2f}, NO: ${cum_no_buy_cost:.2f})")
    print(f"  Total sell revenue:        ${total_sell_revenue:>12.2f}  (YES: ${cum_yes_sell_revenue:.2f}, NO: ${cum_no_sell_revenue:.2f})")
    print(f"  Net cash flow (sells-buys):${net_cash_flow:>+12.2f}")
    print(f"  Expected balance:          ${initial_balance + net_cash_flow:>12.2f}")
    print(f"  Actual balance:            ${final_balance:>12.2f}")
    print(f"  Discrepancy:               ${final_balance - (initial_balance + net_cash_flow):>+12.2f}  (matched pair redemptions, etc.)")

    # Get last day's clearing price for MTM
    with open(os.path.join(RUN_DIR, RUN_FILES[-1])) as f:
        last_data = json.load(f)
    last_clearing = last_data["blocks"][-1]["yes_price"]

    print(f"\n  Final clearing price (YES): ${last_clearing:.4f}")
    print(f"  Final YES shares: {final_yes}, NO shares: {final_no}")
    print(f"  Matched pairs: {min(final_yes, final_no)} (redeemable for ${min(final_yes, final_no):.2f})")

    # MTM value of inventory
    yes_mtm = final_yes * last_clearing
    no_mtm = final_no * (1 - last_clearing)
    total_mtm = yes_mtm + no_mtm
    print(f"  YES MTM value: {final_yes} × ${last_clearing:.4f} = ${yes_mtm:.2f}")
    print(f"  NO MTM value:  {final_no} × ${1-last_clearing:.4f} = ${no_mtm:.2f}")
    print(f"  Total inventory MTM: ${total_mtm:.2f}")
    print(f"  Position value (from leaderboard): ${final_pos_val:.2f}")

    # Cost basis of remaining inventory
    # YES inventory cost basis
    net_yes = cum_yes_bought - cum_yes_sold
    net_no = cum_no_bought - cum_no_sold
    yes_avg_buy = cum_yes_buy_cost / max(cum_yes_bought, 1)
    no_avg_buy = cum_no_buy_cost / max(cum_no_bought, 1)
    yes_avg_sell = cum_yes_sell_revenue / max(cum_yes_sold, 1) if cum_yes_sold else 0
    no_avg_sell = cum_no_sell_revenue / max(cum_no_sold, 1) if cum_no_sold else 0

    print(f"\n  --- Cost Basis Analysis ---")
    print(f"  YES: bought {cum_yes_bought} @ avg ${yes_avg_buy:.4f}, sold {cum_yes_sold} @ avg ${yes_avg_sell:.4f}")
    print(f"  NO:  bought {cum_no_bought} @ avg ${no_avg_buy:.4f}, sold {cum_no_sold} @ avg ${no_avg_sell:.4f}")

    # Realized P&L from round trips (simplified FIFO approximation)
    # YES: sold at avg_sell, assume bought at avg_buy
    yes_realized = cum_yes_sold * (yes_avg_sell - yes_avg_buy) if cum_yes_sold else 0
    no_realized = cum_no_sold * (no_avg_sell - no_avg_buy) if cum_no_sold else 0
    total_realized = yes_realized + no_realized

    # Unrealized P&L
    yes_unrealized = final_yes * (last_clearing - yes_avg_buy)
    no_unrealized = final_no * ((1 - last_clearing) - no_avg_buy)
    total_unrealized = yes_unrealized + no_unrealized

    print(f"\n  --- Realized P&L (approx FIFO) ---")
    print(f"  YES realized: {cum_yes_sold} shares × (${yes_avg_sell:.4f} - ${yes_avg_buy:.4f}) = ${yes_realized:+.2f}")
    print(f"  NO realized:  {cum_no_sold} shares × (${no_avg_sell:.4f} - ${no_avg_buy:.4f}) = ${no_realized:+.2f}")
    print(f"  Total realized: ${total_realized:+.2f}")

    print(f"\n  --- Unrealized P&L ---")
    print(f"  YES unrealized: {final_yes} shares × (${last_clearing:.4f} - ${yes_avg_buy:.4f}) = ${yes_unrealized:+.2f}")
    print(f"  NO unrealized:  {final_no} shares × (${1-last_clearing:.4f} - ${no_avg_buy:.4f}) = ${no_unrealized:+.2f}")
    print(f"  Total unrealized: ${total_unrealized:+.2f}")

    print(f"\n  --- Summary ---")
    print(f"  Realized P&L:   ${total_realized:+.2f}")
    print(f"  Unrealized P&L: ${total_unrealized:+.2f}")
    print(f"  Approx total:   ${total_realized + total_unrealized:+.2f}")
    print(f"  Actual PnL:     ${final_pnl:+.2f}")

    # Capital locked analysis
    print(f"\n  --- Capital locked in matched pairs ---")
    print(f"  {final_matched} matched pairs: cost ${final_matched:.2f} to redeem for ${final_matched:.2f} (net 0)")
    print(f"  BUT: buying both YES+NO at midpoint costs ~$1.00 per pair")
    print(f"  If avg YES buy = ${yes_avg_buy:.4f} and avg NO buy = ${no_avg_buy:.4f},")
    print(f"  then avg pair cost = ${yes_avg_buy + no_avg_buy:.4f}")
    print(f"  Overpayment per pair = ${yes_avg_buy + no_avg_buy - 1:.4f}")
    print(f"  Total overpayment on {final_matched} pairs = ${final_matched * (yes_avg_buy + no_avg_buy - 1):+.2f}")

    # 7. Skip-buy blocks
    print("\n" + "=" * 80)
    print("7. SKIP-BUY BLOCKS")
    print("=" * 80)
    for d in all_days:
        total_blocks = d["blocks_with_mm_orders"]
        skip = d["skip_buy_blocks"]
        pct = skip / max(total_blocks, 1) * 100
        print(f"  {d['day']}: {skip}/{total_blocks} blocks had sells-only (no buys) = {pct:.1f}%")
    print(f"  Total skip-buy blocks: {total_skip_buy_blocks}")

    # Extra: Price trajectory
    print("\n" + "=" * 80)
    print("EXTRA: PRICE TRAJECTORY (first and last 3 blocks per day)")
    print("=" * 80)
    for fname in RUN_FILES:
        path = os.path.join(RUN_DIR, fname)
        with open(path) as f:
            data = json.load(f)
        day_label = fname.split("_day")[1].replace(".json", "")
        blocks = data["blocks"]
        prices = [b["yes_price"] for b in blocks if b["yes_price"] is not None]
        if prices:
            print(f"  {day_label}: start=${prices[0]:.4f}, end=${prices[-1]:.4f}, min=${min(prices):.4f}, max=${max(prices):.4f}, blocks={len(blocks)}")

    # Extra: Per-block fill volume histogram
    print("\n" + "=" * 80)
    print("EXTRA: LARGEST MM FILL BLOCKS (top 10 by qty)")
    print("=" * 80)
    block_fills = []
    for fname in RUN_FILES:
        path = os.path.join(RUN_DIR, fname)
        with open(path) as f:
            data = json.load(f)
        day_label = fname.split("_day")[1].replace(".json", "")
        for b in data["blocks"]:
            total_buy_qty = 0
            total_sell_qty = 0
            for fill in b["mm_fills"]:
                for pd in fill["position_deltas"]:
                    if pd["delta"] > 0:
                        total_buy_qty += pd["delta"]
                    else:
                        total_sell_qty += abs(pd["delta"])
            if total_buy_qty + total_sell_qty > 0:
                block_fills.append({
                    "day": day_label,
                    "block": b["height"],
                    "buy_qty": total_buy_qty,
                    "sell_qty": total_sell_qty,
                    "total": total_buy_qty + total_sell_qty,
                    "price": b["yes_price"],
                })

    block_fills.sort(key=lambda x: x["total"], reverse=True)
    for bf in block_fills[:10]:
        print(f"  {bf['day']} block {bf['block']}: buy={bf['buy_qty']}, sell={bf['sell_qty']}, total={bf['total']}, price=${bf['price']:.4f}")


if __name__ == "__main__":
    analyze()
