"""Simulated clock for time-compressed backtesting."""

import asyncio
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
        real_start: When real time began (set on first call to start())
    """

    sim_start: datetime
    compression_ratio: float = 60.0
    real_start: datetime | None = field(default=None, init=False)
    _paused: bool = field(default=False, init=False)
    _pause_time: datetime | None = field(default=None, init=False)

    def start(self) -> None:
        """Start the clock. Call this when the backtest begins."""
        if self.real_start is None:
            self.real_start = datetime.now()

    def now(self) -> datetime:
        """Get the current simulated time.

        Returns:
            The simulated datetime based on elapsed real time and compression.
        """
        if self.real_start is None:
            return self.sim_start

        if self._paused and self._pause_time:
            real_elapsed = (self._pause_time - self.real_start).total_seconds()
        else:
            real_elapsed = (datetime.now() - self.real_start).total_seconds()

        sim_elapsed = real_elapsed * self.compression_ratio
        return self.sim_start + timedelta(seconds=sim_elapsed)

    def elapsed_sim_time(self) -> timedelta:
        """Get elapsed simulated time since start."""
        return self.now() - self.sim_start

    def elapsed_real_time(self) -> timedelta:
        """Get elapsed real time since start."""
        if self.real_start is None:
            return timedelta(0)
        return datetime.now() - self.real_start

    def sim_to_real_seconds(self, sim_seconds: float) -> float:
        """Convert simulated seconds to real seconds."""
        return sim_seconds / self.compression_ratio

    def real_to_sim_seconds(self, real_seconds: float) -> float:
        """Convert real seconds to simulated seconds."""
        return real_seconds * self.compression_ratio

    async def sleep_until(self, sim_time: datetime) -> None:
        """Sleep until the simulated time is reached.

        Args:
            sim_time: The target simulated datetime to wait for.
        """
        if self.real_start is None:
            self.start()

        current_sim = self.now()
        if sim_time <= current_sim:
            return

        sim_delta = (sim_time - current_sim).total_seconds()
        real_delta = self.sim_to_real_seconds(sim_delta)

        if real_delta > 0:
            await asyncio.sleep(real_delta)

    async def sleep_sim(self, sim_seconds: float) -> None:
        """Sleep for a number of simulated seconds.

        Args:
            sim_seconds: Number of simulated seconds to sleep.
        """
        real_seconds = self.sim_to_real_seconds(sim_seconds)
        if real_seconds > 0:
            await asyncio.sleep(real_seconds)

    def pause(self) -> None:
        """Pause the clock."""
        if not self._paused:
            self._paused = True
            self._pause_time = datetime.now()

    def resume(self) -> None:
        """Resume the clock after pausing."""
        if self._paused and self._pause_time and self.real_start:
            pause_duration = datetime.now() - self._pause_time
            self.real_start += pause_duration
            self._paused = False
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
