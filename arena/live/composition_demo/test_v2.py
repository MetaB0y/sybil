from __future__ import annotations

import unittest

from .agent import deterministic_draft
from .registry import search_instruments, validate_formula
from .sources import build_atom, build_universe, source_question_to_atom, split_kalshi_legs


class SourceImportTests(unittest.TestCase):
    def test_kalshi_parlay_splits_into_legs(self) -> None:
        self.assertEqual(
            split_kalshi_legs("yes A: 2+,yes B: 3+,yes C"),
            ["yes A: 2+", "yes B: 3+", "yes C"],
        )

    def test_build_universe_reaches_requested_atom_count_with_fallback(self) -> None:
        universe = build_universe({"polymarket_events": [], "kalshi_markets": [], "errors": []}, max_atoms=12)
        atoms = [item for item in universe["instruments"] if item["kind"] == "atom"]
        compositions = [item for item in universe["instruments"] if item["kind"] == "composition"]
        self.assertEqual(len(atoms), 12)
        self.assertGreaterEqual(len(compositions), 1)
        self.assertIn("source_counts", universe)

    def test_contest_winner_is_template_param_identity(self) -> None:
        atom = build_atom(
            "contest_winner",
            {"contest": "Democratic presidential nomination", "year": 2028, "option": "Gretchen Whitmer"},
        )
        self.assertEqual(atom["template_id"], "contest_winner")
        self.assertEqual(atom["params"]["year"], 2028)
        self.assertEqual(atom["params"]["option"], "Gretchen Whitmer")
        self.assertIn("contest_winner", atom["canonical_key"])

    def test_polymarket_question_maps_to_template_alias_identity(self) -> None:
        atom = source_question_to_atom(
            "polymarket",
            "Will Gretchen Whitmer win the 2028 Democratic presidential nomination?",
            "Democratic Presidential Nominee 2028",
        )
        self.assertIsNotNone(atom)
        assert atom is not None
        self.assertEqual(atom["template_id"], "contest_winner")
        self.assertEqual(atom["params"]["contest"], "Democratic presidential nomination")
        self.assertEqual(atom["params"]["option"], "Gretchen Whitmer")

    def test_unmatched_source_is_not_promoted(self) -> None:
        atom = source_question_to_atom("polymarket", "Will this extremely vague thing happen?", "Vague")
        self.assertIsNone(atom)

    def test_search_returns_template_atoms_not_unrelated_defaults(self) -> None:
        universe = build_universe(
            {
                "polymarket_events": [
                    {
                        "title": "Democratic Presidential Nominee 2028",
                        "slug": "democratic-presidential-nominee-2028",
                        "markets": [
                            {
                                "question": "Will Gavin Newsom win the 2028 Democratic presidential nomination?",
                                "conditionId": "poly-newsom",
                                "outcomePrices": "[\"0.25\", \"0.75\"]",
                            }
                        ],
                    }
                ],
                "kalshi_markets": [],
                "errors": [],
            },
            max_atoms=90,
        )
        result = search_instruments(universe["instruments"], query="Democratic presidential nomination", limit=3)
        self.assertGreaterEqual(result["total"], 1)
        top = result["items"][0]
        self.assertEqual(top["kind"], "atom")
        self.assertEqual(top["template_id"], "contest_winner")
        self.assertEqual(top["quality"], "source_matched")
        self.assertEqual(top["params"]["contest"], "Democratic presidential nomination")

    def test_agent_recession_draft_uses_macro_k_of_n(self) -> None:
        universe = build_universe({"polymarket_events": [], "kalshi_markets": [], "errors": []}, max_atoms=160)
        draft = deterministic_draft("Build a recession risk composition from macro atoms", universe)
        self.assertEqual(draft["domain"], "macro")
        self.assertEqual(draft["formula"]["op"], "K_OF_N")
        leaf_ids = {arg["atom"] for arg in draft["formula"]["args"]}
        leaves = [item for item in universe["instruments"] if item["id"] in leaf_ids]
        indicators = " ".join(str(item["params"].get("indicator", "")) for item in leaves).lower()
        self.assertIn("gdp", indicators)
        self.assertIn("sahm", indicators)


class FormulaValidationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.instruments = [
            {"id": "a", "kind": "atom"},
            {"id": "b", "kind": "atom"},
            {"id": "c", "kind": "atom"},
        ]

    def test_valid_k_of_n(self) -> None:
        result = validate_formula(
            {"op": "K_OF_N", "k": 2, "args": [{"atom": "a"}, {"atom": "b"}, {"atom": "c"}]},
            self.instruments,
        )
        self.assertTrue(result["valid"], result["errors"])
        self.assertEqual(result["referenced_ids"], ["a", "b", "c"])

    def test_rejects_unknown_atom_and_bad_k(self) -> None:
        result = validate_formula(
            {"op": "K_OF_N", "k": 4, "args": [{"atom": "a"}, {"atom": "missing"}]},
            self.instruments,
        )
        self.assertFalse(result["valid"])
        self.assertTrue(any("unknown atom" in error for error in result["errors"]))
        self.assertTrue(any("K_OF_N" in error for error in result["errors"]))

    def test_valid_if_then(self) -> None:
        result = validate_formula({"op": "IF_THEN", "args": [{"atom": "a"}, {"atom": "b"}]}, self.instruments)
        self.assertTrue(result["valid"], result["errors"])


if __name__ == "__main__":
    unittest.main()
