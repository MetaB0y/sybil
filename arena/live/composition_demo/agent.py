"""Agent logic for composition discovery, trade proposals, and draft creation."""

from __future__ import annotations

import json
import os
from typing import Any

from openai import OpenAI

from .registry import formula_atoms, formula_to_text
from .store import NANOS_PER_DOLLAR

MODEL = "deepseek/deepseek-v4-flash"


def discover(query: str, state: dict[str, Any]) -> dict[str, Any]:
    q = query.lower()
    instruments = [i for i in state["instruments"] if i["kind"] == "composition"]
    scored = []
    for item in instruments:
        text = " ".join(
            [
                item["title"],
                item.get("short_name", ""),
                item.get("description", ""),
                item.get("question", ""),
                formula_to_text(item.get("formula")),
            ]
        ).lower()
        score = 0
        for token in q.replace("?", " ").split():
            if len(token) > 2 and token in text:
                score += 2
        if "strict" in q and "strict" in text:
            score += 8
        if ("normal" in q or "mainstream" in q or "spirit" in q) and "mainstream" in text:
            score += 8
        if ("technical" in q or "helicopter" in q or "low" in q) and "hawkish" in text:
            score += 8
        if "no" in q or "against" in q or "short" in q:
            score += 1
        scored.append((score, item))

    ranked = [item for _, item in sorted(scored, key=lambda pair: pair[0], reverse=True)]
    recommendation = ranked[0] if ranked else None
    if recommendation and recommendation["id"] == "iran_hawkish" and "helicopter" not in q:
        mainstream = next((i for i in ranked if i["id"] == "iran_mainstream"), recommendation)
        recommendation = mainstream

    visible = ranked[:5]
    if recommendation:
        visible = [recommendation] + [item for item in visible if item["id"] != recommendation["id"]]

    return {
        "answer": build_discovery_answer(query, recommendation, ranked[:3]),
        "recommendation_id": recommendation["id"] if recommendation else None,
        "ranked_ids": [item["id"] for item in visible[:5]],
        "actions": [
            "Inspect the formula tree before trading.",
            "Use Mainstream if you mean ordinary-language invasion.",
            "Use Hawkish only if brief military contact should count.",
        ],
    }


def build_discovery_answer(query: str, recommendation: dict[str, Any] | None, ranked: list[dict[str, Any]]) -> str:
    if not recommendation:
        return "I could not find a matching composition. Draft a new one from the creation panel."
    names = ", ".join(item["short_name"] for item in ranked)
    return (
        f"For '{query}', I would start with {recommendation['short_name']}. "
        f"It best matches the ordinary-language intent while alternatives ({names}) expose "
        "how sensitive the trade is to the definition."
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
            return drafted
    return deterministic_draft(prompt)


def draft_with_llm(prompt: str, state: dict[str, Any]) -> dict[str, Any] | None:
    atoms = [
        {
            "id": item["id"],
            "short_name": item["short_name"],
            "description": item["description"],
        }
        for item in state["instruments"]
        if item["kind"] == "atom"
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


def deterministic_draft(prompt: str) -> dict[str, Any]:
    lower = prompt.lower()
    if "occupation" in lower or "strict" in lower:
        formula = {
            "op": "AND",
            "args": [
                {"atom": "troops_soil_1000"},
                {"atom": "troops_duration_72h"},
                {"atom": "occupation_declared"},
            ],
        }
        short = "Occupation-only"
        desc = "A stricter user-drafted definition focused on sustained territorial occupation."
    elif "strike" in lower or "air" in lower:
        formula = {"op": "AND", "args": [{"atom": "strikes_50"}, {"atom": "strikes_7d"}]}
        short = "Strike campaign"
        desc = "A user-drafted definition focused on sustained US strikes rather than ground troops."
    else:
        formula = {
            "op": "OR",
            "args": [
                {"op": "AND", "args": [{"atom": "troops_soil_1000"}, {"atom": "troops_duration_72h"}]},
                {"atom": "formal_declaration"},
            ],
        }
        short = "Ground or declared"
        desc = "A user-drafted definition requiring sustained ground presence or a formal declaration."
    return normalize_draft(
        {
            "title": f"US invades Iran - {short} definition",
            "short_name": short,
            "question": f"Will the US invade Iran before 2027 under the {short.lower()} definition?",
            "description": desc,
            "formula": formula,
        }
    )


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
        "tags": ["composition-demo", "iran", "agent-draft"],
    }


def find_instrument(state: dict[str, Any], instrument_id: str | None) -> dict[str, Any]:
    for item in state["instruments"]:
        if item["id"] == instrument_id:
            return item
    raise KeyError(f"unknown instrument: {instrument_id}")
