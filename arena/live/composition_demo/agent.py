"""Agent logic for composition discovery, trade proposals, and draft creation."""

from __future__ import annotations

import json
import os
from typing import Any

try:
    from openai import OpenAI
except ImportError:  # pragma: no cover - optional unless OPENROUTER_API_KEY is set.
    OpenAI = None  # type: ignore[assignment]

from .registry import formula_atoms, formula_to_text, search_instruments, validate_formula
from .store import NANOS_PER_DOLLAR

MODEL = "deepseek/deepseek-v4-flash"


def discover(query: str, state: dict[str, Any]) -> dict[str, Any]:
    result = search_instruments(state["instruments"], query=query, limit=12)
    ranked = result["items"]
    recommendation = ranked[0] if ranked else None

    visible = ranked[:8]

    return {
        "answer": build_discovery_answer(query, recommendation, ranked[:5]),
        "recommendation_id": recommendation["id"] if recommendation else None,
        "ranked_ids": [item["id"] for item in visible],
        "actions": [
            "Open the Explorer filters to inspect the atom universe.",
            "Draft a composition from the top atoms if no existing market matches.",
            "Check source and resolver primitive before approving a created market.",
        ],
    }


def build_discovery_answer(query: str, recommendation: dict[str, Any] | None, ranked: list[dict[str, Any]]) -> str:
    if not recommendation:
        return "I could not find a matching composition. Draft a new one from the creation panel."
    names = ", ".join(item["short_name"] for item in ranked)
    return (
        f"For '{query}', I would start with {recommendation['short_name']} "
        f"({recommendation.get('domain', 'unknown')}/{recommendation.get('atom_type', recommendation['kind'])}). "
        f"Nearby candidates: {names}. Use these as leaves for a new formula if no single instrument matches."
    )


def propose_trade(payload: dict[str, Any], state: dict[str, Any]) -> dict[str, Any]:
    instrument = find_instrument(state, payload.get("instrument_id"))
    side_hint = str(payload.get("side") or payload.get("intent") or "").lower()
    side = "BUY_NO" if any(word in side_hint for word in ["no", "against", "short"]) else "BUY_YES"
    market = instrument.get("market") or {}
    yes_price = (market.get("yes_price_nanos") or int(instrument.get("fair_value", 0.5) * NANOS_PER_DOLLAR)) / NANOS_PER_DOLLAR
    limit = 1.0 - yes_price if side == "BUY_NO" else yes_price
    limit = min(0.99, max(0.01, limit + 0.02))
    quantity = int(payload.get("quantity") or 25)
    return {
        "instrument_id": instrument["id"],
        "market_id": instrument.get("market_id"),
        "side": side,
        "limit_price": round(limit, 4),
        "quantity": quantity,
        "notional": round(quantity * limit, 2),
        "rationale": (
            f"{instrument['short_name']} is the cleanest target for this thesis. "
            "This is a proposal only; confirm in the trade ticket to submit."
        ),
    }


def explain_instrument(instrument_id: str, state: dict[str, Any]) -> dict[str, Any]:
    item = find_instrument(state, instrument_id)
    leaves = [find_instrument(state, leaf) for leaf in formula_atoms(item.get("formula"))]
    return {
        "instrument_id": item["id"],
        "summary": item["description"],
        "formula_text": formula_to_text(item.get("formula")),
        "leaves": [
            {
                "id": leaf["id"],
                "short_name": leaf["short_name"],
                "oracle_path": leaf["oracle_path"],
                "fair_value": leaf.get("fair_value"),
            }
            for leaf in leaves
        ],
    }


def draft_composition(prompt: str, state: dict[str, Any]) -> dict[str, Any]:
    if os.environ.get("OPENROUTER_API_KEY"):
        drafted = draft_with_llm(prompt, state)
        if drafted:
            validation = validate_formula(drafted.get("formula"), state["instruments"])
            if validation["valid"]:
                return drafted
    return deterministic_draft(prompt, state)


def draft_with_llm(prompt: str, state: dict[str, Any]) -> dict[str, Any] | None:
    if OpenAI is None:
        return None
    domain = infer_domain(prompt)
    atoms = [
        {
            "id": item["id"],
            "short_name": item["short_name"],
            "description": item["description"],
            "domain": item.get("domain"),
            "atom_type": item.get("atom_type"),
            "source": item.get("source"),
        }
        for item in search_instruments(
            state["instruments"],
            query=expand_prompt(prompt),
            domain=domain,
            kind="atom",
            limit=80,
        )["items"]
    ]
    system = (
        "You draft prediction-market compositions. Return strict JSON only with keys: "
        "title, short_name, question, description, formula. Formula uses {'atom': id} "
        "or {'op':'AND'|'OR'|'NOT','args':[...]} and may only reference provided atom ids."
    )
    user = f"Available atoms:\n{json.dumps(atoms)}\n\nUser request:\n{prompt}"
    try:
        client = OpenAI(
            base_url="https://openrouter.ai/api/v1",
            api_key=os.environ["OPENROUTER_API_KEY"],
            timeout=45.0,
            max_retries=0,
        )
        resp = client.chat.completions.create(
            model=MODEL,
            messages=[{"role": "system", "content": system}, {"role": "user", "content": user}],
            temperature=0.2,
            max_tokens=1200,
            extra_body={"reasoning": {"max_tokens": 512}},
        )
        text = resp.choices[0].message.content or ""
        start = text.find("{")
        end = text.rfind("}")
        if start < 0 or end < start:
            return None
        data = json.loads(text[start : end + 1])
        return normalize_draft(data)
    except Exception:
        return None


def deterministic_draft(prompt: str, state: dict[str, Any]) -> dict[str, Any]:
    lower = prompt.lower()
    domain = infer_domain(prompt)
    atoms = search_instruments(
        state["instruments"],
        query=expand_prompt(prompt),
        domain=domain,
        kind="atom",
        limit=24,
    )["items"]
    atoms = pick_diverse_atoms(prompt, atoms)[:6]
    if len(atoms) < 2:
        atoms = [
            item
            for item in state["instruments"]
            if item["kind"] == "atom" and (not domain or item.get("domain") == domain)
        ][:6]
    if len(atoms) < 2:
        atoms = [item for item in state["instruments"] if item["kind"] == "atom"][:6]
    if not atoms:
        raise ValueError("no atoms available to draft from")

    args = [{"atom": item["id"]} for item in atoms[: min(4, len(atoms))]]
    if ("all" in lower or "and" in lower or "parlay" in lower or "strict" in lower) and len(args) >= 2:
        formula = {"op": "AND", "args": args}
        short = "All selected"
        desc = "Agent draft requiring every selected condition to resolve YES."
    elif ("at least" in lower or "k of" in lower or "basket" in lower or "recession" in lower) and len(args) >= 3:
        formula = {"op": "K_OF_N", "k": 2, "args": args}
        short = "Two-of basket"
        desc = "Agent draft requiring at least two of the selected conditions."
    elif ("if" in lower or "conditional" in lower) and len(args) >= 2:
        formula = {"op": "IF_THEN", "args": args[:2]}
        short = "Conditional"
        desc = "Agent draft expressing a conditional relationship between two selected conditions."
    else:
        formula = {"op": "OR", "args": args}
        short = "Any selected"
        desc = "Agent draft paying if any selected condition resolves YES."
    domain = domain or atoms[0].get("domain", "custom")
    return normalize_draft(
        {
            "title": f"{short} composition for {prompt[:56]}",
            "short_name": short,
            "question": f"Will the {short.lower()} formula for '{prompt[:80]}' resolve YES?",
            "description": desc,
            "formula": formula,
            "domain": domain,
            "tags": ["composition-demo", domain, "agent-draft"],
        }
    )


def infer_domain(prompt: str) -> str:
    lower = prompt.lower()
    rules = [
        ("macro", ["macro", "recession", "inflation", "fed", "gdp", "unemployment", "sahm", "cpi", "vix"]),
        ("politics", ["election", "president", "primary", "nomination", "senate", "house", "candidate"]),
        ("geopolitics", ["iran", "ukraine", "taiwan", "war", "strike", "invasion", "conflict"]),
        ("sports", ["nba", "sports", "team", "game", "player", "points", "rebounds", "assists", "parlay"]),
        ("technology", ["ai", "agi", "benchmark", "frontier", "model", "lab", "openai", "anthropic"]),
        ("crypto", ["crypto", "btc", "bitcoin", "eth", "ethereum", "sol", "solana", "hype"]),
        ("culture", ["movie", "album", "music", "release", "drake", "taylor"]),
    ]
    for domain, needles in rules:
        if any(needle in lower for needle in needles):
            return domain
    return ""


def pick_diverse_atoms(prompt: str, atoms: list[dict[str, Any]]) -> list[dict[str, Any]]:
    lower = prompt.lower()
    if "recession" in lower:
        priorities = ["real gdp", "sahm", "unemployment", "drawdown", "vix", "fed funds"]
        ranked: list[dict[str, Any]] = []
        used_indicators: set[str] = set()
        for priority in priorities:
            for atom in atoms:
                indicator = str(atom.get("params", {}).get("indicator", "")).lower()
                if priority in indicator and indicator not in used_indicators:
                    ranked.append(atom)
                    used_indicators.add(indicator)
                    break
        for atom in atoms:
            indicator = str(atom.get("params", {}).get("indicator", atom.get("id", ""))).lower()
            if indicator not in used_indicators:
                ranked.append(atom)
                used_indicators.add(indicator)
        return ranked
    return atoms


def expand_prompt(prompt: str) -> str:
    lower = prompt.lower()
    expansions = [prompt]
    if "recession" in lower:
        expansions.append("GDP unemployment Sahm drawdown VIX")
    elif "macro" in lower:
        expansions.append("GDP unemployment Sahm Fed funds CPI drawdown VIX")
    if any(word in lower for word in ["nomination", "primary"]):
        expansions.append("presidential nomination primary candidate contest")
    if "agi" in lower:
        expansions.append("AI benchmark FrontierMath ARC AGI SWE-bench")
    if "iran" in lower or "invasion" in lower:
        expansions.append("Iran troops strikes declaration AUMF occupation")
    return " ".join(expansions)


def normalize_draft(data: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": data.get("id", ""),
        "kind": "composition",
        "title": data["title"],
        "short_name": data.get("short_name") or data["title"][:24],
        "question": data["question"],
        "description": data["description"],
        "oracle_path": "Composition over demo atoms",
        "formula": data["formula"],
        "author": "Agent draft",
        "fair_value": 0.15,
        "trust_tier": "demo-draft",
        "tags": data.get("tags", ["composition-demo", "agent-draft"]),
        "domain": data.get("domain", "custom"),
        "atom_type": "composition",
        "subject": data.get("short_name") or data["title"][:24],
        "metric": "formula",
        "comparator": "resolves_true",
        "threshold": None,
        "unit": "",
        "time_window": "user-defined",
        "resolver_primitive": "composition_resolution",
        "source": "agent",
        "source_url": "",
        "canonical_key": data.get("id", "") or data["title"],
        "compatible_ops": ["AND", "OR", "NOT", "K_OF_N", "IF_THEN"],
        "exclusivity_group": None,
        "template_id": "composition",
        "params": {},
        "quality": "agent_draft",
        "aliases": [],
    }


def find_instrument(state: dict[str, Any], instrument_id: str | None) -> dict[str, Any]:
    for item in state["instruments"]:
        if item["id"] == instrument_id:
            return item
    raise KeyError(f"unknown instrument: {instrument_id}")
