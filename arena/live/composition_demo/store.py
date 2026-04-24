"""Persistence and Sybil API helpers for the composition demo."""

from __future__ import annotations

import json
import os
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

from .registry import SEED_INSTRUMENTS, Instrument, estimate_formula_value, formula_atoms

NANOS_PER_DOLLAR = 1_000_000_000
DEFAULT_STATE_PATH = Path(__file__).resolve().parent / "state.json"
DEFAULT_SYBIL_URL = os.environ.get("SYBIL_API_URL", "http://localhost:3001")


def load_state(path: str | Path = DEFAULT_STATE_PATH) -> dict[str, Any]:
    p = Path(path)
    if not p.exists():
        return {
            "created_at": time.time(),
            "updated_at": time.time(),
            "instruments": [item.to_dict() for item in SEED_INSTRUMENTS],
            "accounts": {},
            "events": [],
        }
    with p.open("r", encoding="utf-8") as f:
        return json.load(f)


def save_state(state: dict[str, Any], path: str | Path = DEFAULT_STATE_PATH) -> None:
    state["updated_at"] = time.time()
    p = Path(path)
    p.parent.mkdir(parents=True, exist_ok=True)
    tmp = p.with_suffix(".tmp")
    with tmp.open("w", encoding="utf-8") as f:
        json.dump(state, f, indent=2, sort_keys=True)
    tmp.replace(p)


def request_json(
    method: str,
    url: str,
    payload: dict[str, Any] | None = None,
    timeout: float = 20.0,
) -> Any:
    data = None if payload is None else json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=data,
        method=method,
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            body = resp.read().decode("utf-8")
            return json.loads(body) if body else {}
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"{method} {url} failed: HTTP {e.code}: {body}") from e


def create_market(sybil_url: str, instrument: dict[str, Any]) -> int:
    payload = {
        "name": instrument["question"],
        "description": build_description(instrument),
        "category": "composition-demo",
        "tags": instrument.get("tags", ["composition-demo", "iran"]),
        "resolution_criteria": instrument.get("oracle_path", ""),
    }
    data = request_json("POST", f"{sybil_url.rstrip('/')}/v1/markets", payload)
    return int(data["market_id"])


def build_description(instrument: dict[str, Any]) -> str:
    lines = [
        instrument.get("description", ""),
        "",
        f"Demo instrument id: {instrument['id']}",
        f"Kind: {instrument['kind']}",
    ]
    if instrument.get("formula"):
        lines.append(f"Formula: {json.dumps(instrument['formula'], sort_keys=True)}")
    return "\n".join(line for line in lines if line is not None)


def seed_markets(sybil_url: str = DEFAULT_SYBIL_URL) -> dict[str, Any]:
    state = load_state()
    existing = get_markets_by_id(sybil_url)
    changed = False
    for instrument in state["instruments"]:
        mid = instrument.get("market_id")
        if mid is None or int(mid) not in existing:
            instrument["market_id"] = create_market(sybil_url, instrument)
            changed = True
    if changed:
        save_state(state)
    return enrich_state(state, sybil_url)


def add_instrument(instrument: dict[str, Any], sybil_url: str = DEFAULT_SYBIL_URL) -> dict[str, Any]:
    state = load_state()
    existing_ids = {item["id"] for item in state["instruments"]}
    base_id = slugify(instrument.get("id") or instrument.get("short_name") or instrument["title"])
    candidate = base_id
    idx = 2
    while candidate in existing_ids:
        candidate = f"{base_id}_{idx}"
        idx += 1
    instrument["id"] = candidate
    instrument.setdefault("kind", "composition")
    instrument.setdefault("author", "User draft")
    instrument.setdefault("trust_tier", "demo-draft")
    instrument.setdefault("tags", ["composition-demo", "iran", "user-draft"])
    instrument.setdefault("oracle_path", "Composition over demo atoms")
    instrument["market_id"] = create_market(sybil_url, instrument)
    state["instruments"].append(instrument)
    save_state(state)
    return enrich_state(state, sybil_url)


def enrich_state(state: dict[str, Any], sybil_url: str = DEFAULT_SYBIL_URL) -> dict[str, Any]:
    try:
        markets_by_id = get_markets_by_id(sybil_url)
    except Exception as e:  # Keep the UI usable if sybil-api is down.
        markets_by_id = {}
        state = dict(state)
        state["sybil_error"] = str(e)

    values = {item["id"]: float(item.get("fair_value", 0.5)) for item in state["instruments"]}
    enriched = []
    for item in state["instruments"]:
        row = dict(item)
        mid = row.get("market_id")
        market = markets_by_id.get(int(mid)) if mid is not None else None
        if market:
            row["market"] = market
            if market.get("yes_price_nanos"):
                row["last_price"] = market["yes_price_nanos"] / NANOS_PER_DOLLAR
        if row["kind"] == "composition":
            row["leaf_ids"] = formula_atoms(row.get("formula"))
            row["model_value"] = estimate_formula_value(row.get("formula"), values)
        else:
            row["leaf_ids"] = []
            row["model_value"] = row.get("fair_value", 0.5)
        enriched.append(row)

    out = dict(state)
    out["instruments"] = enriched
    out["sybil_url"] = sybil_url
    return out


def get_markets_by_id(sybil_url: str) -> dict[int, dict[str, Any]]:
    markets = request_json("GET", f"{sybil_url.rstrip('/')}/v1/markets")
    return {int(m["market_id"]): m for m in markets}


def create_account(sybil_url: str, dollars: float = 500.0) -> int:
    data = request_json(
        "POST",
        f"{sybil_url.rstrip('/')}/v1/accounts",
        {"initial_balance_nanos": int(dollars * NANOS_PER_DOLLAR)},
    )
    return int(data["account_id"])


def ensure_account(sybil_url: str, account_id: int | None, dollars: float) -> int:
    if account_id is not None:
        try:
            request_json("GET", f"{sybil_url.rstrip('/')}/v1/accounts/{account_id}")
            return account_id
        except Exception:
            pass
    return create_account(sybil_url, dollars)


def submit_order(
    sybil_url: str,
    account_id: int,
    market_id: int,
    side: str,
    price: float,
    quantity: int,
) -> dict[str, Any]:
    order_type = {
        "BUY_YES": "BuyYes",
        "BUY_NO": "BuyNo",
        "SELL_YES": "SellYes",
        "SELL_NO": "SellNo",
    }.get(side.upper(), side)
    payload = {
        "account_id": account_id,
        "orders": [
            {
                "type": order_type,
                "market_id": market_id,
                "limit_price_nanos": int(price * NANOS_PER_DOLLAR),
                "quantity": int(quantity),
            }
        ],
    }
    return request_json("POST", f"{sybil_url.rstrip('/')}/v1/orders", payload)


def quote_once(sybil_url: str = DEFAULT_SYBIL_URL) -> dict[str, Any]:
    state = load_state()
    markets_by_id = get_markets_by_id(sybil_url)
    accounts = state.setdefault("accounts", {})
    accounts["mm"] = ensure_account(sybil_url, accounts.get("mm"), 50_000.0)
    accounts["noise"] = ensure_account(sybil_url, accounts.get("noise"), 2_000.0)
    save_state(state)

    orders = []
    taker_orders = []
    tick = int(state.get("quote_tick", 0))
    for item in state["instruments"]:
        mid = item.get("market_id")
        if mid is None:
            continue
        market = markets_by_id.get(int(mid))
        if not market or market.get("status", "").lower() != "active":
            continue
        fair = float(item.get("fair_value", item.get("model_value", 0.5)))
        if item["kind"] == "composition":
            values = {x["id"]: float(x.get("fair_value", 0.5)) for x in state["instruments"]}
            fair = estimate_formula_value(item.get("formula"), values)
            item["fair_value"] = fair
        spread = 0.035 if item["kind"] == "atom" else 0.05
        bid = max(0.01, fair - spread)
        ask = min(0.99, fair + spread)
        qty = 60 if item["kind"] == "atom" else 35
        if (len(taker_orders) + tick) % 3 == 0:
            taker_orders.append(
                {
                    "type": "BuyYes",
                    "market_id": int(mid),
                    "limit_price_nanos": int(min(0.99, ask + 0.04) * NANOS_PER_DOLLAR),
                    "quantity": 3,
                }
            )
        elif (len(taker_orders) + tick) % 3 == 1:
            taker_orders.append(
                {
                    "type": "BuyNo",
                    "market_id": int(mid),
                    "limit_price_nanos": int(min(0.99, 1.0 - bid + 0.04) * NANOS_PER_DOLLAR),
                    "quantity": 3,
                }
            )
        orders.append(
            {
                "type": "BuyYes",
                "market_id": int(mid),
                "limit_price_nanos": int(bid * NANOS_PER_DOLLAR),
                "quantity": qty,
            }
        )
        orders.append(
            {
                "type": "BuyNo",
                "market_id": int(mid),
                "limit_price_nanos": int((1.0 - ask) * NANOS_PER_DOLLAR),
                "quantity": qty,
            }
        )

    if taker_orders:
        request_json(
            "POST",
            f"{sybil_url.rstrip('/')}/v1/orders",
            {
                "account_id": accounts["noise"],
                "orders": taker_orders[:8],
            },
        )
    if orders:
        request_json(
            "POST",
            f"{sybil_url.rstrip('/')}/v1/orders",
            {
                "account_id": accounts["mm"],
                "orders": orders,
                "mm_budget_nanos": int(5_000 * NANOS_PER_DOLLAR),
            },
        )
    state["quote_tick"] = tick + 1
    save_state(state)
    return {
        "orders": len(orders),
        "taker_orders": min(len(taker_orders), 8),
        "mm_account_id": accounts.get("mm"),
        "noise_account_id": accounts.get("noise"),
    }


def trigger_event(event: str, sybil_url: str = DEFAULT_SYBIL_URL) -> dict[str, Any]:
    state = load_state()
    state.setdefault("events", []).append({"event": event, "timestamp": time.time()})
    if event == "helicopter":
        updates = {
            "troops_soil_1": 0.96,
            "troops_soil_1000": 0.08,
            "troops_duration_72h": 0.06,
            "formal_declaration": 0.03,
            "aumf_passed": 0.06,
            "strikes_50": 0.18,
            "strikes_7d": 0.12,
            "occupation_declared": 0.02,
        }
        for item in state["instruments"]:
            if item["id"] in updates:
                item["fair_value"] = updates[item["id"]]
            if item["id"] == "troops_soil_1" and item.get("market_id") is not None:
                try:
                    request_json(
                        "POST",
                        f"{sybil_url.rstrip('/')}/v1/markets/{item['market_id']}/resolve",
                        {"payout_nanos": NANOS_PER_DOLLAR},
                    )
                except Exception:
                    pass
    save_state(state)
    return quote_once(sybil_url)


def slugify(value: str) -> str:
    out = []
    last_underscore = False
    for ch in value.lower():
        if ch.isalnum():
            out.append(ch)
            last_underscore = False
        elif not last_underscore:
            out.append("_")
            last_underscore = True
    return "".join(out).strip("_")[:64] or "composition"
