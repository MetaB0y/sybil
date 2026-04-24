"""Static registry and formula helpers for the Iran composition demo.

The MVP keeps composition metadata outside the Rust sequencer. Sybil markets
remain ordinary binary markets; this registry explains how those markets relate.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass, field
from typing import Any, Literal


InstrumentKind = Literal["atom", "composition"]
Formula = dict[str, Any]
VALID_OPERATORS = {"AND", "OR", "NOT", "K_OF_N", "IF_THEN"}
DEFAULT_COMPATIBLE_OPS = ["AND", "OR", "NOT", "K_OF_N", "IF_THEN"]
SEARCH_STOPWORDS = {
    "a",
    "an",
    "and",
    "any",
    "be",
    "build",
    "by",
    "for",
    "from",
    "in",
    "is",
    "it",
    "market",
    "of",
    "or",
    "the",
    "to",
    "will",
    "with",
}
SEARCH_SYNONYMS = {
    "nomination": ["primary", "nominee"],
    "nominee": ["nomination", "primary"],
    "recession": ["gdp", "unemployment", "sahm", "drawdown"],
    "macro": ["gdp", "unemployment", "inflation", "fed", "cpi"],
    "agi": ["ai", "benchmark", "frontiermath", "arc"],
    "crypto": ["btc", "eth", "sol"],
    "basket": ["threshold"],
}


@dataclass
class Instrument:
    id: str
    kind: InstrumentKind
    title: str
    short_name: str
    question: str
    description: str
    oracle_path: str
    formula: Formula | None = None
    author: str = "Sybil seed"
    market_id: int | None = None
    fair_value: float = 0.5
    trust_tier: str = "demo"
    tags: list[str] = field(default_factory=lambda: ["composition-demo", "iran"])
    domain: str = "geopolitics"
    atom_type: str = "binary_event"
    subject: str = ""
    metric: str = "event"
    comparator: str = "occurs"
    threshold: float | None = None
    unit: str = ""
    time_window: str = "before 2027"
    resolver_primitive: str = "admin_immediate"
    source: str = "seed"
    source_url: str = ""
    canonical_key: str = ""
    compatible_ops: list[str] = field(default_factory=lambda: list(DEFAULT_COMPATIBLE_OPS))
    exclusivity_group: str | None = None

    def to_dict(self) -> dict[str, Any]:
        data = asdict(self)
        if not data["canonical_key"]:
            data["canonical_key"] = canonical_key_for(data)
        if not data["subject"]:
            data["subject"] = data["short_name"]
        return data


ATOM_IDS = [
    "troops_soil_1",
    "troops_soil_1000",
    "troops_duration_72h",
    "formal_declaration",
    "aumf_passed",
    "strikes_50",
    "strikes_7d",
    "occupation_declared",
]


SEED_INSTRUMENTS: list[Instrument] = [
    Instrument(
        id="troops_soil_1",
        kind="atom",
        title="Any US troops touch Iranian soil",
        short_name="Troops > 0",
        question="Will any US military personnel enter Iranian sovereign territory before 2027?",
        description="Low-bar troop-presence atom. A rescue helicopter landing or brief incursion can trigger it.",
        oracle_path="Reuters/AP + Pentagon confirmation, admin demo resolver",
        fair_value=0.42,
    ),
    Instrument(
        id="troops_soil_1000",
        kind="atom",
        title="1,000+ US troops enter Iran",
        short_name="Troops > 1k",
        question="Will at least 1,000 US military personnel enter Iranian sovereign territory before 2027?",
        description="Ground-force scale atom intended to distinguish a genuine operation from a brief incident.",
        oracle_path="Reuters/AP + Pentagon troop-count reports, admin demo resolver",
        fair_value=0.12,
    ),
    Instrument(
        id="troops_duration_72h",
        kind="atom",
        title="US troops remain in Iran for 72h+",
        short_name="72h presence",
        question="Will US troops remain continuously on Iranian sovereign territory for at least 72 hours before 2027?",
        description="Duration atom for sustained ground presence.",
        oracle_path="Reuters/AP live reporting + Pentagon statements, admin demo resolver",
        fair_value=0.10,
    ),
    Instrument(
        id="formal_declaration",
        kind="atom",
        title="US formally declares war on Iran",
        short_name="War declared",
        question="Will the United States formally declare war on Iran before 2027?",
        description="Clean legal atom based on official US government action.",
        oracle_path="Congress.gov + White House records, admin demo resolver",
        fair_value=0.04,
    ),
    Instrument(
        id="aumf_passed",
        kind="atom",
        title="Congress passes Iran AUMF",
        short_name="AUMF",
        question="Will Congress pass an Authorization for Use of Military Force against Iran before 2027?",
        description="Legal authorization atom; broader than a formal declaration of war.",
        oracle_path="Congress.gov bill status, admin demo resolver",
        fair_value=0.08,
    ),
    Instrument(
        id="strikes_50",
        kind="atom",
        title="50+ US strikes on Iran",
        short_name="50+ strikes",
        question="Will the US conduct at least 50 kinetic strikes on targets in Iran before 2027?",
        description="Large strike-campaign atom.",
        oracle_path="Reuters/AP + Pentagon strike reports, admin demo resolver",
        fair_value=0.22,
    ),
    Instrument(
        id="strikes_7d",
        kind="atom",
        title="US strikes Iran for 7+ days",
        short_name="7d strikes",
        question="Will US strikes on Iran continue across at least 7 calendar days before 2027?",
        description="Sustained-air-campaign atom.",
        oracle_path="Reuters/AP daily strike chronology, admin demo resolver",
        fair_value=0.18,
    ),
    Instrument(
        id="occupation_declared",
        kind="atom",
        title="US declares occupation of Iranian territory",
        short_name="Occupation",
        question="Will the US declare occupation or control of Iranian territory before 2027?",
        description="Strict territorial-control atom.",
        oracle_path="White House/Pentagon official statements, admin demo resolver",
        fair_value=0.03,
    ),
    Instrument(
        id="iran_hawkish",
        kind="composition",
        title="US invades Iran - Hawkish definition",
        short_name="Hawkish",
        question="Will the US invade Iran before 2027 under a low-bar military-action definition?",
        description="Counts any US troops on Iranian soil or a substantial strike campaign as invasion.",
        oracle_path="Composition over seed atoms",
        formula={"op": "OR", "args": [{"atom": "troops_soil_1"}, {"atom": "strikes_50"}]},
        author="Sybil seed / hawkish",
        fair_value=0.48,
    ),
    Instrument(
        id="iran_mainstream",
        kind="composition",
        title="US invades Iran - Mainstream definition",
        short_name="Mainstream",
        question="Will the US invade Iran before 2027 under a mainstream media definition?",
        description="Requires sustained ground presence, formal authorization, or a large sustained strike campaign.",
        oracle_path="Composition over seed atoms",
        formula={
            "op": "OR",
            "args": [
                {
                    "op": "AND",
                    "args": [{"atom": "troops_soil_1000"}, {"atom": "troops_duration_72h"}],
                },
                {"atom": "formal_declaration"},
                {"atom": "aumf_passed"},
                {"op": "AND", "args": [{"atom": "strikes_50"}, {"atom": "strikes_7d"}]},
            ],
        },
        author="Sybil seed / mainstream",
        fair_value=0.16,
    ),
    Instrument(
        id="iran_strict",
        kind="composition",
        title="US invades Iran - Strict definition",
        short_name="Strict",
        question="Will the US invade Iran before 2027 under a strict occupation definition?",
        description="Requires a large sustained ground operation plus declared territorial occupation.",
        oracle_path="Composition over seed atoms",
        formula={
            "op": "AND",
            "args": [
                {"atom": "troops_soil_1000"},
                {"atom": "troops_duration_72h"},
                {"atom": "occupation_declared"},
            ],
        },
        author="Sybil seed / strict",
        fair_value=0.04,
    ),
]


def instruments_by_id(instruments: list[Instrument] | list[dict[str, Any]]) -> dict[str, Any]:
    return {item.id if isinstance(item, Instrument) else item["id"]: item for item in instruments}


def formula_atoms(formula: Formula | None) -> list[str]:
    if not formula:
        return []
    if "atom" in formula:
        return [formula["atom"]]
    atoms: list[str] = []
    for arg in formula.get("args", []):
        atoms.extend(formula_atoms(arg))
    return atoms


def canonical_key_for(item: dict[str, Any]) -> str:
    if item.get("template_id") and item.get("params"):
        return f"{item['template_id']}:{json_like(item['params'])}"
    parts = [
        item.get("domain", ""),
        item.get("atom_type", ""),
        item.get("subject", "") or item.get("short_name", ""),
        item.get("metric", ""),
        item.get("comparator", ""),
        str(item.get("threshold", "")),
        item.get("unit", ""),
        item.get("time_window", ""),
        item.get("resolver_primitive", ""),
    ]
    return "|".join(slug_part(part) for part in parts if part is not None)


def json_like(value: Any) -> str:
    import json

    return json.dumps(value, ensure_ascii=True, sort_keys=True, separators=(",", ":"))


def slug_part(value: Any) -> str:
    out = []
    last_dash = False
    for ch in str(value).lower():
        if ch.isalnum():
            out.append(ch)
            last_dash = False
        elif not last_dash:
            out.append("-")
            last_dash = True
    return "".join(out).strip("-")


def formula_to_text(formula: Formula | None) -> str:
    if not formula:
        return "atomic"
    if "atom" in formula:
        return formula["atom"]
    op = formula.get("op", "?")
    args = [formula_to_text(arg) for arg in formula.get("args", [])]
    if op == "NOT" and args:
        return f"NOT({args[0]})"
    if op == "K_OF_N":
        return f"K_OF_N({formula.get('k', '?')}; {', '.join(args)})"
    if op == "IF_THEN" and len(args) >= 2:
        return f"IF {args[0]} THEN {args[1]}"
    return f"{op}({', '.join(args)})"


def estimate_formula_value(formula: Formula | None, values: dict[str, float]) -> float:
    """Estimate a formula using a deliberately simple, transparent model.

    This is not a pricing engine. It gives the demo MM a stable reference and
    keeps the UI honest about the MVP boundary.
    """
    if not formula:
        return 0.5
    if "atom" in formula:
        return values.get(formula["atom"], 0.5)
    op = formula.get("op")
    parts = [estimate_formula_value(arg, values) for arg in formula.get("args", [])]
    if not parts:
        return 0.5
    if op == "AND":
        out = 1.0
        for p in parts:
            out *= p
        return clamp_probability(out)
    if op == "OR":
        fail = 1.0
        for p in parts:
            fail *= 1.0 - p
        return clamp_probability(1.0 - fail)
    if op == "NOT":
        return clamp_probability(1.0 - parts[0])
    if op == "K_OF_N":
        k = int(formula.get("k", len(parts)))
        return estimate_k_of_n(parts, k)
    if op == "IF_THEN" and len(parts) >= 2:
        # P(A -> B) = P(!A or B). Independence is only a demo prior here.
        return clamp_probability(1.0 - parts[0] * (1.0 - parts[1]))
    return clamp_probability(sum(parts) / len(parts))


def clamp_probability(value: float) -> float:
    return max(0.01, min(0.99, value))


def estimate_k_of_n(parts: list[float], k: int) -> float:
    if k <= 0:
        return 0.99
    if k > len(parts):
        return 0.01
    dist = [1.0] + [0.0] * len(parts)
    for p in parts:
        next_dist = [0.0] * len(dist)
        for yes_count, mass in enumerate(dist):
            if mass == 0:
                continue
            next_dist[yes_count] += mass * (1.0 - p)
            if yes_count + 1 < len(dist):
                next_dist[yes_count + 1] += mass * p
        dist = next_dist
    return clamp_probability(sum(dist[k:]))


def validate_formula(formula: Formula | None, instruments: list[dict[str, Any]]) -> dict[str, Any]:
    ids = {item["id"] for item in instruments}
    errors: list[str] = []
    refs: list[str] = []

    def walk(node: Formula | None, path: str) -> None:
        if not isinstance(node, dict):
            errors.append(f"{path}: formula node must be an object")
            return
        if "atom" in node:
            atom_id = str(node["atom"])
            refs.append(atom_id)
            if atom_id not in ids:
                errors.append(f"{path}: unknown atom '{atom_id}'")
            return
        op = str(node.get("op", "")).upper()
        args = node.get("args")
        if op not in VALID_OPERATORS:
            errors.append(f"{path}: unsupported operator '{op or '?'}'")
        if not isinstance(args, list):
            errors.append(f"{path}: args must be a list")
            return
        if op == "NOT" and len(args) != 1:
            errors.append(f"{path}: NOT requires exactly one argument")
        if op == "IF_THEN" and len(args) != 2:
            errors.append(f"{path}: IF_THEN requires exactly two arguments")
        if op == "K_OF_N":
            k = node.get("k")
            if not isinstance(k, int) or k < 1 or k > len(args):
                errors.append(f"{path}: K_OF_N requires integer k with 1 <= k <= n")
        if op in {"AND", "OR"} and len(args) < 2:
            errors.append(f"{path}: {op} requires at least two arguments")
        for idx, arg in enumerate(args):
            walk(arg, f"{path}.args[{idx}]")

    walk(formula, "$")
    return {
        "valid": not errors,
        "errors": errors,
        "referenced_ids": sorted(set(refs)),
        "operator_count": count_operators(formula),
    }


def count_operators(formula: Formula | None) -> int:
    if not isinstance(formula, dict) or "atom" in formula:
        return 0
    return 1 + sum(count_operators(arg) for arg in formula.get("args", []) if isinstance(arg, dict))


def search_instruments(
    instruments: list[dict[str, Any]],
    query: str = "",
    domain: str = "",
    atom_type: str = "",
    source: str = "",
    kind: str = "",
    template_id: str = "",
    quality: str = "",
    resolver_primitive: str = "",
    limit: int = 80,
) -> dict[str, Any]:
    q_tokens = query_tokens(query)
    rows = []
    for item in instruments:
        if domain and item.get("domain") != domain:
            continue
        if atom_type and item.get("atom_type") != atom_type:
            continue
        if source and item.get("source") != source:
            continue
        if kind and item.get("kind") != kind:
            continue
        if template_id and item.get("template_id") != template_id:
            continue
        if quality and item.get("quality") != quality:
            continue
        if resolver_primitive and item.get("resolver_primitive") != resolver_primitive:
            continue
        text = " ".join(
            str(item.get(key, ""))
            for key in [
                "title",
                "short_name",
                "question",
                "description",
                "domain",
                "atom_type",
                "subject",
                "metric",
                "tags",
                "source",
                "template_id",
                "params",
                "quality",
                "aliases",
            ]
        ).lower()
        score = 0.0
        token_hits = 0
        for token in q_tokens:
            if token in text:
                score += 3.0
                token_hits += 1
            if text.startswith(token):
                score += 2.0
        if q_tokens and token_hits == 0:
            continue
        if q_tokens:
            score += token_hits / max(1, len(q_tokens))
            if item.get("kind") == "atom":
                score += 0.4
            if item.get("quality") == "source_matched":
                score += 0.35
        score += min(float(item.get("fair_value", 0.5)), 0.99) * 0.05
        if not q_tokens:
            score += 1.0
        row = dict(item)
        row["search_score"] = round(score, 4)
        rows.append(row)
    rows.sort(
        key=lambda item: (
            item.get("search_score", 0),
            item.get("quality") == "source_matched",
            item.get("kind") == "atom",
        ),
        reverse=True,
    )
    facets = {
        "domains": sorted({item.get("domain", "") for item in instruments if item.get("domain")}),
        "atom_types": sorted({item.get("atom_type", "") for item in instruments if item.get("atom_type")}),
        "sources": sorted({item.get("source", "") for item in instruments if item.get("source")}),
        "template_ids": sorted({item.get("template_id", "") for item in instruments if item.get("template_id")}),
        "qualities": sorted({item.get("quality", "") for item in instruments if item.get("quality")}),
        "resolver_primitives": sorted(
            {item.get("resolver_primitive", "") for item in instruments if item.get("resolver_primitive")}
        ),
    }
    return {"items": rows[:limit], "total": len(rows), "facets": facets}


def query_tokens(query: str) -> list[str]:
    raw = []
    current = []
    for ch in query.lower():
        if ch.isalnum():
            current.append(ch)
        elif current:
            raw.append("".join(current))
            current = []
    if current:
        raw.append("".join(current))

    tokens: list[str] = []
    seen: set[str] = set()
    for token in raw:
        if len(token) <= 1 or token in SEARCH_STOPWORDS:
            continue
        for expanded in [token, *SEARCH_SYNONYMS.get(token, [])]:
            if expanded not in seen:
                seen.add(expanded)
                tokens.append(expanded)
    return tokens
