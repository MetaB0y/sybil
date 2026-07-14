import json

import pytest

from live import queries


class _Response:
    def __init__(self, payload):
        self.payload = payload

    def read(self):
        return json.dumps(self.payload).encode()


def test_mm_mtm_uses_account_one_sybil_values_and_authenticates(monkeypatch):
    requests = []

    def fake_urlopen(request, timeout):
        requests.append((request, timeout))
        if request.full_url.endswith("/v1/accounts/1/portfolio"):
            return _Response(
                {
                    "balance_nanos": 900_000_000_000,
                    "total_position_value_nanos": 1_400_000_000,
                    "portfolio_value_nanos": 901_400_000_000,
                    "pnl_nanos": -98_600_000_000,
                    "total_deposited_nanos": 1_000_000_000_000,
                    "positions": [
                        {"market_id": 7, "outcome": "YES", "quantity": 1_000},
                        {"market_id": 7, "outcome": "NO", "quantity": 2_000},
                    ],
                }
            )
        raise AssertionError(f"unexpected request: {request.full_url}")

    monkeypatch.setenv("SYBIL_SERVICE_TOKEN", "test-token")
    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)

    result = queries.get_mm_mtm("http://sybil")

    assert result is not None
    assert result["cash"] == 900.0
    assert result["position_value"] == 1.4
    assert result["total"] == pytest.approx(901.4)
    assert result["pnl"] == pytest.approx(-98.6)
    assert result["positions"] == 2
    assert result["initial"] == 1_000.0
    assert all(
        request.get_header("Authorization") == "Bearer test-token" for request, _timeout in requests
    )
