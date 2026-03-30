"""Parameter sweep for FastAnchor MM strategy.

Tests many parameter combinations against historical data to find
the optimal settings before running real simulations.

Usage:
    cd arena && uv run python mm_param_sweep.py
"""

import json
import math
import os
from dataclasses import dataclass, field
from itertools import product

from mm_backtest import (
    MMState,
    MMStrategy,
    extract_counterparty_flow,
    load_blocks,
    simulate_fills,
)
from sybil_client import BuyNo, BuyYes, OrderSpec, SellNo, SellYes


class ParametricMM(MMStrategy):
    """FastAnchor MM with configurable parameters for sweep."""

    def __init__(
        self,
        anchor_alpha: float = 0.20,
        half_spread: float = 0.03,
        max_per_side: float = 100.0,
        num_levels: int = 3,
        level_spacing: float = 0.02,
        max_position: int = 5000,
        vol_lookback: int = 4,
        vol_widen_max: float = 4.0,
        vol_sensitivity: float = 10.0,  # multiplier for vol→spread scaling
        skew_factor: float = 0.15,
        inventory_sell_start: int = 50,
        sell_aggression: float = 2.0,  # higher = sell faster
        max_sell_frac: float = 0.5,
    ):
        name = f"s={half_spread:.2f} $/side={max_per_side:.0f} sell@{inventory_sell_start} maxpos={max_position} vol_max={vol_widen_max}"
        super().__init__(name)
        self.anchor_alpha = anchor_alpha
        self.half_spread = half_spread
        self.max_per_side = max_per_side
        self.num_levels = num_levels
        self.level_spacing = level_spacing
        self.max_position = max_position
        self.vol_lookback = vol_lookback
        self.vol_widen_max = vol_widen_max
        self.vol_sensitivity = vol_sensitivity
        self.skew_factor = skew_factor
        self.inventory_sell_start = inventory_sell_start
        self.sell_aggression = sell_aggression
        self.max_sell_frac = max_sell_frac
        self._buy_taper = [0.50, 0.30, 0.20]

    def _vol_mult(self, state: MMState) -> float:
        h = state.price_history
        if len(h) < 2:
            return 1.0
        recent = h[-self.vol_lookback:]
        w_vol = max(recent) - min(recent)
        s_jump = abs(h[-1] - h[-2]) * 2.0
        eff = max(w_vol, s_jump)
        if eff > 0.02:
            return min(self.vol_widen_max, 1.0 + eff * self.vol_sensitivity)
        return 1.0

    def generate_orders(self, state: MMState, clearing_yes: float) -> list[dict]:
        raw = max(0.05, min(0.95, clearing_yes))
        state.price_history.append(raw)
        if len(state.price_history) > self.vol_lookback + 2:
            del state.price_history[: -self.vol_lookback - 2]

        if state.anchor is None:
            state.anchor = raw
        else:
            state.anchor += self.anchor_alpha * (raw - state.anchor)
        anchor = state.anchor

        net = state.yes_pos - state.no_pos
        if self.max_position > 0:
            skew = -math.tanh(net / self.max_position) * self.skew_factor
        else:
            skew = 0.0
        yes_mid = max(0.05, min(0.95, anchor + skew))
        no_mid = 1.0 - yes_mid

        vol_m = self._vol_mult(state)
        spread = self.half_spread * vol_m

        edge_room = min(yes_mid, no_mid)
        spread = min(spread, edge_room - 0.01)
        if spread < 0.005:
            return []

        # Dampen buying when total inventory is high
        total = state.yes_pos + state.no_pos
        if total > 50:
            buy_scale = max(0.05, 1.0 - min(1.0, total / (self.max_position * 2)) * 2.0)
        else:
            buy_scale = 1.0

        # Hard position cap
        if state.yes_pos >= self.max_position:
            buy_scale_yes = 0.0
        else:
            buy_scale_yes = buy_scale
        if state.no_pos >= self.max_position:
            buy_scale_no = 0.0
        else:
            buy_scale_no = buy_scale

        eff_per_side = self.max_per_side / vol_m

        orders = []

        # Buy orders
        for is_yes, scale in [(True, buy_scale_yes), (False, buy_scale_no)]:
            if scale <= 0:
                continue
            mid = yes_mid if is_yes else no_mid
            side = "BuyYes" if is_yes else "BuyNo"
            spent = 0.0
            for level in range(self.num_levels):
                bid = mid - spread - level * self.level_spacing
                if bid < 0.01:
                    break
                w = self._buy_taper[min(level, len(self._buy_taper) - 1)]
                level_dollars = eff_per_side * w * scale
                room = eff_per_side * scale - spent
                if room <= 0:
                    break
                level_dollars = min(level_dollars, room)
                qty = int(level_dollars / bid)
                if qty > 0:
                    orders.append({"side": side, "qty": qty, "price": round(bid, 4)})
                    spent += qty * bid

        # Sell excess inventory
        for is_yes in [True, False]:
            pos = state.yes_pos if is_yes else state.no_pos
            if pos > self.inventory_sell_start:
                mid = yes_mid if is_yes else no_mid
                side = "SellYes" if is_yes else "SellNo"
                sell_frac = min(
                    self.max_sell_frac,
                    (pos - self.inventory_sell_start) / self.max_position * self.sell_aggression,
                )
                qty = max(1, int(pos * sell_frac))
                remaining = qty
                for level in range(self.num_levels):
                    ask = mid + spread + level * self.level_spacing
                    if ask > 0.99 or remaining <= 0:
                        break
                    w = self._buy_taper[min(level, len(self._buy_taper) - 1)]
                    lqty = max(1, min(int(qty * w), remaining))
                    orders.append({"side": side, "qty": lqty, "price": round(ask, 4)})
                    remaining -= lqty

        return orders


def run_sweep(blocks, final_yes, configs):
    """Run all configs and return sorted results."""
    results = []

    for cfg in configs:
        strat = ParametricMM(**cfg)
        state = MMState()

        for block in blocks:
            if block.yes_price is None:
                continue
            flow = extract_counterparty_flow(block)
            mm_orders = strat.generate_orders(state, block.yes_price)
            fills = simulate_fills(mm_orders, flow, block.yes_price)
            strat.update_state(state, fills)

        final_no = 1.0 - final_yes
        pos_value = state.yes_pos * final_yes + state.no_pos * final_no
        portfolio = state.balance + pos_value
        pnl = portfolio - 50_000.0

        realized = (
            (state.total_sold_yes_rev - state.total_bought_yes_cost)
            + (state.total_sold_no_rev - state.total_bought_no_cost)
        )

        # Compute timing edges
        yes_edge = 0
        if state.total_bought_yes_qty > 0 and state.total_sold_yes_qty > 0:
            yes_edge = (
                state.total_sold_yes_rev / state.total_sold_yes_qty
                - state.total_bought_yes_cost / state.total_bought_yes_qty
            )
        no_edge = 0
        if state.total_bought_no_qty > 0 and state.total_sold_no_qty > 0:
            no_edge = (
                state.total_sold_no_rev / state.total_sold_no_qty
                - state.total_bought_no_cost / state.total_bought_no_qty
            )

        results.append({
            "config": cfg,
            "name": strat.name,
            "pnl": pnl,
            "realized": realized,
            "unrealized": pnl - realized,
            "fills": state.fills_count,
            "blocks_traded": state.blocks_traded,
            "yes_pos": state.yes_pos,
            "no_pos": state.no_pos,
            "yes_edge": yes_edge,
            "no_edge": no_edge,
            "balance": state.balance,
        })

    results.sort(key=lambda r: r["pnl"], reverse=True)
    return results


def main():
    runs_dir = os.path.join(os.path.dirname(__file__), "markets", "iran", "runs")
    print("Loading 36-day block history...")
    blocks = load_blocks(runs_dir)
    print(f"  {len(blocks)} blocks loaded")

    final_yes = 0.16
    for b in reversed(blocks):
        if b.yes_price is not None:
            final_yes = b.yes_price
            break

    # Parameter sweep
    configs = []
    for spread in [0.03, 0.04, 0.05, 0.06, 0.08]:
        for per_side in [30, 50, 100]:
            for sell_start in [15, 30, 50]:
                for max_pos in [500, 1000, 2000]:
                    for vol_max in [4.0, 6.0, 8.0]:
                        for sell_agg in [2.0, 4.0]:
                            configs.append({
                                "half_spread": spread,
                                "max_per_side": per_side,
                                "inventory_sell_start": sell_start,
                                "max_position": max_pos,
                                "vol_widen_max": vol_max,
                                "sell_aggression": sell_agg,
                                "anchor_alpha": 0.20,
                                "level_spacing": spread * 0.6,  # scale with spread
                            })

    print(f"Testing {len(configs)} parameter combinations...")
    results = run_sweep(blocks, final_yes, configs)

    # Print top 20
    print(f"\n{'=' * 100}")
    print(f"  TOP 20 STRATEGIES (by total PnL)")
    print(f"{'=' * 100}")
    print(f"{'#':>3} {'PnL':>10} {'Realized':>10} {'Unreal':>10} {'Fills':>6} {'YPos':>6} {'NPos':>6} {'Y-edge':>8} {'N-edge':>8}  Config")
    print("-" * 100)

    for i, r in enumerate(results[:20]):
        c = r["config"]
        cfg_str = f"s={c['half_spread']:.2f} $/s={c['max_per_side']:.0f} sell@{c['inventory_sell_start']} max={c['max_position']} vol={c['vol_widen_max']:.0f}x agg={c['sell_aggression']:.0f}"
        print(
            f"{i+1:>3} ${r['pnl']:>+9.0f} ${r['realized']:>+9.0f} ${r['unrealized']:>+9.0f}"
            f" {r['fills']:>6} {r['yes_pos']:>6} {r['no_pos']:>6}"
            f" {r['yes_edge']:>+8.4f} {r['no_edge']:>+8.4f}  {cfg_str}"
        )

    # Print bottom 5 (worst)
    print(f"\n  BOTTOM 5 (worst)")
    print("-" * 100)
    for r in results[-5:]:
        c = r["config"]
        cfg_str = f"s={c['half_spread']:.2f} $/s={c['max_per_side']:.0f} sell@{c['inventory_sell_start']} max={c['max_position']} vol={c['vol_widen_max']:.0f}x agg={c['sell_aggression']:.0f}"
        print(
            f"    ${r['pnl']:>+9.0f} ${r['realized']:>+9.0f} ${r['unrealized']:>+9.0f}"
            f" {r['fills']:>6} {r['yes_pos']:>6} {r['no_pos']:>6}"
            f" {r['yes_edge']:>+8.4f} {r['no_edge']:>+8.4f}  {cfg_str}"
        )

    # Current fast-anchor baseline
    print(f"\n  CURRENT FAST-ANCHOR BASELINE")
    print("-" * 100)
    baseline_cfg = {
        "half_spread": 0.03, "max_per_side": 100, "inventory_sell_start": 50,
        "max_position": 5000, "vol_widen_max": 4.0, "sell_aggression": 2.0,
        "anchor_alpha": 0.20, "level_spacing": 0.02,
    }
    baseline = run_sweep(blocks, final_yes, [baseline_cfg])[0]
    print(f"    PnL=${baseline['pnl']:>+9.0f}  Realized=${baseline['realized']:>+9.0f}  Fills={baseline['fills']}  YPos={baseline['yes_pos']}  NPos={baseline['no_pos']}")

    # Analyze patterns in top results
    print(f"\n  PARAMETER PATTERNS (top 20 avg vs bottom 20 avg)")
    print("-" * 60)
    top20 = results[:20]
    bot20 = results[-20:]
    for key in ["half_spread", "max_per_side", "inventory_sell_start", "max_position", "vol_widen_max", "sell_aggression"]:
        top_avg = sum(r["config"][key] for r in top20) / 20
        bot_avg = sum(r["config"][key] for r in bot20) / 20
        print(f"  {key:<25} top={top_avg:>8.2f}  bottom={bot_avg:>8.2f}")


if __name__ == "__main__":
    main()
