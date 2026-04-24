"""Graph registry and formula helpers for the composition demo.

The demo models the product surface as:

    data feeds -> measurements -> conditions -> propositions -> Sybil markets

The Rust sequencer still sees ordinary binary markets. This module keeps the
ontology, canonical identity, formula validation, and demo pricing metadata in
Python until predicate-backed oracle resolution is wired into sybil-oracle.
"""

from __future__ import annotations

import hashlib
import json
from dataclasses import asdict, dataclass, field
from typing import Any, Literal


ObjectKind = Literal["feed", "measurement", "condition", "proposition"]
InstrumentKind = Literal["condition", "proposition", "atom", "composition"]
Formula = dict[str, Any]
VALID_OPERATORS = {"AND", "OR", "NOT", "K_OF_N", "IF_THEN"}
COMMUTATIVE_OPERATORS = {"AND", "OR"}
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
    "crypto": ["btc", "bitcoin", "eth", "ethereum", "sol"],
    "basket": ["threshold"],
    "range": ["between"],
    "invasion": ["troops", "strikes", "occupation"],
}


@dataclass
class DataFeed:
    id: str
    name: str
    domain: str
    trust_tier: str
    resolver_primitive: str
    description: str
    source_url: str = ""

    def to_dict(self) -> dict[str, Any]:
        data = asdict(self)
        data["object_kind"] = "feed"
        return data


@dataclass
class Measurement:
    id: str
    domain: str
    measurement_kind: str
    subject: str
    unit: str
    feed_ids: list[str]
    aggregation_semantics: str
    title: str
    description: str
    resolver_primitive: str
    trust_tier: str = "demo"
    quality: str = "seed"
    canonical_key: str = ""

    def to_dict(self) -> dict[str, Any]:
        data = asdict(self)
        data["object_kind"] = "measurement"
        data["canonical_key"] = self.canonical_key or measurement_key(data)
        return data


@dataclass
class Condition:
    id: str
    measurement_id: str
    domain: str
    title: str
    short_name: str
    question: str
    description: str
    observation_window: str
    aggregation: str
    predicate: dict[str, Any]
    fair_value: float
    resolver_primitive: str
    quality: str = "seed"
    aliases: list[dict[str, Any]] = field(default_factory=list)
    market_id: int | None = None
    canonical_key: str = ""
    formula: Formula | None = None

    def to_dict(self) -> dict[str, Any]:
        data = asdict(self)
        data["kind"] = "condition"
        data["object_kind"] = "condition"
        data["atom_type"] = self.predicate.get("op", "predicate")
        data["template_id"] = "condition"
        data["params"] = {
            "measurement_id": self.measurement_id,
            "observation_window": self.observation_window,
            "aggregation": self.aggregation,
            "predicate": self.predicate,
        }
        data["subject"] = self.title
        data["metric"] = self.aggregation
        data["comparator"] = self.predicate.get("op", "predicate")
        data["threshold"] = self.predicate.get("threshold")
        data["unit"] = self.predicate.get("unit", "")
        data["time_window"] = self.observation_window
        data["source"] = "graph"
        data["oracle_path"] = f"{self.resolver_primitive}: {self.measurement_id}"
        data["compatible_ops"] = list(DEFAULT_COMPATIBLE_OPS)
        data["leaf_ids"] = []
        data["canonical_key"] = self.canonical_key or condition_key(data)
        data["tags"] = ["composition-demo", "graph", self.domain, "condition"]
        data["author"] = "Sybil graph seed"
        data["trust_tier"] = "graph-demo"
        data["source_url"] = ""
        data["exclusivity_group"] = f"{self.measurement_id}:{self.observation_window}:{self.aggregation}"
        return data


@dataclass
class Proposition:
    id: str
    domain: str
    title: str
    short_name: str
    question: str
    description: str
    formula: Formula
    fair_value: float
    quality: str = "seed"
    market_id: int | None = None
    canonical_key: str = ""

    def to_dict(self) -> dict[str, Any]:
        data = asdict(self)
        data["kind"] = "proposition"
        data["object_kind"] = "proposition"
        data["atom_type"] = "composition"
        data["template_id"] = "proposition"
        data["params"] = {}
        data["subject"] = self.title
        data["metric"] = "formula"
        data["comparator"] = "resolves_true"
        data["threshold"] = None
        data["unit"] = ""
        data["time_window"] = "formula-defined"
        data["resolver_primitive"] = "predicate_formula"
        data["source"] = "graph"
        data["oracle_path"] = "Formula over graph conditions"
        data["compatible_ops"] = list(DEFAULT_COMPATIBLE_OPS)
        data["leaf_ids"] = formula_conditions(self.formula)
        data["canonical_key"] = self.canonical_key or proposition_key(self.formula)
        data["tags"] = ["composition-demo", "graph", self.domain, "proposition"]
        data["author"] = "Sybil graph seed"
        data["trust_tier"] = "graph-demo"
        data["source_url"] = ""
        data["exclusivity_group"] = None
        data["aliases"] = []
        return data


def stable_id(prefix: str, key: str) -> str:
    digest = hashlib.sha256(key.encode("utf-8")).hexdigest()[:16]
    return f"{prefix}_{digest}"


def canonical_json(value: Any) -> str:
    return json.dumps(value, ensure_ascii=True, sort_keys=True, separators=(",", ":"))


def normalize_text(value: Any) -> str:
    return " ".join(str(value).strip().lower().split())


def normalized_scalar(value: Any) -> Any:
    if isinstance(value, str):
        stripped = " ".join(value.strip().split())
        try:
            numeric = float(stripped.replace(",", ""))
            return int(numeric) if numeric.is_integer() else numeric
        except ValueError:
            return stripped.lower()
    if isinstance(value, float) and value.is_integer():
        return int(value)
    return value


def measurement_key(measurement: dict[str, Any]) -> str:
    feed_ids = sorted(str(feed).lower() for feed in measurement.get("feed_ids", []))
    payload = {
        "kind": normalize_text(measurement.get("measurement_kind", measurement.get("kind", ""))),
        "subject": normalize_text(measurement.get("subject", "")),
        "unit": normalize_text(measurement.get("unit", "")),
        "feeds": feed_ids,
        "aggregation_semantics": normalize_text(measurement.get("aggregation_semantics", "")),
    }
    return f"measurement:{canonical_json(payload)}"


def condition_key(condition: dict[str, Any]) -> str:
    predicate = {
        key: normalized_scalar(value)
        for key, value in dict(condition.get("predicate") or {}).items()
        if value is not None
    }
    payload = {
        "measurement": condition.get("measurement_key") or condition.get("measurement_id"),
        "window": normalize_text(condition.get("observation_window") or condition.get("time_window", "")),
        "aggregation": normalize_text(condition.get("aggregation", condition.get("metric", ""))),
        "predicate": predicate,
    }
    return f"condition:{canonical_json(payload)}"


def normalize_formula(formula: Formula | None) -> Any:
    if not isinstance(formula, dict):
        return None
    if "condition" in formula or "atom" in formula:
        return {"condition": str(formula.get("condition") or formula.get("atom"))}
    op = str(formula.get("op", "")).upper()
    args = [normalize_formula(arg) for arg in formula.get("args", []) if isinstance(arg, dict)]
    args = [arg for arg in args if arg is not None]
    if op in COMMUTATIVE_OPERATORS:
        args = sorted(args, key=canonical_json)
    payload: dict[str, Any] = {"op": op, "args": args}
    if op == "K_OF_N":
        payload["k"] = int(formula.get("k", len(args)))
    return payload


def proposition_key(formula: Formula | None) -> str:
    return f"proposition:{canonical_json(normalize_formula(formula))}"


def formula_conditions(formula: Formula | None) -> list[str]:
    if not isinstance(formula, dict):
        return []
    if "condition" in formula or "atom" in formula:
        return [str(formula.get("condition") or formula.get("atom"))]
    refs: list[str] = []
    for arg in formula.get("args", []):
        refs.extend(formula_conditions(arg))
    return refs


def formula_atoms(formula: Formula | None) -> list[str]:
    """Compatibility alias for old callers."""
    return formula_conditions(formula)


def formula_to_text(formula: Formula | None) -> str:
    if not formula:
        return "condition"
    if "condition" in formula or "atom" in formula:
        return str(formula.get("condition") or formula.get("atom"))
    op = str(formula.get("op", "?")).upper()
    args = [formula_to_text(arg) for arg in formula.get("args", [])]
    if op == "NOT" and args:
        return f"NOT({args[0]})"
    if op == "K_OF_N":
        return f"K_OF_N({formula.get('k', '?')}; {', '.join(args)})"
    if op == "IF_THEN" and len(args) >= 2:
        return f"IF {args[0]} THEN {args[1]}"
    return f"{op}({', '.join(args)})"


def estimate_formula_value(formula: Formula | None, values: dict[str, float]) -> float:
    if not formula:
        return 0.5
    if "condition" in formula or "atom" in formula:
        return values.get(str(formula.get("condition") or formula.get("atom")), 0.5)
    op = str(formula.get("op", "")).upper()
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
        return estimate_k_of_n(parts, int(formula.get("k", len(parts))))
    if op == "IF_THEN" and len(parts) >= 2:
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


def validate_formula(
    formula: Formula | None,
    instruments_or_conditions: list[dict[str, Any]],
    implication_edges: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    condition_ids = {
        item["id"]
        for item in instruments_or_conditions
        if item.get("object_kind") == "condition" or item.get("kind") in {"condition", "atom"}
    }
    proposition_keys = {
        item.get("canonical_key")
        for item in instruments_or_conditions
        if item.get("object_kind") == "proposition" or item.get("kind") in {"proposition", "composition"}
    }
    errors: list[str] = []
    refs: list[str] = []

    def walk(node: Formula | None, path: str) -> None:
        if not isinstance(node, dict):
            errors.append(f"{path}: formula node must be an object")
            return
        if "condition" in node or "atom" in node:
            condition_id = str(node.get("condition") or node.get("atom"))
            refs.append(condition_id)
            if condition_id not in condition_ids:
                errors.append(f"{path}: unknown condition '{condition_id}'")
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
    key = proposition_key(formula)
    warnings = implication_warnings(sorted(set(refs)), implication_edges or [])
    return {
        "valid": not errors,
        "errors": errors,
        "warnings": warnings,
        "referenced_ids": sorted(set(refs)),
        "referenced_conditions": sorted(set(refs)),
        "operator_count": count_operators(formula),
        "canonical_key": key,
        "duplicate": key in proposition_keys,
    }


def implication_warnings(refs: list[str], edges: list[dict[str, Any]]) -> list[str]:
    ref_set = set(refs)
    warnings: list[str] = []
    for edge in edges:
        if edge.get("from") in ref_set and edge.get("to") in ref_set:
            warnings.append(edge.get("label") or f"{edge['from']} implies {edge['to']}")
    return warnings


def count_operators(formula: Formula | None) -> int:
    if not isinstance(formula, dict) or "condition" in formula or "atom" in formula:
        return 0
    return 1 + sum(count_operators(arg) for arg in formula.get("args", []) if isinstance(arg, dict))


def canonical_key_for(item: dict[str, Any]) -> str:
    kind = item.get("object_kind") or item.get("kind")
    if kind == "measurement":
        return measurement_key(item)
    if kind == "condition":
        return condition_key(item)
    if kind == "proposition" or item.get("formula"):
        return proposition_key(item.get("formula"))
    return f"{kind}:{normalize_text(item.get('id', item.get('title', '')))}"


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
    object_kind: str = "",
    measurement_kind: str = "",
    measurement_id: str = "",
    predicate_op: str = "",
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
        if object_kind and item.get("object_kind") != object_kind:
            continue
        if measurement_kind and item.get("measurement_kind") != measurement_kind:
            continue
        if measurement_id and item.get("measurement_id") != measurement_id:
            continue
        if predicate_op and item.get("predicate", {}).get("op") != predicate_op:
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
                "measurement_id",
                "measurement_kind",
                "predicate",
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
            if item.get("kind") == "condition":
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
            item.get("kind") == "condition",
        ),
        reverse=True,
    )
    facets = build_facets(instruments)
    return {"items": rows[:limit], "total": len(rows), "facets": facets}


def build_facets(instruments: list[dict[str, Any]]) -> dict[str, list[str]]:
    return {
        "domains": sorted({item.get("domain", "") for item in instruments if item.get("domain")}),
        "atom_types": sorted({item.get("atom_type", "") for item in instruments if item.get("atom_type")}),
        "sources": sorted({item.get("source", "") for item in instruments if item.get("source")}),
        "template_ids": sorted({item.get("template_id", "") for item in instruments if item.get("template_id")}),
        "qualities": sorted({item.get("quality", "") for item in instruments if item.get("quality")}),
        "resolver_primitives": sorted(
            {item.get("resolver_primitive", "") for item in instruments if item.get("resolver_primitive")}
        ),
        "object_kinds": sorted({item.get("object_kind", "") for item in instruments if item.get("object_kind")}),
        "measurement_kinds": sorted(
            {item.get("measurement_kind", "") for item in instruments if item.get("measurement_kind")}
        ),
        "measurement_ids": sorted({item.get("measurement_id", "") for item in instruments if item.get("measurement_id")}),
        "predicate_ops": sorted(
            {item.get("predicate", {}).get("op", "") for item in instruments if item.get("predicate", {}).get("op")}
        ),
    }


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
