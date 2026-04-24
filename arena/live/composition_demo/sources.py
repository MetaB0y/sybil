"""Curated v4 graph universe for the composition demo.

Source markets remain aliases/evidence. The canonical ontology starts at
measurements and derives reusable conditions and propositions from them.
"""

from __future__ import annotations

import json
import re
import time
from pathlib import Path
from typing import Any

from .registry import (
    Condition,
    Context,
    DataFeed,
    Entity,
    Measurement,
    Proposition,
    canonical_json,
    clamp_probability,
    condition_key,
    estimate_formula_value,
    formula_conditions,
    measurement_key,
    proposition_key,
    stable_id,
)

SNAPSHOT_PATH = Path(__file__).resolve().parent / "source_snapshot.json"
GENERATED_PATH = Path(__file__).resolve().parent / "generated_registry.json"
COMPATIBLE_OPS = ["AND", "OR", "NOT", "K_OF_N", "IF_THEN"]


def import_universe(max_atoms: int = 110, force: bool = False) -> dict[str, Any]:
    if GENERATED_PATH.exists() and not force:
        with GENERATED_PATH.open("r", encoding="utf-8") as f:
            cached = json.load(f)
        counts = cached.get("source_counts", {})
        if (
            cached.get("universe_version") == 4
            and counts.get("entities", 0) >= 20
            and counts.get("contexts", 0) >= 8
            and counts.get("measurements", 0) >= 50
            and counts.get("conditions", 0) + counts.get("propositions", 0) >= 80
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
    SNAPSHOT_PATH.write_text(json.dumps(snapshot, indent=2, sort_keys=True), encoding="utf-8")
    return snapshot


def build_universe(snapshot: dict[str, Any], max_atoms: int = 110) -> dict[str, Any]:
    feeds = seed_feeds()
    entities = seed_entities()
    contexts = seed_contexts()
    measurements = seed_measurements(feeds)
    conditions = seed_conditions(measurements)
    attach_source_aliases(snapshot, conditions)
    implication_edges = generate_implication_edges(conditions)
    propositions = seed_propositions(conditions)
    instruments = conditions + propositions
    return {
        "universe_version": 4,
        "created_at": time.time(),
        "feeds": feeds,
        "entities": entities,
        "contexts": contexts,
        "measurements": measurements,
        "conditions": conditions,
        "propositions": propositions,
        "markets": [],
        "instruments": instruments,
        "implication_edges": implication_edges,
        "source_counts": {
            "feeds": len(feeds),
            "entities": len(entities),
            "contexts": len(contexts),
            "measurements": len(measurements),
            "conditions": len(conditions),
            "propositions": len(propositions),
            "source_aliases": sum(len(item.get("aliases", [])) for item in conditions),
            "polymarket_events": len(snapshot.get("polymarket_events", [])),
            "kalshi_markets": len(snapshot.get("kalshi_markets", [])),
            "unmatched_sources": 0,
        },
        "source_errors": snapshot.get("errors", []),
        "unmatched_sources": [],
    }


def seed_feeds() -> list[dict[str, Any]]:
    return [
        DataFeed("pyth", "Pyth Network", "crypto", "signed", "signed_price_feed", "Signed crypto spot feeds.").to_dict(),
        DataFeed("chainlink", "Chainlink", "crypto", "signed", "signed_price_feed", "Signed crypto and index feeds.").to_dict(),
        DataFeed("fred", "FRED/BLS/BEA", "macro", "official", "economic_data_feed_signed", "US official economic releases.").to_dict(),
        DataFeed("election_wire", "AP/FEC election feed", "politics", "official", "source_result_signed", "Election calls and certified results.").to_dict(),
        DataFeed("wire", "Reuters/AP wire", "geopolitics", "trusted", "wire_service_or_admin", "Newswire event evidence.").to_dict(),
        DataFeed("sportsdata", "SportsDataIO demo", "sports", "signed", "sports_feed_signed", "Game and player stat feed.").to_dict(),
    ]


def seed_entities() -> list[dict[str, Any]]:
    specs = [
        ("eth", "asset", "ETH", "crypto", ["Ethereum"], {"coingecko": "ethereum"}),
        ("btc", "asset", "BTC", "crypto", ["Bitcoin"], {"coingecko": "bitcoin"}),
        ("sol", "asset", "SOL", "crypto", ["Solana"], {"coingecko": "solana"}),
        ("crypto_market", "sector", "Crypto market", "crypto", [], {}),
        ("stablecoins", "sector", "Stablecoins", "crypto", [], {}),
        ("btc_spot_etfs", "fund_group", "BTC spot ETFs", "crypto", [], {}),
        ("us_economy", "economy", "US economy", "macro", [], {}),
        ("federal_reserve", "institution", "Federal Reserve", "macro", ["Fed"], {}),
        ("sp500", "index", "S&P 500", "macro", ["SPX"], {}),
        ("nasdaq100", "index", "Nasdaq 100", "macro", ["NDX"], {}),
        ("vix", "index", "VIX", "macro", [], {}),
        ("wti_crude", "commodity", "WTI crude oil", "macro", [], {}),
        ("gold", "commodity", "Gold", "macro", [], {}),
        ("dxy", "currency_index", "US Dollar Index", "macro", ["DXY"], {}),
        ("democratic_party", "political_party", "Democratic Party", "politics", ["Democrats"], {}),
        ("republican_party", "political_party", "Republican Party", "politics", ["GOP", "Republicans"], {}),
        ("gavin_newsom", "person", "Gavin Newsom", "politics", [], {}),
        ("gretchen_whitmer", "person", "Gretchen Whitmer", "politics", [], {}),
        ("jd_vance", "person", "JD Vance", "politics", [], {}),
        ("iran", "country", "Iran", "geopolitics", [], {}),
        ("united_states", "country", "United States", "geopolitics", ["US", "USA"], {}),
        ("strait_of_hormuz", "place", "Strait of Hormuz", "geopolitics", [], {}),
        ("nba", "league", "NBA", "sports", [], {}),
        ("boston_celtics", "team", "Boston Celtics", "sports", ["Celtics"], {}),
        ("new_york_knicks", "team", "New York Knicks", "sports", ["Knicks"], {}),
        ("jayson_tatum", "player", "Jayson Tatum", "sports", ["Tatum"], {}),
        ("jaylen_brown", "player", "Jaylen Brown", "sports", ["Brown"], {}),
        ("jalen_brunson", "player", "Jalen Brunson", "sports", ["Brunson"], {}),
    ]
    return [
        Entity(
            id=entity_id,
            kind=kind,
            name=name,
            domain=domain,
            aliases=aliases,
            external_refs=external_refs,
            description=f"{name} entity for composition-demo ontology.",
        ).to_dict()
        for entity_id, kind, name, domain, aliases, external_refs in specs
    ]


def seed_contexts() -> list[dict[str, Any]]:
    specs = [
        ("ctx_2026", "year", "2026", "macro", "Calendar year 2026.", [], "2026-01-01", "2026-12-31"),
        ("ctx_2026_q1", "quarter", "2026 Q1", "macro", "First quarter of 2026.", [], "2026-01-01", "2026-03-31"),
        ("ctx_2026_q2", "quarter", "2026 Q2", "macro", "Second quarter of 2026.", [], "2026-04-01", "2026-06-30"),
        ("ctx_2026_q3", "quarter", "2026 Q3", "macro", "Third quarter of 2026.", [], "2026-07-01", "2026-09-30"),
        ("ctx_2026_q4", "quarter", "2026 Q4", "macro", "Fourth quarter of 2026.", [], "2026-10-01", "2026-12-31"),
        ("ctx_before_2027", "window", "Before 2027", "geopolitics", "Observation window ending before 2027.", [], "", "2026-12-31"),
        ("ctx_2028_nomination", "election_cycle", "2028 presidential nominations", "politics", "US presidential nomination cycle.", ["democratic_party", "republican_party"], "", "2028-08-31"),
        ("ctx_2028_general", "election", "2028 US general election", "politics", "US presidential and congressional general election.", ["democratic_party", "republican_party"], "2028-11-07", "2028-12-31"),
        ("ctx_nba_nyk_bos_2026_04_30", "nba_game", "Knicks at Celtics, 2026-04-30", "sports", "NBA game context for seeded same-game markets.", ["nba", "new_york_knicks", "boston_celtics"], "2026-04-30", "2026-04-30"),
    ]
    return [
        Context(
            id=context_id,
            kind=kind,
            title=title,
            domain=domain,
            description=description,
            entity_ids=entity_ids,
            start=start,
            end=end,
        ).to_dict()
        for context_id, kind, title, domain, description, entity_ids, start, end in specs
    ]


def seed_measurements(feeds: list[dict[str, Any]]) -> list[dict[str, Any]]:
    specs = [
        ("crypto", "price", "ETH/USD spot", "USD", ["pyth", "chainlink"], "intraday max/min/close"),
        ("crypto", "price", "BTC/USD spot", "USD", ["pyth", "chainlink"], "intraday max/min/close"),
        ("crypto", "price", "SOL/USD spot", "USD", ["pyth", "chainlink"], "intraday max/min/close"),
        ("crypto", "market_cap", "Total crypto market cap", "USD", ["chainlink"], "daily close"),
        ("macro", "economic_indicator", "US real GDP growth", "% annualized", ["fred"], "quarterly released value"),
        ("macro", "economic_indicator", "US unemployment rate", "%", ["fred"], "monthly released value"),
        ("macro", "economic_indicator", "Sahm rule indicator", "pp", ["fred"], "monthly released value"),
        ("macro", "economic_indicator", "US CPI YoY", "%", ["fred"], "monthly released value"),
        ("macro", "rate", "Fed funds target upper bound", "%", ["fred"], "FOMC target"),
        ("macro", "index", "S&P 500", "index points", ["chainlink"], "daily close and drawdown"),
        ("macro", "index", "VIX", "index points", ["chainlink"], "daily close and max"),
        ("politics", "election_outcome", "2028 Democratic presidential nominee", "candidate", ["election_wire"], "certified/called winner"),
        ("politics", "election_outcome", "2028 Republican presidential nominee", "candidate", ["election_wire"], "certified/called winner"),
        ("politics", "election_outcome", "2028 US presidential winner", "party/candidate", ["election_wire"], "certified/called winner"),
        ("politics", "election_outcome", "2028 US House control", "party", ["election_wire"], "certified/called control"),
        ("politics", "election_outcome", "2028 US Senate control", "party", ["election_wire"], "certified/called control"),
        ("geopolitics", "conflict_event", "Iran US troop count", "troops", ["wire"], "max confirmed count"),
        ("geopolitics", "conflict_event", "Iran US troop presence duration", "hours", ["wire"], "max continuous duration"),
        ("geopolitics", "conflict_event", "Iran US strike count", "strikes", ["wire"], "cumulative count"),
        ("geopolitics", "legal_action", "US Iran war authorization", "action", ["wire"], "official action"),
        ("geopolitics", "territorial_control", "US control of Iranian territory", "status", ["wire"], "official/confirmed control"),
        ("sports", "game_result", "NBA Knicks at Celtics 2026-04-30 winner", "team", ["sportsdata"], "final score"),
        ("sports", "player_stat", "Jayson Tatum points vs Knicks 2026-04-30", "points", ["sportsdata"], "box score"),
        ("sports", "player_stat", "Jaylen Brown rebounds vs Knicks 2026-04-30", "rebounds", ["sportsdata"], "box score"),
        ("sports", "player_stat", "Jalen Brunson assists vs Celtics 2026-04-30", "assists", ["sportsdata"], "box score"),
        ("crypto", "volatility", "ETH/USD 30-day realized volatility", "% annualized", ["pyth", "chainlink"], "rolling realized volatility"),
        ("crypto", "dominance", "BTC market-cap dominance", "%", ["chainlink"], "daily close"),
        ("crypto", "supply", "Stablecoin circulating supply", "USD", ["chainlink"], "daily close"),
        ("crypto", "flow", "BTC spot ETF net flow", "USD", ["wire"], "daily net flow"),
        ("macro", "rate", "US 10-year Treasury yield", "%", ["fred"], "daily close"),
        ("macro", "rate", "US 2-year Treasury yield", "%", ["fred"], "daily close"),
        ("macro", "spread", "US 10Y-2Y Treasury spread", "bp", ["fred"], "daily close"),
        ("macro", "commodity_price", "WTI crude oil spot", "USD/bbl", ["chainlink"], "daily close"),
        ("macro", "commodity_price", "Gold spot", "USD/oz", ["chainlink"], "daily close"),
        ("macro", "currency_index", "US Dollar Index DXY", "index points", ["chainlink"], "daily close"),
        ("macro", "economic_indicator", "US initial jobless claims", "claims", ["fred"], "weekly released value"),
        ("macro", "economic_indicator", "US nonfarm payrolls", "jobs", ["fred"], "monthly released value"),
        ("macro", "economic_indicator", "UMich consumer sentiment", "index points", ["fred"], "monthly released value"),
        ("macro", "index", "Nasdaq 100", "index points", ["chainlink"], "daily close and drawdown"),
        ("politics", "polling", "2028 Democratic national primary polling", "%", ["election_wire"], "polling average"),
        ("politics", "polling", "2028 Republican national primary polling", "%", ["election_wire"], "polling average"),
        ("politics", "approval", "US presidential approval", "%", ["election_wire"], "polling average"),
        ("politics", "turnout", "2028 US presidential turnout", "voters", ["election_wire"], "certified turnout"),
        ("politics", "election_margin", "2028 US presidential popular vote margin", "pp", ["election_wire"], "certified margin"),
        ("geopolitics", "sanctions", "US Iran sanctions package count", "packages", ["wire"], "cumulative official actions"),
        ("geopolitics", "diplomacy", "Iran nuclear talks status", "status", ["wire"], "confirmed diplomatic status"),
        ("geopolitics", "security_event", "Strait of Hormuz tanker incident count", "incidents", ["wire"], "cumulative count"),
        ("geopolitics", "security_event", "Iran-linked regional casualty count", "casualties", ["wire"], "cumulative confirmed count"),
        ("sports", "game_total", "NBA Knicks at Celtics 2026-04-30 total points", "points", ["sportsdata"], "final score total"),
        ("sports", "injury_status", "Jayson Tatum injury status vs Knicks 2026-04-30", "status", ["sportsdata"], "pre-game injury report"),
    ]
    rows: list[dict[str, Any]] = []
    feed_domains = {feed["id"]: feed["domain"] for feed in feeds}
    for domain, kind, subject, unit, feed_ids, aggregation in specs:
        resolver = resolver_for_feed(feed_ids[0])
        meta = measurement_metadata(subject, kind, domain)
        key = measurement_key(
            {
                "measurement_kind": kind,
                "subject": subject,
                "unit": unit,
                "feed_ids": feed_ids,
                "aggregation_semantics": aggregation,
            }
        )
        row = Measurement(
            id=stable_id("meas", key),
            domain=domain or feed_domains.get(feed_ids[0], "demo"),
            measurement_kind=kind,
            subject=subject,
            unit=unit,
            feed_ids=feed_ids,
            aggregation_semantics=aggregation,
            title=meta["display_title"],
            description=meta["description"],
            resolver_primitive=resolver,
            trust_tier="official" if domain in {"macro", "politics"} else "signed",
            canonical_key=key,
            entity_ids=meta["entity_ids"],
            context_id=meta["context_id"],
            path=meta["path"],
            display_title=meta["display_title"],
        ).to_dict()
        rows.append(row)
    return rows


def measurement_metadata(subject: str, kind: str, domain: str) -> dict[str, Any]:
    if subject == "Jayson Tatum points vs Knicks 2026-04-30":
        return path_meta(
            "NBA / Knicks at Celtics / Jayson Tatum / points",
            ["nba", "ctx_nba_nyk_bos_2026_04_30", "jayson_tatum", "points"],
            ["nba", "boston_celtics", "new_york_knicks", "jayson_tatum"],
            "ctx_nba_nyk_bos_2026_04_30",
            "Final box-score points for Jayson Tatum in Knicks at Celtics.",
        )
    if subject == "Jaylen Brown rebounds vs Knicks 2026-04-30":
        return path_meta(
            "NBA / Knicks at Celtics / Jaylen Brown / rebounds",
            ["nba", "ctx_nba_nyk_bos_2026_04_30", "jaylen_brown", "rebounds"],
            ["nba", "boston_celtics", "new_york_knicks", "jaylen_brown"],
            "ctx_nba_nyk_bos_2026_04_30",
            "Final box-score rebounds for Jaylen Brown in Knicks at Celtics.",
        )
    if subject == "Jalen Brunson assists vs Celtics 2026-04-30":
        return path_meta(
            "NBA / Knicks at Celtics / Jalen Brunson / assists",
            ["nba", "ctx_nba_nyk_bos_2026_04_30", "jalen_brunson", "assists"],
            ["nba", "boston_celtics", "new_york_knicks", "jalen_brunson"],
            "ctx_nba_nyk_bos_2026_04_30",
            "Final box-score assists for Jalen Brunson in Knicks at Celtics.",
        )
    if subject == "Jayson Tatum injury status vs Knicks 2026-04-30":
        return path_meta(
            "NBA / Knicks at Celtics / Jayson Tatum / injury status",
            ["nba", "ctx_nba_nyk_bos_2026_04_30", "jayson_tatum", "injury_status"],
            ["nba", "boston_celtics", "new_york_knicks", "jayson_tatum"],
            "ctx_nba_nyk_bos_2026_04_30",
            "Pre-game injury-report status for Jayson Tatum before Knicks at Celtics.",
        )
    if subject == "NBA Knicks at Celtics 2026-04-30 winner":
        return path_meta(
            "NBA / Knicks at Celtics / game winner",
            ["nba", "ctx_nba_nyk_bos_2026_04_30", "game_winner"],
            ["nba", "boston_celtics", "new_york_knicks"],
            "ctx_nba_nyk_bos_2026_04_30",
            "Final winner for Knicks at Celtics.",
        )
    if subject == "NBA Knicks at Celtics 2026-04-30 total points":
        return path_meta(
            "NBA / Knicks at Celtics / total points",
            ["nba", "ctx_nba_nyk_bos_2026_04_30", "total_points"],
            ["nba", "boston_celtics", "new_york_knicks"],
            "ctx_nba_nyk_bos_2026_04_30",
            "Combined final score for Knicks at Celtics.",
        )

    entity_map = {
        "ETH/USD spot": ["eth"],
        "ETH/USD 30-day realized volatility": ["eth"],
        "BTC/USD spot": ["btc"],
        "BTC market-cap dominance": ["btc", "crypto_market"],
        "BTC spot ETF net flow": ["btc", "btc_spot_etfs"],
        "SOL/USD spot": ["sol"],
        "Total crypto market cap": ["crypto_market"],
        "Stablecoin circulating supply": ["stablecoins", "crypto_market"],
        "US real GDP growth": ["us_economy"],
        "US unemployment rate": ["us_economy"],
        "Sahm rule indicator": ["us_economy"],
        "US CPI YoY": ["us_economy"],
        "Fed funds target upper bound": ["federal_reserve"],
        "US 10-year Treasury yield": ["us_economy"],
        "US 2-year Treasury yield": ["us_economy"],
        "US 10Y-2Y Treasury spread": ["us_economy"],
        "S&P 500": ["sp500"],
        "Nasdaq 100": ["nasdaq100"],
        "VIX": ["vix"],
        "WTI crude oil spot": ["wti_crude"],
        "Gold spot": ["gold"],
        "US Dollar Index DXY": ["dxy"],
        "2028 Democratic presidential nominee": ["democratic_party"],
        "2028 Republican presidential nominee": ["republican_party"],
        "2028 US presidential winner": ["democratic_party", "republican_party"],
        "2028 US House control": ["democratic_party", "republican_party"],
        "2028 US Senate control": ["democratic_party", "republican_party"],
        "2028 Democratic national primary polling": ["democratic_party"],
        "2028 Republican national primary polling": ["republican_party"],
        "US presidential approval": ["united_states"],
        "2028 US presidential turnout": ["united_states"],
        "2028 US presidential popular vote margin": ["democratic_party", "republican_party"],
        "Iran US troop count": ["iran", "united_states"],
        "Iran US troop presence duration": ["iran", "united_states"],
        "Iran US strike count": ["iran", "united_states"],
        "US Iran war authorization": ["iran", "united_states"],
        "US control of Iranian territory": ["iran", "united_states"],
        "US Iran sanctions package count": ["iran", "united_states"],
        "Iran nuclear talks status": ["iran", "united_states"],
        "Strait of Hormuz tanker incident count": ["strait_of_hormuz", "iran"],
        "Iran-linked regional casualty count": ["iran"],
    }
    context = context_for_subject(subject)
    entity_ids = entity_map.get(subject, ["us_economy"] if domain == "macro" else [])
    return path_meta(
        f"{domain.title()} / {subject}",
        [domain, *entity_ids, kind],
        entity_ids,
        context,
        f"Observable {kind} measurement for {subject}.",
    )


def path_meta(
    display_title: str,
    path: list[str],
    entity_ids: list[str],
    context_id: str,
    description: str,
) -> dict[str, Any]:
    return {
        "display_title": display_title,
        "path": path,
        "entity_ids": entity_ids,
        "context_id": context_id,
        "description": description,
    }


def context_for_subject(subject: str) -> str:
    if "2026-Q1" in subject:
        return "ctx_2026_q1"
    if "2026-Q2" in subject:
        return "ctx_2026_q2"
    if "2026-Q3" in subject:
        return "ctx_2026_q3"
    if "2026-Q4" in subject:
        return "ctx_2026_q4"
    if subject.startswith("2028 Democratic") or subject.startswith("2028 Republican"):
        return "ctx_2028_nomination"
    if subject.startswith("2028 US"):
        return "ctx_2028_general"
    if subject.startswith("Iran") or subject.startswith("US Iran") or subject.startswith("US control") or subject.startswith("Strait"):
        return "ctx_before_2027"
    return "ctx_2026"


def resolver_for_feed(feed_id: str) -> str:
    return {
        "pyth": "signed_price_feed",
        "chainlink": "signed_price_feed",
        "fred": "economic_data_feed_signed",
        "election_wire": "source_result_signed",
        "wire": "wire_service_or_admin",
        "sportsdata": "sports_feed_signed",
    }.get(feed_id, "admin_demo")


def seed_conditions(measurements: list[dict[str, Any]]) -> list[dict[str, Any]]:
    by_subject = {item["subject"]: item for item in measurements}
    specs: list[tuple[str, str, str, dict[str, Any], float, str, str]] = [
        ("ETH/USD spot", "2026", "max", {"op": ">", "threshold": 3000, "unit": "USD"}, 0.74, "ETH > 3000", "Will ETH/USD trade above $3,000 during 2026?"),
        ("ETH/USD spot", "2026", "max", {"op": ">", "threshold": 6000, "unit": "USD"}, 0.31, "ETH > 6000", "Will ETH/USD trade above $6,000 during 2026?"),
        ("ETH/USD spot", "2026", "max", {"op": "between", "low": 3000, "high": 6000, "unit": "USD"}, 0.43, "3000 < ETH < 6000", "Will max ETH/USD during 2026 land between $3,000 and $6,000?"),
        ("BTC/USD spot", "2026", "max", {"op": ">", "threshold": 100000, "unit": "USD"}, 0.57, "BTC > 100k", "Will BTC/USD trade above $100,000 during 2026?"),
        ("BTC/USD spot", "2026", "max", {"op": ">", "threshold": 150000, "unit": "USD"}, 0.24, "BTC > 150k", "Will BTC/USD trade above $150,000 during 2026?"),
        ("SOL/USD spot", "2026", "max", {"op": ">", "threshold": 250, "unit": "USD"}, 0.39, "SOL > 250", "Will SOL/USD trade above $250 during 2026?"),
        ("Total crypto market cap", "2026", "max", {"op": ">", "threshold": 5_000_000_000_000, "unit": "USD"}, 0.28, "Crypto mcap > 5T", "Will total crypto market cap exceed $5T during 2026?"),
        ("US real GDP growth", "2026-Q1", "released", {"op": "<", "threshold": 0, "unit": "%"}, 0.23, "GDP Q1 < 0", "Will real GDP growth be negative in 2026 Q1?"),
        ("US real GDP growth", "2026-Q2", "released", {"op": "<", "threshold": 0, "unit": "%"}, 0.26, "GDP Q2 < 0", "Will real GDP growth be negative in 2026 Q2?"),
        ("US real GDP growth", "2026-Q3", "released", {"op": "<", "threshold": 0, "unit": "%"}, 0.24, "GDP Q3 < 0", "Will real GDP growth be negative in 2026 Q3?"),
        ("US real GDP growth", "2026-Q4", "released", {"op": "<", "threshold": 0, "unit": "%"}, 0.22, "GDP Q4 < 0", "Will real GDP growth be negative in 2026 Q4?"),
        ("US unemployment rate", "2026", "max", {"op": ">", "threshold": 5.0, "unit": "%"}, 0.34, "Unemployment > 5%", "Will US unemployment exceed 5.0% during 2026?"),
        ("Sahm rule indicator", "2026", "max", {"op": ">", "threshold": 0.5, "unit": "pp"}, 0.29, "Sahm > 0.5", "Will the Sahm rule indicator exceed 0.5pp during 2026?"),
        ("US CPI YoY", "2026", "max", {"op": ">", "threshold": 4.0, "unit": "%"}, 0.21, "CPI > 4%", "Will US CPI YoY exceed 4% during 2026?"),
        ("Fed funds target upper bound", "2026", "min", {"op": "<", "threshold": 3.0, "unit": "%"}, 0.47, "Fed < 3%", "Will the Fed funds upper bound fall below 3% during 2026?"),
        ("S&P 500", "2026", "drawdown", {"op": ">", "threshold": 20, "unit": "% drawdown"}, 0.19, "SPX drawdown > 20%", "Will the S&P 500 draw down more than 20% during 2026?"),
        ("VIX", "2026", "max", {"op": ">", "threshold": 40, "unit": "index points"}, 0.27, "VIX > 40", "Will VIX trade above 40 during 2026?"),
        ("2028 Democratic presidential nominee", "2028 nomination", "winner", {"op": "=", "value": "Gavin Newsom"}, 0.24, "Newsom nominee", "Will Gavin Newsom win the 2028 Democratic presidential nomination?"),
        ("2028 Democratic presidential nominee", "2028 nomination", "winner", {"op": "=", "value": "Gretchen Whitmer"}, 0.15, "Whitmer nominee", "Will Gretchen Whitmer win the 2028 Democratic presidential nomination?"),
        ("2028 Republican presidential nominee", "2028 nomination", "winner", {"op": "=", "value": "JD Vance"}, 0.32, "Vance nominee", "Will JD Vance win the 2028 Republican presidential nomination?"),
        ("2028 US presidential winner", "2028 general", "winner", {"op": "=", "value": "Republican Party"}, 0.51, "GOP president", "Will the Republican Party win the 2028 US presidential election?"),
        ("2028 US presidential winner", "2028 general", "winner", {"op": "=", "value": "Democratic Party"}, 0.47, "Dem president", "Will the Democratic Party win the 2028 US presidential election?"),
        ("2028 US House control", "2028 general", "winner", {"op": "=", "value": "Republican Party"}, 0.46, "GOP House", "Will Republicans control the House after the 2028 election?"),
        ("2028 US Senate control", "2028 general", "winner", {"op": "=", "value": "Republican Party"}, 0.49, "GOP Senate", "Will Republicans control the Senate after the 2028 election?"),
        ("Iran US troop count", "before 2027", "max", {"op": ">", "threshold": 0, "unit": "troops"}, 0.42, "Iran troops > 0", "Will any US military personnel enter Iranian sovereign territory before 2027?"),
        ("Iran US troop count", "before 2027", "max", {"op": ">", "threshold": 1000, "unit": "troops"}, 0.12, "Iran troops > 1k", "Will at least 1,000 US military personnel enter Iranian sovereign territory before 2027?"),
        ("Iran US troop presence duration", "before 2027", "max", {"op": ">", "threshold": 72, "unit": "hours"}, 0.10, "Iran 72h presence", "Will US troops remain in Iran for at least 72 continuous hours before 2027?"),
        ("Iran US strike count", "before 2027", "count", {"op": ">", "threshold": 50, "unit": "strikes"}, 0.22, "Iran strikes > 50", "Will the US conduct at least 50 kinetic strikes on Iran before 2027?"),
        ("US Iran war authorization", "before 2027", "official", {"op": "=", "value": "formal declaration"}, 0.04, "Iran war declared", "Will the US formally declare war on Iran before 2027?"),
        ("US Iran war authorization", "before 2027", "official", {"op": "=", "value": "AUMF passed"}, 0.08, "Iran AUMF", "Will Congress pass an Iran AUMF before 2027?"),
        ("US control of Iranian territory", "before 2027", "official", {"op": "=", "value": "declared occupation"}, 0.03, "Iran occupation", "Will the US declare occupation or control of Iranian territory before 2027?"),
        ("NBA Knicks at Celtics 2026-04-30 winner", "2026-04-30", "winner", {"op": "=", "value": "Boston Celtics"}, 0.58, "Celtics win", "Will the Boston Celtics beat the Knicks on 2026-04-30?"),
        ("NBA Knicks at Celtics 2026-04-30 winner", "2026-04-30", "winner", {"op": "=", "value": "New York Knicks"}, 0.42, "Knicks win", "Will the New York Knicks beat the Celtics on 2026-04-30?"),
        ("Jayson Tatum points vs Knicks 2026-04-30", "2026-04-30", "box_score", {"op": ">", "threshold": 29.5, "unit": "points"}, 0.52, "Tatum points > 29.5", "Will Jayson Tatum score over 29.5 points vs the Knicks?"),
        ("Jaylen Brown rebounds vs Knicks 2026-04-30", "2026-04-30", "box_score", {"op": ">", "threshold": 6.5, "unit": "rebounds"}, 0.48, "Brown rebounds > 6.5", "Will Jaylen Brown record over 6.5 rebounds vs the Knicks?"),
        ("Jalen Brunson assists vs Celtics 2026-04-30", "2026-04-30", "box_score", {"op": ">", "threshold": 7.5, "unit": "assists"}, 0.44, "Brunson assists > 7.5", "Will Jalen Brunson record over 7.5 assists vs the Celtics?"),
        ("ETH/USD spot", "2026", "min", {"op": "<", "threshold": 2000, "unit": "USD"}, 0.18, "ETH < 2000", "Will ETH/USD trade below $2,000 during 2026?"),
        ("ETH/USD spot", "2026", "close", {"op": ">", "threshold": 4500, "unit": "USD"}, 0.38, "ETH close > 4500", "Will ETH/USD close 2026 above $4,500?"),
        ("BTC/USD spot", "2026", "max", {"op": ">", "threshold": 200000, "unit": "USD"}, 0.11, "BTC > 200k", "Will BTC/USD trade above $200,000 during 2026?"),
        ("BTC/USD spot", "2026", "max", {"op": "between", "low": 100000, "high": 150000, "unit": "USD"}, 0.33, "100k < BTC < 150k", "Will max BTC/USD during 2026 land between $100,000 and $150,000?"),
        ("BTC/USD spot", "2026", "min", {"op": "<", "threshold": 70000, "unit": "USD"}, 0.22, "BTC < 70k", "Will BTC/USD trade below $70,000 during 2026?"),
        ("SOL/USD spot", "2026", "max", {"op": ">", "threshold": 400, "unit": "USD"}, 0.16, "SOL > 400", "Will SOL/USD trade above $400 during 2026?"),
        ("SOL/USD spot", "2026", "max", {"op": "between", "low": 250, "high": 400, "unit": "USD"}, 0.23, "250 < SOL < 400", "Will max SOL/USD during 2026 land between $250 and $400?"),
        ("Total crypto market cap", "2026", "max", {"op": ">", "threshold": 3_000_000_000_000, "unit": "USD"}, 0.52, "Crypto mcap > 3T", "Will total crypto market cap exceed $3T during 2026?"),
        ("Total crypto market cap", "2026", "max", {"op": ">", "threshold": 7_000_000_000_000, "unit": "USD"}, 0.12, "Crypto mcap > 7T", "Will total crypto market cap exceed $7T during 2026?"),
        ("US unemployment rate", "2026", "max", {"op": ">", "threshold": 4.5, "unit": "%"}, 0.52, "Unemployment > 4.5%", "Will US unemployment exceed 4.5% during 2026?"),
        ("US unemployment rate", "2026", "max", {"op": ">", "threshold": 6.0, "unit": "%"}, 0.14, "Unemployment > 6%", "Will US unemployment exceed 6.0% during 2026?"),
        ("US unemployment rate", "2026", "min", {"op": "<", "threshold": 3.5, "unit": "%"}, 0.20, "Unemployment < 3.5%", "Will US unemployment fall below 3.5% during 2026?"),
        ("Sahm rule indicator", "2026", "max", {"op": ">", "threshold": 1.0, "unit": "pp"}, 0.13, "Sahm > 1.0", "Will the Sahm rule indicator exceed 1.0pp during 2026?"),
        ("US CPI YoY", "2026", "max", {"op": ">", "threshold": 3.0, "unit": "%"}, 0.42, "CPI > 3%", "Will US CPI YoY exceed 3% during 2026?"),
        ("US CPI YoY", "2026", "min", {"op": "<", "threshold": 2.0, "unit": "%"}, 0.19, "CPI < 2%", "Will US CPI YoY fall below 2% during 2026?"),
        ("Fed funds target upper bound", "2026", "max", {"op": ">", "threshold": 5.0, "unit": "%"}, 0.17, "Fed > 5%", "Will the Fed funds upper bound exceed 5% during 2026?"),
        ("Fed funds target upper bound", "2026", "min", {"op": "<", "threshold": 2.0, "unit": "%"}, 0.18, "Fed < 2%", "Will the Fed funds upper bound fall below 2% during 2026?"),
        ("S&P 500", "2026", "close", {"op": ">", "threshold": 7000, "unit": "index points"}, 0.31, "SPX close > 7000", "Will the S&P 500 close 2026 above 7,000?"),
        ("S&P 500", "2026", "drawdown", {"op": ">", "threshold": 10, "unit": "% drawdown"}, 0.48, "SPX drawdown > 10%", "Will the S&P 500 draw down more than 10% during 2026?"),
        ("VIX", "2026", "max", {"op": ">", "threshold": 30, "unit": "index points"}, 0.45, "VIX > 30", "Will VIX trade above 30 during 2026?"),
        ("VIX", "2026", "max", {"op": ">", "threshold": 50, "unit": "index points"}, 0.12, "VIX > 50", "Will VIX trade above 50 during 2026?"),
        ("2028 Democratic presidential nominee", "2028 nomination", "winner", {"op": "=", "value": "Josh Shapiro"}, 0.12, "Shapiro nominee", "Will Josh Shapiro win the 2028 Democratic presidential nomination?"),
        ("2028 Democratic presidential nominee", "2028 nomination", "winner", {"op": "=", "value": "Pete Buttigieg"}, 0.10, "Buttigieg nominee", "Will Pete Buttigieg win the 2028 Democratic presidential nomination?"),
        ("2028 Republican presidential nominee", "2028 nomination", "winner", {"op": "=", "value": "Ron DeSantis"}, 0.11, "DeSantis nominee", "Will Ron DeSantis win the 2028 Republican presidential nomination?"),
        ("2028 Republican presidential nominee", "2028 nomination", "winner", {"op": "=", "value": "Nikki Haley"}, 0.08, "Haley nominee", "Will Nikki Haley win the 2028 Republican presidential nomination?"),
        ("2028 US House control", "2028 general", "winner", {"op": "=", "value": "Democratic Party"}, 0.52, "Dem House", "Will Democrats control the House after the 2028 election?"),
        ("2028 US Senate control", "2028 general", "winner", {"op": "=", "value": "Democratic Party"}, 0.48, "Dem Senate", "Will Democrats control the Senate after the 2028 election?"),
        ("Iran US troop count", "before 2027", "max", {"op": ">", "threshold": 100, "unit": "troops"}, 0.20, "Iran troops > 100", "Will at least 100 US military personnel enter Iranian sovereign territory before 2027?"),
        ("Iran US troop count", "before 2027", "max", {"op": ">", "threshold": 5000, "unit": "troops"}, 0.05, "Iran troops > 5k", "Will at least 5,000 US military personnel enter Iranian sovereign territory before 2027?"),
        ("Iran US troop presence duration", "before 2027", "max", {"op": ">", "threshold": 24, "unit": "hours"}, 0.18, "Iran 24h presence", "Will US troops remain in Iran for at least 24 continuous hours before 2027?"),
        ("Iran US strike count", "before 2027", "count", {"op": ">", "threshold": 10, "unit": "strikes"}, 0.36, "Iran strikes > 10", "Will the US conduct at least 10 kinetic strikes on Iran before 2027?"),
        ("Iran US strike count", "before 2027", "count", {"op": ">", "threshold": 100, "unit": "strikes"}, 0.11, "Iran strikes > 100", "Will the US conduct at least 100 kinetic strikes on Iran before 2027?"),
        ("US control of Iranian territory", "before 2027", "official", {"op": "=", "value": "temporary control"}, 0.07, "Iran temporary control", "Will the US temporarily control Iranian territory before 2027?"),
        ("Jayson Tatum points vs Knicks 2026-04-30", "2026-04-30", "box_score", {"op": ">", "threshold": 24.5, "unit": "points"}, 0.66, "Tatum points > 24.5", "Will Jayson Tatum score over 24.5 points vs the Knicks?"),
        ("Jayson Tatum points vs Knicks 2026-04-30", "2026-04-30", "box_score", {"op": ">", "threshold": 34.5, "unit": "points"}, 0.28, "Tatum points > 34.5", "Will Jayson Tatum score over 34.5 points vs the Knicks?"),
        ("Jaylen Brown rebounds vs Knicks 2026-04-30", "2026-04-30", "box_score", {"op": ">", "threshold": 4.5, "unit": "rebounds"}, 0.66, "Brown rebounds > 4.5", "Will Jaylen Brown record over 4.5 rebounds vs the Knicks?"),
        ("Jaylen Brown rebounds vs Knicks 2026-04-30", "2026-04-30", "box_score", {"op": ">", "threshold": 8.5, "unit": "rebounds"}, 0.24, "Brown rebounds > 8.5", "Will Jaylen Brown record over 8.5 rebounds vs the Knicks?"),
        ("Jalen Brunson assists vs Celtics 2026-04-30", "2026-04-30", "box_score", {"op": ">", "threshold": 5.5, "unit": "assists"}, 0.61, "Brunson assists > 5.5", "Will Jalen Brunson record over 5.5 assists vs the Celtics?"),
        ("Jalen Brunson assists vs Celtics 2026-04-30", "2026-04-30", "box_score", {"op": ">", "threshold": 9.5, "unit": "assists"}, 0.26, "Brunson assists > 9.5", "Will Jalen Brunson record over 9.5 assists vs the Celtics?"),
    ]
    rows: list[dict[str, Any]] = []
    for subject, window, aggregation, predicate, fair, short, question in specs:
        measurement = by_subject[subject]
        key = condition_key(
            {
                "measurement_id": measurement["id"],
                "measurement_key": measurement["canonical_key"],
                "observation_window": window,
                "aggregation": aggregation,
                "predicate": predicate,
            }
        )
        row = Condition(
            id=stable_id("cond", key),
            measurement_id=measurement["id"],
            domain=measurement["domain"],
            title=short,
            short_name=short,
            question=question,
            description=f"Predicate over {measurement['subject']} using {aggregation} during {window}.",
            observation_window=window,
            aggregation=aggregation,
            predicate=predicate,
            fair_value=fair,
            resolver_primitive=measurement["resolver_primitive"],
            canonical_key=key,
        ).to_dict()
        rows.append(row)
    return rows


def seed_propositions(conditions: list[dict[str, Any]]) -> list[dict[str, Any]]:
    by_short = {item["short_name"]: item for item in conditions}
    specs: list[tuple[str, str, str, str, dict[str, Any], str]] = [
        ("ETH above 6000", "ETH > 6000", "Will ETH trade above $6,000 in 2026?", "Single-leaf proposition for ETH threshold liquidity.", leaf(by_short, "ETH > 6000"), "crypto"),
        ("ETH range 3000 to 6000", "ETH range", "Will ETH finish the 2026 max range between $3,000 and $6,000?", "Range proposition over one condition.", leaf(by_short, "3000 < ETH < 6000"), "crypto"),
        ("ETH and BTC breakout", "ETH + BTC", "Will ETH top $3,000 and BTC top $100,000 in 2026?", "Two-asset crypto breakout composition.", {"op": "AND", "args": [leaf(by_short, "ETH > 3000"), leaf(by_short, "BTC > 100k")]}, "crypto"),
        ("Technical recession 2026", "Technical recession", "Will at least two quarters of 2026 real GDP growth be negative?", "Two-negative-quarter recession definition.", {"op": "K_OF_N", "k": 2, "args": [leaf(by_short, "GDP Q1 < 0"), leaf(by_short, "GDP Q2 < 0"), leaf(by_short, "GDP Q3 < 0"), leaf(by_short, "GDP Q4 < 0")]}, "macro"),
        ("Sahm recession 2026", "Sahm recession", "Will the Sahm rule trigger during 2026?", "Single-leaf recession definition.", leaf(by_short, "Sahm > 0.5"), "macro"),
        ("Market-stress recession 2026", "Stress recession", "Will Sahm trigger or SPX/VIX stress confirm recession in 2026?", "Composite market-stress definition.", {"op": "OR", "args": [leaf(by_short, "Sahm > 0.5"), {"op": "AND", "args": [leaf(by_short, "SPX drawdown > 20%"), leaf(by_short, "VIX > 40")]}]}, "macro"),
        ("Republican sweep 2028", "GOP sweep", "Will Republicans win the presidency, House, and Senate in 2028?", "Party-control sweep composition.", {"op": "AND", "args": [leaf(by_short, "GOP president"), leaf(by_short, "GOP House"), leaf(by_short, "GOP Senate")]}, "politics"),
        ("Democratic nominee chain", "Dem chain", "Will a Democrat win the presidency if Newsom wins the nomination?", "Election-chain conditional composition.", {"op": "IF_THEN", "args": [leaf(by_short, "Newsom nominee"), leaf(by_short, "Dem president")]}, "politics"),
        ("Iran hawkish invasion", "Iran hawkish", "Will Iran meet a hawkish invasion definition before 2027?", "Low-bar definition: any troop presence or a large strike campaign.", {"op": "OR", "args": [leaf(by_short, "Iran troops > 0"), leaf(by_short, "Iran strikes > 50")]}, "geopolitics"),
        ("Iran mainstream invasion", "Iran mainstream", "Will Iran meet a mainstream invasion definition before 2027?", "Requires sustained ground presence, authorization, or substantial strikes.", {"op": "OR", "args": [{"op": "AND", "args": [leaf(by_short, "Iran troops > 1k"), leaf(by_short, "Iran 72h presence")]}, leaf(by_short, "Iran war declared"), leaf(by_short, "Iran AUMF"), leaf(by_short, "Iran strikes > 50")]}, "geopolitics"),
        ("Iran strict invasion", "Iran strict", "Will Iran meet a strict occupation definition before 2027?", "Strict territorial-control invasion definition.", {"op": "AND", "args": [leaf(by_short, "Iran troops > 1k"), leaf(by_short, "Iran 72h presence"), leaf(by_short, "Iran occupation")]}, "geopolitics"),
        ("Celtics same-game parlay", "Celtics SGP", "Will the Celtics win, Tatum go over points, and Brown go over rebounds?", "Focused NBA same-game parlay composition.", {"op": "AND", "args": [leaf(by_short, "Celtics win"), leaf(by_short, "Tatum points > 29.5"), leaf(by_short, "Brown rebounds > 6.5")]}, "sports"),
        ("Crypto supercycle 2026", "Crypto supercycle", "Will crypto enter a broad supercycle in 2026?", "High-threshold basket over BTC, ETH, SOL, and total market cap.", {"op": "K_OF_N", "k": 3, "args": [leaf(by_short, "ETH > 6000"), leaf(by_short, "BTC > 150k"), leaf(by_short, "SOL > 400"), leaf(by_short, "Crypto mcap > 5T")]}, "crypto"),
        ("Crypto downside shock 2026", "Crypto shock", "Will major crypto assets see downside stress in 2026?", "OR condition over ETH and BTC downside thresholds.", {"op": "OR", "args": [leaf(by_short, "ETH < 2000"), leaf(by_short, "BTC < 70k")]}, "crypto"),
        ("Soft landing 2026", "Soft landing", "Will 2026 avoid recession while inflation cools?", "Macro soft-landing definition.", {"op": "AND", "args": [{"op": "NOT", "args": [leaf(by_short, "Sahm > 0.5")]}, leaf(by_short, "CPI < 2%"), leaf(by_short, "SPX close > 7000")]}, "macro"),
        ("Inflation scare 2026", "Inflation scare", "Will inflation and rates both run hot in 2026?", "Macro upside-inflation definition.", {"op": "AND", "args": [leaf(by_short, "CPI > 4%"), leaf(by_short, "Fed > 5%")]}, "macro"),
        ("Hard landing 2026", "Hard landing", "Will unemployment and market stress both flash hard landing in 2026?", "Labor-market plus market-stress recession composition.", {"op": "AND", "args": [leaf(by_short, "Unemployment > 6%"), leaf(by_short, "SPX drawdown > 20%"), leaf(by_short, "VIX > 40")]}, "macro"),
        ("Democratic sweep 2028", "Dem sweep", "Will Democrats win the presidency, House, and Senate in 2028?", "Party-control sweep composition.", {"op": "AND", "args": [leaf(by_short, "Dem president"), leaf(by_short, "Dem House"), leaf(by_short, "Dem Senate")]}, "politics"),
        ("Open Democratic primary 2028", "Dem primary open", "Will Newsom, Whitmer, Shapiro, or Buttigieg win the 2028 Democratic nomination?", "OR basket across major Democratic contenders.", {"op": "OR", "args": [leaf(by_short, "Newsom nominee"), leaf(by_short, "Whitmer nominee"), leaf(by_short, "Shapiro nominee"), leaf(by_short, "Buttigieg nominee")]}, "politics"),
        ("Republican continuity 2028", "GOP continuity", "Will Vance win the nomination and Republicans win the presidency?", "Nomination-to-general chain composition.", {"op": "AND", "args": [leaf(by_short, "Vance nominee"), leaf(by_short, "GOP president")]}, "politics"),
        ("Iran ground escalation", "Iran ground", "Will Iran escalation involve meaningful US ground presence?", "Troop count and duration composition.", {"op": "AND", "args": [leaf(by_short, "Iran troops > 100"), leaf(by_short, "Iran 24h presence")]}, "geopolitics"),
        ("Iran air campaign", "Iran air campaign", "Will US strikes on Iran escalate into a major air campaign?", "Nested strike-threshold composition.", {"op": "AND", "args": [leaf(by_short, "Iran strikes > 50"), leaf(by_short, "Iran strikes > 100")]}, "geopolitics"),
        ("Iran legal escalation", "Iran legal", "Will formal US authorization accompany Iran escalation?", "Legal authorization composition.", {"op": "OR", "args": [leaf(by_short, "Iran war declared"), leaf(by_short, "Iran AUMF")]}, "geopolitics"),
        ("Celtics alt parlay", "Celtics alt SGP", "Will the Celtics win with lower Tatum and Brown stat legs?", "Lower-threshold same-game parlay.", {"op": "AND", "args": [leaf(by_short, "Celtics win"), leaf(by_short, "Tatum points > 24.5"), leaf(by_short, "Brown rebounds > 4.5")]}, "sports"),
        ("Knicks upset script", "Knicks upset", "Will the Knicks win while Brunson clears assists?", "Upset game-script proposition.", {"op": "AND", "args": [leaf(by_short, "Knicks win"), leaf(by_short, "Brunson assists > 7.5")]}, "sports"),
        ("NBA star overs", "NBA star overs", "Will at least two seeded NBA player overs hit?", "K-of-N player stat parlay.", {"op": "K_OF_N", "k": 2, "args": [leaf(by_short, "Tatum points > 29.5"), leaf(by_short, "Brown rebounds > 6.5"), leaf(by_short, "Brunson assists > 7.5")]}, "sports"),
    ]
    values = {item["id"]: float(item.get("fair_value", 0.5)) for item in conditions}
    rows: list[dict[str, Any]] = []
    seen: set[str] = set()
    for title, short, question, description, formula, domain in specs:
        if not formula_conditions(formula):
            continue
        key = proposition_key(formula)
        if key in seen:
            continue
        seen.add(key)
        rows.append(
            Proposition(
                id=stable_id("prop", key),
                domain=domain,
                title=title,
                short_name=short,
                question=question,
                description=description,
                formula=formula,
                fair_value=estimate_formula_value(formula, values),
                canonical_key=key,
            ).to_dict()
        )
    return rows


def leaf(by_short: dict[str, dict[str, Any]], short_name: str) -> dict[str, str]:
    return {"condition": by_short[short_name]["id"]}


def attach_source_aliases(snapshot: dict[str, Any], conditions: list[dict[str, Any]]) -> None:
    # Best-effort evidence only. The default snapshot may be empty in offline demos.
    for condition in conditions:
        aliases = []
        for event in snapshot.get("polymarket_events", [])[:50]:
            title = str(event.get("title", ""))
            if title and any(token in title.lower() for token in condition["short_name"].lower().split()[:2]):
                aliases.append({"source": "polymarket", "source_id": event.get("id") or event.get("slug", ""), "question": title})
                break
        condition["aliases"] = aliases
        if aliases:
            condition["quality"] = "source_matched"


def generate_implication_edges(conditions: list[dict[str, Any]]) -> list[dict[str, Any]]:
    edges: list[dict[str, Any]] = []
    grouped: dict[tuple[str, str, str], list[dict[str, Any]]] = {}
    for condition in conditions:
        grouped.setdefault(
            (condition["measurement_id"], condition["observation_window"], condition["aggregation"]),
            [],
        ).append(condition)

    for rows in grouped.values():
        greater = [row for row in rows if row.get("predicate", {}).get("op") == ">" and "threshold" in row.get("predicate", {})]
        less = [row for row in rows if row.get("predicate", {}).get("op") == "<" and "threshold" in row.get("predicate", {})]
        ranges = [row for row in rows if row.get("predicate", {}).get("op") == "between"]
        for high in greater:
            for low in greater:
                if high is low:
                    continue
                if float(high["predicate"]["threshold"]) > float(low["predicate"]["threshold"]):
                    add_edge(edges, high, low, f"{high['short_name']} -> {low['short_name']}", "nested_threshold")
        for low in less:
            for high in less:
                if high is low:
                    continue
                if float(low["predicate"]["threshold"]) < float(high["predicate"]["threshold"]):
                    add_edge(edges, low, high, f"{low['short_name']} -> {high['short_name']}", "nested_threshold")
        for range_condition in ranges:
            low = float(range_condition["predicate"]["low"])
            high = float(range_condition["predicate"]["high"])
            for row in greater:
                if low >= float(row["predicate"]["threshold"]):
                    add_edge(edges, range_condition, row, f"{range_condition['short_name']} -> {row['short_name']}", "range_bound")
            for row in less:
                if high <= float(row["predicate"]["threshold"]):
                    add_edge(edges, range_condition, row, f"{range_condition['short_name']} -> {row['short_name']}", "range_bound")
    return edges


def add_edge(edges: list[dict[str, Any]], source: dict[str, Any], target: dict[str, Any], label: str, edge_type: str) -> None:
    edges.append(
        {
            "from": source["id"],
            "to": target["id"],
            "type": edge_type,
            "label": label,
            "no_arb": f"P({source['short_name']}) <= P({target['short_name']})",
        }
    )


def build_condition(
    measurement: dict[str, Any],
    observation_window: str,
    aggregation: str,
    predicate: dict[str, Any],
    fair_value: float = 0.5,
    short_name: str | None = None,
) -> dict[str, Any]:
    key = condition_key(
        {
            "measurement_id": measurement["id"],
            "measurement_key": measurement.get("canonical_key"),
            "observation_window": observation_window,
            "aggregation": aggregation,
            "predicate": predicate,
        }
    )
    short = short_name or f"{measurement['subject']} {predicate_label(predicate)}"
    return Condition(
        id=stable_id("cond", key),
        measurement_id=measurement["id"],
        domain=measurement["domain"],
        title=short,
        short_name=short,
        question=f"Will {short}?",
        description=f"Predicate over {measurement['subject']}.",
        observation_window=observation_window,
        aggregation=aggregation,
        predicate=predicate,
        fair_value=clamp_probability(fair_value),
        resolver_primitive=measurement["resolver_primitive"],
        canonical_key=key,
    ).to_dict()


def predicate_label(predicate: dict[str, Any]) -> str:
    if predicate.get("op") == "between":
        return f"between {predicate.get('low')} and {predicate.get('high')}"
    if "threshold" in predicate:
        return f"{predicate.get('op')} {predicate.get('threshold')}"
    return f"{predicate.get('op')} {predicate.get('value')}"


def split_kalshi_legs(title: str) -> list[str]:
    return [part.strip() for part in re.split(r",\s*(?=yes\s+)", title, flags=re.IGNORECASE) if part.strip()]


# Compatibility helpers for the old v3 tests/importers. They now return graph
# conditions rather than ontology-root atoms.
def build_atom(template_id: str, params: dict[str, Any], fair_value: float = 0.5, **_: Any) -> dict[str, Any]:
    measurement = Measurement(
        id=stable_id("meas", f"{template_id}:{canonical_json(params)}"),
        domain=params.get("domain", "demo"),
        measurement_kind=template_id,
        subject=str(params.get("indicator") or params.get("contest") or params.get("asset") or template_id),
        unit=str(params.get("unit", "")),
        feed_ids=["wire"],
        aggregation_semantics="compatibility",
        title=str(params.get("indicator") or params.get("contest") or params.get("asset") or template_id),
        description="Compatibility measurement.",
        resolver_primitive="admin_demo",
    ).to_dict()
    predicate = {
        "op": params.get("comparator", "="),
        "threshold": params.get("threshold"),
        "value": params.get("option"),
        "unit": params.get("unit", ""),
    }
    return build_condition(
        measurement,
        str(params.get("period") or params.get("by_date") or params.get("year") or "source-defined"),
        "compatibility",
        {k: v for k, v in predicate.items() if v is not None},
        fair_value=fair_value,
    )


def source_question_to_atom(source: str, question: str, event_title: str = "") -> dict[str, Any] | None:
    text = f"{question} {event_title}".lower()
    if "democratic presidential nomination" in text:
        return build_atom(
            "election_outcome",
            {
                "domain": "politics",
                "contest": "Democratic presidential nomination",
                "year": 2028,
                "option": extract_candidate(question),
            },
            fair_value=0.5,
        )
    return None


def extract_candidate(question: str) -> str:
    match = re.search(r"will\s+(.+?)\s+win", question, flags=re.IGNORECASE)
    return match.group(1).strip() if match else "Unknown"
