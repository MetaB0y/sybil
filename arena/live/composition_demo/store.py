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

from .registry import (
    Formula,
    canonical_key_for,
    estimate_formula_value,
    formula_conditions,
    search_instruments,
    validate_formula,
)
from .sources import import_universe

NANOS_PER_DOLLAR = 1_000_000_000
DEFAULT_STATE_PATH = Path(__file__).resolve().parent / "state.json"
DEFAULT_SYBIL_URL = os.environ.get("SYBIL_API_URL", "http://localhost:3001")
DEFAULT_MAX_ATOMS = int(os.environ.get("COMPOSITION_DEMO_CONDITIONS", "110"))


def load_state(path: str | Path = DEFAULT_STATE_PATH) -> dict[str, Any]:
    p = Path(path)
    if not p.exists():
        universe = import_universe(max_atoms=DEFAULT_MAX_ATOMS)
        return {
            "created_at": time.time(),
            "updated_at": time.time(),
            "universe_version": universe.get("universe_version", 4),
            "feeds": universe.get("feeds", []),
            "entities": universe.get("entities", []),
            "contexts": universe.get("contexts", []),
            "measurements": universe.get("measurements", []),
            "conditions": universe.get("conditions", []),
            "propositions": universe.get("propositions", []),
            "markets": universe.get("markets", []),
            "implication_edges": universe.get("implication_edges", []),
            "instruments": universe.get("instruments", []),
            "accounts": {},
            "events": [],
            "source_counts": universe.get("source_counts", {}),
            "source_errors": universe.get("source_errors", []),
        }
    with p.open("r", encoding="utf-8") as f:
        state = json.load(f)
    if state.get("universe_version") != 4 or len(state.get("measurements", [])) < 50 or not state.get("entities"):
        universe = import_universe(max_atoms=DEFAULT_MAX_ATOMS, force=True)
        market_ids = {item["id"]: item.get("market_id") for item in state.get("instruments", [])}
        for item in universe.get("instruments", []):
            if market_ids.get(item["id"]) is not None:
                item["market_id"] = market_ids[item["id"]]
        state.update(
            {
                "universe_version": 4,
                "feeds": universe.get("feeds", []),
                "entities": universe.get("entities", []),
                "contexts": universe.get("contexts", []),
                "measurements": universe.get("measurements", []),
                "conditions": universe.get("conditions", []),
                "propositions": universe.get("propositions", []),
                "markets": universe.get("markets", []),
                "implication_edges": universe.get("implication_edges", []),
                "instruments": universe.get("instruments", []),
                "source_counts": universe.get("source_counts", {}),
                "source_errors": universe.get("source_errors", []),
            }
        )
        save_state(state, path)
    return state


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


def import_sources(force: bool = False, max_atoms: int = DEFAULT_MAX_ATOMS) -> dict[str, Any]:
    universe = import_universe(max_atoms=max_atoms, force=force)
    state = load_state()
    market_ids = {item["id"]: item.get("market_id") for item in state.get("instruments", [])}
    instruments = universe.get("instruments", [])
    for item in instruments:
        if market_ids.get(item["id"]) is not None:
            item["market_id"] = market_ids[item["id"]]
    state["universe_version"] = universe.get("universe_version", 4)
    state["feeds"] = universe.get("feeds", [])
    state["entities"] = universe.get("entities", [])
    state["contexts"] = universe.get("contexts", [])
    state["measurements"] = universe.get("measurements", [])
    state["conditions"] = universe.get("conditions", [])
    state["propositions"] = universe.get("propositions", [])
    state["markets"] = universe.get("markets", [])
    state["implication_edges"] = universe.get("implication_edges", [])
    state["instruments"] = instruments
    state["source_counts"] = universe.get("source_counts", {})
    state["source_errors"] = universe.get("source_errors", [])
    save_state(state)
    return state


def add_instrument(instrument: dict[str, Any], sybil_url: str = DEFAULT_SYBIL_URL) -> dict[str, Any]:
    state = load_state()
    validation = validate_formula(instrument.get("formula"), state["instruments"], state.get("implication_edges", []))
    if not validation["valid"]:
        raise ValueError("; ".join(validation["errors"]))
    existing_ids = {item["id"] for item in state["instruments"]}
    base_id = slugify(instrument.get("id") or instrument.get("short_name") or instrument["title"])
    candidate = base_id
    idx = 2
    while candidate in existing_ids:
        candidate = f"{base_id}_{idx}"
        idx += 1
    instrument["id"] = candidate
    instrument.setdefault("kind", "proposition")
    instrument.setdefault("object_kind", "proposition")
    instrument.setdefault("author", "User draft")
    instrument.setdefault("trust_tier", "demo-draft")
    instrument.setdefault("tags", ["composition-demo", "user-draft"])
    instrument.setdefault("oracle_path", "Formula over graph conditions")
    instrument.setdefault("domain", "custom")
    instrument.setdefault("atom_type", "composition")
    instrument.setdefault("subject", instrument.get("short_name", instrument["title"]))
    instrument.setdefault("metric", "formula")
    instrument.setdefault("comparator", "resolves_true")
    instrument.setdefault("threshold", None)
    instrument.setdefault("unit", "")
    instrument.setdefault("time_window", "user-defined")
    instrument.setdefault("resolver_primitive", "predicate_formula")
    instrument.setdefault("source", "user")
    instrument.setdefault("source_url", "")
    instrument.setdefault("canonical_key", validation.get("canonical_key") or canonical_key_for(instrument))
    instrument.setdefault("compatible_ops", ["AND", "OR", "NOT", "K_OF_N", "IF_THEN"])
    instrument.setdefault("exclusivity_group", None)
    instrument.setdefault("template_id", "proposition")
    instrument.setdefault("params", {})
    instrument.setdefault("quality", "user_draft")
    instrument.setdefault("aliases", [])
    values = {item["id"]: float(item.get("fair_value", 0.5)) for item in state["instruments"]}
    instrument["leaf_ids"] = formula_conditions(instrument.get("formula"))
    instrument["fair_value"] = estimate_formula_value(instrument.get("formula"), values)
    instrument["market_id"] = create_market(sybil_url, instrument)
    state["instruments"].append(instrument)
    state.setdefault("propositions", []).append(instrument)
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
    measurements_by_id = {item["id"]: item for item in state.get("measurements", [])}
    entities_by_id = {item["id"]: item for item in state.get("entities", [])}
    contexts_by_id = {item["id"]: item for item in state.get("contexts", [])}
    enriched = []
    for item in state["instruments"]:
        row = dict(item)
        row.setdefault("object_kind", row.get("kind"))
        mid = row.get("market_id")
        market = markets_by_id.get(int(mid)) if mid is not None else None
        if market:
            row["market"] = market
            if market.get("yes_price_nanos"):
                row["last_price"] = market["yes_price_nanos"] / NANOS_PER_DOLLAR
        if row.get("measurement_id") and row["measurement_id"] in measurements_by_id:
            row["measurement"] = measurements_by_id[row["measurement_id"]]
            row["measurement_kind"] = measurements_by_id[row["measurement_id"]].get("measurement_kind")
            row["entity_ids"] = measurements_by_id[row["measurement_id"]].get("entity_ids", [])
            row["context_id"] = measurements_by_id[row["measurement_id"]].get("context_id", "")
            row["path"] = measurements_by_id[row["measurement_id"]].get("path", [])
            row["context"] = contexts_by_id.get(row.get("context_id", ""))
            row["entities"] = [entities_by_id[entity_id] for entity_id in row.get("entity_ids", []) if entity_id in entities_by_id]
        if row["kind"] in {"proposition", "composition"}:
            row["leaf_ids"] = formula_conditions(row.get("formula"))
            row["model_value"] = estimate_formula_value(row.get("formula"), values)
        else:
            row["leaf_ids"] = []
            row["model_value"] = row.get("fair_value", 0.5)
        enriched.append(row)

    out = dict(state)
    out["instruments"] = enriched
    out["conditions"] = [item for item in enriched if item.get("object_kind") == "condition"]
    out["propositions"] = [item for item in enriched if item.get("object_kind") == "proposition"]
    out["markets"] = [
        {
            "instrument_id": item["id"],
            "market_id": item.get("market_id"),
            "kind": item.get("kind"),
            "question": item.get("question"),
        }
        for item in enriched
        if item.get("market_id") is not None
    ]
    out["entities"] = state.get("entities", [])
    out["contexts"] = state.get("contexts", [])
    out["sybil_url"] = sybil_url
    out["facets"] = search_instruments(enriched, limit=0)["facets"]
    out["instrument_counts"] = {
        "atoms": len([item for item in enriched if item["kind"] == "condition"]),
        "conditions": len([item for item in enriched if item["kind"] == "condition"]),
        "compositions": len([item for item in enriched if item["kind"] == "proposition"]),
        "propositions": len([item for item in enriched if item["kind"] == "proposition"]),
        "measurements": len(out.get("measurements", [])),
        "entities": len(out.get("entities", [])),
        "contexts": len(out.get("contexts", [])),
        "feeds": len(out.get("feeds", [])),
        "seeded": len([item for item in enriched if item.get("market_id") is not None]),
        "quoted": len([item for item in enriched if item.get("market_id") is not None])
        and int(state.get("last_quote", {}).get("markets_quoted", 0)),
    }
    return out


def explorer_search(payload: dict[str, Any], sybil_url: str = DEFAULT_SYBIL_URL) -> dict[str, Any]:
    state = enrich_state(load_state(), sybil_url)
    rows = [*state.get("entities", []), *state.get("contexts", []), *state.get("measurements", []), *state["instruments"]]
    return search_instruments(
        rows,
        query=str(payload.get("query", "")),
        domain=str(payload.get("domain", "")),
        atom_type=str(payload.get("atom_type", "")),
        source=str(payload.get("source", "")),
        kind=str(payload.get("kind", "")),
        template_id=str(payload.get("template_id", "")),
        quality=str(payload.get("quality", "")),
        resolver_primitive=str(payload.get("resolver_primitive", "")),
        object_kind=str(payload.get("object_kind", "")),
        measurement_kind=str(payload.get("measurement_kind", "")),
        measurement_id=str(payload.get("measurement_id", "")),
        predicate_op=str(payload.get("predicate_op", "")),
        limit=int(payload.get("limit", 80)),
    )


def validate_formula_payload(payload: dict[str, Any]) -> dict[str, Any]:
    state = load_state()
    return validate_formula(payload.get("formula"), state["instruments"], state.get("implication_edges", []))


def create_wizard_draft(payload: dict[str, Any]) -> dict[str, Any]:
    state = load_state()
    prompt = str(payload.get("prompt") or payload.get("intent") or "")
    formula = payload.get("formula") or draft_formula_from_prompt(prompt, state)
    title = payload.get("title") or draft_title_from_prompt(prompt, formula, state)
    draft = {
        "draft_id": f"draft_{int(time.time() * 1000)}",
        "title": title,
        "short_name": payload.get("short_name") or title[:34],
        "question": payload.get("question") or f"Will '{title}' resolve YES?",
        "description": payload.get("description") or "Unpublished market-creation wizard draft.",
        "domain": payload.get("domain") or infer_formula_domain(formula, state),
        "formula": formula,
        "operations": [],
    }
    state.setdefault("drafts", {})[draft["draft_id"]] = draft
    save_state(state)
    return enrich_draft(draft, state)


def edit_wizard_draft(payload: dict[str, Any]) -> dict[str, Any]:
    state = load_state()
    draft = dict(state.setdefault("drafts", {}).get(payload.get("draft_id")) or payload.get("draft") or {})
    if not draft:
        draft = create_wizard_draft(payload)
        state = load_state()
    op = str(payload.get("operation") or payload.get("op") or "").lower()
    formula = draft.get("formula")
    if op == "add_condition":
        condition_id = str(payload["condition_id"])
        formula = add_condition_to_formula(formula, condition_id, str(payload.get("operator") or "AND").upper())
    elif op == "remove_condition":
        formula = remove_condition_from_formula(formula, str(payload["condition_id"]))
    elif op == "replace_condition":
        formula = replace_condition_in_formula(formula, str(payload["from_condition_id"]), str(payload["to_condition_id"]))
    elif op == "wrap":
        formula = {"op": str(payload.get("operator") or "AND").upper(), "args": formula_args(formula)}
        if formula["op"] == "K_OF_N":
            formula["k"] = int(payload.get("k") or min(2, len(formula["args"])))
    elif op == "change_k":
        if isinstance(formula, dict) and str(formula.get("op", "")).upper() == "K_OF_N":
            formula["k"] = int(payload.get("k", formula.get("k", 1)))
    elif op == "set_formula":
        formula = payload.get("formula")
    else:
        raise ValueError(f"unsupported wizard operation: {op}")
    draft["formula"] = formula
    draft.setdefault("operations", []).append({key: value for key, value in payload.items() if key != "sybil_url"})
    state.setdefault("drafts", {})[draft["draft_id"]] = draft
    save_state(state)
    return enrich_draft(draft, state)


def validate_wizard_draft(payload: dict[str, Any]) -> dict[str, Any]:
    state = load_state()
    draft = state.get("drafts", {}).get(payload.get("draft_id")) or payload.get("draft") or payload
    return enrich_draft(draft, state)


def publish_wizard_draft(payload: dict[str, Any], sybil_url: str = DEFAULT_SYBIL_URL) -> dict[str, Any]:
    state = load_state()
    draft = dict(state.get("drafts", {}).get(payload.get("draft_id")) or payload.get("draft") or {})
    if not draft:
        raise ValueError("draft not found")
    validation = validate_formula(draft.get("formula"), state["instruments"], state.get("implication_edges", []))
    if not validation["valid"]:
        raise ValueError("; ".join(validation["errors"]))
    instrument = {
        "id": draft.get("id", ""),
        "kind": "proposition",
        "object_kind": "proposition",
        "title": draft["title"],
        "short_name": draft.get("short_name") or draft["title"][:34],
        "question": draft.get("question") or f"Will {draft['title']} resolve YES?",
        "description": draft.get("description") or "Published wizard proposition.",
        "formula": draft["formula"],
        "domain": draft.get("domain") or infer_formula_domain(draft.get("formula"), state),
        "quality": "wizard_published",
        "source": "wizard",
    }
    next_state = add_instrument(instrument, sybil_url)
    state = load_state()
    state.setdefault("drafts", {}).pop(draft.get("draft_id", ""), None)
    save_state(state)
    return next_state


def enrich_draft(draft: dict[str, Any], state: dict[str, Any]) -> dict[str, Any]:
    validation = validate_formula(draft.get("formula"), state["instruments"], state.get("implication_edges", []))
    by_id = {item["id"]: item for item in state["instruments"]}
    refs = validation.get("referenced_ids", [])
    related_edges = [
        edge
        for edge in state.get("implication_edges", [])
        if edge.get("from") in refs or edge.get("to") in refs
    ]
    return {
        **draft,
        "validation": validation,
        "referenced_conditions": [by_id[ref] for ref in refs if ref in by_id],
        "implication_edges": related_edges,
    }


def draft_formula_from_prompt(prompt: str, state: dict[str, Any]) -> Formula:
    lower = prompt.lower()
    by_short = {item["short_name"].lower(): item for item in state["instruments"] if item.get("kind") == "condition"}

    def pick(*needles: str) -> dict[str, str] | None:
        for item in state["instruments"]:
            if item.get("kind") != "condition":
                continue
            text = f"{item.get('short_name', '')} {item.get('question', '')}".lower()
            if all(needle in text for needle in needles):
                return {"condition": item["id"]}
        return None

    if "3000" in lower and "6000" in lower and ("between" in lower or "range" in lower):
        return {"condition": by_short["3000 < eth < 6000"]["id"]}
    if "eth" in lower and "6000" in lower:
        return {"condition": by_short["eth > 6000"]["id"]}
    if "eth" in lower and "btc" in lower:
        return {"op": "AND", "args": [by_short_leaf(by_short, "eth > 3000"), by_short_leaf(by_short, "btc > 100k")]}
    if "recession" in lower:
        return {
            "op": "K_OF_N",
            "k": 2,
            "args": [
                by_short_leaf(by_short, "gdp q1 < 0"),
                by_short_leaf(by_short, "gdp q2 < 0"),
                by_short_leaf(by_short, "gdp q3 < 0"),
                by_short_leaf(by_short, "gdp q4 < 0"),
            ],
        }
    if "iran" in lower or "invasion" in lower:
        return {"op": "OR", "args": [by_short_leaf(by_short, "iran troops > 0"), by_short_leaf(by_short, "iran strikes > 50")]}
    if "nba" in lower or "parlay" in lower:
        return {
            "op": "AND",
            "args": [
                by_short_leaf(by_short, "celtics win"),
                by_short_leaf(by_short, "tatum points > 29.5"),
                by_short_leaf(by_short, "brown rebounds > 6.5"),
            ],
        }
    found = pick(*[token for token in prompt.lower().split()[:2] if len(token) > 2])
    if found:
        return found
    condition = next(item for item in state["instruments"] if item.get("kind") == "condition")
    return {"condition": condition["id"]}


def by_short_leaf(by_short: dict[str, dict[str, Any]], short: str) -> dict[str, str]:
    return {"condition": by_short[short]["id"]}


def draft_title_from_prompt(prompt: str, formula: Formula, state: dict[str, Any]) -> str:
    if prompt:
        return prompt[:80]
    refs = formula_conditions(formula)
    by_id = {item["id"]: item for item in state["instruments"]}
    names = [by_id[ref]["short_name"] for ref in refs if ref in by_id]
    return " + ".join(names[:3]) or "Custom proposition"


def infer_formula_domain(formula: Formula | None, state: dict[str, Any]) -> str:
    refs = formula_conditions(formula)
    by_id = {item["id"]: item for item in state["instruments"]}
    domains = [by_id[ref].get("domain") for ref in refs if ref in by_id]
    return str(domains[0] if domains else "custom")


def formula_args(formula: Formula | None) -> list[Formula]:
    if not isinstance(formula, dict):
        return []
    if "condition" in formula or "atom" in formula:
        return [formula]
    return list(formula.get("args", []))


def add_condition_to_formula(formula: Formula | None, condition_id: str, operator: str) -> Formula:
    leaf = {"condition": condition_id}
    if not formula:
        return leaf
    if isinstance(formula, dict) and str(formula.get("op", "")).upper() == operator and operator in {"AND", "OR"}:
        return {**formula, "args": [*formula.get("args", []), leaf]}
    return {"op": operator if operator in {"AND", "OR"} else "AND", "args": [formula, leaf]}


def remove_condition_from_formula(formula: Formula | None, condition_id: str) -> Formula | None:
    if not isinstance(formula, dict):
        return formula
    if str(formula.get("condition") or formula.get("atom")) == condition_id:
        return None
    args = [remove_condition_from_formula(arg, condition_id) for arg in formula.get("args", [])]
    args = [arg for arg in args if arg]
    if "op" in formula:
        if len(args) == 1 and formula.get("op") != "NOT":
            return args[0]
        return {**formula, "args": args}
    return formula


def replace_condition_in_formula(formula: Formula | None, old: str, new: str) -> Formula | None:
    if not isinstance(formula, dict):
        return formula
    if str(formula.get("condition") or formula.get("atom")) == old:
        return {"condition": new}
    if "args" in formula:
        return {**formula, "args": [replace_condition_in_formula(arg, old, new) for arg in formula.get("args", [])]}
    return formula


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
    markets_quoted = 0
    tick = int(state.get("quote_tick", 0))
    for item in state["instruments"]:
        mid = item.get("market_id")
        if mid is None:
            continue
        market = markets_by_id.get(int(mid))
        if not market or market.get("status", "").lower() != "active":
            continue
        markets_quoted += 1
        fair = float(item.get("fair_value", item.get("model_value", 0.5)))
        if item["kind"] in {"proposition", "composition"}:
            values = {x["id"]: float(x.get("fair_value", 0.5)) for x in state["instruments"]}
            fair = estimate_formula_value(item.get("formula"), values)
            item["fair_value"] = fair
        spread = 0.035 if item["kind"] in {"condition", "atom"} else 0.05
        bid = max(0.01, fair - spread)
        ask = min(0.99, fair + spread)
        qty = 60 if item["kind"] in {"condition", "atom"} else 35
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
    result = {
        "orders": len(orders),
        "taker_orders": min(len(taker_orders), 8),
        "markets_quoted": markets_quoted,
        "mm_account_id": accounts.get("mm"),
        "noise_account_id": accounts.get("noise"),
    }
    state["last_quote"] = dict(result, timestamp=time.time())
    save_state(state)
    return result


def trigger_event(event: str, sybil_url: str = DEFAULT_SYBIL_URL) -> dict[str, Any]:
    state = load_state()
    state.setdefault("events", []).append({"event": event, "timestamp": time.time()})
    if event == "helicopter":
        updates = {
            "Iran troops > 0": 0.96,
            "Iran troops > 1k": 0.08,
            "Iran 72h presence": 0.06,
            "Iran war declared": 0.03,
            "Iran AUMF": 0.06,
            "Iran strikes > 50": 0.18,
            "Iran occupation": 0.02,
        }
        for item in state["instruments"]:
            if item.get("short_name") in updates:
                item["fair_value"] = updates[item["short_name"]]
            if item.get("short_name") == "Iran troops > 0" and item.get("market_id") is not None:
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
