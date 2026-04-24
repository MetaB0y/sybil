"""Seed the composition demo markets."""

from __future__ import annotations

import argparse

from .store import DEFAULT_SYBIL_URL, seed_markets


def main() -> None:
    parser = argparse.ArgumentParser(description="Seed composition demo markets")
    parser.add_argument("--sybil-url", default=DEFAULT_SYBIL_URL)
    args = parser.parse_args()
    state = seed_markets(args.sybil_url)
    counts = state.get("instrument_counts", {})
    print(
        f"Seeded {len(state['instruments'])} instruments "
        f"({counts.get('atoms', 0)} atoms, {counts.get('compositions', 0)} compositions)"
    )
    for item in state["instruments"][:24]:
        print(f"  {item['id']:<24} market={item.get('market_id')} {item['short_name']}")
    remaining = len(state["instruments"]) - 24
    if remaining > 0:
        print(f"  ... {remaining} more instruments")


if __name__ == "__main__":
    main()
