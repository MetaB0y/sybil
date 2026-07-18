"""Simulated clock for time-compressed backtesting."""

import asyncio
import math
import time
from dataclasses import dataclass, field
from datetime import datetime, timedelta


@dataclass
class SimulatedClock:
    """A clock that compresses simulated time relative to real time.

    With compression_ratio=60:
    - 1 real second = 1 simulated minute
    - A 3-hour game spans 3 real minutes

    Attributes:
        sim_start: When simulated time begins (e.g., first event time)
        compression_ratio: How much faster simulated time runs (default: 60)
        real_start: Monotonic clock value when the backtest began
    """

    sim_start: datetime
    compression_ratio: float = 60.0
    real_start: float | None = field(default=None, init=False)
    _pause_count: int = field(default=0, init=False)
    _pause_time: float | None = field(default=None, init=False)

    def __post_init__(self) -> None:
        if not math.isfinite(self.compression_ratio) or self.compression_ratio <= 0:
            raise ValueError("compression_ratio must be finite and greater than zero")

    def start(self) -> None:
        """Start the clock. Call this when the backtest begins."""
        if self.real_start is None:
            self.real_start = time.monotonic()

    def now(self) -> datetime:
        """Get the current simulated time."""
        if self.real_start is None:
            return self.sim_start

        if self._pause_count > 0 and self._pause_time:
            real_elapsed = self._pause_time - self.real_start
        else:
            real_elapsed = time.monotonic() - self.real_start

        sim_elapsed = real_elapsed * self.compression_ratio
        return self.sim_start + timedelta(seconds=sim_elapsed)

    def elapsed_sim_time(self) -> timedelta:
        """Get elapsed simulated time since start."""
        return self.now() - self.sim_start

    def elapsed_real_time(self) -> timedelta:
        """Get elapsed real time since start."""
        if self.real_start is None:
            return timedelta(0)
        end = self._pause_time if self._pause_count > 0 else time.monotonic()
        return timedelta(seconds=end - self.real_start)

    def sim_to_real_seconds(self, sim_seconds: float) -> float:
        """Convert simulated seconds to real seconds."""
        return sim_seconds / self.compression_ratio

    def real_to_sim_seconds(self, real_seconds: float) -> float:
        """Convert real seconds to simulated seconds."""
        return real_seconds * self.compression_ratio

    async def sleep_until(self, sim_time: datetime) -> None:
        """Sleep until the simulated time is reached.

        Loops to account for clock pauses that may occur during the sleep.
        """
        if self.real_start is None:
            self.start()

        while True:
            current_sim = self.now()
            if sim_time <= current_sim:
                return

            sim_delta = (sim_time - current_sim).total_seconds()
            real_delta = self.sim_to_real_seconds(sim_delta)

            if real_delta <= 0:
                return

            # Sleep in chunks so we recheck after any pauses
            await asyncio.sleep(min(real_delta, 1.0))

    async def sleep_sim(self, sim_seconds: float) -> None:
        """Sleep for simulated time, respecting pauses that occur mid-sleep."""
        await self.sleep_until(self.now() + timedelta(seconds=max(0, sim_seconds)))

    def pause(self) -> None:
        """Pause the clock (reference-counted: multiple callers can pause independently)."""
        if self._pause_count == 0:
            self._pause_time = time.monotonic()
        self._pause_count += 1

    def resume(self) -> None:
        """Resume the clock (only actually resumes when all pausers have resumed)."""
        if self._pause_count <= 0:
            return
        self._pause_count -= 1
        if self._pause_count == 0 and self._pause_time and self.real_start:
            pause_duration = time.monotonic() - self._pause_time
            self.real_start += pause_duration
            self._pause_time = None

    def is_past(self, sim_time: datetime) -> bool:
        """Check if a simulated time has passed."""
        return self.now() >= sim_time

    def time_until(self, sim_time: datetime) -> timedelta:
        """Get the simulated time remaining until a target time."""
        return sim_time - self.now()

    def real_time_until(self, sim_time: datetime) -> timedelta:
        """Get the real time remaining until a simulated target time."""
        sim_remaining = self.time_until(sim_time).total_seconds()
        real_remaining = self.sim_to_real_seconds(sim_remaining)
        return timedelta(seconds=max(0, real_remaining))
