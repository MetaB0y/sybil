#!/usr/bin/env python3
"""Exact SYB-247 fixture assertions used by the Compose HTTP harness."""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


def load(path: str) -> Any:
    with Path(path).open(encoding="utf-8") as handle:
        return json.load(handle)


def one_by(items: list[dict[str, Any]], field: str, value: Any) -> dict[str, Any]:
    matches = [item for item in items if item.get(field) == value]
    assert len(matches) == 1, f"expected one {field}={value!r}, got {matches!r}"
    return matches[0]


def assert_summary(summary: dict[str, Any]) -> None:
    assert summary["schema"] == "sybil.seed_book.v1"
    assert summary["fixture_version"] == "SYB-247-v1:0"
    assert summary["semantics"] == "single_use_fresh_state"
    assert summary["account_count"] == 2
    assert summary["guard"]["health_status"] == "ok"
    assert summary["guard"]["explicit_dev_ack"] is True

    expected = summary["expected"]
    assert expected == {
        "matched_volume": 1_000,
        "total_fill_quantity": 2_000,
        "fill_count": 2,
        "yes_price_nanos": 500_000_000,
        "no_price_nanos": 500_000_000,
        "total_volume_nanos": 1_000_000_000,
        "total_welfare_nanos": 100_000_000,
        "funded_balance_total_nanos": 20_000_000_000,
        "marked_position_value_nanos": 1_000_000_000,
        "post_trade_balance_total_nanos": 19_000_000_000,
    }

    accounts = summary["accounts"]
    assert len(accounts) == 2
    yes_account = one_by(accounts, "role", "buy_yes")
    no_account = one_by(accounts, "role", "buy_no")
    assert yes_account["funded_balance_nanos"] == 10_000_000_000
    assert no_account["funded_balance_nanos"] == 10_000_000_000
    assert yes_account["order_nonce"] == 247_000_001
    assert no_account["order_nonce"] == 247_000_002
    assert len(yes_account["public_key_hex"]) == 66
    assert len(no_account["public_key_hex"]) == 66
    assert yes_account["public_key_hex"] != no_account["public_key_hex"]

    orders = summary["orders"]
    assert len(orders) == 2
    yes_order = one_by(orders, "side", "BuyYes")
    no_order = one_by(orders, "side", "BuyNo")
    assert yes_order["account_id"] == yes_account["account_id"]
    assert no_order["account_id"] == no_account["account_id"]
    assert (yes_order["limit_price_nanos"], yes_order["quantity"]) == (
        600_000_000,
        1_000,
    )
    assert (no_order["limit_price_nanos"], no_order["quantity"]) == (
        500_000_000,
        2_000,
    )
    for order in orders:
        assert order["expected_fill_quantity"] == 1_000
        assert order["expected_fill_price_nanos"] == 500_000_000

    steps = summary["http_steps"]
    assert all(step["status"] == 200 for step in steps), steps
    assert Counter(step["name"] for step in steps) == Counter(
        {
            "health": 1,
            "create_market": 1,
            "create_account": 2,
            "register_key": 2,
            "fund_account": 2,
            "submit_signed_order": 2,
        }
    )


def assert_result(
    summary: dict[str, Any],
    block: dict[str, Any],
    yes_account_actual: dict[str, Any],
    no_account_actual: dict[str, Any],
    yes_fills: list[dict[str, Any]],
    no_fills: list[dict[str, Any]],
) -> None:
    assert_summary(summary)
    expected = summary["expected"]
    market_id = summary["market"]["market_id"]
    yes_account = one_by(summary["accounts"], "role", "buy_yes")
    no_account = one_by(summary["accounts"], "role", "buy_no")
    yes_order = one_by(summary["orders"], "side", "BuyYes")
    no_order = one_by(summary["orders"], "side", "BuyNo")

    assert len(yes_fills) == 1, yes_fills
    assert len(no_fills) == 1, no_fills
    yes_fill = yes_fills[0]
    no_fill = no_fills[0]
    for fill, order in ((yes_fill, yes_order), (no_fill, no_order)):
        assert fill["order_id"] == order["order_id"]
        assert fill["fill_qty"] == order["expected_fill_quantity"]
        assert fill["fill_price_nanos"] == order["expected_fill_price_nanos"]
    assert yes_fill["block_height"] == no_fill["block_height"] == block["height"]

    assert block["order_count"] == 2
    assert block["fill_count"] == expected["fill_count"]
    assert block["orders_filled"] == expected["fill_count"]
    assert block["total_volume_nanos"] == expected["total_volume_nanos"]
    assert block["total_welfare_nanos"] == expected["total_welfare_nanos"]
    assert block["clearing_prices_nanos"][str(market_id)] == [
        expected["yes_price_nanos"],
        expected["no_price_nanos"],
    ]
    block_fills = {
        (fill["order_id"], fill["account_id"], fill["fill_qty"], fill["fill_price_nanos"])
        for fill in block["fills"]
    }
    assert block_fills == {
        (yes_order["order_id"], yes_account["account_id"], 1_000, 500_000_000),
        (no_order["order_id"], no_account["account_id"], 1_000, 500_000_000),
    }

    assert yes_account_actual["account_id"] == yes_account["account_id"]
    assert no_account_actual["account_id"] == no_account["account_id"]
    assert (
        yes_account_actual["balance_nanos"],
        yes_account_actual["available_balance_nanos"],
        yes_account_actual["reserved_balance_nanos"],
    ) == (9_500_000_000, 9_500_000_000, 0)
    assert (
        no_account_actual["balance_nanos"],
        no_account_actual["available_balance_nanos"],
        no_account_actual["reserved_balance_nanos"],
    ) == (9_500_000_000, 9_000_000_000, 500_000_000)
    assert yes_account_actual["positions"] == [
        {"market_id": market_id, "outcome": "YES", "quantity": 1_000}
    ]
    assert no_account_actual["positions"] == [
        {"market_id": market_id, "outcome": "NO", "quantity": 1_000}
    ]

    for account in (yes_account_actual, no_account_actual):
        assert (
            account["available_balance_nanos"] + account["reserved_balance_nanos"]
            == account["balance_nanos"]
        )

    cash_total = yes_account_actual["balance_nanos"] + no_account_actual["balance_nanos"]
    marked_positions = sum(
        position["quantity"] * expected["yes_price_nanos"] // 1_000
        for position in yes_account_actual["positions"]
    ) + sum(
        position["quantity"] * expected["no_price_nanos"] // 1_000
        for position in no_account_actual["positions"]
    )
    assert cash_total == expected["post_trade_balance_total_nanos"]
    assert marked_positions == expected["marked_position_value_nanos"]
    assert cash_total + marked_positions == expected["funded_balance_total_nanos"]


def self_test() -> None:
    summary = {
        "schema": "sybil.seed_book.v1",
        "fixture_version": "SYB-247-v1:0",
        "semantics": "single_use_fresh_state",
        "account_count": 2,
        "guard": {
            "health_status": "ok",
            "positive_dev_marker": False,
            "explicit_dev_ack": True,
        },
        "market": {"market_id": 7, "name": "fixture"},
        "accounts": [
            {
                "role": "buy_yes",
                "account_id": 11,
                "public_key_hex": "02" + "11" * 32,
                "funded_balance_nanos": 10_000_000_000,
                "order_nonce": 247_000_001,
            },
            {
                "role": "buy_no",
                "account_id": 12,
                "public_key_hex": "03" + "22" * 32,
                "funded_balance_nanos": 10_000_000_000,
                "order_nonce": 247_000_002,
            },
        ],
        "orders": [
            {
                "side": "BuyYes",
                "order_id": 21,
                "account_id": 11,
                "limit_price_nanos": 600_000_000,
                "quantity": 1_000,
                "expected_fill_quantity": 1_000,
                "expected_fill_price_nanos": 500_000_000,
            },
            {
                "side": "BuyNo",
                "order_id": 22,
                "account_id": 12,
                "limit_price_nanos": 500_000_000,
                "quantity": 2_000,
                "expected_fill_quantity": 1_000,
                "expected_fill_price_nanos": 500_000_000,
            },
        ],
        "expected": {
            "matched_volume": 1_000,
            "total_fill_quantity": 2_000,
            "fill_count": 2,
            "yes_price_nanos": 500_000_000,
            "no_price_nanos": 500_000_000,
            "total_volume_nanos": 1_000_000_000,
            "total_welfare_nanos": 100_000_000,
            "funded_balance_total_nanos": 20_000_000_000,
            "marked_position_value_nanos": 1_000_000_000,
            "post_trade_balance_total_nanos": 19_000_000_000,
        },
        "http_steps": [
            {"name": name, "status": 200}
            for name in (
                "health",
                "create_market",
                "create_account",
                "create_account",
                "register_key",
                "register_key",
                "fund_account",
                "fund_account",
                "submit_signed_order",
                "submit_signed_order",
            )
        ],
    }
    block = {
        "height": 3,
        "order_count": 2,
        "fill_count": 2,
        "orders_filled": 2,
        "total_volume_nanos": 1_000_000_000,
        "total_welfare_nanos": 100_000_000,
        "clearing_prices_nanos": {"7": [500_000_000, 500_000_000]},
        "fills": [
            {"order_id": 21, "account_id": 11, "fill_qty": 1_000, "fill_price_nanos": 500_000_000},
            {"order_id": 22, "account_id": 12, "fill_qty": 1_000, "fill_price_nanos": 500_000_000},
        ],
    }
    yes_account = {
        "account_id": 11,
        "balance_nanos": 9_500_000_000,
        "available_balance_nanos": 9_500_000_000,
        "reserved_balance_nanos": 0,
        "positions": [{"market_id": 7, "outcome": "YES", "quantity": 1_000}],
    }
    no_account = {
        "account_id": 12,
        "balance_nanos": 9_500_000_000,
        "available_balance_nanos": 9_000_000_000,
        "reserved_balance_nanos": 500_000_000,
        "positions": [{"market_id": 7, "outcome": "NO", "quantity": 1_000}],
    }
    yes_fills = [{"order_id": 21, "fill_qty": 1_000, "fill_price_nanos": 500_000_000, "block_height": 3}]
    no_fills = [{"order_id": 22, "fill_qty": 1_000, "fill_price_nanos": 500_000_000, "block_height": 3}]
    assert_result(summary, block, yes_account, no_account, yes_fills, no_fills)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--summary")
    parser.add_argument("--block")
    parser.add_argument("--yes-account")
    parser.add_argument("--no-account")
    parser.add_argument("--yes-fills")
    parser.add_argument("--no-fills")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        print("seed-book assertion self-test: ok")
        return
    required = (
        args.summary,
        args.block,
        args.yes_account,
        args.no_account,
        args.yes_fills,
        args.no_fills,
    )
    if any(value is None for value in required):
        parser.error("all JSON file arguments are required unless --self-test is used")
    assert_result(*(load(path) for path in required))
    print("exact fills, prices, reservations, and balance conservation: ok")


if __name__ == "__main__":
    main()
