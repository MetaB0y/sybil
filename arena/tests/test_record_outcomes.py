import asyncio
import logging
import sqlite3
import threading
from concurrent.futures import ThreadPoolExecutor

import httpx
import pytest

from live.outcomes import (
    InvalidOutcomeResponseError,
    OutcomeConflictError,
    record_outcomes,
    record_outcomes_loop,
)
from live.db import DecisionDB
from live.runner import (
    LiveConfig,
    _resolve_outcome_record_interval,
    _start_outcome_recorder_task,
)
from live.sqlite_utils import BUSY_TIMEOUT_MS, connect_writer

GENESIS_A = "a" * 64
GENESIS_B = "b" * 64


def _decisions_db(path):
    conn = sqlite3.connect(path)
    conn.executescript("""
        CREATE TABLE decisions (market_id INTEGER);
        INSERT INTO decisions (market_id) VALUES (1), (1), (2);
    """)
    conn.commit()
    conn.close()


def test_outcome_record_interval_cli_precedes_env_and_env_has_safe_default():
    assert _resolve_outcome_record_interval(None, {}) == 900
    assert (
        _resolve_outcome_record_interval(
            None,
            {"ARENA_OUTCOME_RECORD_INTERVAL_S": "120"},
        )
        == 120
    )
    assert (
        _resolve_outcome_record_interval(
            30,
            {"ARENA_OUTCOME_RECORD_INTERVAL_S": "malformed"},
        )
        == 30
    )
    assert (
        _resolve_outcome_record_interval(
            None,
            {"ARENA_OUTCOME_RECORD_INTERVAL_S": "dormant-malformed"},
            experiment_active=False,
        )
        == 900
    )
    with pytest.raises(ValueError, match="requires an active Stage 1 A/B experiment"):
        _resolve_outcome_record_interval(30, {}, experiment_active=False)


@pytest.mark.parametrize("raw", ["0", "-1", "nan", "inf", "malformed"])
def test_outcome_record_interval_rejects_nonpositive_or_malformed_env(raw):
    with pytest.raises(ValueError, match="outcome record interval"):
        _resolve_outcome_record_interval(
            None,
            {"ARENA_OUTCOME_RECORD_INTERVAL_S": raw},
        )


def test_record_outcomes_is_idempotent_and_uses_live_wal_writer(tmp_path, monkeypatch):
    db_path = tmp_path / "decisions.db"
    _decisions_db(db_path)
    writer_settings = []

    def tracked_writer(path):
        conn = connect_writer(path)
        writer_settings.append(
            (
                conn.execute("PRAGMA journal_mode").fetchone()[0],
                conn.execute("PRAGMA busy_timeout").fetchone()[0],
            )
        )
        return conn

    monkeypatch.setattr("live.outcomes.connect_writer", tracked_writer)

    def handler(request: httpx.Request) -> httpx.Response:
        market_id = int(request.url.path.split("/")[-2])
        if market_id == 1:
            return httpx.Response(
                200,
                json={
                    "market_id": 1,
                    "status": "resolved",
                    "payout_nanos": "1000000000",
                    "resolved_at_ms": 1_767_225_600_000,
                },
            )
        return httpx.Response(200, json={"market_id": 2, "status": "active", "payout_nanos": None})

    transport = httpx.MockTransport(handler)
    dry = record_outcomes(str(db_path), dry_run=True, transport=transport)
    assert dry["would_insert"] == 1
    conn = sqlite3.connect(db_path)
    assert (
        conn.execute(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='market_outcomes'"
        ).fetchone()
        is None
    )
    conn.close()

    first = record_outcomes(str(db_path), transport=transport)
    second = record_outcomes(str(db_path), transport=transport)
    assert writer_settings == [("wal", BUSY_TIMEOUT_MS)] * 3
    assert first["inserted"] == 1
    assert second == {
        "markets_seen": 2,
        "resolved": 1,
        "already_recorded": 1,
        "inserted": 0,
    }
    conn = sqlite3.connect(db_path)
    assert conn.execute(
        "SELECT market_id, outcome, resolved_at FROM market_outcomes"
    ).fetchone() == (1, 1.0, "2026-01-01T00:00:00+00:00")
    conn.close()


def test_record_outcomes_exact_cohort_does_not_depend_on_decision_rows(tmp_path):
    db_path = tmp_path / "outcomes.db"
    requested = []

    def handler(request: httpx.Request) -> httpx.Response:
        market_id = int(request.url.path.split("/")[-2])
        requested.append(market_id)
        return httpx.Response(
            200,
            json={
                "market_id": market_id,
                "status": "resolved",
                "payout_nanos": 250_000_000,
                "resolved_at_ms": 1_767_225_600_000,
            },
        )

    result = record_outcomes(
        str(db_path),
        market_ids=[11],
        transport=httpx.MockTransport(handler),
    )
    assert result == {
        "markets_seen": 1,
        "resolved": 1,
        "already_recorded": 0,
        "inserted": 1,
    }
    assert requested == [11]
    conn = sqlite3.connect(db_path)
    assert conn.execute("SELECT market_id, outcome FROM market_outcomes").fetchone() == (11, 0.25)
    conn.close()


def test_record_outcomes_rejects_genesis_mismatch_before_fetch(tmp_path):
    db_path = tmp_path / "outcomes.db"
    requested = []

    def handler(request: httpx.Request) -> httpx.Response:
        requested.append(request.url.path)
        return httpx.Response(200, json={"genesis_hash": GENESIS_B})

    with pytest.raises(InvalidOutcomeResponseError, match="chain identity mismatch"):
        record_outcomes(
            str(db_path),
            market_ids=[1],
            expected_genesis_hash=GENESIS_A,
            transport=httpx.MockTransport(handler),
        )
    assert requested == ["/v1/health"]
    conn = sqlite3.connect(db_path)
    assert conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='market_outcomes'"
    ).fetchone() is None
    conn.close()


def test_record_outcomes_rechecks_genesis_before_write(tmp_path):
    db_path = tmp_path / "outcomes.db"
    health_checks = 0

    def handler(request: httpx.Request) -> httpx.Response:
        nonlocal health_checks
        if request.url.path == "/v1/health":
            health_checks += 1
            return httpx.Response(
                200,
                json={"genesis_hash": GENESIS_A if health_checks == 1 else GENESIS_B},
            )
        return httpx.Response(
            200,
            json={
                "market_id": 1,
                "status": "resolved",
                "payout_nanos": 1_000_000_000,
                "resolved_at_ms": 1_767_225_600_000,
            },
        )

    with pytest.raises(InvalidOutcomeResponseError, match="chain identity mismatch"):
        record_outcomes(
            str(db_path),
            market_ids=[1],
            expected_genesis_hash=GENESIS_A,
            transport=httpx.MockTransport(handler),
        )
    assert health_checks == 2
    conn = sqlite3.connect(db_path)
    assert conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='market_outcomes'"
    ).fetchone() is None
    conn.close()


def test_exact_cohort_404_is_fatal_but_decisions_derived_mode_keeps_compatibility(tmp_path):
    exact_db = tmp_path / "exact.db"
    transport = httpx.MockTransport(lambda _request: httpx.Response(404))
    with pytest.raises(InvalidOutcomeResponseError, match="exact outcome cohort market 1"):
        record_outcomes(str(exact_db), market_ids=[1], transport=transport)

    manual_db = tmp_path / "manual.db"
    _decisions_db(manual_db)
    assert record_outcomes(str(manual_db), transport=transport) == {
        "markets_seen": 2,
        "resolved": 0,
        "already_recorded": 0,
        "inserted": 0,
    }


def test_record_outcomes_shares_wal_with_live_decision_writer(tmp_path):
    db_path = tmp_path / "shared.db"
    live_db = DecisionDB(str(db_path))
    fetch_started = threading.Event()
    release_fetch = threading.Event()

    def handler(request: httpx.Request) -> httpx.Response:
        fetch_started.set()
        assert release_fetch.wait(timeout=2), "test did not release paused outcome fetch"
        return httpx.Response(
            200,
            json={
                "market_id": 1,
                "status": "resolved",
                "payout_nanos": 750_000_000,
                "resolved_at_ms": 1_767_225_600_000,
            },
        )

    try:
        with ThreadPoolExecutor(max_workers=1) as executor:
            future = executor.submit(
                record_outcomes,
                str(db_path),
                market_ids=[1],
                transport=httpx.MockTransport(handler),
            )
            assert fetch_started.wait(timeout=2), "outcome recorder never reached HTTP fetch"

            snapshot_id = live_db.log_snapshot(
                "Concurrent trader",
                balance=100.0,
                portfolio_value=101.0,
                pnl=1.0,
                positions={"1": {"yes": 1}},
            )
            assert snapshot_id > 0

            release_fetch.set()
            assert future.result(timeout=2)["inserted"] == 1

        snapshot = live_db.conn.execute(
            "SELECT trader_name, pnl FROM portfolio_snapshots WHERE id = ?",
            (snapshot_id,),
        ).fetchone()
        outcome = live_db.conn.execute(
            "SELECT market_id, outcome FROM market_outcomes WHERE market_id = 1"
        ).fetchone()
        assert tuple(snapshot) == ("Concurrent trader", 1.0)
        assert tuple(outcome) == (1, 0.75)
    finally:
        release_fetch.set()
        live_db.close()


def test_record_outcomes_reports_same_value_concurrent_insert_truthfully(tmp_path):
    db_path = tmp_path / "race.db"
    fetch_started = threading.Event()
    release_fetch = threading.Event()

    def handler(_request: httpx.Request) -> httpx.Response:
        fetch_started.set()
        assert release_fetch.wait(timeout=2), "test did not release paused outcome fetch"
        return httpx.Response(
            200,
            json={
                "market_id": 1,
                "status": "resolved",
                "payout_nanos": 500_000_000,
                "resolved_at_ms": 1_767_225_600_000,
            },
        )

    try:
        with ThreadPoolExecutor(max_workers=1) as executor:
            future = executor.submit(
                record_outcomes,
                str(db_path),
                market_ids=[1],
                transport=httpx.MockTransport(handler),
            )
            assert fetch_started.wait(timeout=2)
            concurrent = connect_writer(str(db_path))
            concurrent.execute(
                "CREATE TABLE market_outcomes "
                "(market_id INTEGER PRIMARY KEY, outcome REAL, resolved_at TEXT)"
            )
            concurrent.execute(
                "INSERT INTO market_outcomes VALUES (1, 0.5, '2026-01-01T00:00:00Z')"
            )
            concurrent.commit()
            concurrent.close()
            release_fetch.set()
            result = future.result(timeout=2)
        assert result == {
            "markets_seen": 1,
            "resolved": 1,
            "already_recorded": 1,
            "inserted": 0,
        }
    finally:
        release_fetch.set()


def test_record_outcomes_raises_on_conflicting_existing_outcome(tmp_path):
    db_path = tmp_path / "decisions.db"
    _decisions_db(db_path)
    conn = sqlite3.connect(db_path)
    conn.execute(
        "CREATE TABLE market_outcomes "
        "(market_id INTEGER PRIMARY KEY, outcome REAL, resolved_at TEXT)"
    )
    conn.execute("INSERT INTO market_outcomes VALUES (1, 0.0, '2026-01-01T00:00:00Z')")
    conn.commit()
    conn.close()

    def handler(request: httpx.Request) -> httpx.Response:
        market_id = int(request.url.path.split("/")[-2])
        if market_id == 1:
            return httpx.Response(
                200,
                json={
                    "market_id": 1,
                    "status": "resolved",
                    "payout_nanos": 1_000_000_000,
                    "resolved_at_ms": 1_767_225_600_000,
                },
            )
        return httpx.Response(404)

    with pytest.raises(OutcomeConflictError, match="outcome conflict"):
        record_outcomes(str(db_path), transport=httpx.MockTransport(handler))

    conn = sqlite3.connect(db_path)
    assert (
        conn.execute("SELECT outcome FROM market_outcomes WHERE market_id = 1").fetchone()[0] == 0
    )
    conn.close()


def test_record_outcomes_rejects_invalid_authoritative_response(tmp_path):
    db_path = tmp_path / "decisions.db"
    _decisions_db(db_path)

    def handler(request: httpx.Request) -> httpx.Response:
        market_id = int(request.url.path.split("/")[-2])
        return httpx.Response(
            200,
            json={
                "market_id": market_id,
                "status": "resolved",
                "payout_nanos": 1.5,
                "resolved_at_ms": 1_767_225_600_000,
            },
        )

    with pytest.raises(InvalidOutcomeResponseError, match="invalid payout_nanos"):
        record_outcomes(str(db_path), transport=httpx.MockTransport(handler))


def test_record_outcomes_rejects_boolean_market_id_alias(tmp_path):
    db_path = tmp_path / "outcomes.db"

    def handler(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(
            200,
            json={
                "market_id": True,
                "status": "resolved",
                "payout_nanos": 1_000_000_000,
                "resolved_at_ms": 1_767_225_600_000,
            },
        )

    with pytest.raises(InvalidOutcomeResponseError, match="identified market True"):
        record_outcomes(
            str(db_path),
            market_ids=[1],
            transport=httpx.MockTransport(handler),
        )


async def test_outcome_recorder_loop_runs_immediately_and_stops_promptly():
    stop_event = asyncio.Event()
    called = asyncio.Event()
    loop = asyncio.get_running_loop()
    calls = []

    def recorder(
        db_path,
        api_base,
        *,
        market_ids,
        expected_genesis_hash,
        should_stop,
    ):
        calls.append((db_path, api_base, market_ids, expected_genesis_hash))
        assert not should_stop()
        loop.call_soon_threadsafe(called.set)
        return {"inserted": 0}

    task = asyncio.create_task(
        record_outcomes_loop(
            "test.db",
            "http://sybil.test",
            [11, 7],
            stop_event,
            interval_s=60,
            recorder=recorder,
        )
    )
    await asyncio.wait_for(called.wait(), timeout=1)
    assert calls == [("test.db", "http://sybil.test", (7, 11), None)]
    stop_event.set()
    await asyncio.wait_for(task, timeout=1)


async def test_outcome_recorder_stop_during_fetch_skips_remaining_cohort_and_write(tmp_path):
    db_path = tmp_path / "cancelled.db"
    stop_event = asyncio.Event()
    first_fetch_started = threading.Event()
    release_first_fetch = threading.Event()
    requested = []

    def handler(request: httpx.Request) -> httpx.Response:
        market_id = int(request.url.path.split("/")[-2])
        requested.append(market_id)
        if market_id == 1:
            first_fetch_started.set()
            assert release_first_fetch.wait(timeout=2)
        return httpx.Response(
            200,
            json={
                "market_id": market_id,
                "status": "resolved",
                "payout_nanos": 1_000_000_000,
                "resolved_at_ms": 1_767_225_600_000,
            },
        )

    transport = httpx.MockTransport(handler)

    def recorder(db_path, api_base, **kwargs):
        return record_outcomes(
            db_path,
            api_base,
            transport=transport,
            **kwargs,
        )

    task = asyncio.create_task(
        record_outcomes_loop(
            str(db_path),
            "http://sybil.test",
            [1, 2],
            stop_event,
            interval_s=60,
            recorder=recorder,
        )
    )
    assert await asyncio.to_thread(first_fetch_started.wait, 2)
    stop_event.set()
    release_first_fetch.set()
    await asyncio.wait_for(task, timeout=1)
    assert requested == [1]

    conn = sqlite3.connect(db_path)
    assert conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='market_outcomes'"
    ).fetchone() is None
    conn.close()


@pytest.mark.parametrize(
    "error",
    [
        httpx.ConnectError("offline"),
        sqlite3.OperationalError("database is locked"),
    ],
)
async def test_outcome_recorder_loop_retries_transient_failures(error, caplog):
    stop_event = asyncio.Event()
    succeeded = asyncio.Event()
    loop = asyncio.get_running_loop()
    attempts = 0

    def recorder(
        _db_path,
        _api_base,
        *,
        market_ids,
        expected_genesis_hash,
        should_stop,
    ):
        nonlocal attempts
        attempts += 1
        assert market_ids == (7,)
        assert expected_genesis_hash is None
        assert not should_stop()
        if attempts == 1:
            raise error
        loop.call_soon_threadsafe(succeeded.set)
        return {"inserted": 0}

    with caplog.at_level(logging.WARNING, logger="live.outcomes"):
        task = asyncio.create_task(
            record_outcomes_loop(
                "test.db",
                "http://sybil.test",
                [7],
                stop_event,
                interval_s=0.01,
                recorder=recorder,
            )
        )
        await asyncio.wait_for(succeeded.wait(), timeout=1)
        stop_event.set()
        await asyncio.wait_for(task, timeout=1)

    assert attempts == 2
    assert "transient failure; retrying next interval" in caplog.text


@pytest.mark.parametrize(
    "error",
    [
        OutcomeConflictError("conflict"),
        InvalidOutcomeResponseError("invalid payout"),
        RuntimeError("programmer bug"),
    ],
)
async def test_outcome_recorder_fatal_failure_disarms_without_exiting(error, caplog):
    stop_event = asyncio.Event()
    attempted = asyncio.Event()
    loop = asyncio.get_running_loop()
    attempts = 0

    def recorder(
        _db_path,
        _api_base,
        *,
        market_ids,
        expected_genesis_hash,
        should_stop,
    ):
        nonlocal attempts
        attempts += 1
        assert expected_genesis_hash is None
        assert not should_stop()
        loop.call_soon_threadsafe(attempted.set)
        raise error

    with caplog.at_level(logging.CRITICAL, logger="live.outcomes"):
        task = asyncio.create_task(
            record_outcomes_loop(
                "test.db",
                "http://sybil.test",
                [7],
                stop_event,
                interval_s=0.01,
                recorder=recorder,
            )
        )
        await asyncio.wait_for(attempted.wait(), timeout=1)
        await asyncio.sleep(0.03)
        assert not task.done()
        assert attempts == 1
        assert "permanently disarmed" in caplog.text
        stop_event.set()
        await asyncio.wait_for(task, timeout=1)


async def test_outcome_recorder_task_is_default_off_and_experiment_scoped(monkeypatch):
    stop_event = asyncio.Event()
    assert _start_outcome_recorder_task(LiveConfig(), "test.db", stop_event) is None

    started = asyncio.Event()
    captured = {}

    async def fake_loop(
        db_path,
        api_base,
        market_ids,
        event,
        *,
        expected_genesis_hash,
        interval_s,
    ):
        captured.update(
            db_path=db_path,
            api_base=api_base,
            market_ids=market_ids,
            expected_genesis_hash=expected_genesis_hash,
            interval_s=interval_s,
        )
        started.set()
        await event.wait()

    monkeypatch.setattr("live.runner.record_outcomes_loop", fake_loop)
    config = LiveConfig(
        sybil_url="http://sybil.test",
        stage1_ab_experiment_id="stage1-july",
        market_ids=[7, 11],
        outcome_record_interval_s=123,
    )
    task = _start_outcome_recorder_task(
        config,
        "experiment.db",
        stop_event,
        GENESIS_A,
    )
    assert task is not None and task.get_name() == "outcome_recorder"
    await asyncio.wait_for(started.wait(), timeout=1)
    assert captured == {
        "db_path": "experiment.db",
        "api_base": "http://sybil.test",
        "market_ids": (7, 11),
        "expected_genesis_hash": GENESIS_A,
        "interval_s": 123,
    }
    stop_event.set()
    await asyncio.wait_for(task, timeout=1)
