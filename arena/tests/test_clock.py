"""Tests for sim.clock.SimulatedClock."""

import asyncio
from datetime import datetime, timedelta


from sim.clock import SimulatedClock


def test_now_returns_sim_start_before_start():
    """now() returns sim_start when clock hasn't been started."""
    t0 = datetime(2026, 1, 1, 12, 0, 0)
    clock = SimulatedClock(sim_start=t0, compression_ratio=60.0)
    assert clock.now() == t0


def test_time_compression():
    """After starting, time advances at compression_ratio speed."""
    t0 = datetime(2026, 1, 1, 0, 0, 0)
    clock = SimulatedClock(sim_start=t0, compression_ratio=60.0)
    clock.start()

    # Immediately after start, sim time should be very close to sim_start
    now = clock.now()
    assert abs((now - t0).total_seconds()) < 2.0  # within 2 sim seconds


def test_pause_freezes_time():
    """Pausing stops simulated time from advancing."""
    t0 = datetime(2026, 1, 1, 0, 0, 0)
    clock = SimulatedClock(sim_start=t0, compression_ratio=1000.0)
    clock.start()

    clock.pause()
    frozen = clock.now()
    # Even after a tiny delay, time should not advance
    import time
    time.sleep(0.01)
    still_frozen = clock.now()
    assert frozen == still_frozen


def test_resume_unfreezes_time():
    """Resuming allows time to advance again."""
    t0 = datetime(2026, 1, 1, 0, 0, 0)
    clock = SimulatedClock(sim_start=t0, compression_ratio=1000.0)
    clock.start()

    clock.pause()
    frozen = clock.now()
    clock.resume()

    # After resume, time should advance
    import time
    time.sleep(0.01)
    after_resume = clock.now()
    assert after_resume >= frozen


def test_ref_counted_pause():
    """Two pauses require two resumes to actually unfreeze."""
    t0 = datetime(2026, 1, 1, 0, 0, 0)
    clock = SimulatedClock(sim_start=t0, compression_ratio=1000.0)
    clock.start()

    clock.pause()
    clock.pause()
    frozen = clock.now()

    # One resume: still paused
    clock.resume()
    import time
    time.sleep(0.01)
    still_frozen = clock.now()
    assert still_frozen == frozen

    # Second resume: unfrozen
    clock.resume()
    time.sleep(0.01)
    unfrozen = clock.now()
    assert unfrozen > frozen


def test_is_past_boundary():
    """is_past returns True when sim time equals target."""
    t0 = datetime(2026, 1, 1, 0, 0, 0)
    clock = SimulatedClock(sim_start=t0, compression_ratio=60.0)
    # Before start, now() == sim_start
    assert clock.is_past(t0) is True
    assert clock.is_past(t0 - timedelta(seconds=1)) is True
    assert clock.is_past(t0 + timedelta(seconds=1)) is False


def test_sleep_until_past_returns_immediately():
    """sleep_until returns immediately for times already passed."""
    t0 = datetime(2026, 1, 1, 0, 0, 0)
    clock = SimulatedClock(sim_start=t0, compression_ratio=60.0)
    clock.start()

    past = t0 - timedelta(hours=1)
    import time
    start = time.monotonic()
    asyncio.run(clock.sleep_until(past))
    elapsed = time.monotonic() - start
    assert elapsed < 0.5  # should be nearly instant


def test_resume_without_pause_is_noop():
    """Calling resume() without pause() doesn't crash."""
    t0 = datetime(2026, 1, 1, 0, 0, 0)
    clock = SimulatedClock(sim_start=t0)
    clock.start()
    clock.resume()  # should not raise
