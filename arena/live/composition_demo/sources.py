"""Template-driven atom universe for the composition demo.

Source markets are evidence, not ontology. The canonical atom identity is
template_id + canonical params; Polymarket/Kalshi questions become aliases
when they can be mapped safely.
"""

from __future__ import annotations

import hashlib
import json
import re
import time
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any

SNAPSHOT_PATH = Path(__file__).resolve().parent / "source_snapshot.json"
GENERATED_PATH = Path(__file__).resolve().parent / "generated_registry.json"
POLYMARKET_EVENTS_URL = "https://gamma-api.polymarket.com/events"
KALSHI_MARKETS_URL = "https://api.elections.kalshi.com/trade-api/v2/markets"
COMPATIBLE_OPS = ["AND", "OR", "NOT", "K_OF_N", "IF_THEN"]


@dataclass(frozen=True)
class AtomTemplate:
    id: str
    domain: str
    resolver_primitive: str
    required_params: tuple[str, ...]
    title: str
    question: str
    short: str
    description: str


TEMPLATES: dict[str, AtomTemplate] = {
    "contest_winner": AtomTemplate(
        id="contest_winner",
        domain="politics",
        resolver_primitive="source_result_signed",
        required_params=("contest", "year", "option"),
        title="{option} wins {year} {contest}",
        question="Will {option} win the {year} {contest}?",
        short="{option}",
        description="Canonical contest-outcome atom. All options in the same contest share one template family.",
    ),
    "candidate_wins_primary": AtomTemplate(
        id="candidate_wins_primary",
        domain="politics",
        resolver_primitive="election_result_signed",
        required_params=("party", "candidate", "year"),
        title="{candidate} wins the {year} {party} primary",
        question="Will {candidate} win the {year} {party} presidential primary?",
        short="{candidate} primary",
        description="Election-chain atom for nominee outcomes.",
    ),
    "candidate_wins_general": AtomTemplate(
        id="candidate_wins_general",
        domain="politics",
        resolver_primitive="election_result_signed",
        required_params=("candidate", "year"),
        title="{candidate} wins the {year} US presidential election",
        question="Will {candidate} win the {year} US presidential election?",
        short="{candidate} general",
        description="Election-chain atom for general-election outcomes.",
    ),
    "team_wins_championship": AtomTemplate(
        id="team_wins_championship",
        domain="sports",
        resolver_primitive="sports_feed_signed",
        required_params=("league", "team", "season"),
        title="{team} wins the {season} {league} championship",
        question="Will {team} win the {season} {league} championship?",
        short="{team} title",
        description="Sports futures atom with league, season, and team parameters.",
    ),
    "team_wins_game": AtomTemplate(
        id="team_wins_game",
        domain="sports",
        resolver_primitive="sports_feed_signed",
        required_params=("league", "team", "game_id"),
        title="{team} wins {game_id}",
        question="Will {team} win {game_id}?",
        short="{team} wins",
        description="Single-game team-result atom for parlay construction.",
    ),
    "player_stat_over": AtomTemplate(
        id="player_stat_over",
        domain="sports",
        resolver_primitive="sports_feed_signed",
        required_params=("league", "player", "stat", "threshold", "period"),
        title="{player} {stat} over {threshold} in {period}",
        question="Will {player} record over {threshold} {stat} in {period}?",
        short="{player} {stat}>{threshold}",
        description="Typed player-prop atom. Same player/stat/threshold collapses across source markets.",
    ),
    "macro_indicator_threshold": AtomTemplate(
        id="macro_indicator_threshold",
        domain="macro",
        resolver_primitive="economic_data_feed_signed",
        required_params=("indicator", "comparator", "threshold", "unit", "period"),
        title="{indicator} {comparator} {threshold}{unit} in {period}",
        question="Will {indicator} be {comparator} {threshold}{unit} in {period}?",
        short="{indicator} {comparator} {threshold}",
        description="Macro data-feed atom for thresholds over public economic indicators.",
    ),
    "conflict_event_threshold": AtomTemplate(
        id="conflict_event_threshold",
        domain="geopolitics",
        resolver_primitive="wire_service_or_admin",
        required_params=("conflict", "event_type", "comparator", "threshold", "unit", "period"),
        title="{conflict}: {event_type} {comparator} {threshold} {unit}",
        question="Will {event_type} for {conflict} be {comparator} {threshold} {unit} during {period}?",
        short="{event_type} {threshold}",
        description="Conflict/escalation atom parameterized by event type and threshold.",
    ),
    "ai_benchmark_threshold": AtomTemplate(
        id="ai_benchmark_threshold",
        domain="technology",
        resolver_primitive="benchmark_or_tee_llm_graded",
        required_params=("benchmark", "threshold", "unit", "by_date"),
        title="{benchmark} reaches {threshold}{unit} by {by_date}",
        question="Will an AI system reach {threshold}{unit} on {benchmark} by {by_date}?",
        short="{benchmark} {threshold}",
        description="AI capability atom over a benchmark, threshold, and deadline.",
    ),
    "ai_lab_announcement": AtomTemplate(
        id="ai_lab_announcement",
        domain="technology",
        resolver_primitive="tee_llm_graded_with_sources",
        required_params=("lab", "claim_type", "by_date"),
        title="{lab} announces {claim_type} by {by_date}",
        question="Will {lab} publicly announce {claim_type} by {by_date}?",
        short="{lab} {claim_type}",
        description="AI announcement atom with a tight claim-type rubric.",
    ),
    "asset_price_threshold": AtomTemplate(
        id="asset_price_threshold",
        domain="crypto",
        resolver_primitive="market_data_feed_signed",
        required_params=("asset", "comparator", "threshold", "quote_currency", "by_date"),
        title="{asset} {comparator} {threshold} {quote_currency} by {by_date}",
        question="Will {asset} trade {comparator} {threshold} {quote_currency} by {by_date}?",
        short="{asset} {threshold}",
        description="Market-data atom for crypto and liquid assets.",
    ),
    "entertainment_release": AtomTemplate(
        id="entertainment_release",
        domain="culture",
        resolver_primitive="media_platform_or_wire_signed",
        required_params=("artist_or_studio", "work", "release_type", "by_date"),
        title="{artist_or_studio} releases {work} by {by_date}",
        question="Will {artist_or_studio} release {work} as a {release_type} by {by_date}?",
        short="{work} release",
        description="Already-atomic entertainment release event.",
    ),
}


def import_universe(max_atoms: int = 300, force: bool = False) -> dict[str, Any]:
    if GENERATED_PATH.exists() and not force:
        with GENERATED_PATH.open("r", encoding="utf-8") as f:
            cached = json.load(f)
        if (
            cached.get("universe_version") == 3
            and int(cached.get("source_counts", {}).get("atoms", 0)) >= max_atoms
        ):
            return cached

    snapshot = fetch_snapshot(force=force)
    universe = build_universe(snapshot, max_atoms=max_atoms)
    GENERATED_PATH.write_text(json.dumps(universe, indent=2, sort_keys=True), encoding="utf-8")
    return universe


def fetch_snapshot(force: bool = False) -> dict[str, Any]:
    if SNAPSHOT_PATH.exists() and not force:
        with SNAPSHOT_PATH.open("r", encoding="utf-8") as f:
            return json.load(f)

    snapshot = {"created_at": time.time(), "polymarket_events": [], "kalshi_markets": [], "errors": []}
    try:
        snapshot["polymarket_events"] = fetch_polymarket_events(max_events=90)
    except Exception as e:
        snapshot["errors"].append(f"polymarket: {e}")
    try:
        snapshot["kalshi_markets"] = fetch_kalshi_markets(limit=250)
    except Exception as e:
        snapshot["errors"].append(f"kalshi: {e}")

    SNAPSHOT_PATH.write_text(json.dumps(snapshot, indent=2, sort_keys=True), encoding="utf-8")
    return snapshot


def fetch_json(url: str, query: dict[str, Any], timeout: float = 20.0) -> Any:
    full_url = f"{url}?{urllib.parse.urlencode(query)}"
    req = urllib.request.Request(full_url, headers={"User-Agent": "sybil-composition-demo/0.3"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read().decode("utf-8"))


def fetch_polymarket_events(max_events: int) -> list[dict[str, Any]]:
    return fetch_json(
        POLYMARKET_EVENTS_URL,
        {
            "active": "true",
            "closed": "false",
            "limit": str(max_events),
            "order": "volume",
            "ascending": "false",
        },
    )


def fetch_kalshi_markets(limit: int) -> list[dict[str, Any]]:
    data = fetch_json(KALSHI_MARKETS_URL, {"limit": str(limit), "status": "open"})
    return data.get("markets", [])


def build_universe(snapshot: dict[str, Any], max_atoms: int = 300) -> dict[str, Any]:
    atoms = generate_seed_atoms(max_atoms=max_atoms)
    aliases, unmatched = match_source_aliases(snapshot, atoms)
    for atom in atoms:
        atom_aliases = aliases.get(atom["canonical_key"], [])
        atom["aliases"] = atom_aliases
        if atom_aliases:
            atom["quality"] = "source_matched"
            atom["fair_value"] = alias_fair_value(atom_aliases, atom["fair_value"])

    compositions = generate_seed_compositions(atoms)
    return {
        "universe_version": 3,
        "created_at": time.time(),
        "source_counts": {
            "polymarket_events": len(snapshot.get("polymarket_events", [])),
            "kalshi_markets": len(snapshot.get("kalshi_markets", [])),
            "atoms": len(atoms),
            "compositions": len(compositions),
            "source_aliases": sum(len(item.get("aliases", [])) for item in atoms),
            "unmatched_sources": len(unmatched),
        },
        "source_errors": snapshot.get("errors", []),
        "unmatched_sources": unmatched[:100],
        "instruments": atoms + compositions,
    }


def generate_seed_atoms(max_atoms: int = 300) -> list[dict[str, Any]]:
    specs: list[tuple[str, dict[str, Any], float, str]] = []
    specs.extend(politics_specs())
    specs.extend(macro_specs())
    specs.extend(sports_specs())
    specs.extend(geopolitics_specs())
    specs.extend(ai_specs())
    specs.extend(crypto_specs())
    specs.extend(culture_specs())

    atoms: list[dict[str, Any]] = []
    seen: set[str] = set()
    for template_id, params, fair, domain_hint in specs:
        atom = build_atom(template_id, params, fair_value=fair, quality="seed", domain_hint=domain_hint)
        if atom["canonical_key"] in seen:
            continue
        seen.add(atom["canonical_key"])
        atoms.append(atom)
        if len(atoms) >= max_atoms:
            return atoms

    idx = 0
    while len(atoms) < max_atoms:
        idx += 1
        atom = build_atom(
            "macro_indicator_threshold",
            {
                "indicator": f"demo_indicator_{idx}",
                "comparator": ">=",
                "threshold": 10 + idx,
                "unit": "%",
                "period": f"2026-Q{(idx % 4) + 1}",
            },
            fair_value=0.2 + (idx % 50) / 100,
            quality="synthetic_demo",
            domain_hint="macro",
        )
        if atom["canonical_key"] not in seen:
            seen.add(atom["canonical_key"])
            atoms.append(atom)
    return atoms


def build_atom(
    template_id: str,
    params: dict[str, Any],
    fair_value: float = 0.5,
    quality: str = "seed",
    aliases: list[dict[str, Any]] | None = None,
    domain_hint: str | None = None,
) -> dict[str, Any]:
    template = TEMPLATES[template_id]
    canonical_params = canonicalize_params(template_id, params)
    missing = [key for key in template.required_params if key not in canonical_params]
    if missing:
        raise ValueError(f"{template_id} missing params: {missing}")
    text = dict(canonical_params)
    title = template.title.format(**text)
    question = template.question.format(**text)
    short = compact_name(template.short.format(**text), 34)
    canonical_key = f"{template_id}:{canonical_json(canonical_params)}"
    atom_id = f"atom_{template_id}_{stable_slug(canonical_json(canonical_params))}"
    domain = domain_hint or template.domain
    return {
        "id": atom_id,
        "kind": "atom",
        "title": title,
        "short_name": short,
        "question": question,
        "description": template.description,
        "oracle_path": resolver_label(template.resolver_primitive),
        "formula": None,
        "author": "Sybil template seed",
        "market_id": None,
        "fair_value": clamp(fair_value),
        "trust_tier": "template-demo",
        "tags": ["composition-demo", "template", domain, template_id],
        "domain": domain,
        "atom_type": template_id,
        "template_id": template_id,
        "params": canonical_params,
        "subject": subject_for(template_id, canonical_params),
        "metric": metric_for(template_id, canonical_params),
        "comparator": str(canonical_params.get("comparator", "resolves_yes")),
        "threshold": numeric_or_none(canonical_params.get("threshold")),
        "unit": str(canonical_params.get("unit", "")),
        "time_window": str(
            canonical_params.get("period")
            or canonical_params.get("by_date")
            or canonical_params.get("season")
            or canonical_params.get("year")
            or "source-defined"
        ),
        "resolver_primitive": template.resolver_primitive,
        "source": "template",
        "source_url": "",
        "canonical_key": canonical_key,
        "compatible_ops": COMPATIBLE_OPS,
        "exclusivity_group": exclusivity_group_for(template_id, canonical_params),
        "quality": quality,
        "aliases": aliases or [],
    }


def canonicalize_params(template_id: str, params: dict[str, Any]) -> dict[str, Any]:
    out: dict[str, Any] = {}
    for key, value in params.items():
        if isinstance(value, str):
            value = " ".join(value.strip().split())
        if key in {"year", "season"} and isinstance(value, str) and value.isdigit():
            value = int(value)
        if key == "threshold":
            try:
                value = int(value) if float(value).is_integer() else float(value)
            except (TypeError, ValueError):
                pass
        out[key] = value
    required = TEMPLATES[template_id].required_params
    return {key: out[key] for key in required if key in out}


def generate_seed_compositions(atoms: list[dict[str, Any]]) -> list[dict[str, Any]]:
    by_template = group_by(atoms, "template_id")
    by_id = {atom["id"]: atom for atom in atoms}

    def first_ids(template_id: str, pred=lambda _atom: True, n: int = 4) -> list[str]:
        return [atom["id"] for atom in by_template.get(template_id, []) if pred(atom)][:n]

    compositions: list[dict[str, Any]] = []
    add_comp(
        compositions,
        "Republican sweep 2028",
        "R sweep",
        "Will Republicans win the presidency, House, and Senate in 2028?",
        "Template-backed election scenario over contest-winner atoms.",
        {
            "op": "AND",
            "args": [
                {"atom": atom_id}
                for atom_id in first_ids(
                    "contest_winner",
                    lambda a: a["params"].get("option") == "Republican Party"
                    and str(a["params"].get("contest")) in {"US presidency", "US House control", "US Senate control"},
                    3,
                )
            ],
        },
        by_id,
        "politics",
    )
    add_comp(
        compositions,
        "Technical recession 2026",
        "Technical recession",
        "Will at least two selected GDP-growth quarters be negative in 2026?",
        "Two-negative-quarter style macro composition.",
        {"op": "K_OF_N", "k": 2, "args": [{"atom": atom_id} for atom_id in first_ids("macro_indicator_threshold", lambda a: "GDP" in str(a["params"].get("indicator")), 4)]},
        by_id,
        "macro",
    )
    sahm_ids = first_ids("macro_indicator_threshold", lambda a: "Sahm" in str(a["params"].get("indicator")), 1)
    if sahm_ids:
        add_comp(
            compositions,
            "Sahm recession 2026",
            "Sahm recession",
            "Will the unemployment-delta atom indicate recession in 2026?",
            "Single-atom composition used to compare recession definitions.",
            {"atom": sahm_ids[0]},
            by_id,
            "macro",
        )
    add_comp(
        compositions,
        "Iran mainstream invasion",
        "Iran mainstream",
        "Will the Iran conflict meet a mainstream invasion definition?",
        "Requires sustained troops, declaration, or a substantial strike campaign.",
        {
            "op": "OR",
            "args": [
                {"op": "AND", "args": [{"atom": aid} for aid in first_ids("conflict_event_threshold", lambda a: a["params"].get("conflict") == "Iran" and a["params"].get("event_type") in {"troops_on_soil", "troop_presence_duration"}, 2)]},
                *[{"atom": aid} for aid in first_ids("conflict_event_threshold", lambda a: a["params"].get("conflict") == "Iran" and a["params"].get("event_type") in {"formal_declaration", "aumf_passed"}, 2)],
            ],
        },
        by_id,
        "geopolitics",
    )
    add_comp(
        compositions,
        "NBA same-game parlay",
        "NBA parlay",
        "Will a seeded team win and two player stat legs hit?",
        "Parlay-native composition over sports atoms.",
        {"op": "AND", "args": [{"atom": atom_id} for atom_id in first_ids("team_wins_game", n=1) + first_ids("player_stat_over", n=2)]},
        by_id,
        "sports",
    )
    add_comp(
        compositions,
        "AGI benchmark definition",
        "AGI K-of-N",
        "Will at least three selected AI benchmark/economic atoms hit by 2030?",
        "K-of-N AGI operational definition.",
        {"op": "K_OF_N", "k": 3, "args": [{"atom": atom_id} for atom_id in first_ids("ai_benchmark_threshold", n=5)]},
        by_id,
        "technology",
    )
    add_comp(
        compositions,
        "Crypto breakout basket",
        "Crypto basket",
        "Will any selected crypto price-threshold atom hit?",
        "OR basket over major crypto assets.",
        {"op": "OR", "args": [{"atom": atom_id} for atom_id in first_ids("asset_price_threshold", n=4)]},
        by_id,
        "crypto",
    )
    add_comp(
        compositions,
        "Entertainment release basket",
        "Release basket",
        "Will any selected major entertainment releases occur by their deadlines?",
        "Simple already-atomic release basket.",
        {"op": "OR", "args": [{"atom": atom_id} for atom_id in first_ids("entertainment_release", n=4)]},
        by_id,
        "culture",
    )

    # Conditional seed instruments expose the correlation-trading surface.
    conditional_pairs = [
        ("Fed cut given recession", "macro", first_ids("macro_indicator_threshold", lambda a: "Fed funds" in str(a["params"].get("indicator")), 1), first_ids("macro_indicator_threshold", lambda a: "Sahm" in str(a["params"].get("indicator")), 1)),
        ("General win given primary", "politics", first_ids("candidate_wins_general", n=1), first_ids("contest_winner", lambda a: "nomination" in str(a["params"].get("contest")).lower(), 1)),
        ("Player points given team win", "sports", first_ids("player_stat_over", n=1), first_ids("team_wins_game", n=1)),
        ("AI revenue given benchmark", "technology", first_ids("ai_benchmark_threshold", lambda a: "AI coding revenue" in str(a["params"].get("benchmark")), 1), first_ids("ai_benchmark_threshold", n=1)),
    ]
    for title, domain, consequent, antecedent in conditional_pairs:
        if consequent and antecedent:
            add_comp(
                compositions,
                title,
                compact_name(title, 22),
                f"Conditional composition: {title}.",
                "IF_THEN seed instrument for correlation discovery.",
                {"op": "IF_THEN", "args": [{"atom": antecedent[0]}, {"atom": consequent[0]}]},
                by_id,
                domain,
            )

    if not compositions and len(atoms) >= 2:
        add_comp(
            compositions,
            "Template demo basket",
            "Template basket",
            "Will any selected template seed atom resolve YES?",
            "Fallback composition for small generated universes.",
            {"op": "OR", "args": [{"atom": atom["id"]} for atom in atoms[:3]]},
            by_id,
            atoms[0].get("domain", "demo"),
        )

    return compositions


def add_comp(
    out: list[dict[str, Any]],
    title: str,
    short: str,
    question: str,
    description: str,
    formula: dict[str, Any],
    atoms_by_id: dict[str, dict[str, Any]],
    domain: str,
) -> None:
    if not formula_atom_refs(formula) or not formula_references_known_atoms(formula, atoms_by_id):
        return
    values = {atom_id: atom["fair_value"] for atom_id, atom in atoms_by_id.items()}
    fair = estimate_formula(formula, values)
    out.append(
        {
            "id": f"comp_{stable_slug(title)[:52]}",
            "kind": "composition",
            "title": title,
            "short_name": short,
            "question": question,
            "description": description,
            "oracle_path": "Composition over template atoms",
            "formula": formula,
            "author": "Sybil template seed",
            "market_id": None,
            "fair_value": fair,
            "trust_tier": "template-demo",
            "tags": ["composition-demo", "template-composition", domain],
            "domain": domain,
            "atom_type": "composition",
            "template_id": "composition",
            "params": {},
            "subject": title,
            "metric": "formula",
            "comparator": "resolves_true",
            "threshold": None,
            "unit": "",
            "time_window": "template-defined",
            "resolver_primitive": "composition_resolution",
            "source": "template",
            "source_url": "",
            "canonical_key": f"composition:{stable_slug(title)}",
            "compatible_ops": COMPATIBLE_OPS,
            "exclusivity_group": None,
            "quality": "seed",
            "aliases": [],
        }
    )


def match_source_aliases(
    snapshot: dict[str, Any],
    atoms: list[dict[str, Any]],
) -> tuple[dict[str, list[dict[str, Any]]], list[dict[str, Any]]]:
    by_key = {atom["canonical_key"]: atom for atom in atoms}
    aliases: dict[str, list[dict[str, Any]]] = {}
    unmatched: list[dict[str, Any]] = []

    for event in snapshot.get("polymarket_events", []):
        event_title = event.get("title") or ""
        for market in event.get("markets", []):
            question = market.get("question") or market.get("title") or ""
            matched = source_question_to_atom("polymarket", question, event_title)
            price = parse_yes_price(market.get("outcomePrices"))
            alias = {
                "source": "polymarket",
                "source_id": str(market.get("conditionId") or market.get("id") or question),
                "question": question,
                "event_title": event_title,
                "url": f"https://polymarket.com/event/{event.get('slug', '')}",
                "fair_value": price,
            }
            if matched and matched["canonical_key"] in by_key:
                aliases.setdefault(matched["canonical_key"], []).append(alias)
            else:
                unmatched.append(alias)

    for market in snapshot.get("kalshi_markets", []):
        title = market.get("title") or ""
        for leg in split_kalshi_legs(title):
            matched = source_question_to_atom("kalshi", leg, title)
            alias = {
                "source": "kalshi",
                "source_id": str(market.get("ticker") or title),
                "question": leg,
                "event_title": title,
                "url": f"https://kalshi.com/markets/{market.get('ticker', '')}",
                "fair_value": parse_kalshi_mid(market),
            }
            if matched and matched["canonical_key"] in by_key:
                aliases.setdefault(matched["canonical_key"], []).append(alias)
            else:
                unmatched.append(alias)

    return aliases, unmatched


def source_question_to_atom(source: str, question: str, event_title: str = "") -> dict[str, Any] | None:
    q = question.strip().removeprefix("yes ").strip()
    patterns = [
        (
            r"^Will (?P<option>.+?) win the (?P<year>20\d{2}) Democratic presidential nomination\??$",
            lambda m: ("contest_winner", {"contest": "Democratic presidential nomination", "year": int(m["year"]), "option": m["option"]}, "politics"),
        ),
        (
            r"^Will (?P<option>.+?) win the (?P<year>20\d{2}) Republican presidential nomination\??$",
            lambda m: ("contest_winner", {"contest": "Republican presidential nomination", "year": int(m["year"]), "option": m["option"]}, "politics"),
        ),
        (
            r"^Will (?P<option>.+?) win the (?P<year>20\d{2}) US Presidential Election\??$",
            lambda m: ("candidate_wins_general", {"candidate": m["option"], "year": int(m["year"])}, "politics"),
        ),
        (
            r"^Will (?P<option>.+?) win the (?P<year>20\d{2}) FIFA World Cup\??$",
            lambda m: ("contest_winner", {"contest": "FIFA World Cup", "year": int(m["year"]), "option": m["option"]}, "sports"),
        ),
        (
            r"^Will (?P<option>.+?) win the (?P<year>20\d{2}) NBA Finals\??$",
            lambda m: ("team_wins_championship", {"league": "NBA", "team": m["option"], "season": int(m["year"])}, "sports"),
        ),
    ]
    for pattern, builder in patterns:
        match = re.match(pattern, q)
        if match:
            template_id, params, domain = builder(match.groupdict())
            return build_atom(template_id, params, fair_value=0.5, quality="source_matched", domain_hint=domain)

    kalshi_prop = re.match(r"^(?P<player>[A-Z][^:]{2,40}): (?P<threshold>\d+(?:\.\d+)?)\+$", q)
    if source == "kalshi" and kalshi_prop:
        return build_atom(
            "player_stat_over",
            {
                "league": infer_league(event_title),
                "player": kalshi_prop.group("player").strip(),
                "stat": "points_or_stat_count",
                "threshold": float(kalshi_prop.group("threshold")),
                "period": infer_period(event_title),
            },
            fair_value=0.5,
            quality="source_matched",
            domain_hint="sports",
        )
    return None


def politics_specs() -> list[tuple[str, dict[str, Any], float, str]]:
    dem = [
        "Gavin Newsom",
        "Gretchen Whitmer",
        "Josh Shapiro",
        "Pete Buttigieg",
        "Alexandria Ocasio-Cortez",
        "Jared Polis",
        "Andy Beshear",
        "Cory Booker",
        "Wes Moore",
        "Raphael Warnock",
        "Amy Klobuchar",
        "JB Pritzker",
        "Jon Ossoff",
        "Michelle Obama",
        "Ro Khanna",
    ]
    rep = [
        "JD Vance",
        "Ron DeSantis",
        "Nikki Haley",
        "Vivek Ramaswamy",
        "Marco Rubio",
        "Glenn Youngkin",
        "Josh Hawley",
        "Ted Cruz",
        "Kristi Noem",
        "Tucker Carlson",
        "Donald Trump Jr.",
        "Tim Scott",
        "Elise Stefanik",
        "Mike Johnson",
        "Sarah Huckabee Sanders",
    ]
    specs = []
    for i, candidate in enumerate(dem):
        specs.append(("contest_winner", {"contest": "Democratic presidential nomination", "option": candidate, "year": 2028}, 0.05 + i * 0.01, "politics"))
        specs.append(("candidate_wins_general", {"candidate": candidate, "year": 2028}, 0.03 + i * 0.006, "politics"))
    for i, candidate in enumerate(rep):
        specs.append(("contest_winner", {"contest": "Republican presidential nomination", "option": candidate, "year": 2028}, 0.06 + i * 0.008, "politics"))
        specs.append(("candidate_wins_general", {"candidate": candidate, "year": 2028}, 0.035 + i * 0.005, "politics"))
    specs.extend(
        [
            ("contest_winner", {"contest": "US presidency", "year": 2028, "option": "Republican Party"}, 0.48, "politics"),
            ("contest_winner", {"contest": "US presidency", "year": 2028, "option": "Democratic Party"}, 0.50, "politics"),
            ("contest_winner", {"contest": "US House control", "year": 2028, "option": "Republican Party"}, 0.52, "politics"),
            ("contest_winner", {"contest": "US House control", "year": 2028, "option": "Democratic Party"}, 0.48, "politics"),
            ("contest_winner", {"contest": "US Senate control", "year": 2028, "option": "Republican Party"}, 0.55, "politics"),
            ("contest_winner", {"contest": "US Senate control", "year": 2028, "option": "Democratic Party"}, 0.45, "politics"),
        ]
    )
    return specs[:70]


def macro_specs() -> list[tuple[str, dict[str, Any], float, str]]:
    specs = []
    for quarter, fair in zip(["2026-Q1", "2026-Q2", "2026-Q3", "2026-Q4"], [0.22, 0.28, 0.31, 0.34]):
        specs.append(("macro_indicator_threshold", {"indicator": "Real GDP growth", "comparator": "<", "threshold": 0, "unit": "%", "period": quarter}, fair, "macro"))
    for threshold in [4.5, 5.0, 5.5, 6.0, 6.5]:
        specs.append(("macro_indicator_threshold", {"indicator": "US unemployment rate", "comparator": ">=", "threshold": threshold, "unit": "%", "period": "any month in 2026"}, 0.38 - threshold * 0.035, "macro"))
    specs.append(("macro_indicator_threshold", {"indicator": "Sahm rule unemployment delta", "comparator": ">=", "threshold": 0.5, "unit": "pp", "period": "2026"}, 0.29, "macro"))
    for threshold in [3.0, 3.5, 4.0, 4.5]:
        specs.append(("macro_indicator_threshold", {"indicator": "Core CPI YoY", "comparator": ">=", "threshold": threshold, "unit": "%", "period": "Dec 2026"}, 0.45 - threshold * 0.06, "macro"))
    for threshold in [4000, 4500, 5000, 5500, 6000]:
        specs.append(("macro_indicator_threshold", {"indicator": "S&P 500 close", "comparator": ">=", "threshold": threshold, "unit": "index points", "period": "Dec 31 2026"}, 0.75 - threshold / 15000, "macro"))
    for threshold in [15, 20, 25, 30]:
        specs.append(("macro_indicator_threshold", {"indicator": "S&P 500 peak-to-trough drawdown", "comparator": ">=", "threshold": threshold, "unit": "%", "period": "2026"}, 0.42 - threshold / 100, "macro"))
    for threshold in [25, 30, 35, 40]:
        specs.append(("macro_indicator_threshold", {"indicator": "VIX close", "comparator": ">=", "threshold": threshold, "unit": "", "period": "any day in 2026"}, 0.45 - threshold / 120, "macro"))
    for threshold in [0, 1, 2, 3, 4]:
        specs.append(("macro_indicator_threshold", {"indicator": "Fed funds target upper bound", "comparator": "<=", "threshold": threshold, "unit": "%", "period": "Dec 2026"}, 0.12 + threshold * 0.12, "macro"))
    indicators = ["Industrial production YoY", "Retail sales YoY", "Nonfarm payrolls monthly change"]
    for indicator in indicators:
        for threshold in [-1, 0, 1, 2]:
            specs.append(("macro_indicator_threshold", {"indicator": indicator, "comparator": "<", "threshold": threshold, "unit": "%", "period": "any 2026 release"}, 0.22 + abs(threshold) * 0.05, "macro"))
    return specs[:45]


def sports_specs() -> list[tuple[str, dict[str, Any], float, str]]:
    teams = ["Thunder", "Celtics", "Nuggets", "Knicks", "Lakers", "Warriors", "Timberwolves", "Bucks", "Mavericks", "76ers"]
    specs = [("team_wins_championship", {"league": "NBA", "team": team, "season": 2026}, 0.18 if team == "Thunder" else 0.04 + i * 0.015, "sports") for i, team in enumerate(teams)]
    games = ["NBA-2026-OKC-LAL", "NBA-2026-BOS-NYK", "NBA-2026-DEN-MIN", "NBA-2026-GSW-DAL", "NBA-2026-MIL-PHI"]
    for game in games:
        for team in game.split("-")[-2:]:
            specs.append(("team_wins_game", {"league": "NBA", "team": team, "game_id": game}, 0.5, "sports"))
    players = ["Shai Gilgeous-Alexander", "Nikola Jokic", "Jayson Tatum", "LeBron James", "Anthony Edwards", "Luka Doncic", "Giannis Antetokounmpo", "Stephen Curry", "Jalen Brunson", "Tyrese Maxey"]
    for player in players:
        for stat, threshold in [("points", 25), ("assists", 7), ("rebounds", 8)]:
            specs.append(("player_stat_over", {"league": "NBA", "player": player, "stat": stat, "threshold": threshold, "period": "next listed game"}, 0.48, "sports"))
    return specs[:45]


def geopolitics_specs() -> list[tuple[str, dict[str, Any], float, str]]:
    specs = []
    conflicts = ["Iran", "Ukraine", "Taiwan Strait", "Red Sea"]
    event_defs = [
        ("troops_on_soil", [1, 100, 1000, 5000], "personnel"),
        ("troop_presence_duration", [24, 72, 168], "hours"),
        ("kinetic_strikes", [1, 10, 50, 100], "strikes"),
        ("formal_declaration", [1], "declaration"),
        ("aumf_passed", [1], "authorization"),
        ("territorial_occupation_declared", [1], "declaration"),
    ]
    for conflict in conflicts:
        for event_type, thresholds, unit in event_defs:
            for threshold in thresholds:
                specs.append(("conflict_event_threshold", {"conflict": conflict, "event_type": event_type, "comparator": ">=", "threshold": threshold, "unit": unit, "period": "before 2027"}, 0.35 / max(1, threshold if threshold > 10 else 1), "geopolitics"))
    return specs[:40]


def ai_specs() -> list[tuple[str, dict[str, Any], float, str]]:
    specs = []
    benchmarks = [("ARC-AGI-3", "human-level"), ("FrontierMath", "50%"), ("SWE-bench verified", "95%"), ("MMLU-Pro", "90%"), ("GPQA Diamond", "85%"), ("AI coding revenue", "$50B"), ("Remote worker displacement", "1M jobs")]
    for benchmark, threshold in benchmarks:
        for by_date in ["2027-12-31", "2030-12-31"]:
            specs.append(("ai_benchmark_threshold", {"benchmark": benchmark, "threshold": threshold, "unit": "", "by_date": by_date}, 0.2 if "2030" in by_date else 0.08, "technology"))
    labs = ["OpenAI", "Anthropic", "Google DeepMind", "Meta", "xAI", "Mistral", "Safe Superintelligence"]
    for lab in labs:
        for claim in ["AGI achieved", "autonomous research agent", "frontier model pause"]:
            specs.append(("ai_lab_announcement", {"lab": lab, "claim_type": claim, "by_date": "2030-12-31"}, 0.18, "technology"))
    return specs[:35]


def crypto_specs() -> list[tuple[str, dict[str, Any], float, str]]:
    specs = []
    for asset, thresholds in {"BTC": [50000, 75000, 100000, 150000, 200000], "ETH": [2500, 4000, 6000, 8000], "SOL": [100, 200, 300], "HYPE": [25, 50, 100], "BNB": [500, 750, 1000]}.items():
        for threshold in thresholds:
            specs.append(("asset_price_threshold", {"asset": asset, "comparator": ">=", "threshold": threshold, "quote_currency": "USD", "by_date": "2026-12-31"}, 0.65 - min(threshold, 100000) / 220000, "crypto"))
    for asset in ["BTC", "ETH", "SOL", "HYPE", "BNB"]:
        specs.append(("asset_price_threshold", {"asset": asset, "comparator": "<=", "threshold": 0.5, "quote_currency": "cycle_high_ratio", "by_date": "2026-12-31"}, 0.22, "crypto"))
    return specs[:30]


def culture_specs() -> list[tuple[str, dict[str, Any], float, str]]:
    releases = [
        ("Drake", "Iceman", "album"),
        ("Taylor Swift", "next studio album", "album"),
        ("Nintendo", "next 3D Mario", "game"),
        ("Rockstar Games", "GTA VI", "game"),
        ("Marvel Studios", "Avengers: Doomsday", "film"),
        ("A24", "highest grossing 2026 release", "film"),
        ("Netflix", "Stranger Things finale", "series"),
        ("HBO", "House of the Dragon S3", "series"),
        ("Beyonce", "next tour film", "film"),
        ("OpenAI", "consumer hardware device", "product"),
    ]
    specs = []
    for artist, work, release_type in releases:
        for by_date in ["2026-06-30", "2026-12-31"]:
            specs.append(("entertainment_release", {"artist_or_studio": artist, "work": work, "release_type": release_type, "by_date": by_date}, 0.55, "culture"))
    for i in range(5):
        specs.append(("entertainment_release", {"artist_or_studio": f"Studio {i+1}", "work": f"Untitled franchise sequel {i+1}", "release_type": "film", "by_date": "2027-12-31"}, 0.35, "culture"))
    return specs[:25]


def split_kalshi_legs(title: str) -> list[str]:
    normalized = title.replace("YES ", "yes ").replace("Yes ", "yes ")
    if ",yes " not in normalized:
        return [normalized.strip()]
    return [part.strip() for part in normalized.split(",") if part.strip()]


def alias_fair_value(aliases: list[dict[str, Any]], fallback: float) -> float:
    prices = [float(alias["fair_value"]) for alias in aliases if alias.get("fair_value") is not None]
    if not prices:
        return fallback
    return clamp(sum(prices) / len(prices))


def parse_yes_price(raw: Any) -> float:
    if raw is None:
        return 0.5
    try:
        values = json.loads(raw) if isinstance(raw, str) else raw
        return clamp(float(values[0]))
    except Exception:
        return 0.5


def parse_kalshi_mid(market: dict[str, Any]) -> float:
    bid = market.get("yes_bid")
    ask = market.get("yes_ask")
    if isinstance(bid, (int, float)) and isinstance(ask, (int, float)) and ask > 0:
        return clamp((float(bid) + float(ask)) / 200.0)
    return 0.5


def resolver_label(resolver: str) -> str:
    return {
        "source_result_signed": "Trusted result source + demo resolver",
        "election_result_signed": "Election result feed + demo resolver",
        "sports_feed_signed": "Sports data feed + demo resolver",
        "economic_data_feed_signed": "Economic data feed + demo resolver",
        "wire_service_or_admin": "Wire-service evidence + demo resolver",
        "benchmark_or_tee_llm_graded": "Benchmark feed or TEE-graded evidence",
        "tee_llm_graded_with_sources": "TEE LLM grader with cited sources",
        "market_data_feed_signed": "Market data feed + demo resolver",
        "media_platform_or_wire_signed": "Media platform/wire feed + demo resolver",
    }.get(resolver, resolver)


def subject_for(template_id: str, params: dict[str, Any]) -> str:
    return str(
        params.get("option")
        or params.get("candidate")
        or params.get("team")
        or params.get("player")
        or params.get("indicator")
        or params.get("conflict")
        or params.get("benchmark")
        or params.get("asset")
        or params.get("work")
        or template_id
    )


def metric_for(template_id: str, params: dict[str, Any]) -> str:
    return str(params.get("stat") or params.get("indicator") or params.get("event_type") or template_id)


def exclusivity_group_for(template_id: str, params: dict[str, Any]) -> str | None:
    if template_id == "contest_winner":
        return f"contest:{params.get('year')}:{params.get('contest')}"
    if template_id == "candidate_wins_primary":
        return f"primary:{params.get('year')}:{params.get('party')}"
    if template_id == "candidate_wins_general":
        return f"general:{params.get('year')}"
    if template_id == "team_wins_championship":
        return f"championship:{params.get('season')}:{params.get('league')}"
    return None


def group_by(items: list[dict[str, Any]], key: str) -> dict[str, list[dict[str, Any]]]:
    out: dict[str, list[dict[str, Any]]] = {}
    for item in items:
        out.setdefault(str(item.get(key, "")), []).append(item)
    return out


def formula_references_known_atoms(formula: dict[str, Any], atoms_by_id: dict[str, dict[str, Any]]) -> bool:
    if "atom" in formula:
        return formula["atom"] in atoms_by_id
    args = formula.get("args", [])
    return bool(args) and all(formula_references_known_atoms(arg, atoms_by_id) for arg in args)


def formula_atom_refs(formula: dict[str, Any]) -> list[str]:
    if "atom" in formula:
        return [str(formula["atom"])]
    refs: list[str] = []
    for arg in formula.get("args", []):
        refs.extend(formula_atom_refs(arg))
    return refs


def estimate_formula(formula: dict[str, Any], values: dict[str, float]) -> float:
    if "atom" in formula:
        return values.get(formula["atom"], 0.5)
    parts = [estimate_formula(arg, values) for arg in formula.get("args", [])]
    op = formula.get("op")
    if not parts:
        return 0.5
    if op == "AND":
        out = 1.0
        for part in parts:
            out *= part
        return clamp(out)
    if op == "OR":
        fail = 1.0
        for part in parts:
            fail *= 1.0 - part
        return clamp(1.0 - fail)
    if op == "K_OF_N":
        return clamp(sum(parts) / max(1, len(parts)))
    if op == "IF_THEN" and len(parts) >= 2:
        return clamp(1.0 - parts[0] * (1.0 - parts[1]))
    return clamp(sum(parts) / len(parts))


def infer_league(text: str) -> str:
    lower = text.lower()
    if any(word in lower for word in ["nba", "jaylen", "jayson", "lebron", "durant", "nikola"]):
        return "NBA"
    if any(word in lower for word in ["mlb", "pitcher", "strikeout"]):
        return "MLB"
    if "nfl" in lower:
        return "NFL"
    return "sports"


def infer_period(text: str) -> str:
    return "source-listed game" if text else "next listed game"


def canonical_json(params: dict[str, Any]) -> str:
    return json.dumps(params, ensure_ascii=True, sort_keys=True, separators=(",", ":"))


def stable_slug(value: str) -> str:
    digest = hashlib.sha1(value.encode("utf-8")).hexdigest()[:10]
    out = []
    last = False
    for ch in str(value).lower():
        if ch.isalnum():
            out.append(ch)
            last = False
        elif not last:
            out.append("_")
            last = True
    return f"{''.join(out).strip('_')[:72]}_{digest}" or digest


def compact_name(text: str, limit: int) -> str:
    clean = " ".join(str(text).split())
    return clean if len(clean) <= limit else f"{clean[: max(0, limit - 1)].rstrip()}."


def numeric_or_none(value: Any) -> float | None:
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def clamp(value: float) -> float:
    return max(0.01, min(0.99, float(value)))


def count_atoms(instruments: list[dict[str, Any]]) -> int:
    return len([item for item in instruments if item["kind"] == "atom"])
