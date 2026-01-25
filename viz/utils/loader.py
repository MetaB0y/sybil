"""JSON loading utilities for pipeline snapshots."""

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import pandas as pd


@dataclass
class SnapshotData:
    """Parsed snapshot data with convenient accessors."""

    raw: dict[str, Any]

    @property
    def scenario_name(self) -> str:
        return self.raw.get("scenario_name", "Unknown")

    @property
    def config(self) -> dict[str, Any]:
        return self.raw.get("config", {})

    @property
    def iterations(self) -> list[dict[str, Any]]:
        return self.raw.get("iterations", [])

    @property
    def final_result(self) -> dict[str, Any]:
        return self.raw.get("final_result", {})

    @property
    def phase_times(self) -> dict[str, float]:
        return self.raw.get("phase_times", {})

    @property
    def orders(self) -> list[dict[str, Any]]:
        return self.raw.get("orders", [])

    @property
    def fills_by_iteration(self) -> list[dict[str, Any]]:
        return self.raw.get("fills_by_iteration", [])

    @property
    def initial_liquidity(self) -> dict[str, Any] | None:
        """Get initial liquidity snapshot (if captured with viz feature)."""
        return self.raw.get("initial_liquidity")

    @property
    def final_liquidity(self) -> dict[str, Any] | None:
        """Get final liquidity snapshot (if captured with viz feature)."""
        return self.raw.get("final_liquidity")

    @property
    def phase_snapshots(self) -> list[dict[str, Any]]:
        """Get phase snapshots (if captured with viz feature)."""
        return self.raw.get("phase_snapshots", [])

    def iterations_df(self) -> pd.DataFrame:
        """Get iterations as a DataFrame."""
        if not self.iterations:
            return pd.DataFrame()

        records = []
        for it in self.iterations:
            records.append(
                {
                    "iteration": it.get("iteration", 0),
                    "welfare": it.get("welfare", 0),
                    "welfare_dollars": it.get("welfare", 0) / 1e9,
                    "welfare_delta": it.get("welfare_delta", 0),
                    "welfare_delta_dollars": it.get("welfare_delta", 0) / 1e9,
                    "volume": it.get("volume", 0),
                    "volume_delta": it.get("volume_delta", 0),
                    "fills": it.get("fills", 0),
                    "fills_delta": it.get("fills_delta", 0),
                    "price_discovery_fills": it.get("price_discovery_fills", 0),
                    "bundle_fills": it.get("bundle_fills", 0),
                }
            )
        return pd.DataFrame(records)

    def orders_df(self) -> pd.DataFrame:
        """Get orders as a DataFrame."""
        if not self.orders:
            return pd.DataFrame()

        records = []
        for order in self.orders:
            records.append(
                {
                    "id": order.get("id", 0),
                    "markets": ", ".join(order.get("markets", [])),
                    "order_type": order.get("order_type", "unknown"),
                    "is_aon": order.get("is_aon", False),
                    "is_mm": order.get("is_mm", False),
                    "limit_price": order.get("limit_price", 0.0),
                    "max_qty": order.get("max_qty", 0),
                }
            )
        return pd.DataFrame(records)

    def fills_df(self) -> pd.DataFrame:
        """Get all fills as a DataFrame."""
        if not self.fills_by_iteration:
            return pd.DataFrame()

        records = []
        for iter_fills in self.fills_by_iteration:
            iteration = iter_fills.get("iteration", 0)
            for fill in iter_fills.get("fills", []):
                records.append(
                    {
                        "iteration": iteration,
                        "order_id": fill.get("order_id", 0),
                        "fill_qty": fill.get("fill_qty", 0),
                        "fill_price": fill.get("fill_price", 0.0),
                        "welfare": fill.get("welfare", 0.0),
                        "source": fill.get("source", "unknown"),
                    }
                )
        return pd.DataFrame(records)

    def market_prices_df(self, iteration: int | None = None) -> pd.DataFrame:
        """Get market prices for a specific iteration (or latest if None)."""
        if not self.iterations:
            return pd.DataFrame()

        if iteration is None:
            iteration = max(it.get("iteration", 0) for it in self.iterations)

        iter_data = next(
            (it for it in self.iterations if it.get("iteration") == iteration), None
        )
        if not iter_data:
            return pd.DataFrame()

        market_prices = iter_data.get("market_prices", {})
        records = []
        for market_name, prices in market_prices.items():
            records.append(
                {
                    "market": market_name,
                    "yes_price": prices.get("yes_price", 0.0),
                    "no_price": prices.get("no_price", 0.0),
                    "volume": prices.get("volume", 0),
                    "welfare": prices.get("welfare", 0),
                }
            )
        return pd.DataFrame(records)

    def get_market_names(self) -> list[str]:
        """Get list of all market names from orders."""
        markets = set()
        for order in self.orders:
            for m in order.get("markets", []):
                markets.add(m)
        return sorted(markets)

    def get_market_price_history(self, market_name: str) -> pd.DataFrame:
        """Get price history for a specific market across iterations."""
        records = []
        for it in self.iterations:
            iteration = it.get("iteration", 0)
            market_prices = it.get("market_prices", {})
            if market_name in market_prices:
                prices = market_prices[market_name]
                records.append(
                    {
                        "iteration": iteration,
                        "yes_price": prices.get("yes_price", 0.0),
                        "no_price": prices.get("no_price", 0.0),
                        "volume": prices.get("volume", 0),
                        "welfare": prices.get("welfare", 0),
                    }
                )
        return pd.DataFrame(records)

    def get_order_fills(self, order_id: int) -> pd.DataFrame:
        """Get fills for a specific order across iterations."""
        records = []
        for iter_fills in self.fills_by_iteration:
            iteration = iter_fills.get("iteration", 0)
            for fill in iter_fills.get("fills", []):
                if fill.get("order_id") == order_id:
                    records.append(
                        {
                            "iteration": iteration,
                            "fill_qty": fill.get("fill_qty", 0),
                            "fill_price": fill.get("fill_price", 0.0),
                            "welfare": fill.get("welfare", 0.0),
                            "source": fill.get("source", "unknown"),
                        }
                    )
        return pd.DataFrame(records)

    def get_orders_for_market(self, market_name: str) -> pd.DataFrame:
        """Get orders that involve a specific market."""
        records = []
        for order in self.orders:
            if market_name in order.get("markets", []):
                records.append(
                    {
                        "id": order.get("id", 0),
                        "markets": ", ".join(order.get("markets", [])),
                        "order_type": order.get("order_type", "unknown"),
                        "is_aon": order.get("is_aon", False),
                        "is_mm": order.get("is_mm", False),
                        "limit_price": order.get("limit_price", 0.0),
                        "max_qty": order.get("max_qty", 0),
                    }
                )
        return pd.DataFrame(records)

    def get_book_markets(self, liquidity: dict[str, Any] | None = None) -> list[str]:
        """Get list of unique market names from orderbook data."""
        if liquidity is None:
            liquidity = self.initial_liquidity
        if not liquidity:
            return []
        markets = set()
        for book in liquidity.get("books", []):
            markets.add(book.get("market_name", ""))
        return sorted(markets)

    def get_book_for_market(
        self, market_name: str, outcome: int = 0, liquidity: dict[str, Any] | None = None
    ) -> dict[str, Any] | None:
        """Get orderbook for a specific market and outcome."""
        if liquidity is None:
            liquidity = self.initial_liquidity
        if not liquidity:
            return None
        for book in liquidity.get("books", []):
            if book.get("market_name") == market_name and book.get("outcome") == outcome:
                return book
        return None

    def get_books_for_market(
        self, market_name: str, liquidity: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Get all orderbooks (YES and NO) for a specific market."""
        if liquidity is None:
            liquidity = self.initial_liquidity
        if not liquidity:
            return []
        return [
            book
            for book in liquidity.get("books", [])
            if book.get("market_name") == market_name
        ]

    def get_phase_liquidity(self, phase_index: int) -> dict[str, Any] | None:
        """Get liquidity snapshot for a specific phase index."""
        if phase_index < 0 or phase_index >= len(self.phase_snapshots):
            return None
        return self.phase_snapshots[phase_index].get("liquidity")

    def get_clearing_price(
        self, market_name: str, outcome: int = 0, iteration: int = -1
    ) -> float | None:
        """
        Get clearing price for a market/outcome at a specific iteration.

        Args:
            market_name: Name of the market
            outcome: 0 for YES, 1 for NO
            iteration: Iteration number (-1 for latest)

        Returns:
            Clearing price as a fraction (0.0-1.0), or None if not available
        """
        history = self.get_market_price_history(market_name)
        if history.empty:
            return None

        idx = iteration if iteration >= 0 else len(history) - 1
        if idx >= len(history):
            idx = len(history) - 1

        row = history.iloc[idx]
        if outcome == 0:
            return row.get("yes_price")
        else:
            return row.get("no_price")

    def get_phase_info(self, phase_index: int) -> dict[str, Any] | None:
        """Get phase snapshot metadata (phase name, fills, welfare, etc.)."""
        if phase_index < 0 or phase_index >= len(self.phase_snapshots):
            return None
        return self.phase_snapshots[phase_index]

    def phases_df(self) -> pd.DataFrame:
        """Get phase snapshots as a DataFrame for analysis."""
        if not self.phase_snapshots:
            return pd.DataFrame()

        records = []
        for i, snap in enumerate(self.phase_snapshots):
            phase_name = snap.get("phase", "Unknown")
            if isinstance(phase_name, dict):
                # Handle enum serialization like {"PriceDiscovery": null}
                phase_name = list(phase_name.keys())[0] if phase_name else "Unknown"

            # Extract phase-specific metadata
            metadata = snap.get("phase_metadata", {})
            metadata_str = ""
            if metadata:
                if "PriceDiscovery" in metadata:
                    md = metadata["PriceDiscovery"]
                    metadata_str = f"{md.get('markets_priced', 0)} markets"
                elif "PriceProjection" in metadata:
                    md = metadata["PriceProjection"]
                    metadata_str = f"{md.get('violations_fixed', 0)} violations, ${md.get('max_adjustment', 0):.2f} max adj"
                elif "NegriskArbitrage" in metadata:
                    md = metadata["NegriskArbitrage"]
                    metadata_str = f"{md.get('opportunities_found', 0)} arbs, {md.get('total_shares', 0)} shares, +${md.get('welfare_added', 0):.2f}"
                elif "MmAllocation" in metadata:
                    md = metadata["MmAllocation"]
                    metadata_str = f"{md.get('orders_activated', 0)} orders, {md.get('mm_count', 0)} MMs"
                elif "Merged" in metadata:
                    md = metadata["Merged"]
                    metadata_str = f"{md.get('single_market_fills', 0)} single-market fills"
                elif "BundleMatching" in metadata:
                    md = metadata["BundleMatching"]
                    metadata_str = md.get("solver_name", "")
                elif "PartialSolving" in metadata:  # Legacy support
                    md = metadata["PartialSolving"]
                    metadata_str = md.get("solver_name", "")

            records.append(
                {
                    "index": i,
                    "phase": phase_name,
                    "iteration": snap.get("iteration", 0),
                    "fills_count": snap.get("fills_count", 0),
                    "welfare": snap.get("welfare", 0),
                    "welfare_dollars": snap.get("welfare", 0) / 1e9,
                    "elapsed_secs": snap.get("elapsed_secs", 0.0),
                    "phase_fills": snap.get("phase_fills"),
                    "phase_welfare": snap.get("phase_welfare"),
                    "phase_welfare_dollars": (snap.get("phase_welfare") or 0) / 1e9,
                    "metadata": metadata_str,
                }
            )
        return pd.DataFrame(records)


def load_snapshot(path: str | Path) -> SnapshotData:
    """Load a snapshot from a JSON file."""
    path = Path(path)
    if not path.exists():
        raise FileNotFoundError(f"Snapshot file not found: {path}")

    with open(path) as f:
        data = json.load(f)

    return SnapshotData(raw=data)
