"""Offline unit tests for the Polymarket spike capture projection."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("capture_polymarket_spikes.py")
SPEC = importlib.util.spec_from_file_location("capture_polymarket_spikes", SCRIPT)
assert SPEC and SPEC.loader
capture = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = capture
SPEC.loader.exec_module(capture)


def market(settlement: int = 1_000_000_000):
    return capture.Market(
        market_id="7",
        condition_id="0xcondition",
        question="Question?",
        slug="question",
        yes_token_id="yes-token",
        no_token_id="no-token",
        settlement_yes_nanos=settlement,
    )


def row(
    *,
    wallet: str,
    side: str,
    outcome_index: int,
    size: str,
    price: str,
):
    return {
        "proxyWallet": wallet,
        "transactionHash": "0xtx",
        "conditionId": "0xcondition",
        "asset": "yes-token" if outcome_index == 0 else "no-token",
        "side": side,
        "outcomeIndex": outcome_index,
        "size": size,
        "price": price,
        "timestamp": 123,
    }


class CaptureTests(unittest.TestCase):
    def test_buy_no_normalizes_to_sell_yes(self):
        normalized = capture.normalize_trade(
            row(
                wallet="maker",
                side="BUY",
                outcome_index=1,
                size="2.5",
                price="0.2",
            ),
            market(),
        )
        self.assertEqual(normalized.effective_yes_side, "sell")
        self.assertEqual(normalized.effective_yes_price_nanos, 800_000_000)
        self.assertEqual(normalized.quantity_microshares, 2_500_000)

    def test_counterpart_rows_reconcile_without_retaining_identity(self):
        taker = row(
            wallet="taker-wallet",
            side="BUY",
            outcome_index=0,
            size="3",
            price="0.7",
        )
        maker_yes = row(
            wallet="maker-one",
            side="SELL",
            outcome_index=0,
            size="1",
            price="0.8",
        )
        maker_no = row(
            wallet="maker-two",
            side="BUY",
            outcome_index=1,
            size="2",
            price="0.35",
        )
        records = capture.reconstruct_market(
            market(), [taker], [taker, maker_yes, maker_no], {"0xtx"}
        )
        self.assertEqual(len(records), 1)
        reconstructed = records[0]["counterpart_reconstruction"]
        self.assertEqual(reconstructed["status"], "exact")
        self.assertEqual(reconstructed["quantity_microshares"], 3_000_000)
        self.assertEqual(reconstructed["effective_yes_price_min_nanos"], 650_000_000)
        self.assertEqual(reconstructed["effective_yes_price_max_nanos"], 800_000_000)
        self.assertNotIn("proxyWallet", capture.canonical_json(records[0]))
        self.assertNotIn("maker-one", capture.canonical_json(records[0]))

    def test_quantity_requires_exact_microshares(self):
        with self.assertRaises(ValueError):
            capture.quantity_microshares("0.0000001")


if __name__ == "__main__":
    unittest.main()
