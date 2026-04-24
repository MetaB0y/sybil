from __future__ import annotations

import unittest

from .agent import deterministic_draft, discover
from .registry import condition_key, measurement_key, proposition_key, search_instruments, validate_formula
from .sources import build_condition, build_universe, generate_implication_edges, split_kalshi_legs
from .store import (
    add_condition_to_formula,
    build_graph_projection,
    build_threshold_curves,
    draft_formula_from_prompt,
    edit_wizard_draft,
    enrich_draft,
    ontology_diagnostics,
    replace_condition_in_formula,
)


class GraphUniverseTests(unittest.TestCase):
    def test_kalshi_parlay_splits_into_legs(self) -> None:
        self.assertEqual(
            split_kalshi_legs("yes A: 2+,yes B: 3+,yes C"),
            ["yes A: 2+", "yes B: 3+", "yes C"],
        )

    def test_build_universe_seeds_graph_counts(self) -> None:
        universe = build_universe({"polymarket_events": [], "kalshi_markets": [], "errors": []})
        self.assertGreaterEqual(len(universe["entities"]), 20)
        self.assertGreaterEqual(len(universe["contexts"]), 8)
        self.assertGreaterEqual(len(universe["feeds"]), 5)
        self.assertGreaterEqual(len(universe["measurements"]), 50)
        self.assertGreaterEqual(len(universe["conditions"]), 30)
        self.assertGreaterEqual(len(universe["propositions"]), 10)
        self.assertIn("implication_edges", universe)

    def test_measurements_have_paths_and_entities(self) -> None:
        universe = build_universe({"polymarket_events": [], "kalshi_markets": [], "errors": []})
        for measurement in universe["measurements"]:
            self.assertTrue(measurement.get("path"), measurement["subject"])
            self.assertTrue(measurement.get("display_title"), measurement["subject"])
            self.assertTrue(measurement.get("entity_ids"), measurement["subject"])

    def test_sports_measurements_use_entity_context_paths(self) -> None:
        universe = build_universe({"polymarket_events": [], "kalshi_markets": [], "errors": []})
        tatum = next(item for item in universe["measurements"] if item["subject"] == "Jayson Tatum injury status vs Knicks 2026-04-30")
        self.assertEqual(tatum["context_id"], "ctx_nba_nyk_bos_2026_04_30")
        self.assertIn("jayson_tatum", tatum["entity_ids"])
        self.assertEqual(tatum["display_title"], "NBA / Knicks at Celtics / Jayson Tatum / injury status")

    def test_ontology_diagnostics_catches_no_seed_errors(self) -> None:
        universe = build_universe({"polymarket_events": [], "kalshi_markets": [], "errors": []})
        diagnostics = ontology_diagnostics(universe, universe["instruments"])
        self.assertEqual(diagnostics["status"], "ok", diagnostics["errors"])
        self.assertEqual(diagnostics["checks"]["legacy_atoms"], 0)
        self.assertGreaterEqual(diagnostics["checks"]["measurements"], 50)

    def test_threshold_curves_group_same_measurement_conditions(self) -> None:
        universe = build_universe({"polymarket_events": [], "kalshi_markets": [], "errors": []})
        measurements = {item["id"]: item for item in universe["measurements"]}
        enriched = []
        for item in universe["instruments"]:
            row = dict(item)
            if row.get("measurement_id"):
                row["measurement"] = measurements[row["measurement_id"]]
            enriched.append(row)
        curves = build_threshold_curves(enriched)
        eth_curve = next(curve for curve in curves if curve["title"] == "Crypto / ETH/USD spot")
        names = [item["short_name"] for item in eth_curve["conditions"]]
        self.assertIn("ETH > 3000", names)
        self.assertIn("ETH > 6000", names)
        self.assertIn("3000 < ETH < 6000", names)

    def test_graph_projection_links_entities_to_definitions(self) -> None:
        universe = build_universe({"polymarket_events": [], "kalshi_markets": [], "errors": []})
        projection = build_graph_projection({**universe, "conditions": universe["conditions"], "propositions": universe["propositions"]})
        node_kinds = {node["kind"] for node in projection["nodes"]}
        self.assertIn("entity", node_kinds)
        self.assertIn("measurement", node_kinds)
        self.assertIn("condition", node_kinds)
        self.assertIn("definition", node_kinds)
        edge_types = {edge["type"] for edge in projection["edges"]}
        self.assertIn("entity_measurement", edge_types)
        self.assertIn("measurement_condition", edge_types)
        self.assertIn("condition_definition", edge_types)

    def test_search_returns_conditions_not_flat_atoms(self) -> None:
        universe = build_universe({"polymarket_events": [], "kalshi_markets": [], "errors": []})
        result = search_instruments(universe["instruments"], query="ETH 6000", kind="condition", limit=3)
        self.assertGreaterEqual(result["total"], 1)
        top = result["items"][0]
        self.assertEqual(top["kind"], "condition")
        self.assertEqual(top["object_kind"], "condition")
        self.assertIn("ETH", top["short_name"])

    def test_agent_recession_draft_uses_macro_k_of_n_conditions(self) -> None:
        universe = build_universe({"polymarket_events": [], "kalshi_markets": [], "errors": []})
        draft = deterministic_draft("Build a recession risk composition from macro conditions", universe)
        self.assertEqual(draft["domain"], "macro")
        self.assertEqual(draft["formula"]["op"], "K_OF_N")
        leaf_ids = {arg["condition"] for arg in draft["formula"]["args"]}
        leaves = [item for item in universe["instruments"] if item["id"] in leaf_ids]
        names = " ".join(item["short_name"].lower() for item in leaves)
        self.assertIn("gdp", names)
        self.assertIn("sahm", names)

    def test_agent_hedge_mode_finds_downside_markets(self) -> None:
        universe = build_universe({"polymarket_events": [], "kalshi_markets": [], "errors": []})
        result = discover("I am long ETH and worried about downside in 2026", universe, mode="hedge")
        self.assertEqual(result["mode"], "hedge")
        self.assertGreaterEqual(len(result["questions"]), 2)
        names = " ".join(
            item["short_name"].lower() for item in universe["instruments"] if item["id"] in set(result["ranked_ids"])
        )
        self.assertTrue("eth < 2000" in names or "crypto shock" in names, names)

    def test_agent_news_mode_finds_proxy_markets(self) -> None:
        universe = build_universe({"polymarket_events": [], "kalshi_markets": [], "errors": []})
        result = discover("New Iran strike reports look underappreciated", universe, mode="news")
        self.assertEqual(result["mode"], "news")
        self.assertGreaterEqual(len(result["proxy_markets"]), 1)
        names = " ".join(
            item["short_name"].lower() for item in universe["instruments"] if item["id"] in set(result["ranked_ids"])
        )
        self.assertIn("iran", names)

    def test_agent_interview_mode_asks_questions(self) -> None:
        universe = build_universe({"polymarket_events": [], "kalshi_markets": [], "errors": []})
        result = discover("I have opinions about macro and crypto", universe, mode="interview")
        self.assertEqual(result["mode"], "interview")
        self.assertGreaterEqual(len(result["questions"]), 3)
        self.assertIn("measurable", result["answer"])


class CanonicalKeyTests(unittest.TestCase):
    def test_equivalent_measurements_collapse_to_same_key(self) -> None:
        left = measurement_key(
            {
                "measurement_kind": "price",
                "subject": " ETH/USD spot ",
                "unit": "USD",
                "feed_ids": ["chainlink", "pyth"],
                "aggregation_semantics": "intraday max",
            }
        )
        right = measurement_key(
            {
                "measurement_kind": "PRICE",
                "subject": "eth/usd   spot",
                "unit": "usd",
                "feed_ids": ["pyth", "chainlink"],
                "aggregation_semantics": "intraday max",
            }
        )
        self.assertEqual(left, right)

    def test_equivalent_conditions_collapse_to_same_key(self) -> None:
        measurement = {"id": "m", "canonical_key": "measurement:x"}
        left = condition_key(
            {
                "measurement_id": measurement["id"],
                "measurement_key": measurement["canonical_key"],
                "observation_window": "2026",
                "aggregation": "max",
                "predicate": {"op": ">", "threshold": "6000", "unit": "USD"},
            }
        )
        right = condition_key(
            {
                "measurement_id": measurement["id"],
                "measurement_key": measurement["canonical_key"],
                "observation_window": " 2026 ",
                "aggregation": "MAX",
                "predicate": {"op": ">", "threshold": 6000.0, "unit": "usd"},
            }
        )
        self.assertEqual(left, right)

    def test_commutative_proposition_key_sorts_args(self) -> None:
        left = proposition_key({"op": "AND", "args": [{"condition": "b"}, {"condition": "a"}]})
        right = proposition_key({"op": "AND", "args": [{"condition": "a"}, {"condition": "b"}]})
        self.assertEqual(left, right)


class FormulaValidationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.instruments = [
            {"id": "a", "kind": "condition", "object_kind": "condition"},
            {"id": "b", "kind": "condition", "object_kind": "condition"},
            {"id": "c", "kind": "condition", "object_kind": "condition"},
        ]

    def test_valid_one_leaf_proposition(self) -> None:
        result = validate_formula({"condition": "a"}, self.instruments)
        self.assertTrue(result["valid"], result["errors"])
        self.assertEqual(result["referenced_ids"], ["a"])

    def test_valid_k_of_n(self) -> None:
        result = validate_formula(
            {"op": "K_OF_N", "k": 2, "args": [{"condition": "a"}, {"condition": "b"}, {"condition": "c"}]},
            self.instruments,
        )
        self.assertTrue(result["valid"], result["errors"])
        self.assertEqual(result["referenced_ids"], ["a", "b", "c"])

    def test_rejects_unknown_condition_and_bad_k(self) -> None:
        result = validate_formula(
            {"op": "K_OF_N", "k": 4, "args": [{"condition": "a"}, {"condition": "missing"}]},
            self.instruments,
        )
        self.assertFalse(result["valid"])
        self.assertTrue(any("unknown condition" in error for error in result["errors"]))
        self.assertTrue(any("K_OF_N" in error for error in result["errors"]))

    def test_old_atom_leaf_read_shim(self) -> None:
        result = validate_formula({"op": "IF_THEN", "args": [{"atom": "a"}, {"condition": "b"}]}, self.instruments)
        self.assertTrue(result["valid"], result["errors"])


class ImplicationAndWizardTests(unittest.TestCase):
    def test_implication_generation_for_nested_thresholds_and_range(self) -> None:
        measurement = {
            "id": "m",
            "canonical_key": "measurement:eth",
            "domain": "crypto",
            "subject": "ETH/USD spot",
            "resolver_primitive": "signed_price_feed",
        }
        low = build_condition(measurement, "2026", "max", {"op": ">", "threshold": 3000, "unit": "USD"}, short_name="ETH > 3000")
        high = build_condition(measurement, "2026", "max", {"op": ">", "threshold": 6000, "unit": "USD"}, short_name="ETH > 6000")
        range_condition = build_condition(
            measurement,
            "2026",
            "max",
            {"op": "between", "low": 3000, "high": 6000, "unit": "USD"},
            short_name="3000 < ETH < 6000",
        )
        edges = generate_implication_edges([low, high, range_condition])
        pairs = {(edge["from"], edge["to"]) for edge in edges}
        self.assertIn((high["id"], low["id"]), pairs)
        self.assertIn((range_condition["id"], low["id"]), pairs)

    def test_draft_formula_examples(self) -> None:
        universe = build_universe({"polymarket_events": [], "kalshi_markets": [], "errors": []})
        eth_range = draft_formula_from_prompt("ETH between 3000 and 6000 by end of 2026", universe)
        self.assertIn("condition", eth_range)
        crypto_pair = draft_formula_from_prompt("ETH > 3000 and BTC > 100000", universe)
        self.assertEqual(crypto_pair["op"], "AND")
        recession = draft_formula_from_prompt("recession definition", universe)
        self.assertEqual(recession["op"], "K_OF_N")

    def test_wizard_operations_add_replace_wrap(self) -> None:
        formula = {"condition": "a"}
        formula = add_condition_to_formula(formula, "b", "AND")
        self.assertEqual(formula["op"], "AND")
        formula = replace_condition_in_formula(formula, "b", "c")
        self.assertEqual(formula["args"][1]["condition"], "c")

    def test_enrich_draft_returns_validation_and_implications(self) -> None:
        universe = build_universe({"polymarket_events": [], "kalshi_markets": [], "errors": []})
        formula = draft_formula_from_prompt("ETH > 6000", universe)
        draft = {"title": "ETH high", "formula": formula}
        enriched = enrich_draft(draft, universe)
        self.assertTrue(enriched["validation"]["valid"], enriched["validation"]["errors"])
        self.assertGreaterEqual(len(enriched["referenced_conditions"]), 1)


if __name__ == "__main__":
    unittest.main()
