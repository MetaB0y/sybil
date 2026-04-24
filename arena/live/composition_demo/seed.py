"""Seed the Iran composition demo markets."""

from __future__ import annotations

import argparse

from .store import DEFAULT_SYBIL_URL, seed_markets


def main() -> None:
    parser = argparse.ArgumentParser(description="Seed composition demo markets")
    parser.add_argument("--sybil-url", default=DEFAULT_SYBIL_URL)
    args = parser.parse_args()
    state = seed_markets(args.sybil_url)
    print(f"Seeded {len(state['instruments'])} instruments")
    for item in state["instruments"]:
        print(f"  {item['id']:<24} market={item.get('market_id')} {item['short_name']}")


if __name__ == "__main__":
    main()

