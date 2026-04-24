"""Build the graph universe and attach source aliases for the demo."""

from __future__ import annotations

import argparse

from .store import import_sources


def main() -> None:
    parser = argparse.ArgumentParser(description="Build the measurement/condition graph and attach source aliases")
    parser.add_argument("--max-conditions", "--max-atoms", dest="max_atoms", type=int, default=110)
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()
    state = import_sources(force=args.force, max_atoms=args.max_atoms)
    counts = state.get("source_counts", {})
    print(
        "Built "
        f"{counts.get('measurements', 0)} measurements, "
        f"{counts.get('conditions', 0)} conditions, and "
        f"{counts.get('propositions', 0)} propositions "
        f"with {counts.get('source_aliases', 0)} source aliases "
        f"from {counts.get('polymarket_events', 0)} Polymarket events and "
        f"{counts.get('kalshi_markets', 0)} Kalshi markets "
        f"({counts.get('unmatched_sources', 0)} unmatched)"
    )
    for error in state.get("source_errors", []):
        print(f"source warning: {error}")


if __name__ == "__main__":
    main()
