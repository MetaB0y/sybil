"""LLM provider classification, backoff, and observability tests."""

from live.metrics import ArenaMetrics
from live.provider_health import ProviderCircuit, classify_provider_error


class ProviderError(Exception):
    def __init__(self, status_code: int):
        self.status_code = status_code
        super().__init__(f"provider status {status_code}")


def _value(metrics: ArenaMetrics, name: str, labels: dict | None = None):
    return metrics.registry.get_sample_value(name, labels)


def test_provider_errors_are_classified_by_operational_action():
    assert classify_provider_error(ProviderError(401)) == "authentication"
    assert classify_provider_error(ProviderError(402)) == "credit"
    assert classify_provider_error(ProviderError(429)) == "rate_limit"
    assert classify_provider_error(ProviderError(503)) == "upstream"
    assert classify_provider_error(TimeoutError()) == "timeout"
    assert classify_provider_error(ValueError()) == "other"


def test_credit_failure_degrades_and_backs_off_until_success():
    now = [100.0]
    metrics = ArenaMetrics()
    circuit = ProviderCircuit(
        "news-gate",
        metrics,
        monotonic_fn=lambda: now[0],
        non_retryable_base_s=60,
    )

    failure = circuit.record_failure(ProviderError(402))

    assert failure.kind == "credit"
    assert failure.backoff_seconds == 60
    assert circuit.can_attempt() is False
    assert _value(
        metrics,
        "sybil_arena_llm_provider_failures_total",
        {"component": "news-gate", "kind": "credit"},
    ) == 1
    assert _value(
        metrics,
        "sybil_arena_llm_provider_degraded",
        {"component": "news-gate"},
    ) == 1

    now[0] = 160
    assert circuit.can_attempt() is True
    circuit.record_success()
    assert _value(
        metrics,
        "sybil_arena_llm_provider_degraded",
        {"component": "news-gate"},
    ) == 0
    assert _value(
        metrics,
        "sybil_arena_llm_provider_last_success_timestamp_seconds",
        {"component": "news-gate"},
    ) > 0
