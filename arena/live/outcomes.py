"""Authoritative market outcome recording for live Arena experiments."""

from __future__ import annotations

import asyncio
import logging
import math
import os
import re
import sqlite3
from collections.abc import Collection
from datetime import datetime, timezone
from typing import Any, Callable

import httpx

from .sqlite_utils import connect_writer

log = logging.getLogger(__name__)

NANOS_PER_DOLLAR = 1_000_000_000
DEFAULT_OUTCOME_RECORD_INTERVAL_S = 15 * 60


class OutcomeConflictError(RuntimeError):
    """The DB already contains a different outcome for a market."""


class InvalidOutcomeResponseError(ValueError):
    """The authoritative resolution endpoint returned an invalid response."""


class _OutcomeRecordingCancelled(Exception):
    """Internal cooperative cancellation before an outcome write."""


def _raise_if_stopped(should_stop: Callable[[], bool] | None) -> None:
    if should_stop is not None and should_stop():
        raise _OutcomeRecordingCancelled


def _normalize_genesis_hash(value: Any, *, source: str) -> str:
    normalized = str(value or "").strip().lower()
    if not re.fullmatch(r"[0-9a-f]{64}", normalized) or set(normalized) == {"0"}:
        raise InvalidOutcomeResponseError(
            f"{source} returned invalid nonzero 32-byte genesis_hash={value!r}"
        )
    return normalized


def _fetch_genesis_hash(client: httpx.Client) -> str:
    response = client.get("/v1/health")
    response.raise_for_status()
    try:
        payload = response.json()
    except ValueError as exc:
        raise InvalidOutcomeResponseError("/v1/health returned non-JSON response") from exc
    if not isinstance(payload, dict):
        raise InvalidOutcomeResponseError("/v1/health returned a non-object response")
    return _normalize_genesis_hash(payload.get("genesis_hash"), source="/v1/health")


def _require_expected_genesis(client: httpx.Client, expected_genesis_hash: str) -> None:
    actual = _fetch_genesis_hash(client)
    if actual != expected_genesis_hash:
        raise InvalidOutcomeResponseError(
            "authoritative outcome recorder chain identity mismatch: "
            f"expected {expected_genesis_hash}, /v1/health returned {actual}"
        )


def _resolved_at_iso(market_id: int, resolved_at_ms: Any) -> str:
    if (
        not isinstance(resolved_at_ms, int)
        or isinstance(resolved_at_ms, bool)
        or resolved_at_ms < 0
    ):
        raise InvalidOutcomeResponseError(
            f"market {market_id} returned invalid resolved_at_ms={resolved_at_ms!r}"
        )
    try:
        return datetime.fromtimestamp(
            resolved_at_ms / 1000,
            tz=timezone.utc,
        ).isoformat()
    except (OverflowError, OSError, ValueError) as exc:
        raise InvalidOutcomeResponseError(
            f"market {market_id} returned invalid resolved_at_ms={resolved_at_ms!r}"
        ) from exc


def _fetch_resolution(
    client: httpx.Client,
    market_id: int,
    *,
    allow_missing: bool,
) -> tuple[float, str] | None:
    response = client.get(f"/v1/markets/{market_id}/resolution")
    if response.status_code == 404:
        if allow_missing:
            return None
        raise InvalidOutcomeResponseError(
            f"exact outcome cohort market {market_id} disappeared with HTTP 404"
        )
    response.raise_for_status()
    try:
        payload = response.json()
    except ValueError as exc:
        raise InvalidOutcomeResponseError(
            f"market {market_id} returned non-JSON resolution response"
        ) from exc
    if not isinstance(payload, dict):
        raise InvalidOutcomeResponseError(
            f"market {market_id} returned a non-object resolution response"
        )

    response_market_id = payload.get("market_id")
    if (
        not isinstance(response_market_id, int)
        or isinstance(response_market_id, bool)
        or response_market_id != market_id
    ):
        raise InvalidOutcomeResponseError(
            f"market {market_id} resolution response identified market {response_market_id!r}"
        )

    status = str(payload.get("status", "")).lower()
    payout_nanos = payload.get("payout_nanos")
    if status not in {"active", "proposed", "challenged", "resolved", "voided"}:
        raise InvalidOutcomeResponseError(
            f"market {market_id} returned invalid resolution status={status!r}"
        )
    if status != "resolved":
        if payout_nanos is not None:
            raise InvalidOutcomeResponseError(
                f"market {market_id} returned payout_nanos while status={status!r}"
            )
        return None
    if not isinstance(payout_nanos, int) or isinstance(payout_nanos, bool):
        raise InvalidOutcomeResponseError(
            f"market {market_id} returned invalid payout_nanos={payout_nanos!r}"
        )
    if not 0 <= payout_nanos <= NANOS_PER_DOLLAR:
        raise InvalidOutcomeResponseError(
            f"market {market_id} returned invalid payout_nanos={payout_nanos}"
        )
    return payout_nanos / NANOS_PER_DOLLAR, _resolved_at_iso(
        market_id,
        payload.get("resolved_at_ms"),
    )


def _normalize_market_ids(market_ids: Collection[int]) -> list[int]:
    normalized = list(market_ids)
    if any(
        not isinstance(market_id, int) or isinstance(market_id, bool) for market_id in normalized
    ):
        raise ValueError("market_ids must contain only integers")
    if any(market_id < 0 for market_id in normalized):
        raise ValueError("market_ids must contain only nonnegative ids")
    if len(set(normalized)) != len(normalized):
        raise ValueError("market_ids must not contain duplicates")
    return sorted(normalized)


def record_outcomes(
    db_path: str,
    api_base: str = "http://localhost:3000",
    *,
    market_ids: Collection[int] | None = None,
    expected_genesis_hash: str | None = None,
    should_stop: Callable[[], bool] | None = None,
    dry_run: bool = False,
    transport: httpx.BaseTransport | None = None,
) -> dict[str, int]:
    """Fetch and persist newly resolved outcomes; return operation counts.

    When ``market_ids`` is omitted, the manual CLI behavior derives its scope
    from distinct decision rows. Live experiments pass their immutable cohort
    explicitly, including markets that do not yet have a decision. An expected
    genesis pins automatic writes before fetch and immediately before commit;
    ``should_stop`` cooperatively abandons the sweep without committing.
    """
    exact_cohort = market_ids is not None
    normalized_expected_genesis = (
        _normalize_genesis_hash(expected_genesis_hash, source="configured experiment")
        if expected_genesis_hash is not None
        else None
    )
    _raise_if_stopped(should_stop)
    conn = connect_writer(db_path)
    try:
        if market_ids is None:
            decisions_exists = conn.execute(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='decisions'"
            ).fetchone()
            if decisions_exists is None:
                raise RuntimeError("decisions table does not exist")
            selected_market_ids = [
                int(row[0])
                for row in conn.execute(
                    "SELECT DISTINCT market_id FROM decisions WHERE market_id IS NOT NULL "
                    "ORDER BY market_id"
                )
            ]
        else:
            selected_market_ids = _normalize_market_ids(market_ids)
        selected_market_id_set = set(selected_market_ids)

        outcomes_exists = conn.execute(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='market_outcomes'"
        ).fetchone()
        existing = (
            {
                int(row[0]): float(row[1])
                for row in conn.execute("SELECT market_id, outcome FROM market_outcomes")
                if int(row[0]) in selected_market_id_set
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
            if normalized_expected_genesis is not None:
                _raise_if_stopped(should_stop)
                _require_expected_genesis(client, normalized_expected_genesis)
                _raise_if_stopped(should_stop)
            for market_id in selected_market_ids:
                _raise_if_stopped(should_stop)
                resolution = _fetch_resolution(
                    client,
                    market_id,
                    allow_missing=not exact_cohort,
                )
                _raise_if_stopped(should_stop)
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
        inserted = 0
        if not dry_run and new_rows:
            _raise_if_stopped(should_stop)
            if normalized_expected_genesis is not None:
                with httpx.Client(
                    base_url=api_base.rstrip("/"),
                    headers=headers,
                    timeout=10.0,
                    transport=transport,
                ) as client:
                    _require_expected_genesis(client, normalized_expected_genesis)
                _raise_if_stopped(should_stop)
            _raise_if_stopped(should_stop)
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
                _raise_if_stopped(should_stop)
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
                inserted += 1
            _raise_if_stopped(should_stop)
            conn.commit()

        return {
            "markets_seen": len(selected_market_ids),
            "resolved": len(fetched),
            "already_recorded": len(fetched) - (len(new_rows) if dry_run else inserted),
            "would_insert" if dry_run else "inserted": len(new_rows) if dry_run else inserted,
        }
    except Exception:
        conn.rollback()
        raise
    finally:
        conn.close()


async def record_outcomes_loop(
    db_path: str,
    api_base: str,
    market_ids: Collection[int],
    stop_event: asyncio.Event,
    *,
    expected_genesis_hash: str | None = None,
    interval_s: float = DEFAULT_OUTCOME_RECORD_INTERVAL_S,
    recorder: Callable[..., dict[str, int]] = record_outcomes,
) -> None:
    """Record immediately and periodically; disarm safely on integrity failure."""
    try:
        if not math.isfinite(interval_s) or interval_s <= 0:
            raise ValueError("outcome record interval must be a positive finite number")
        frozen_market_ids = tuple(_normalize_market_ids(market_ids))
    except Exception:
        log.critical(
            "Authoritative outcome recorder permanently disarmed after invalid setup; "
            "trading continues",
            exc_info=True,
        )
        await stop_event.wait()
        return

    while not stop_event.is_set():
        try:
            result = await asyncio.to_thread(
                recorder,
                db_path,
                api_base,
                market_ids=frozen_market_ids,
                expected_genesis_hash=expected_genesis_hash,
                should_stop=stop_event.is_set,
            )
            log.info("Authoritative outcome recorder: %s", result)
        except _OutcomeRecordingCancelled:
            return
        except (httpx.HTTPError, sqlite3.OperationalError) as exc:
            log.warning(
                "Authoritative outcome recorder transient failure; retrying next interval: %s",
                exc,
            )
        except Exception:
            log.critical(
                "Authoritative outcome recorder permanently disarmed after fatal failure; "
                "trading continues",
                exc_info=True,
            )
            await stop_event.wait()
            return

        try:
            await asyncio.wait_for(stop_event.wait(), timeout=interval_s)
        except TimeoutError:
            pass
