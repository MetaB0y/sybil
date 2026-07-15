"""Startup ordering and initial portfolio-baseline coverage for the live runner."""

import asyncio
from types import SimpleNamespace
from unittest.mock import AsyncMock, MagicMock

import pytest

import live.runner as runner


def _trader(name: str, account_id: int, *, pnl: float = 0.0):
    trader = MagicMock()
    trader.name = name
    trader.account_id = account_id
    trader.positions = {(7, "YES"): 3, (8, "NO"): 0}
    trader._fill_history = [object()]
    trader.total_orders_submitted = 2
    trader.client.get_portfolio = AsyncMock(
        return_value=SimpleNamespace(
            balance_dollars=500.0,
            portfolio_value_dollars=500.0 + pnl,
            pnl_dollars=pnl,
        )
    )
    return trader


async def test_one_shot_snapshot_records_every_account_baseline():
    traders = [_trader("Control (Flat)", 11), _trader("Stage1 (Flat)", 12)]
    db = MagicMock()

    recorded = await runner.snapshot_portfolios_once(
        traders,
        db,
        required_trader_names={trader.name for trader in traders},
    )

    assert recorded == 2
    assert db.log_snapshot.call_count == 2
    assert [call.kwargs["trader_name"] for call in db.log_snapshot.call_args_list] == [
        "Control (Flat)",
        "Stage1 (Flat)",
    ]
    assert all(call.kwargs["positions"] == {"7": {"YES": 3}} for call in db.log_snapshot.call_args_list)
    assert all(call.kwargs["total_fills"] == 1 for call in db.log_snapshot.call_args_list)
    assert all(call.kwargs["total_orders"] == 2 for call in db.log_snapshot.call_args_list)


async def test_initial_baseline_is_awaited_before_any_worker_starts(monkeypatch):
    baseline_complete = False
    release_workers = asyncio.Event()
    worker_starts: list[str] = []
    trader = _trader("Control (Flat)", 11)
    fast = _trader("Fast-0", 12)
    noise = _trader("Noise-0", 13)

    async def baseline(snapshot_traders, _db, *, required_trader_names=None):
        nonlocal baseline_complete
        assert snapshot_traders == [trader, fast, noise]
        assert required_trader_names == {trader.name}
        baseline_complete = True
        return 1

    class Feed:
        reference_prices = MagicMock()

        async def run(self):
            assert baseline_complete
            worker_starts.append("feed")
            await release_workers.wait()

        def drain_all_new(self):
            return []

    class Analyst:
        name = "Control Analyst"

        async def run(self):
            assert baseline_complete
            worker_starts.append("analyst")
            await release_workers.wait()

    async def trader_run():
        assert baseline_complete
        worker_starts.append("trader")
        await release_workers.wait()

    trader.run = trader_run

    async def fast_run():
        assert baseline_complete
        worker_starts.append("fast")
        await release_workers.wait()

    async def noise_run():
        assert baseline_complete
        worker_starts.append("noise")
        await release_workers.wait()

    fast.run = fast_run
    noise.run = noise_run
    monkeypatch.setattr(runner, "snapshot_portfolios_once", baseline)

    stop_event = asyncio.Event()
    client = MagicMock()
    client.list_markets = AsyncMock(return_value=[])
    tasks = await runner._start_live_tasks(
        client,
        Feed(),
        [{}],
        [Analyst()],
        [trader],
        [fast],
        [noise],
        MagicMock(),
        stop_event,
        required_baseline_trader_names={trader.name},
    )
    try:
        await asyncio.sleep(0)
        assert baseline_complete is True
        assert set(worker_starts) == {"feed", "analyst", "trader", "fast", "noise"}
    finally:
        stop_event.set()
        release_workers.set()
        for task in tasks:
            task.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)


async def test_reference_refresh_replaces_shared_market_views_and_cache():
    old = SimpleNamespace(
        id=7,
        reference_price_nanos=400_000_000,
        reference_price_expires_at_ms=1_000,
    )
    fresh = SimpleNamespace(
        id=7,
        reference_price_nanos=450_000_000,
        reference_price_expires_at_ms=2_000,
    )
    client = MagicMock()
    client.list_markets = AsyncMock(return_value=[fresh])
    feed = SimpleNamespace(reference_prices=MagicMock())
    view = {7: old}
    stop_event = asyncio.Event()

    task = asyncio.create_task(
        runner._reference_price_refresh_loop(client, feed, [view], stop_event)
    )
    while client.list_markets.await_count == 0:
        await asyncio.sleep(0)
    stop_event.set()
    await task

    feed.reference_prices.replace.assert_called_once_with([fresh], {7})
    assert view[7] is fresh


async def test_reference_refresh_failure_clears_every_live_view():
    market = SimpleNamespace(
        id=7,
        reference_price_nanos=400_000_000,
        reference_price_expires_at_ms=1_000,
    )
    client = MagicMock()
    client.list_markets = AsyncMock(side_effect=RuntimeError("API unavailable"))
    feed = SimpleNamespace(reference_prices=MagicMock())
    stop_event = asyncio.Event()

    task = asyncio.create_task(
        runner._reference_price_refresh_loop(client, feed, [{7: market}], stop_event)
    )
    while client.list_markets.await_count == 0:
        await asyncio.sleep(0)
    stop_event.set()
    await task

    feed.reference_prices.clear.assert_called_once_with()
    assert market.reference_price_nanos is None
    assert market.reference_price_expires_at_ms is None


async def test_only_experiment_arm_baseline_failures_abort_startup():
    experiment = _trader("Experiment (Flat)", 11)
    synthetic = _trader("Noise-0", 12)
    experiment.client.get_portfolio.side_effect = RuntimeError("experiment unavailable")
    synthetic.client.get_portfolio.side_effect = RuntimeError("synthetic unavailable")

    # Default/synthetic accounts retain the periodic loop's fail-open behavior.
    assert await runner.snapshot_portfolios_once([synthetic], MagicMock()) == 0

    # An experiment without its time-zero baseline is not a valid PnL window.
    with pytest.raises(RuntimeError, match="window invalidated.*new experiment id"):
        await runner.snapshot_portfolios_once(
            [experiment, synthetic],
            MagicMock(),
            required_trader_names={experiment.name},
        )
