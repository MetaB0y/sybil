#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = [
#   "blake3==1.0.8",
#   "msgpack==1.1.1",
# ]
# ///
"""Capture a compact, public Polymarket CLOB-depth solver corpus.

The source books contain anonymous aggregated levels, so they cannot recover
maker identities, capital, or already-matched batch arrivals. Each selected
event therefore combines real resting depth with clearly labelled synthetic,
depth-calibrated arrivals and a synthetic two-MM overlay. A raw portfolio case
preserves the untouched resting-depth control.

The MessagePack layout mirrors Rust's rmp-serde tuple encoding for
SolverReplayCorpusV1. The checked-in benchmark runner is the authoritative
decoder and validator.
"""

from __future__ import annotations

import argparse
import json
import os
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from datetime import datetime, timezone
from decimal import Decimal, InvalidOperation, ROUND_DOWN, ROUND_HALF_EVEN
from pathlib import Path
from typing import Any, Iterable

import blake3
import msgpack


GAMMA_EVENTS_URL = "https://gamma-api.polymarket.com/events"
CLOB_BOOKS_URL = "https://clob.polymarket.com/books"
NANOS_PER_DOLLAR = 1_000_000_000
SHARE_SCALE = 1_000
MARKET_ID_NONE = 2**32 - 1
MAX_ORDER_QTY = 1_000_000 * SHARE_SCALE
MAX_PRICE = NANOS_PER_DOLLAR

BUCKETS: tuple[tuple[str, frozenset[str]], ...] = (
    ("politics", frozenset({"politics", "elections", "geopolitics"})),
    ("sports", frozenset({"sports"})),
    ("crypto", frozenset({"crypto", "bitcoin"})),
    ("economics", frozenset({"economy", "finance", "economic-policy"})),
    ("technology", frozenset({"technology", "tech", "big-tech", "ai"})),
    (
        "culture",
        frozenset({"pop-culture", "entertainment", "music", "movies", "box-office"}),
    ),
)


@dataclass(frozen=True)
class SelectedEvent:
    bucket: str
    event_id: str
    title: str
    volume_24h: str
    neg_risk: bool
    tags: tuple[str, ...]
    markets: tuple[dict[str, Any], ...]


@dataclass(frozen=True)
class CapturedMarket:
    event: SelectedEvent
    market: dict[str, Any]
    token_ids: tuple[str, str]
    books: tuple[dict[str, Any], dict[str, Any]]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Capture public CLOB depth as a validated Sybil solver corpus"
    )
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--corpus-id", required=True)
    parser.add_argument("--event-page-limit", type=int, default=500)
    parser.add_argument("--max-markets-per-event", type=int, default=12)
    parser.add_argument("--max-levels-per-side", type=int, default=20)
    parser.add_argument("--timeout-seconds", type=float, default=30.0)
    return parser.parse_args()


def request_json(
    url: str,
    *,
    timeout: float,
    body: Any | None = None,
) -> Any:
    encoded = None if body is None else json.dumps(body, separators=(",", ":")).encode()
    headers = {
        "Accept": "application/json",
        "User-Agent": "sybil-solver-benchmark-capture/1",
    }
    if encoded is not None:
        headers["Content-Type"] = "application/json"
    request = urllib.request.Request(url, data=encoded, headers=headers)
    for attempt in range(3):
        try:
            with urllib.request.urlopen(request, timeout=timeout) as response:
                return json.load(response)
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError):
            if attempt == 2:
                raise
            time.sleep(0.25 * 2**attempt)
    raise AssertionError("unreachable")


def parse_json_list(value: Any) -> list[Any]:
    if isinstance(value, list):
        return value
    if not isinstance(value, str) or not value:
        return []
    parsed = json.loads(value)
    return parsed if isinstance(parsed, list) else []


def numeric_text(value: Any) -> str:
    if value is None or value == "":
        return "0"
    return str(value)


def market_volume(market: dict[str, Any]) -> Decimal:
    for field in ("volume24hr", "volume24hrClob", "volume"):
        try:
            return Decimal(numeric_text(market.get(field)))
        except InvalidOperation:
            continue
    return Decimal(0)


def eligible_markets(event: dict[str, Any]) -> list[dict[str, Any]]:
    eligible = []
    for market in event.get("markets", []):
        if not market.get("active") or market.get("closed"):
            continue
        if market.get("enableOrderBook") is False:
            continue
        token_ids = parse_json_list(market.get("clobTokenIds"))
        outcomes = parse_json_list(market.get("outcomes"))
        if len(token_ids) != 2 or len(outcomes) != 2:
            continue
        if not all(isinstance(token_id, str) and token_id for token_id in token_ids):
            continue
        eligible.append(market)
    return sorted(
        eligible,
        key=lambda market: (
            -market_volume(market),
            str(market.get("conditionId", "")),
        ),
    )


def event_tags(event: dict[str, Any]) -> tuple[str, ...]:
    return tuple(
        sorted(
            {
                str(tag.get("slug", "")).strip().lower()
                for tag in event.get("tags", [])
                if str(tag.get("slug", "")).strip()
            }
        )
    )


def select_events(
    events: list[dict[str, Any]], max_markets_per_event: int
) -> list[SelectedEvent]:
    selected: list[SelectedEvent] = []
    selected_ids: set[str] = set()
    for bucket, accepted_tags in BUCKETS:
        for event in events:
            event_id = str(event.get("id", ""))
            tags = event_tags(event)
            if event_id in selected_ids or not accepted_tags.intersection(tags):
                continue
            markets = eligible_markets(event)
            neg_risk = bool(event.get("negRisk") or event.get("enableNegRisk"))
            # A partial NegRisk outcome set would make sum(price)=1 false.
            if not markets or (neg_risk and len(markets) > max_markets_per_event):
                continue
            if not neg_risk:
                markets = markets[:max_markets_per_event]
            selected.append(
                SelectedEvent(
                    bucket=bucket,
                    event_id=event_id,
                    title=str(event.get("title", "")),
                    volume_24h=numeric_text(event.get("volume24hr")),
                    neg_risk=neg_risk,
                    tags=tags,
                    markets=tuple(markets),
                )
            )
            selected_ids.add(event_id)
            break
        else:
            raise RuntimeError(
                f"could not select an eligible {bucket} event from the Gamma page"
            )
    return selected


def fetch_books(
    selected: list[SelectedEvent], timeout: float
) -> dict[str, dict[str, Any]]:
    token_ids = [
        str(token_id)
        for event in selected
        for market in event.markets
        for token_id in parse_json_list(market["clobTokenIds"])
    ]
    response = request_json(
        CLOB_BOOKS_URL,
        timeout=timeout,
        body=[{"token_id": token_id} for token_id in token_ids],
    )
    if not isinstance(response, list):
        raise RuntimeError("CLOB /books did not return an array")
    return {str(book.get("asset_id", "")): book for book in response}


def capture_markets(
    selected: list[SelectedEvent], books: dict[str, dict[str, Any]]
) -> list[CapturedMarket]:
    captured = []
    for event in selected:
        for market in event.markets:
            token_ids = tuple(
                str(value) for value in parse_json_list(market["clobTokenIds"])
            )
            assert len(token_ids) == 2
            if any(token_id not in books for token_id in token_ids):
                if event.neg_risk:
                    raise RuntimeError(
                        f"CLOB /books omitted part of NegRisk event {event.event_id}"
                    )
                continue
            pair = (books[token_ids[0]], books[token_ids[1]])
            condition_id = str(market.get("conditionId", "")).lower()
            if any(
                str(book.get("market", "")).lower() != condition_id for book in pair
            ):
                raise RuntimeError(
                    f"CLOB condition mismatch for selected market {condition_id}"
                )
            captured.append(
                CapturedMarket(
                    event=event,
                    market=market,
                    token_ids=(token_ids[0], token_ids[1]),
                    books=pair,
                )
            )
        if not any(market.event.event_id == event.event_id for market in captured):
            raise RuntimeError(
                f"selected event {event.event_id} has no usable order books"
            )
    return captured


def decimal_units(value: Any, scale: int, *, rounding: str) -> int:
    try:
        decimal = Decimal(str(value))
    except InvalidOperation as error:
        raise ValueError(f"invalid decimal {value!r}") from error
    if not decimal.is_finite() or decimal < 0:
        raise ValueError(f"invalid non-finite or negative decimal {value!r}")
    return int((decimal * scale).to_integral_value(rounding=rounding))


def price_nanos(value: Any) -> int:
    price = decimal_units(value, NANOS_PER_DOLLAR, rounding=ROUND_HALF_EVEN)
    if not 0 < price < MAX_PRICE:
        raise ValueError(f"price {value!r} is outside the open unit interval")
    return price


def quantity_units(value: Any) -> int:
    quantity = decimal_units(value, SHARE_SCALE, rounding=ROUND_DOWN)
    return min(quantity, MAX_ORDER_QTY)


def normalized_levels(
    book: dict[str, Any], side: str, limit: int
) -> list[tuple[int, int]]:
    levels = []
    for level in book.get(side, []):
        try:
            price = price_nanos(level["price"])
            quantity = quantity_units(level["size"])
        except (KeyError, ValueError):
            continue
        if quantity > 0:
            levels.append((price, quantity))
    reverse = side == "bids"
    levels.sort(key=lambda level: level[0], reverse=reverse)
    return levels[:limit]


def replay_order(
    order_id: int,
    market_id: int,
    outcome: int,
    *,
    sell: bool,
    price: int,
    quantity: int,
) -> list[Any]:
    payoffs = [0] * 32
    payoffs[outcome] = -1 if sell else 1
    return [
        order_id,
        [market_id, MARKET_ID_NONE, MARKET_ID_NONE, MARKET_ID_NONE, MARKET_ID_NONE],
        1,
        payoffs,
        2,
        price,
        quantity,
        None,
        None,
    ]


def side_name(outcome: int, sell: bool) -> str:
    if outcome == 0:
        return "SellYes" if sell else "BuyYes"
    return "SellNo" if sell else "BuyNo"


def required_capital(price: int, quantity: int, *, sell: bool) -> int:
    capital_price = MAX_PRICE - price if sell else price
    return (capital_price * quantity + SHARE_SCALE - 1) // SHARE_SCALE


def public_orders(
    markets: list[CapturedMarket],
    market_ids: dict[str, int],
    max_levels: int,
    next_order_id: int = 1,
) -> tuple[list[list[Any]], int]:
    orders = []
    order_id = next_order_id
    for captured in markets:
        condition_id = str(captured.market["conditionId"]).lower()
        market_id = market_ids[condition_id]
        for outcome, book in enumerate(captured.books):
            for price, quantity in normalized_levels(book, "bids", max_levels):
                orders.append(
                    replay_order(
                        order_id,
                        market_id,
                        outcome,
                        sell=False,
                        price=price,
                        quantity=quantity,
                    )
                )
                order_id += 1
            for price, quantity in normalized_levels(book, "asks", max_levels):
                orders.append(
                    replay_order(
                        order_id,
                        market_id,
                        outcome,
                        sell=True,
                        price=price,
                        quantity=quantity,
                    )
                )
                order_id += 1
    return orders, order_id


def synthetic_batch_arrivals(
    markets: list[CapturedMarket],
    market_ids: dict[str, int],
    max_levels: int,
    next_order_id: int,
) -> tuple[list[list[Any]], int]:
    """Create a deterministic directional shock that sweeps observed depth.

    Continuous books normally contain no crossing orders. For each market this
    adds either the BuyYes/SellNo or SellYes/BuyNo pair, alternating direction
    by market. Each arrival can sweep up to three public levels plus half of
    the touch quantity, which also gives the synthetic MM overlay useful flow.
    """
    orders = []
    order_id = next_order_id
    for market_index, captured in enumerate(markets):
        condition_id = str(captured.market["conditionId"]).lower()
        market_id = market_ids[condition_id]
        bullish = market_index % 2 == 0
        for outcome, book in enumerate(captured.books):
            sell = (outcome == 1) if bullish else (outcome == 0)
            opposing_side = "bids" if sell else "asks"
            levels = normalized_levels(book, opposing_side, max_levels)
            if not levels:
                continue
            swept = levels[:3]
            touch_quantity = swept[0][1]
            quantity = min(
                MAX_ORDER_QTY,
                sum(level_quantity for _, level_quantity in swept)
                + min(100 * SHARE_SCALE, max(SHARE_SCALE, touch_quantity // 2)),
            )
            orders.append(
                replay_order(
                    order_id,
                    market_id,
                    outcome,
                    sell=sell,
                    price=swept[-1][0],
                    quantity=quantity,
                )
            )
            order_id += 1
    if not orders:
        raise RuntimeError("synthetic batch arrivals produced no usable orders")
    return orders, order_id


def synthetic_mm_overlay(
    markets: list[CapturedMarket],
    market_ids: dict[str, int],
    max_levels: int,
    next_order_id: int,
) -> tuple[list[list[Any]], list[list[Any]]]:
    orders = []
    constraints = []
    order_id = next_order_id
    for mm_id in range(2):
        mm_order_ids: list[int] = []
        mm_sides: list[list[Any]] = []
        max_capital = 0
        for captured in markets:
            condition_id = str(captured.market["conditionId"]).lower()
            market_id = market_ids[condition_id]
            for outcome, book in enumerate(captured.books):
                tick = max(
                    1,
                    decimal_units(
                        book.get("tick_size", "0.01"),
                        NANOS_PER_DOLLAR,
                        rounding=ROUND_HALF_EVEN,
                    ),
                )
                bids = normalized_levels(book, "bids", max_levels)
                asks = normalized_levels(book, "asks", max_levels)
                for sell, levels in ((False, bids), (True, asks)):
                    if not levels:
                        continue
                    touch_price, touch_quantity = levels[0]
                    price = (
                        touch_price + mm_id * tick
                        if sell
                        else touch_price - mm_id * tick
                    )
                    if not 0 < price < MAX_PRICE:
                        continue
                    quantity = max(
                        SHARE_SCALE,
                        min(100 * SHARE_SCALE, touch_quantity // (mm_id + 2)),
                    )
                    orders.append(
                        replay_order(
                            order_id,
                            market_id,
                            outcome,
                            sell=sell,
                            price=price,
                            quantity=quantity,
                        )
                    )
                    mm_order_ids.append(order_id)
                    mm_sides.append([order_id, side_name(outcome, sell)])
                    max_capital += required_capital(price, quantity, sell=sell)
                    order_id += 1
        if not mm_order_ids or max_capital <= 0:
            raise RuntimeError(f"synthetic MM {mm_id} produced no usable quotes")
        constraints.append([mm_id, max_capital, mm_order_ids, mm_sides])
    return orders, constraints


def grouped_market_ids(
    events: Iterable[SelectedEvent],
    market_ids: dict[str, int],
) -> list[list[int]]:
    groups = []
    for event in events:
        if not event.neg_risk:
            continue
        groups.append(
            [market_ids[str(market["conditionId"]).lower()] for market in event.markets]
        )
    return groups


def build_case(
    case_id: str,
    traits: list[str],
    events: list[SelectedEvent],
    markets: list[CapturedMarket],
    *,
    arrivals: bool,
    overlay: bool,
    max_levels: int,
) -> list[Any]:
    market_ids = {
        str(captured.market["conditionId"]).lower(): index
        for index, captured in enumerate(markets)
    }
    orders, next_order_id = public_orders(markets, market_ids, max_levels)
    if arrivals:
        arrival_orders, next_order_id = synthetic_batch_arrivals(
            markets, market_ids, max_levels, next_order_id
        )
        orders.extend(arrival_orders)
    mm_constraints: list[list[Any]] = []
    if overlay:
        mm_orders, mm_constraints = synthetic_mm_overlay(
            markets, market_ids, max_levels, next_order_id
        )
        orders.extend(mm_orders)
    market_groups = grouped_market_ids(events, market_ids)
    case_traits = [
        "public-clob-depth",
        "aggregated-anonymous-levels",
        *traits,
    ]
    if overlay:
        case_traits.append("synthetic-mm-overlay")
    if arrivals:
        case_traits.append("synthetic-depth-calibrated-batch-arrivals")
    if market_groups:
        case_traits.append("market-groups")
    return [
        case_id,
        case_traits,
        len(markets),
        list(range(len(markets))),
        orders,
        mm_constraints,
        market_groups,
    ]


def event_markets(
    captured: list[CapturedMarket], event_id: str
) -> list[CapturedMarket]:
    return [market for market in captured if market.event.event_id == event_id]


def build_corpus(
    corpus_id: str,
    selected: list[SelectedEvent],
    captured: list[CapturedMarket],
    max_levels: int,
) -> list[Any]:
    cases = []
    for event in selected:
        markets = event_markets(captured, event.event_id)
        cases.append(
            build_case(
                f"{corpus_id}-{event.bucket}",
                ["event-local", f"category-{event.bucket}"],
                [event],
                markets,
                arrivals=True,
                overlay=True,
                max_levels=max_levels,
            )
        )
    cases.append(
        build_case(
            f"{corpus_id}-portfolio-raw",
            ["cross-event-portfolio", "no-mm-budget"],
            selected,
            captured,
            arrivals=False,
            overlay=False,
            max_levels=max_levels,
        )
    )
    cases.append(
        build_case(
            f"{corpus_id}-portfolio-budgeted",
            ["cross-event-portfolio", "shared-mm-budgets"],
            selected,
            captured,
            arrivals=True,
            overlay=True,
            max_levels=max_levels,
        )
    )
    return [1, corpus_id, "public-polymarket-clob-plus-synthetic-batch", cases]


def manifest_event(
    event: SelectedEvent,
    captured: list[CapturedMarket],
    max_levels: int,
) -> dict[str, Any]:
    markets = []
    for item in event_markets(captured, event.event_id):
        books = []
        for outcome, (token_id, book) in enumerate(zip(item.token_ids, item.books)):
            books.append(
                {
                    "outcome_index": outcome,
                    "token_id": token_id,
                    "book_hash": str(book.get("hash", "")),
                    "book_timestamp": str(book.get("timestamp", "")),
                    "tick_size": str(book.get("tick_size", "")),
                    "bid_levels_source": len(book.get("bids", [])),
                    "ask_levels_source": len(book.get("asks", [])),
                    "bid_levels_retained": len(
                        normalized_levels(book, "bids", max_levels)
                    ),
                    "ask_levels_retained": len(
                        normalized_levels(book, "asks", max_levels)
                    ),
                }
            )
        markets.append(
            {
                "condition_id": str(item.market.get("conditionId", "")),
                "volume_24h": numeric_text(item.market.get("volume24hr")),
                "books": books,
            }
        )
    return {
        "bucket": event.bucket,
        "event_id": event.event_id,
        "title": event.title,
        "volume_24h": event.volume_24h,
        "neg_risk": event.neg_risk,
        "tags": list(event.tags),
        "markets": markets,
    }


def write_atomic(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    temporary.write_bytes(data)
    os.replace(temporary, path)


def main() -> None:
    args = parse_args()
    if args.event_page_limit < len(BUCKETS):
        raise ValueError("--event-page-limit is too small for the category buckets")
    if args.max_markets_per_event < 2:
        raise ValueError("--max-markets-per-event must be at least two")
    if args.max_levels_per_side < 1:
        raise ValueError("--max-levels-per-side must be positive")

    query = urllib.parse.urlencode(
        {
            "active": "true",
            "closed": "false",
            "limit": args.event_page_limit,
            "order": "volume24hr",
            "ascending": "false",
        }
    )
    events = request_json(
        f"{GAMMA_EVENTS_URL}?{query}",
        timeout=args.timeout_seconds,
    )
    if not isinstance(events, list):
        raise RuntimeError("Gamma /events did not return an array")
    selected = select_events(events, args.max_markets_per_event)
    books = fetch_books(selected, args.timeout_seconds)
    captured = capture_markets(selected, books)
    corpus = build_corpus(
        args.corpus_id,
        selected,
        captured,
        args.max_levels_per_side,
    )
    corpus_bytes = msgpack.packb(corpus, use_bin_type=True)
    corpus_hash = blake3.blake3(corpus_bytes).hexdigest()

    captured_at = datetime.now(timezone.utc).replace(microsecond=0).isoformat()
    manifest = {
        "schema_version": 1,
        "corpus_id": args.corpus_id,
        "corpus_blake3": corpus_hash,
        "captured_at_utc": captured_at,
        "evidence_status": (
            "public anonymous aggregated CLOB levels; not Sybil order flow; "
            "batch arrivals, MM identities, and MM budgets are synthetic"
        ),
        "sources": {
            "gamma_events": GAMMA_EVENTS_URL,
            "clob_books": CLOB_BOOKS_URL,
        },
        "selection": {
            "gamma_order": "volume24hr descending",
            "event_page_limit": args.event_page_limit,
            "buckets": [bucket for bucket, _ in BUCKETS],
            "max_markets_per_event": args.max_markets_per_event,
            "complete_neg_risk_events_only": True,
            "max_levels_per_side": args.max_levels_per_side,
        },
        "transformation": {
            "price": "decimal token price rounded to nearest nanodollar",
            "quantity": "decimal shares floored to 0.001-share protocol units",
            "public_depth": (
                "YES/NO token bids and asks become anonymous one-hot buy/sell orders"
            ),
            "synthetic_batch_arrivals": (
                "one directional BuyYes/SellNo or SellYes/BuyNo pair per market "
                "alternates by market and sweeps up to three observed levels plus "
                "half the touch quantity"
            ),
            "synthetic_mm_overlay": (
                "two shared-budget MMs quote each retained token at the public touch "
                "and one tick wider; base budget is total worst-case quote capital"
            ),
            "cases": (
                "one shocked budgeted case per category event, one untouched raw "
                "fragmented portfolio, and one shocked budgeted connected portfolio"
            ),
        },
        "case_count": len(corpus[3]),
        "cases": [
            {
                "case_id": case[0],
                "traits": case[1],
                "markets": len(case[3]),
                "orders": len(case[4]),
                "market_makers": len(case[5]),
                "market_group_sizes": [len(group) for group in case[6]],
            }
            for case in corpus[3]
        ],
        "events": [
            manifest_event(event, captured, args.max_levels_per_side)
            for event in selected
        ],
    }
    manifest_bytes = (
        json.dumps(manifest, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
    ).encode()

    write_atomic(args.output, corpus_bytes)
    write_atomic(args.manifest, manifest_bytes)
    print(
        f"wrote {len(corpus[3])} cases, {len(captured)} markets, "
        f"{len(corpus_bytes)} bytes, blake3={corpus_hash}"
    )


if __name__ == "__main__":
    main()
