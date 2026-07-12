"""CLI wrapper for authoritative Arena market outcome recording."""

from __future__ import annotations

import argparse
from pathlib import Path

from live.outcomes import record_outcomes


def main() -> None:
    parser = argparse.ArgumentParser(description="Record resolved outcomes for arena decisions")
    parser.add_argument("--db", default="live/decisions.db", help="Path to decisions DB")
    parser.add_argument(
        "--api-base", default="http://localhost:3000", help="Local Sybil API base URL"
    )
    parser.add_argument(
        "--market-ids",
        nargs="+",
        type=int,
        default=None,
        help="Exact market cohort; default derives distinct IDs from decisions",
    )
    parser.add_argument(
        "--dry-run", action="store_true", help="Query and validate resolutions without DB writes"
    )
    args = parser.parse_args()

    result = record_outcomes(
        args.db,
        args.api_base,
        market_ids=args.market_ids,
        dry_run=args.dry_run,
    )
    action = "Dry run" if args.dry_run else "Recorded outcomes"
    print(f"{action} for {Path(args.db)}: {result}")


if __name__ == "__main__":
    main()
