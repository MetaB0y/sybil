"""Shared failure classification and backoff for Arena LLM provider calls."""

from __future__ import annotations

import asyncio
import time
from dataclasses import dataclass
from typing import TYPE_CHECKING, Callable, Literal

if TYPE_CHECKING:
    from .metrics import ArenaMetrics

ProviderFailureKind = Literal[
    "authentication",
    "credit",
    "rate_limit",
    "timeout",
    "upstream",
    "contract",
    "other",
]


class ProviderContractError(ValueError):
    """The provider returned a response outside the requested safety contract."""


def classify_provider_error(error: Exception) -> ProviderFailureKind:
    """Classify OpenAI-compatible errors without coupling to one SDK version."""
    status_code = getattr(error, "status_code", None)
    if status_code is None:
        response = getattr(error, "response", None)
        status_code = getattr(response, "status_code", None)

    if status_code == 401:
        return "authentication"
    if status_code == 402:
        return "credit"
    if status_code == 429:
        return "rate_limit"
    if isinstance(error, (TimeoutError, asyncio.TimeoutError)) or "timeout" in type(
        error
    ).__name__.lower():
        return "timeout"
    if isinstance(status_code, int) and status_code >= 500:
        return "upstream"
    if isinstance(error, ProviderContractError):
        return "contract"
    return "other"


@dataclass(frozen=True)
class ProviderFailure:
    kind: ProviderFailureKind
    backoff_seconds: float


class ProviderCircuit:
    """Per-caller provider state using one repository-wide policy."""

    def __init__(
        self,
        component: str,
        metrics: ArenaMetrics | None = None,
        *,
        monotonic_fn: Callable[[], float] = time.monotonic,
        non_retryable_base_s: float = 60.0,
        non_retryable_max_s: float = 900.0,
        rate_limit_base_s: float = 30.0,
        rate_limit_max_s: float = 300.0,
    ):
        self.component = component
        self.metrics = metrics
        self._monotonic = monotonic_fn
        self.non_retryable_base_s = non_retryable_base_s
        self.non_retryable_max_s = non_retryable_max_s
        self.rate_limit_base_s = rate_limit_base_s
        self.rate_limit_max_s = rate_limit_max_s
        self.consecutive_failures = 0
        self.last_failure_kind: ProviderFailureKind | None = None
        self._retry_at = 0.0

    def can_attempt(self) -> bool:
        return self._monotonic() >= self._retry_at

    def backoff_remaining(self) -> float:
        return max(0.0, self._retry_at - self._monotonic())

    def record_failure(self, error: Exception) -> ProviderFailure:
        kind = classify_provider_error(error)
        self.consecutive_failures += 1
        self.last_failure_kind = kind

        if kind in ("authentication", "credit"):
            delay = min(
                self.non_retryable_max_s,
                self.non_retryable_base_s * (2 ** (self.consecutive_failures - 1)),
            )
        elif kind == "rate_limit":
            delay = min(
                self.rate_limit_max_s,
                self.rate_limit_base_s * (2 ** (self.consecutive_failures - 1)),
            )
        else:
            delay = 0.0
        self._retry_at = self._monotonic() + delay

        if self.metrics is not None:
            self.metrics.record_llm_provider_failure(self.component, kind, delay)
        return ProviderFailure(kind=kind, backoff_seconds=delay)

    def record_success(self) -> None:
        self.consecutive_failures = 0
        self.last_failure_kind = None
        self._retry_at = 0.0
        if self.metrics is not None:
            self.metrics.record_llm_provider_success(self.component)
