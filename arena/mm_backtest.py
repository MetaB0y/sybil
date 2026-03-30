"""MM strategy backtest using historical price + order data.

Replays 36 days of Iran market simulation data and simulates different MM
strategies against the actual noise/LLM order flow. Estimates PnL for each
strategy by modeling which orders would have filled at the historical
clearing prices.

Usage:
    cd arena && uv run python mm_backtest.py
"""

import json
import math
import os
from abc import ABC, abstractmethod
from dataclasses import dataclass, field


# ── Data loading ──────────────────────────────────────────────────────────

@dataclass
class BlockSnapshot:
    height: int
    yes_price: float | None
    # All orders submitted in this block (parsed from human-readable strings)
    mm_orders: list[dict]
    noise_orders: list[dict]
    trader_orders: list[dict]
    # Actual fills
    mm_fills: list[dict]
    noise_fills: list[dict]
    trader_fills: list[dict]


def parse_order_str(s: str) -> dict | None:
    """Parse 'BuyYes 162 @ $0.3227' into structured dict."""
    parts = s.split()
    if len(parts) < 4:
        return None
    side = parts[0]   # BuyYes, BuyNo, SellYes, SellNo
    try:
        qty = int(parts[1])
        price = float(parts[3].lstrip("$"))
    except (ValueError, IndexError):
        return None
    return {"side": side, "qty": qty, "price": price}


def load_blocks(runs_dir: str, prefix: str = "20260317_") -> list[BlockSnapshot]:
    """Load all blocks from a batch of run files."""
    files = sorted(f for f in os.listdir(runs_dir) if f.startswith(prefix))
    blocks = []
    for fname in files:
        data = json.load(open(os.path.join(runs_dir, fname)))
        for b in data["blocks"]:
            snap = BlockSnapshot(
                height=b["height"],
                yes_price=b.get("yes_price"),
                mm_orders=[parse_order_str(s) for s in b.get("mm_orders", [])],
                noise_orders=[parse_order_str(s) for s in b.get("noise_orders", [])],
                trader_orders=[parse_order_str(s) for s in b.get("trader_orders", [])],
                mm_fills=b.get("mm_fills", []),
                noise_fills=b.get("noise_fills", []),
                trader_fills=b.get("trader_fills", []),
            )
            # Filter None from parse failures
            snap.mm_orders = [o for o in snap.mm_orders if o]
            snap.noise_orders = [o for o in snap.noise_orders if o]
            snap.trader_orders = [o for o in snap.trader_orders if o]
            blocks.append(snap)
    return blocks


# ── Counterparty flow model ──────────────────────────────────────────────

@dataclass
class CounterpartyFlow:
    """Aggregate non-MM order flow in a block (noise + LLM traders).

    From the MM's perspective, these are the orders it might match against.
    """
    buy_yes: list[tuple[float, int]]   # (price, qty)
    buy_no: list[tuple[float, int]]
    sell_yes: list[tuple[float, int]]
    sell_no: list[tuple[float, int]]


def extract_counterparty_flow(block: BlockSnapshot) -> CounterpartyFlow:
    """Extract non-MM orders from a block."""
    flow = CounterpartyFlow([], [], [], [])
    for o in block.noise_orders + block.trader_orders:
        side = o["side"]
        entry = (o["price"], o["qty"])
        if side == "BuyYes":
            flow.buy_yes.append(entry)
        elif side == "BuyNo":
            flow.buy_no.append(entry)
        elif side == "SellYes":
            flow.sell_yes.append(entry)
        elif side == "SellNo":
            flow.sell_no.append(entry)
    return flow


# ── Fill simulation ──────────────────────────────────────────────────────

def simulate_fills(
    mm_orders: list[dict],
    flow: CounterpartyFlow,
    clearing_yes: float,
) -> list[dict]:
    """Simulate which MM orders would fill given counterparty flow.

    Models the FBA matching mechanics:
    1. MINTING: MM BuyYes at p_y matches counterparty BuyNo at p_n when p_y + p_n >= 1.
       Fill price = MM's limit (conservative estimate; real FBA gives clearing price).
    2. DIRECT TRADE: MM BuyYes matches counterparty SellYes when MM bid >= Sell ask.
       Fill price = midpoint of the two limits.
    3. MM SellYes matches counterparty BuyYes when counterparty bid >= MM ask.
       Fill price = midpoint.

    We use the MM's limit price as fill price (worst case for MM — if it's profitable
    even at its own limit, it's profitable at any better clearing price).
    """
    fills = []

    # Sort counterparty orders by aggressiveness for greedy matching
    cp_buy_no = sorted(flow.buy_no, key=lambda x: -x[0])     # highest first
    cp_buy_yes = sorted(flow.buy_yes, key=lambda x: -x[0])
    cp_sell_yes = sorted(flow.sell_yes, key=lambda x: x[0])   # lowest first
    cp_sell_no = sorted(flow.sell_no, key=lambda x: x[0])

    # Track remaining counterparty volume (mutable copies)
    rem_buy_no = [[p, q] for p, q in cp_buy_no]
    rem_buy_yes = [[p, q] for p, q in cp_buy_yes]
    rem_sell_yes = [[p, q] for p, q in cp_sell_yes]
    rem_sell_no = [[p, q] for p, q in cp_sell_no]

    def _consume(pool: list, max_qty: int) -> int:
        """Consume up to max_qty from a pool, return amount consumed."""
        consumed = 0
        for entry in pool:
            if consumed >= max_qty:
                break
            take = min(entry[1], max_qty - consumed)
            entry[1] -= take
            consumed += take
        return consumed

    for o in mm_orders:
        side = o["side"]
        limit = o["price"]
        qty = o["qty"]

        if side == "BuyYes":
            # Match via minting: find counterparty BuyNo where limit + cp_price >= 1
            mint_avail = sum(q for p, q in rem_buy_no if limit + p >= 0.999)
            # Match via direct: find counterparty SellYes where cp_price <= limit
            sell_avail = sum(q for p, q in rem_sell_yes if p <= limit + 0.001)
            total_avail = mint_avail + sell_avail
            fill_qty = min(qty, total_avail)
            if fill_qty > 0:
                # Consume from mint pool first (more common in this market)
                minted = _consume(
                    [e for e in rem_buy_no if limit + e[0] >= 0.999],
                    fill_qty,
                )
                # Also consume from the actual rem_buy_no list
                left = fill_qty
                for e in rem_buy_no:
                    if left <= 0:
                        break
                    if limit + e[0] >= 0.999:
                        take = min(e[1], left)
                        e[1] -= take
                        left -= take
                direct = 0
                if left > 0:
                    for e in rem_sell_yes:
                        if left <= 0:
                            break
                        if e[0] <= limit + 0.001:
                            take = min(e[1], left)
                            e[1] -= take
                            left -= take
                            direct += take
                actual = fill_qty - left
                if actual > 0:
                    # Use limit as fill price (conservative for MM)
                    fills.append({
                        "side": side, "qty": actual,
                        "price": limit, "outcome": "YES", "delta": +actual,
                    })

        elif side == "BuyNo":
            mint_avail = sum(q for p, q in rem_buy_yes if limit + p >= 0.999)
            sell_avail = sum(q for p, q in rem_sell_no if p <= limit + 0.001)
            fill_qty = min(qty, mint_avail + sell_avail)
            if fill_qty > 0:
                left = fill_qty
                for e in rem_buy_yes:
                    if left <= 0:
                        break
                    if limit + e[0] >= 0.999:
                        take = min(e[1], left)
                        e[1] -= take
                        left -= take
                for e in rem_sell_no:
                    if left <= 0:
                        break
                    if e[0] <= limit + 0.001:
                        take = min(e[1], left)
                        e[1] -= take
                        left -= take
                actual = fill_qty - left
                if actual > 0:
                    fills.append({
                        "side": side, "qty": actual,
                        "price": limit, "outcome": "NO", "delta": +actual,
                    })

        elif side == "SellYes":
            # Match with counterparty BuyYes where cp_bid >= MM ask
            avail = sum(q for p, q in rem_buy_yes if p >= limit - 0.001)
            fill_qty = min(qty, avail)
            if fill_qty > 0:
                left = fill_qty
                for e in rem_buy_yes:
                    if left <= 0:
                        break
                    if e[0] >= limit - 0.001:
                        take = min(e[1], left)
                        e[1] -= take
                        left -= take
                actual = fill_qty - left
                if actual > 0:
                    fills.append({
                        "side": side, "qty": actual,
                        "price": limit, "outcome": "YES", "delta": -actual,
                    })

        elif side == "SellNo":
            avail = sum(q for p, q in rem_buy_no if p >= limit - 0.001)
            fill_qty = min(qty, avail)
            if fill_qty > 0:
                left = fill_qty
                for e in rem_buy_no:
                    if left <= 0:
                        break
                    if e[0] >= limit - 0.001:
                        take = min(e[1], left)
                        e[1] -= take
                        left -= take
                actual = fill_qty - left
                if actual > 0:
                    fills.append({
                        "side": side, "qty": actual,
                        "price": limit, "outcome": "NO", "delta": -actual,
                    })

    return fills


# ── MM Strategy Interface ────────────────────────────────────────────────

@dataclass
class MMState:
    """Mutable state shared across blocks for one MM strategy."""
    balance: float = 50_000.0
    yes_pos: int = 0
    no_pos: int = 0
    price_history: list[float] = field(default_factory=list)
    anchor: float | None = None
    fast_ema: float | None = None
    # Tracking
    total_bought_yes_cost: float = 0.0
    total_bought_yes_qty: int = 0
    total_sold_yes_rev: float = 0.0
    total_sold_yes_qty: int = 0
    total_bought_no_cost: float = 0.0
    total_bought_no_qty: int = 0
    total_sold_no_rev: float = 0.0
    total_sold_no_qty: int = 0
    blocks_traded: int = 0
    fills_count: int = 0


class MMStrategy(ABC):
    """Abstract MM strategy that generates orders for a block."""

    def __init__(self, name: str):
        self.name = name

    @abstractmethod
    def generate_orders(self, state: MMState, clearing_yes: float) -> list[dict]:
        """Return list of order dicts: {side, qty, price}."""
        ...

    def update_state(self, state: MMState, fills: list[dict]) -> None:
        """Apply fills to state."""
        for f in fills:
            qty = f["qty"]
            price = f["price"]
            if f["side"] == "BuyYes":
                state.balance -= price * qty
                state.yes_pos += qty
                state.total_bought_yes_cost += price * qty
                state.total_bought_yes_qty += qty
            elif f["side"] == "BuyNo":
                state.balance -= price * qty
                state.no_pos += qty
                state.total_bought_no_cost += price * qty
                state.total_bought_no_qty += qty
            elif f["side"] == "SellYes":
                state.balance += price * qty
                state.yes_pos -= qty
                state.total_sold_yes_rev += price * qty
                state.total_sold_yes_qty += qty
            elif f["side"] == "SellNo":
                state.balance += price * qty
                state.no_pos -= qty
                state.total_sold_no_rev += price * qty
                state.total_sold_no_qty += qty
        if fills:
            state.fills_count += len(fills)
            state.blocks_traded += 1


# ── Concrete Strategies ──────────────────────────────────────────────────

class OriginalMM(MMStrategy):
    """Replicates the current BalancedMarketMaker logic (slow anchor)."""

    def __init__(
        self,
        anchor_alpha: float = 0.03,
        half_spread: float = 0.01,
        max_per_side: float = 100.0,
        num_levels: int = 3,
        level_spacing: float = 0.015,
        max_position: int = 5000,
        vol_lookback: int = 4,
        vol_widen_max: float = 6.0,
        deviation_threshold: float = 0.05,
        skew_factor: float = 0.1,
    ):
        super().__init__(f"Original(α={anchor_alpha})")
        self.anchor_alpha = anchor_alpha
        self.half_spread = half_spread
        self.max_per_side = max_per_side
        self.num_levels = num_levels
        self.level_spacing = level_spacing
        self.max_position = max_position
        self.vol_lookback = vol_lookback
        self.vol_widen_max = vol_widen_max
        self.deviation_threshold = deviation_threshold
        self.skew_factor = skew_factor
        self._buy_taper = [0.50, 0.30, 0.20]

    def _vol_mult(self, state: MMState, mid: float) -> float:
        h = state.price_history
        h.append(mid)
        if len(h) > self.vol_lookback + 1:
            del h[: -self.vol_lookback - 1]
        if len(h) < 2:
            return 1.0
        recent = h[-self.vol_lookback :]
        w_vol = max(recent) - min(recent)
        s_jump = abs(h[-1] - h[-2]) * 2.0 if len(h) >= 2 else 0
        eff = max(w_vol, s_jump)
        if eff > 0.01:
            return min(self.vol_widen_max, 1.0 + eff * 15)
        return 1.0

    def _should_skip(self, state: MMState, raw: float, anchor: float) -> bool:
        h = state.price_history
        if len(h) >= 2 and abs(h[-1] - h[-2]) > 0.05:
            return True
        if abs(raw - anchor) > 0.08:
            return True
        if len(h) >= 4:
            deltas = [h[i] - h[i - 1] for i in range(-3, 0)]
            if all(d > 0.005 for d in deltas) or all(d < -0.005 for d in deltas):
                return True
        return False

    def generate_orders(self, state: MMState, clearing_yes: float) -> list[dict]:
        raw = max(0.05, min(0.95, clearing_yes))

        # Update anchor
        if state.anchor is None:
            state.anchor = raw
        else:
            state.anchor += self.anchor_alpha * (raw - state.anchor)
        anchor = state.anchor

        net = state.yes_pos - state.no_pos
        # Skew
        max_skew = min(anchor, 1.0 - anchor) * 0.20
        if max_skew > 0.005 and self.max_position > 0:
            norm = net * self.skew_factor * 0.01 / max_skew
            skew = max_skew * math.tanh(norm)
        else:
            skew = 0.0
        yes_mid = max(0.05, min(0.95, anchor - skew))
        no_mid = 1.0 - yes_mid

        vol_m = self._vol_mult(state, raw)
        spread = self.half_spread * vol_m
        edge_room = min(yes_mid, no_mid)
        spread = min(spread, edge_room - 0.01)
        if spread < 0.005:
            return []

        skip = self._should_skip(state, raw, anchor)

        # Deviation scales
        dev = raw - anchor
        intensity = min(1.0, abs(dev) / self.deviation_threshold)
        if dev > 0:
            yes_scale = max(0.0, 1.0 - intensity * 2.0)
            no_scale = 1.0
        else:
            yes_scale = 1.0
            no_scale = max(0.0, 1.0 - intensity * 2.0)

        # Inventory fractions (simplified)
        total = state.yes_pos + state.no_pos
        buy_dampen = max(0.1, 1.0 - min(1.0, total / (self.max_position * 2)) * 2.0) if total > 100 else 1.0
        yes_scale *= buy_dampen
        no_scale *= buy_dampen

        eff_per_side = self.max_per_side / vol_m
        budget = min(state.balance, eff_per_side)

        orders = []

        # Buy orders
        if not skip:
            for is_yes in [True, False]:
                mid = yes_mid if is_yes else no_mid
                scale = yes_scale if is_yes else no_scale
                if scale <= 0:
                    continue
                side = "BuyYes" if is_yes else "BuyNo"
                for level in range(self.num_levels):
                    bid = mid - spread - level * self.level_spacing
                    if bid < 0.01:
                        break
                    w = self._buy_taper[min(level, len(self._buy_taper) - 1)]
                    level_dollars = eff_per_side * w * scale
                    qty = int(level_dollars / bid)
                    if qty > 0:
                        orders.append({"side": side, "qty": qty, "price": round(bid, 4)})

        # Sell orders (simplified — net imbalance only)
        abs_net = abs(net)
        if abs_net > 30:
            sell_frac = min(0.80, (abs_net / self.max_position) * 3.0)
            if net > 0 and state.yes_pos > 0 and raw >= anchor:
                qty = max(1, int(state.yes_pos * sell_frac))
                ask = yes_mid + spread
                if ask <= 0.99:
                    orders.append({"side": "SellYes", "qty": qty, "price": round(ask, 4)})
            elif net < 0 and state.no_pos > 0 and raw <= anchor:
                qty = max(1, int(state.no_pos * sell_frac))
                ask = no_mid + spread
                if ask <= 0.99:
                    orders.append({"side": "SellNo", "qty": qty, "price": round(ask, 4)})

        return orders


class CurrentPriceMM(MMStrategy):
    """Quote off current clearing price with inventory skew. No anchor for pricing."""

    def __init__(
        self,
        half_spread: float = 0.03,
        max_per_side: float = 100.0,
        num_levels: int = 3,
        level_spacing: float = 0.02,
        max_position: int = 5000,
        vol_lookback: int = 4,
        vol_widen_max: float = 4.0,
        skew_factor: float = 0.15,
        inventory_sell_start: int = 50,
    ):
        super().__init__(f"CurrentPrice(s={half_spread})")
        self.half_spread = half_spread
        self.max_per_side = max_per_side
        self.num_levels = num_levels
        self.level_spacing = level_spacing
        self.max_position = max_position
        self.vol_lookback = vol_lookback
        self.vol_widen_max = vol_widen_max
        self.skew_factor = skew_factor
        self.inventory_sell_start = inventory_sell_start
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
            return min(self.vol_widen_max, 1.0 + eff * 10)
        return 1.0

    def generate_orders(self, state: MMState, clearing_yes: float) -> list[dict]:
        raw = max(0.05, min(0.95, clearing_yes))
        state.price_history.append(raw)
        if len(state.price_history) > self.vol_lookback + 2:
            del state.price_history[: -self.vol_lookback - 2]

        net = state.yes_pos - state.no_pos
        # Inventory skew: shift mid AWAY from net position
        if self.max_position > 0:
            skew = -math.tanh(net / self.max_position) * self.skew_factor
        else:
            skew = 0.0
        yes_mid = max(0.05, min(0.95, raw + skew))
        no_mid = 1.0 - yes_mid

        vol_m = self._vol_mult(state)
        spread = self.half_spread * vol_m
        edge_room = min(yes_mid, no_mid)
        spread = min(spread, edge_room - 0.01)
        if spread < 0.005:
            return []

        # Dampen buying when total inventory is high
        total = state.yes_pos + state.no_pos
        buy_scale = max(0.1, 1.0 - min(1.0, total / (self.max_position * 2)) * 2.0) if total > 100 else 1.0

        eff_per_side = self.max_per_side / vol_m

        orders = []

        # Buy orders on both sides (symmetric, skew handles direction)
        for is_yes in [True, False]:
            mid = yes_mid if is_yes else no_mid
            side = "BuyYes" if is_yes else "BuyNo"
            for level in range(self.num_levels):
                bid = mid - spread - level * self.level_spacing
                if bid < 0.01:
                    break
                w = self._buy_taper[min(level, len(self._buy_taper) - 1)]
                level_dollars = eff_per_side * w * buy_scale
                qty = int(level_dollars / bid)
                if qty > 0:
                    orders.append({"side": side, "qty": qty, "price": round(bid, 4)})

        # Sell excess inventory on BOTH sides when positions grow
        for is_yes in [True, False]:
            pos = state.yes_pos if is_yes else state.no_pos
            if pos > self.inventory_sell_start:
                mid = yes_mid if is_yes else no_mid
                side = "SellYes" if is_yes else "SellNo"
                sell_frac = min(0.5, (pos - self.inventory_sell_start) / self.max_position * 2.0)
                qty = max(1, int(pos * sell_frac))
                ask = mid + spread
                if 0.01 <= ask <= 0.99:
                    orders.append({"side": side, "qty": qty, "price": round(ask, 4)})

        return orders


class FastAnchorMM(MMStrategy):
    """Like Original but with much faster anchor and wider spread."""

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
        skew_factor: float = 0.15,
        inventory_sell_start: int = 50,
    ):
        super().__init__(f"FastAnchor(α={anchor_alpha},s={half_spread})")
        self.anchor_alpha = anchor_alpha
        self.half_spread = half_spread
        self.max_per_side = max_per_side
        self.num_levels = num_levels
        self.level_spacing = level_spacing
        self.max_position = max_position
        self.vol_lookback = vol_lookback
        self.vol_widen_max = vol_widen_max
        self.skew_factor = skew_factor
        self.inventory_sell_start = inventory_sell_start
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
            return min(self.vol_widen_max, 1.0 + eff * 10)
        return 1.0

    def generate_orders(self, state: MMState, clearing_yes: float) -> list[dict]:
        raw = max(0.05, min(0.95, clearing_yes))
        state.price_history.append(raw)
        if len(state.price_history) > self.vol_lookback + 2:
            del state.price_history[: -self.vol_lookback - 2]

        # Fast anchor
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
        # Use anchor (fast) as mid — sits between current price and history
        yes_mid = max(0.05, min(0.95, anchor + skew))
        no_mid = 1.0 - yes_mid

        vol_m = self._vol_mult(state)
        spread = self.half_spread * vol_m
        edge_room = min(yes_mid, no_mid)
        spread = min(spread, edge_room - 0.01)
        if spread < 0.005:
            return []

        total = state.yes_pos + state.no_pos
        buy_scale = max(0.1, 1.0 - min(1.0, total / (self.max_position * 2)) * 2.0) if total > 100 else 1.0
        eff_per_side = self.max_per_side / vol_m

        orders = []
        for is_yes in [True, False]:
            mid = yes_mid if is_yes else no_mid
            side = "BuyYes" if is_yes else "BuyNo"
            for level in range(self.num_levels):
                bid = mid - spread - level * self.level_spacing
                if bid < 0.01:
                    break
                w = self._buy_taper[min(level, len(self._buy_taper) - 1)]
                level_dollars = eff_per_side * w * buy_scale
                qty = int(level_dollars / bid)
                if qty > 0:
                    orders.append({"side": side, "qty": qty, "price": round(bid, 4)})

        # Sell excess inventory
        for is_yes in [True, False]:
            pos = state.yes_pos if is_yes else state.no_pos
            if pos > self.inventory_sell_start:
                mid = yes_mid if is_yes else no_mid
                side = "SellYes" if is_yes else "SellNo"
                sell_frac = min(0.5, (pos - self.inventory_sell_start) / self.max_position * 2.0)
                qty = max(1, int(pos * sell_frac))
                ask = mid + spread
                if 0.01 <= ask <= 0.99:
                    orders.append({"side": side, "qty": qty, "price": round(ask, 4)})

        return orders


class WidenOnlyMM(MMStrategy):
    """Original anchor logic but with wider spread (3c) — isolate spread effect."""

    def __init__(self):
        super().__init__("WideSpreadOriginal(s=0.03)")
        self._inner = OriginalMM(half_spread=0.03)

    def generate_orders(self, state: MMState, clearing_yes: float) -> list[dict]:
        return self._inner.generate_orders(state, clearing_yes)


class TrueReversion(MMStrategy):
    """Only buy the cheap side relative to a medium-speed anchor.

    True mean-reversion: when price > anchor, only buy NO (sell YES exposure).
    When price < anchor, only buy YES (sell NO exposure).
    """

    def __init__(
        self,
        anchor_alpha: float = 0.10,
        half_spread: float = 0.03,
        max_per_side: float = 150.0,
        num_levels: int = 3,
        level_spacing: float = 0.02,
        max_position: int = 5000,
        vol_lookback: int = 4,
        vol_widen_max: float = 4.0,
        min_deviation: float = 0.02,  # Only trade when deviation > this
    ):
        super().__init__(f"TrueReversion(α={anchor_alpha})")
        self.anchor_alpha = anchor_alpha
        self.half_spread = half_spread
        self.max_per_side = max_per_side
        self.num_levels = num_levels
        self.level_spacing = level_spacing
        self.max_position = max_position
        self.vol_lookback = vol_lookback
        self.vol_widen_max = vol_widen_max
        self.min_deviation = min_deviation
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
            return min(self.vol_widen_max, 1.0 + eff * 10)
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

        deviation = raw - anchor  # positive = price above anchor

        vol_m = self._vol_mult(state)
        spread = self.half_spread * vol_m

        orders = []
        eff_per_side = self.max_per_side / vol_m

        # Only provide liquidity on the CHEAP side
        # Price above anchor → YES is expensive → buy NO (the cheap one)
        # Price below anchor → YES is cheap → buy YES
        if abs(deviation) >= self.min_deviation:
            # Scale by deviation magnitude
            scale = min(2.0, abs(deviation) / self.min_deviation)

            if deviation > 0:
                # YES is expensive → buy NO
                no_mid = 1.0 - raw
                for level in range(self.num_levels):
                    bid = no_mid - spread - level * self.level_spacing
                    if bid < 0.01:
                        break
                    w = self._buy_taper[min(level, len(self._buy_taper) - 1)]
                    qty = int(eff_per_side * w * scale / bid)
                    if qty > 0:
                        orders.append({"side": "BuyNo", "qty": qty, "price": round(bid, 4)})

                # Sell YES if we hold any (sell the expensive side)
                if state.yes_pos > 10:
                    sell_frac = min(0.6, scale * 0.3)
                    qty = max(1, int(state.yes_pos * sell_frac))
                    ask = raw + spread
                    if ask <= 0.99:
                        orders.append({"side": "SellYes", "qty": qty, "price": round(ask, 4)})
            else:
                # YES is cheap → buy YES
                yes_mid = raw
                for level in range(self.num_levels):
                    bid = yes_mid - spread - level * self.level_spacing
                    if bid < 0.01:
                        break
                    w = self._buy_taper[min(level, len(self._buy_taper) - 1)]
                    qty = int(eff_per_side * w * scale / bid)
                    if qty > 0:
                        orders.append({"side": "BuyYes", "qty": qty, "price": round(bid, 4)})

                # Sell NO if we hold any (sell the expensive side)
                if state.no_pos > 10:
                    sell_frac = min(0.6, scale * 0.3)
                    qty = max(1, int(state.no_pos * sell_frac))
                    ask = (1.0 - raw) + spread
                    if ask <= 0.99:
                        orders.append({"side": "SellNo", "qty": qty, "price": round(ask, 4)})
        else:
            # Near anchor — provide symmetric liquidity with wider spread
            wide_spread = spread * 1.5
            for is_yes in [True, False]:
                mid = raw if is_yes else (1.0 - raw)
                side = "BuyYes" if is_yes else "BuyNo"
                for level in range(self.num_levels):
                    bid = mid - wide_spread - level * self.level_spacing
                    if bid < 0.01:
                        break
                    w = self._buy_taper[min(level, len(self._buy_taper) - 1)]
                    qty = int(eff_per_side * 0.5 * w / bid)
                    if qty > 0:
                        orders.append({"side": side, "qty": qty, "price": round(bid, 4)})

        return orders


# ── Backtest Runner ──────────────────────────────────────────────────────

def run_backtest(
    blocks: list[BlockSnapshot],
    strategy: MMStrategy,
) -> MMState:
    """Run a strategy through all blocks and return final state."""
    state = MMState()

    for block in blocks:
        if block.yes_price is None:
            continue

        clearing_yes = block.yes_price
        flow = extract_counterparty_flow(block)

        # Generate MM orders
        mm_orders = strategy.generate_orders(state, clearing_yes)

        # Simulate fills
        fills = simulate_fills(mm_orders, flow, clearing_yes)

        # Update state
        strategy.update_state(state, fills)

    return state


def print_results(name: str, state: MMState, final_yes: float):
    """Print strategy results."""
    final_no = 1.0 - final_yes
    pos_value = state.yes_pos * final_yes + state.no_pos * final_no
    portfolio = state.balance + pos_value
    pnl = portfolio - 50_000.0

    print(f"\n{'─' * 60}")
    print(f"  {name}")
    print(f"{'─' * 60}")
    print(f"  Balance:    ${state.balance:>12,.2f}")
    print(f"  YES pos:    {state.yes_pos:>12,}  (${state.yes_pos * final_yes:>10,.2f})")
    print(f"  NO pos:     {state.no_pos:>12,}  (${state.no_pos * final_no:>10,.2f})")
    print(f"  Pos value:  ${pos_value:>12,.2f}")
    print(f"  Portfolio:  ${portfolio:>12,.2f}")
    print(f"  PnL:        ${pnl:>+12,.2f}")
    print(f"  Fills:      {state.fills_count:>12,}  ({state.blocks_traded} blocks)")

    # Timing analysis
    if state.total_bought_yes_qty > 0 and state.total_sold_yes_qty > 0:
        avg_buy_yes = state.total_bought_yes_cost / state.total_bought_yes_qty
        avg_sell_yes = state.total_sold_yes_rev / state.total_sold_yes_qty
        print(f"  YES: buy@{avg_buy_yes:.4f} sell@{avg_sell_yes:.4f}  edge={avg_sell_yes - avg_buy_yes:+.4f}")
    if state.total_bought_no_qty > 0 and state.total_sold_no_qty > 0:
        avg_buy_no = state.total_bought_no_cost / state.total_bought_no_qty
        avg_sell_no = state.total_sold_no_rev / state.total_sold_no_qty
        print(f"  NO:  buy@{avg_buy_no:.4f} sell@{avg_sell_no:.4f}  edge={avg_sell_no - avg_buy_no:+.4f}")

    realized = (
        (state.total_sold_yes_rev - state.total_bought_yes_cost)
        + (state.total_sold_no_rev - state.total_bought_no_cost)
    )
    print(f"  Realized:   ${realized:>+12,.2f}")
    print(f"  Unrealized: ${pnl - realized:>+12,.2f}")


# ── Main ─────────────────────────────────────────────────────────────────

def main():
    runs_dir = os.path.join(os.path.dirname(__file__), "markets", "iran", "runs")

    print("Loading 36-day block history...")
    blocks = load_blocks(runs_dir)
    print(f"  {len(blocks)} blocks loaded")

    # Find final clearing price
    final_yes = 0.16
    for b in reversed(blocks):
        if b.yes_price is not None:
            final_yes = b.yes_price
            break
    print(f"  Final YES price: {final_yes:.4f}")

    # Price stats
    prices = [b.yes_price for b in blocks if b.yes_price is not None]
    print(f"  Price range: {min(prices):.4f} – {max(prices):.4f}")
    print(f"  Avg blocks/day with flow: {sum(1 for b in blocks if b.noise_orders or b.trader_orders) / 36:.0f}")

    strategies = [
        OriginalMM(),
        OriginalMM(anchor_alpha=0.10, half_spread=0.01),
        WidenOnlyMM(),
        CurrentPriceMM(half_spread=0.03),
        CurrentPriceMM(half_spread=0.05),
        FastAnchorMM(anchor_alpha=0.20, half_spread=0.03),
        FastAnchorMM(anchor_alpha=0.10, half_spread=0.03),
        TrueReversion(anchor_alpha=0.10),
        TrueReversion(anchor_alpha=0.05),
    ]

    print(f"\n{'=' * 60}")
    print("  MM STRATEGY BACKTEST — 36 days Iran market")
    print(f"{'=' * 60}")

    for strat in strategies:
        state = run_backtest(blocks, strat)
        print_results(strat.name, state, final_yes)

    # Also show what the actual MM did (from leaderboard)
    print(f"\n{'─' * 60}")
    print(f"  Actual MM (from simulation)")
    print(f"{'─' * 60}")
    print(f"  PnL:        $-2,767.47")
    print(f"  YES: buy@0.2607 sell@0.2487  edge=-0.0120")
    print(f"  NO:  buy@0.7193 sell@0.7119  edge=-0.0074")


if __name__ == "__main__":
    main()
