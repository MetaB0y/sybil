"""Record authoritative Sybil market resolutions in an arena decisions DB.

Only market IDs already present in ``decisions`` are queried. The local Sybil
API's resolution endpoint is the source of truth; unresolved markets are left
alone. Existing outcomes are immutable: seeing a different payout is an error.
"""

from __future__ import annotations

import argparse
import os
import sqlite3
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import httpx

NANOS_PER_DOLLAR = 1_000_000_000


class OutcomeConflictError(RuntimeError):
    """The DB already contains a different outcome for a market."""


def _resolved_at_iso(resolved_at_ms: Any) -> str:
    if resolved_at_ms is None:
        raise ValueError("resolved market response omitted resolved_at_ms")
    return datetime.fromtimestamp(int(resolved_at_ms) / 1000, tz=timezone.utc).isoformat()


def _fetch_resolution(client: httpx.Client, market_id: int) -> tuple[float, str] | None:
    response = client.get(f"/v1/markets/{market_id}/resolution")
    if response.status_code == 404:
        return None
    response.raise_for_status()
    payload = response.json()
    payout_nanos = payload.get("payout_nanos")
    if payout_nanos is None or str(payload.get("status", "")).lower() != "resolved":
        return None
    payout_nanos = int(payout_nanos)
    if not 0 <= payout_nanos <= NANOS_PER_DOLLAR:
        raise ValueError(f"market {market_id} returned invalid payout_nanos={payout_nanos}")
    return payout_nanos / NANOS_PER_DOLLAR, _resolved_at_iso(payload.get("resolved_at_ms"))


def record_outcomes(
    db_path: str,
    api_base: str = "http://localhost:3000",
    *,
    dry_run: bool = False,
    transport: httpx.BaseTransport | None = None,
) -> dict[str, int]:
    """Fetch and persist newly resolved outcomes; return operation counts."""
    conn = sqlite3.connect(db_path)
    try:
        decisions_exists = conn.execute(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='decisions'"
        ).fetchone()
        if decisions_exists is None:
            raise RuntimeError("decisions table does not exist")
        market_ids = [
            int(row[0])
            for row in conn.execute(
                "SELECT DISTINCT market_id FROM decisions WHERE market_id IS NOT NULL "
                "ORDER BY market_id"
            )
        ]
        outcomes_exists = conn.execute(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='market_outcomes'"
        ).fetchone()
        existing = (
            {
                int(row[0]): float(row[1])
                for row in conn.execute("SELECT market_id, outcome FROM market_outcomes")
            }
            if outcomes_exists
            else {}
        )

        headers = {}
        if token := os.environ.get("SYBIL_SERVICE_TOKEN"):
            headers["authorization"] = f"Bearer {token}"
        fetched: dict[int, tuple[float, str]] = {}
        with httpx.Client(
            base_url=api_base.rstrip("/"),
            headers=headers,
            timeout=10.0,
            transport=transport,
        ) as client:
            for market_id in market_ids:
                resolution = _fetch_resolution(client, market_id)
                if resolution is not None:
                    fetched[market_id] = resolution

        for market_id, (outcome, _resolved_at) in fetched.items():
            old = existing.get(market_id)
            if old is not None and abs(old - outcome) > 1e-12:
                raise OutcomeConflictError(
                    f"market {market_id} outcome conflict: DB has {old}, API returned {outcome}"
                )

        new_rows = {
            market_id: resolution
            for market_id, resolution in fetched.items()
            if market_id not in existing
        }
        if not dry_run and new_rows:
            conn.execute("BEGIN IMMEDIATE")
            conn.execute(
                """CREATE TABLE IF NOT EXISTS market_outcomes (
                       market_id INTEGER PRIMARY KEY,
                       outcome REAL NOT NULL CHECK (outcome >= 0 AND outcome <= 1),
                       resolved_at TEXT NOT NULL
                   )"""
            )
            outcome_columns = {row[1] for row in conn.execute("PRAGMA table_info(market_outcomes)")}
            if "resolved_at" not in outcome_columns:
                conn.execute("ALTER TABLE market_outcomes ADD COLUMN resolved_at TEXT")
            for market_id, (outcome, resolved_at) in new_rows.items():
                current = conn.execute(
                    "SELECT outcome FROM market_outcomes WHERE market_id = ?", (market_id,)
                ).fetchone()
                if current is not None:
                    if abs(float(current[0]) - outcome) > 1e-12:
                        raise OutcomeConflictError(
                            f"market {market_id} outcome changed during write: "
                            f"DB has {current[0]}, API returned {outcome}"
                        )
                    continue
                conn.execute(
                    "INSERT INTO market_outcomes (market_id, outcome, resolved_at) VALUES (?, ?, ?)",
                    (market_id, outcome, resolved_at),
                )
            conn.commit()

        return {
            "markets_seen": len(market_ids),
            "resolved": len(fetched),
            "already_recorded": len(fetched) - len(new_rows),
            "would_insert" if dry_run else "inserted": len(new_rows),
        }
    except Exception:
        conn.rollback()
        raise
    finally:
        conn.close()


def main() -> None:
    parser = argparse.ArgumentParser(description="Record resolved outcomes for arena decisions")
    parser.add_argument("--db", default="live/decisions.db", help="Path to decisions DB")
    parser.add_argument(
        "--api-base", default="http://localhost:3000", help="Local Sybil API base URL"
    )
    parser.add_argument(
        "--dry-run", action="store_true", help="Query and validate resolutions without DB writes"
    )
    args = parser.parse_args()

    result = record_outcomes(args.db, args.api_base, dry_run=args.dry_run)
    action = "Dry run" if args.dry_run else "Recorded outcomes"
    print(f"{action} for {Path(args.db)}: {result}")


if __name__ == "__main__":
    main()
