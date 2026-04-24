"""Run the demo reference market maker loop."""

from __future__ import annotations

import argparse
import time

from .store import DEFAULT_SYBIL_URL, quote_once


def main() -> None:
    parser = argparse.ArgumentParser(description="Composition demo MM loop")
    parser.add_argument("--sybil-url", default=DEFAULT_SYBIL_URL)
    parser.add_argument("--interval", type=float, default=2.0)
    args = parser.parse_args()
    while True:
        result = quote_once(args.sybil_url)
        print(f"quoted {result['orders']} orders with mm account {result.get('mm_account_id')}")
        time.sleep(args.interval)


if __name__ == "__main__":
    main()

