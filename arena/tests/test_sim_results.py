"""Regression tests for simulation result evidence boundaries."""

from datetime import datetime
from types import SimpleNamespace

from sim.results import build_block_records
from sybil_client.types import PricePoint


def _price(height: int) -> PricePoint:
    return PricePoint(
        height=height,
        timestamp_ms=height * 1000,
        yes_price_nanos=500_000_000,
        no_price_nanos=500_000_000,
        volume_nanos=0,
    )


def test_block_records_use_strict_day_boundary_and_observed_sim_time():
    mm = SimpleNamespace(block_log=[(10, []), (11, [])])
    trader = SimpleNamespace(name="Trader", block_log=[], trade_log=[])
    observed = datetime(2026, 1, 2, 3, 4, 5)

    records = build_block_records(
        [mm, trader],
        mm,
        [],
        [trader],
        [_price(10), _price(11), _price(12)],
        sim_time_by_height={11: observed},
        after_block=10,
    )

    assert [record["height"] for record in records] == [11, 12]
    assert records[0]["sim_time"] == observed.isoformat()
    assert records[1]["sim_time"] is None
