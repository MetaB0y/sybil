#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = [
#   "blake3==1.0.8",
# ]
# ///
"""Capture an identity-free, transaction-auditable Polymarket event tape.

This case study is deliberately narrower than a CLOB replay. Public trade rows
can expose the taker summary and its resting-side execution records, but not
cancelled quotes, message arrival times, maker strategy, fees, or inventory.
The output therefore supports descriptive markouts only; it cannot identify an
FBA counterfactual or attribute resting-side records to professional makers.
"""

from __future__ import annotations

import argparse
import gzip
import json
import os
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import Counter, defaultdict
from dataclasses import dataclass
from datetime import datetime, timezone
from decimal import Decimal, ROUND_HALF_EVEN
from pathlib import Path
from typing import Any, Iterable

import blake3


GAMMA_EVENT_URL = "https://gamma-api.polymarket.com/events/{event_id}"
DATA_TRADES_URL = "https://data-api.polymarket.com/trades"
TRADE_PAGE_LIMIT = 10_000
MAX_API_OFFSET = 10_000
NANOS_PER_DOLLAR = 1_000_000_000
MICROSHARES_PER_SHARE = 1_000_000


@dataclass(frozen=True)
class Market:
    market_id: str
    condition_id: str
    question: str
    slug: str
    yes_token_id: str
    no_token_id: str
    settlement_yes_nanos: int


@dataclass(frozen=True)
class NormalizedTrade:
    condition_id: str
    transaction_hash: str
    timestamp: int
    effective_yes_side: str
    effective_yes_price_nanos: int
    quantity_microshares: int
    asset_id: str
    outcome_index: int
    source_side: str
    source_price_text: str
    source_size_text: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Capture the preregistered Polymarket spike-event case study"
    )
    parser.add_argument(
        "--protocol",
        type=Path,
        default=Path("benchmarks/market-structure/protocol-development.json"),
    )
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--timeout-seconds", type=float, default=30.0)
    parser.add_argument(
        "--captured-at",
        help="UTC ISO-8601 provenance time; defaults to the current time",
    )
    return parser.parse_args()


def request_json(url: str, *, timeout: float) -> Any:
    request = urllib.request.Request(
        url,
        headers={
            "Accept": "application/json",
            "User-Agent": "sybil-market-structure-capture/1",
        },
    )
    for attempt in range(3):
        try:
            with urllib.request.urlopen(request, timeout=timeout) as response:
                return json.loads(response.read(), parse_float=Decimal)
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError):
            if attempt == 2:
                raise
            time.sleep(0.25 * 2**attempt)
    raise AssertionError("unreachable")


def parse_json_list(value: Any) -> list[Any]:
    if isinstance(value, list):
        return value
    if not isinstance(value, str):
        raise ValueError(f"expected JSON list text, got {type(value).__name__}")
    decoded = json.loads(value, parse_float=Decimal)
    if not isinstance(decoded, list):
        raise ValueError("expected JSON list")
    return decoded


def price_nanos(value: Any) -> int:
    scaled = Decimal(str(value)) * NANOS_PER_DOLLAR
    landed = int(scaled.to_integral_value(rounding=ROUND_HALF_EVEN))
    if not 0 <= landed <= NANOS_PER_DOLLAR:
        raise ValueError(f"price outside [0, 1]: {value}")
    return landed


def quantity_microshares(value: Any) -> int:
    scaled = Decimal(str(value)) * MICROSHARES_PER_SHARE
    landed = int(scaled.to_integral_value(rounding=ROUND_HALF_EVEN))
    if Decimal(landed) != scaled or landed <= 0:
        raise ValueError(
            f"quantity is not a positive integer microshare amount: {value}"
        )
    return landed


def normalize_trade(row: dict[str, Any], market: Market) -> NormalizedTrade:
    if str(row.get("conditionId", "")).lower() != market.condition_id:
        raise ValueError(f"trade condition does not match {market.condition_id}")
    outcome_index = int(row["outcomeIndex"])
    if outcome_index not in (0, 1):
        raise ValueError(f"unsupported outcome index {outcome_index}")
    source_side = str(row["side"]).upper()
    if source_side not in ("BUY", "SELL"):
        raise ValueError(f"unsupported side {source_side}")
    raw_price = price_nanos(row["price"])
    effective_side = source_side.lower()
    effective_price = raw_price
    if outcome_index == 1:
        effective_side = "sell" if source_side == "BUY" else "buy"
        effective_price = NANOS_PER_DOLLAR - raw_price
    return NormalizedTrade(
        condition_id=market.condition_id,
        transaction_hash=str(row["transactionHash"]).lower(),
        timestamp=int(row["timestamp"]),
        effective_yes_side=effective_side,
        effective_yes_price_nanos=effective_price,
        quantity_microshares=quantity_microshares(row["size"]),
        asset_id=str(row["asset"]),
        outcome_index=outcome_index,
        source_side=source_side.lower(),
        source_price_text=str(row["price"]),
        source_size_text=str(row["size"]),
    )


def private_row_key(row: dict[str, Any]) -> tuple[str, ...]:
    """Key used transiently to remove the taker summary from all trade rows."""
    return (
        str(row.get("proxyWallet", "")).lower(),
        str(row.get("transactionHash", "")).lower(),
        str(row.get("conditionId", "")).lower(),
        str(row.get("asset", "")),
        str(row.get("side", "")).upper(),
        str(row.get("outcomeIndex", "")),
        str(row.get("size", "")),
        str(row.get("price", "")),
        str(row.get("timestamp", "")),
    )


def public_row_key(row: dict[str, Any]) -> tuple[str, ...]:
    return private_row_key(row)[1:]


def parse_markets(event: dict[str, Any], expected_slug: str) -> list[Market]:
    if str(event.get("slug", "")) != expected_slug:
        raise ValueError(
            f"Gamma event slug changed: expected {expected_slug}, "
            f"got {event.get('slug')}"
        )
    parsed: list[Market] = []
    for raw in event.get("markets", []):
        outcomes = [str(value).lower() for value in parse_json_list(raw["outcomes"])]
        token_ids = [str(value) for value in parse_json_list(raw["clobTokenIds"])]
        settlements = [
            price_nanos(value) for value in parse_json_list(raw["outcomePrices"])
        ]
        if outcomes != ["yes", "no"] or len(token_ids) != 2:
            raise ValueError(f"market {raw.get('id')} is not a Yes/No CLOB market")
        if settlements not in ([NANOS_PER_DOLLAR, 0], [0, NANOS_PER_DOLLAR]):
            raise ValueError(f"market {raw.get('id')} lacks binary settlement")
        parsed.append(
            Market(
                market_id=str(raw["id"]),
                condition_id=str(raw["conditionId"]).lower(),
                question=str(raw["question"]),
                slug=str(raw["slug"]),
                yes_token_id=token_ids[0],
                no_token_id=token_ids[1],
                settlement_yes_nanos=settlements[0],
            )
        )
    if len(parsed) != 31 or len({market.condition_id for market in parsed}) != 31:
        raise ValueError(f"expected 31 unique markets, got {len(parsed)}")
    return sorted(parsed, key=lambda market: market.market_id)


def fetch_trade_rows(
    condition_id: str, *, taker_only: bool, timeout: float
) -> tuple[list[dict[str, Any]], list[dict[str, int]]]:
    rows: list[dict[str, Any]] = []
    pages: list[dict[str, int]] = []
    for offset in (0, MAX_API_OFFSET):
        query = urllib.parse.urlencode(
            {
                "market": condition_id,
                "limit": TRADE_PAGE_LIMIT,
                "offset": offset,
                "takerOnly": str(taker_only).lower(),
            }
        )
        page = request_json(f"{DATA_TRADES_URL}?{query}", timeout=timeout)
        if not isinstance(page, list) or not all(isinstance(row, dict) for row in page):
            raise ValueError("Data API trades endpoint did not return object rows")
        pages.append({"offset": offset, "rows": len(page)})
        rows.extend(page)
        if len(page) < TRADE_PAGE_LIMIT:
            break
    else:
        raise RuntimeError(
            f"{condition_id} reached the public offset cap; refusing a partial capture"
        )
    return rows, pages


def counterpart_markout_nanos(
    side: str, price: int, settlement: int, quantity: int
) -> int:
    per_share = settlement - price if side == "buy" else price - settlement
    return per_share * quantity // MICROSHARES_PER_SHARE


def reconstruct_transaction(
    market: Market,
    taker_row: dict[str, Any],
    counterpart_rows: Iterable[dict[str, Any]],
    *,
    known_hashes: set[str],
) -> dict[str, Any]:
    taker = normalize_trade(taker_row, market)
    normalized = sorted(
        (normalize_trade(row, market) for row in counterpart_rows),
        key=lambda row: (
            row.effective_yes_price_nanos,
            row.quantity_microshares,
            row.asset_id,
            row.source_side,
        ),
    )
    expected_side = "sell" if taker.effective_yes_side == "buy" else "buy"
    side_valid = all(row.effective_yes_side == expected_side for row in normalized)
    counterpart_quantity = sum(row.quantity_microshares for row in normalized)
    quantity_delta = counterpart_quantity - taker.quantity_microshares
    exact = bool(normalized) and side_valid and quantity_delta == 0
    status = "exact" if exact else "unreconciled"

    weighted_price = None
    price_min = None
    price_max = None
    counterpart_markout = None
    if normalized:
        weighted_price = sum(
            row.effective_yes_price_nanos * row.quantity_microshares
            for row in normalized
        ) // counterpart_quantity
        price_min = min(row.effective_yes_price_nanos for row in normalized)
        price_max = max(row.effective_yes_price_nanos for row in normalized)
        counterpart_markout = sum(
            counterpart_markout_nanos(
                row.effective_yes_side,
                row.effective_yes_price_nanos,
                market.settlement_yes_nanos,
                row.quantity_microshares,
            )
            for row in normalized
        )

    taker_markout = counterpart_markout_nanos(
        taker.effective_yes_side,
        taker.effective_yes_price_nanos,
        market.settlement_yes_nanos,
        taker.quantity_microshares,
    )
    return {
        "schema_version": 1,
        "record_type": "taker_transaction",
        "market_id": market.market_id,
        "condition_id": market.condition_id,
        "market_slug": market.slug,
        "transaction_hash": taker.transaction_hash,
        "known_case": taker.transaction_hash in known_hashes,
        "timestamp": taker.timestamp,
        "settlement_yes_nanos": market.settlement_yes_nanos,
        "taker": {
            "effective_yes_side": taker.effective_yes_side,
            "effective_yes_price_nanos": taker.effective_yes_price_nanos,
            "quantity_microshares": taker.quantity_microshares,
            "gross_settlement_markout_nanos": taker_markout,
            "source_outcome_index": taker.outcome_index,
            "source_side": taker.source_side,
            "source_price_text": taker.source_price_text,
            "source_size_text": taker.source_size_text,
        },
        "counterpart_reconstruction": {
            "status": status,
            "row_count": len(normalized),
            "side_consistent": side_valid,
            "quantity_microshares": counterpart_quantity,
            "quantity_delta_microshares": quantity_delta,
            "effective_yes_price_min_nanos": price_min,
            "effective_yes_price_max_nanos": price_max,
            "effective_yes_weighted_price_nanos": weighted_price,
            "gross_settlement_markout_nanos": counterpart_markout,
            "rows": [
                {
                    "effective_yes_side": row.effective_yes_side,
                    "effective_yes_price_nanos": row.effective_yes_price_nanos,
                    "quantity_microshares": row.quantity_microshares,
                    "asset_id": row.asset_id,
                    "source_outcome_index": row.outcome_index,
                    "source_side": row.source_side,
                    "source_price_text": row.source_price_text,
                    "source_size_text": row.source_size_text,
                }
                for row in normalized
            ],
        },
    }


def reconstruct_market(
    market: Market,
    taker_rows: list[dict[str, Any]],
    all_rows: list[dict[str, Any]],
    known_hashes: set[str],
) -> list[dict[str, Any]]:
    transaction_hashes = [
        str(row.get("transactionHash", "")).lower() for row in taker_rows
    ]
    if len(transaction_hashes) != len(set(transaction_hashes)):
        raise ValueError(
            f"{market.condition_id} has multiple taker summaries per transaction"
        )
    all_by_transaction: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in all_rows:
        all_by_transaction[str(row.get("transactionHash", "")).lower()].append(row)

    private_counts = Counter(private_row_key(row) for row in all_rows)
    public_counts = Counter(public_row_key(row) for row in all_rows)
    records = []
    for taker_row in taker_rows:
        private_key = private_row_key(taker_row)
        public_key = public_row_key(taker_row)
        if private_counts[private_key] != 1:
            raise ValueError(
                f"taker row {taker_row.get('transactionHash')} not unique in all rows"
            )
        if public_counts[public_key] < 1:
            raise ValueError("taker row missing from all-row response")
        tx_hash = str(taker_row["transactionHash"]).lower()
        removed = False
        counterparts = []
        for row in all_by_transaction[tx_hash]:
            if not removed and private_row_key(row) == private_key:
                removed = True
                continue
            counterparts.append(row)
        if not removed:
            raise ValueError(f"could not remove taker summary {tx_hash}")
        records.append(
            reconstruct_transaction(
                market,
                taker_row,
                counterparts,
                known_hashes=known_hashes,
            )
        )
    return sorted(records, key=lambda row: (row["timestamp"], row["transaction_hash"]))


def market_record(market: Market) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "record_type": "market",
        "market_id": market.market_id,
        "condition_id": market.condition_id,
        "question": market.question,
        "slug": market.slug,
        "yes_token_id": market.yes_token_id,
        "no_token_id": market.no_token_id,
        "settlement_yes_nanos": market.settlement_yes_nanos,
    }


def canonical_json(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)


def atomic_write(path: Path, content: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp-{os.getpid()}")
    try:
        with temporary.open("wb") as output:
            output.write(content)
            output.flush()
            os.fsync(output.fileno())
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def hash_file_content(content: bytes) -> str:
    return blake3.blake3(content).hexdigest()


def capture(args: argparse.Namespace) -> None:
    protocol_bytes = args.protocol.read_bytes()
    protocol = json.loads(protocol_bytes)
    case = protocol["historical_case_study"]
    event_id = str(case["source_event_id"])
    event_slug = str(case["source_event_slug"])
    known_hashes = {str(value).lower() for value in case["known_transaction_hashes"]}

    event = request_json(
        GAMMA_EVENT_URL.format(event_id=urllib.parse.quote(event_id)),
        timeout=args.timeout_seconds,
    )
    if not isinstance(event, dict) or str(event.get("id")) != event_id:
        raise ValueError(f"Gamma did not return event {event_id}")
    markets = parse_markets(event, event_slug)

    records: list[dict[str, Any]] = []
    pagination: list[dict[str, Any]] = []
    for market in markets:
        records.append(market_record(market))
        taker_rows, taker_pages = fetch_trade_rows(
            market.condition_id, taker_only=True, timeout=args.timeout_seconds
        )
        all_rows, all_pages = fetch_trade_rows(
            market.condition_id, taker_only=False, timeout=args.timeout_seconds
        )
        reconstructed = reconstruct_market(market, taker_rows, all_rows, known_hashes)
        records.extend(reconstructed)
        pagination.append(
            {
                "condition_id": market.condition_id,
                "taker_only": taker_pages,
                "all_rows": all_pages,
                "transactions": len(reconstructed),
                "counterpart_rows": sum(
                    row["counterpart_reconstruction"]["row_count"]
                    for row in reconstructed
                ),
                "exact_reconstructions": sum(
                    row["counterpart_reconstruction"]["status"] == "exact"
                    for row in reconstructed
                ),
            }
        )

    transaction_records = [
        row for row in records if row["record_type"] == "taker_transaction"
    ]
    found_known = {
        row["transaction_hash"] for row in transaction_records if row["known_case"]
    }
    if found_known != known_hashes:
        missing = sorted(known_hashes - found_known)
        raise ValueError(
            f"known transaction hashes absent from complete capture: {missing}"
        )

    payload_content = b"".join(
        canonical_json(record).encode() + b"\n" for record in records
    )
    if args.output.suffix == ".gz":
        output_content = gzip.compress(payload_content, compresslevel=9, mtime=0)
        compression = "gzip"
    else:
        output_content = payload_content
        compression = "none"
    captured_at = args.captured_at or datetime.now(timezone.utc).isoformat()
    manifest = {
        "schema_version": 1,
        "corpus_id": "polymarket-israel-gaza-january-2026-complete-event-v1",
        "captured_at": captured_at,
        "protocol": {
            "path": str(args.protocol),
            "protocol_id": protocol["protocol_id"],
            "blake3": hash_file_content(protocol_bytes),
        },
        "source": {
            "event_id": event_id,
            "event_slug": event_slug,
            "gamma_endpoint": GAMMA_EVENT_URL.format(event_id=event_id),
            "trades_endpoint": DATA_TRADES_URL,
            "trade_query": {
                "market": "one condition ID at a time",
                "limit": TRADE_PAGE_LIMIT,
                "offsets": [0, MAX_API_OFFSET],
                "takerOnly": [True, False],
            },
        },
        "projection": {
            "identity_fields_retained": False,
            "retained_provenance": ["condition_id", "transaction_hash", "timestamp"],
            "price_unit": "integer nanos per $1 payoff",
            "quantity_unit": "integer microshares",
            "no_maker_classification": True,
        },
        "completeness": {
            "expected_markets": 31,
            "captured_markets": len(markets),
            "taker_transactions": len(transaction_records),
            "counterpart_rows": sum(
                row["counterpart_reconstruction"]["row_count"]
                for row in transaction_records
            ),
            "exact_reconstructions": sum(
                row["counterpart_reconstruction"]["status"] == "exact"
                for row in transaction_records
            ),
            "unreconciled_reconstructions": sum(
                row["counterpart_reconstruction"]["status"] != "exact"
                for row in transaction_records
            ),
            "known_transactions_found": sorted(found_known),
            "pagination": pagination,
        },
        "limitations": case["limits"],
        "artifact": {
            "path": str(args.output),
            "bytes": len(output_content),
            "blake3": hash_file_content(output_content),
            "compression": compression,
            "uncompressed_bytes": len(payload_content),
            "uncompressed_blake3": hash_file_content(payload_content),
        },
    }
    manifest_content = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    atomic_write(args.output, output_content)
    atomic_write(args.manifest, manifest_content)


def main() -> None:
    capture(parse_args())


if __name__ == "__main__":
    main()
