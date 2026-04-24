"""Static registry and formula helpers for the Iran composition demo.

The MVP keeps composition metadata outside the Rust sequencer. Sybil markets
remain ordinary binary markets; this registry explains how those markets relate.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass, field
from typing import Any, Literal


InstrumentKind = Literal["atom", "composition"]
Formula = dict[str, Any]


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

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


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


def formula_to_text(formula: Formula | None) -> str:
    if not formula:
        return "atomic"
    if "atom" in formula:
        return formula["atom"]
    op = formula.get("op", "?")
    args = [formula_to_text(arg) for arg in formula.get("args", [])]
    if op == "NOT" and args:
        return f"NOT({args[0]})"
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
        return clamp_probability(sum(parts) / max(k, 1) / len(parts))
    return clamp_probability(sum(parts) / len(parts))


def clamp_probability(value: float) -> float:
    return max(0.01, min(0.99, value))

