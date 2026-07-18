"""Regression tests for simulation orchestration boundaries."""

import asyncio
import json
from datetime import datetime, timedelta, timezone

import pytest

from sim.clock import SimulatedClock
from sim.runner import _lookup_polymarket_price, _wait_until_or_task_failure


def test_polymarket_price_lookup_never_looks_into_the_future(tmp_path):
    prices = tmp_path / "prices.json"
    prices.write_text(
        json.dumps(
            [
                {
                    "timestamp": "2026-01-26T00:00:36+00:00",
                    "yes_price": 0.585,
                },
                {
                    "timestamp": "2026-01-26T00:05:00+00:00",
                    "yes_price": 0.6,
                },
            ]
        )
    )

    midnight = datetime(2026, 1, 26, tzinfo=timezone.utc)
    assert _lookup_polymarket_price(prices, midnight) is None
    assert (
        _lookup_polymarket_price(prices, midnight + timedelta(minutes=3))
        == 0.585
    )


def test_polymarket_price_lookup_rejects_invalid_history(tmp_path, caplog):
    prices = tmp_path / "prices.json"
    prices.write_text('[{"timestamp": "2026-01-26T00:00:00Z", "yes_price": 2}]')

    assert (
        _lookup_polymarket_price(
            prices,
            datetime(2026, 1, 26, tzinfo=timezone.utc),
        )
        is None
    )
    assert "invalid YES price" in caplog.text


@pytest.mark.anyio
async def test_simulation_supervision_surfaces_background_task_failure():
    clock = SimulatedClock(datetime(2026, 1, 1), compression_ratio=100.0)

    async def fail():
        await asyncio.sleep(0)
        raise ValueError("broken bot")

    task = asyncio.create_task(fail(), name="bot:broken")
    with pytest.raises(RuntimeError, match="bot:broken"):
        await _wait_until_or_task_failure(
            clock,
            clock.sim_start + timedelta(hours=1),
            [task],
        )
    assert isinstance(task.exception(), ValueError)


@pytest.mark.anyio
async def test_simulation_supervision_allows_expected_early_task_exit():
    clock = SimulatedClock(datetime(2026, 1, 1), compression_ratio=100.0)

    async def finish():
        return None

    task = asyncio.create_task(finish(), name="bot:bounded-mm")
    await _wait_until_or_task_failure(
        clock,
        clock.sim_start + timedelta(seconds=1),
        [task],
    )
    assert task.done()
