"""Harness unit tests: checkers, stats, conditions, fixture determinism.
Run: python3 -m unittest discover -s harness/tests -v
"""

import json
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))

from harness import config, fixtures_gen, stats
from harness.conditions import adverse_temperature, condition_temps
from harness.checkers import (CHECKERS, extraction_checker, exact_set_checker,
                              rouge_l_f1, creative_checker, conversational_checker,
                              synthesis_checker)


class TestConditions(unittest.TestCase):
    def test_adverse_is_beyond_farthest_edge(self):
        band = {"low": 0.0, "ideal": 0.05, "high": 0.15}
        self.assertAlmostEqual(adverse_temperature(band), 0.35)

    def test_adverse_prefers_low_edge_when_farther(self):
        band = {"low": 0.0, "ideal": 0.9, "high": 1.1}
        self.assertAlmostEqual(adverse_temperature(band), max(0.0, -0.2))

    def test_c1_is_verbatim_band_ideal(self):
        task = {"as_found": 0.7}
        band = {"low": 0.0, "ideal": 0.05, "high": 0.15}
        temps = condition_temps(task, band, config.config_for("smoke"))
        self.assertEqual(temps["C1"], 0.05)
        self.assertEqual(temps["C0"], 0.7)

    def test_implicit_wild_uses_provider_default(self):
        temps = condition_temps({"as_found": None},
                                {"low": 0, "ideal": 0.5, "high": 0.8},
                                config.config_for("smoke"))
        self.assertEqual(temps["C0"], config.DEFAULTS["provider_default_temperature"])


class TestCheckers(unittest.TestCase):
    def test_extraction_exact_pass_and_fabrication(self):
        gold = {"order_id": "ORD-1", "customer": None, "sku": "AX", "quantity": 3}
        ok = json.dumps({"order_id": "ORD-1", "customer": None, "sku": "AX", "quantity": 3})
        passed, score, det = extraction_checker(ok, gold)
        self.assertTrue(passed)
        self.assertEqual(det["fabricated"], 0)
        fab = json.dumps({"order_id": "ORD-1", "customer": "Invented Co", "sku": "AX", "quantity": 3})
        passed, score, det = extraction_checker(fab, gold)
        self.assertFalse(passed)
        self.assertEqual(det["fabricated"], 1)

    def test_exact_set(self):
        passed, score, _ = exact_set_checker('["b", "a"]', ["a", "b"])
        self.assertTrue(passed)
        passed, score, _ = exact_set_checker('["a", "zzz"]', ["a", "b"])
        self.assertFalse(passed)
        self.assertLess(score, 1.0)

    def test_rouge_l_known_values(self):
        self.assertEqual(rouge_l_f1("the cat sat", "the cat sat"), 1.0)
        self.assertEqual(rouge_l_f1("completely different", "the cat sat"), 0.0)

    def test_conversational_bullets_and_banned(self):
        gold = {"bullets": 3, "banned": ["refund", "$"]}
        passed, _, _ = conversational_checker("- a\n- b\n- c", gold)
        self.assertTrue(passed)
        passed, _, det = conversational_checker("- a\n- b\n- we can refund $5", gold)
        self.assertFalse(passed)
        self.assertEqual(sorted(det["banned_hits"]), ["$", "refund"])

    def test_synthesis_forbidden_claim_fails(self):
        gold = {"required_claims": ["replacement", "south"], "forbidden": ["refund the customer"]}
        passed, _, _ = synthesis_checker(
            "Ship a replacement from the south hub.", gold)
        self.assertTrue(passed)
        passed, _, det = synthesis_checker(
            "Ship a replacement from the south hub. Or refund the customer.", gold)
        self.assertFalse(passed)
        self.assertTrue(det["forbidden_hits"])

    def test_creative_format_and_banned(self):
        gold = {"banned": ["nova", "pulse"]}
        passed, _, _ = creative_checker("cedar-lantern", gold)
        self.assertTrue(passed)
        passed, _, _ = creative_checker("cedar nova", gold)
        self.assertFalse(passed)
        passed, _, _ = creative_checker("Cedar-Lantern", gold)
        self.assertFalse(passed)


class TestStats(unittest.TestCase):
    def test_mcnemar_extremes(self):
        self.assertEqual(stats.mcnemar_exact(0, 0), 1.0)
        self.assertLess(stats.mcnemar_exact(0, 10), 0.01)
        self.assertEqual(stats.mcnemar_exact(5, 5), 1.0)

    def test_wilcoxon_direction(self):
        improving = [0.1 * i + 0.01 for i in range(12)]
        z_up, p_up = stats.wilcoxon_signed_rank(improving)
        z_down, p_down = stats.wilcoxon_signed_rank([-x for x in improving])
        # Two-sided test: same p in both directions, opposite z signs.
        self.assertLess(p_up, 0.05)
        self.assertAlmostEqual(p_up, p_down)
        self.assertGreater(z_up, 0)
        self.assertLess(z_down, 0)

    def test_bootstrap_ci_covers_mean(self):
        deltas = [0.1] * 15 + [-0.05] * 5
        lo, hi = stats.paired_bootstrap_ci(deltas, draws=500, seed=7)
        self.assertLessEqual(lo, 0.065)
        self.assertGreaterEqual(hi, 0.065)

    def test_holm_monotone_and_capped(self):
        adj = stats.holm({"a": 0.001, "b": 0.02, "c": 0.9})
        self.assertLessEqual(adj["a"], adj["b"])
        self.assertLessEqual(adj["b"], adj["c"])
        self.assertLessEqual(adj["c"], 1.0)

    def test_spearman_perfect_and_inverse(self):
        self.assertAlmostEqual(stats.spearman([1, 2, 3, 4], [10, 20, 30, 40]), 1.0)
        self.assertAlmostEqual(stats.spearman([1, 2, 3, 4], [40, 30, 20, 10]), -1.0)


class TestFixtures(unittest.TestCase):
    def test_suite_is_deterministic(self):
        a = fixtures_gen.build_suite(3, 42)
        b = fixtures_gen.build_suite(3, 42)
        self.assertEqual([t["id"] for t in a], [t["id"] for t in b])
        self.assertEqual(json.dumps([t["gold"] for t in a], sort_keys=True),
                         json.dumps([t["gold"] for t in b], sort_keys=True))

    def test_prompts_are_condition_blind(self):
        """No temperature/condition vocabulary may leak into prompts (the
        provider must not be able to infer its condition)."""
        banned = ["temperature", " c0", "c1", "c2", "c3", "condition",
                  "checker", "gold"]
        for task in fixtures_gen.build_suite(2, 42):
            blob = json.dumps(task["messages"]).lower()
            for word in banned:
                self.assertNotIn(word, blob,
                                 f"{task['id']} prompt leaks {word!r}")

    def test_unanswerable_probes_are_truly_absent(self):
        """Closed-world guarantee for the hallucination endpoint: when the
        gold customer is None, no customer name may appear in the prompt."""
        for task in fixtures_gen.build_suite(6, 42):
            if task["class"] != "deterministic-extraction":
                continue
            blob = json.dumps(task["messages"])
            if task["gold"]["customer"] is None:
                for customer in fixtures_gen.CUSTOMERS:
                    self.assertNotIn(customer, blob,
                                     f"{task['id']} fabricatable name present")

    def test_mock_answers_reach_expected_scores(self):
        suite = {t["id"]: t for t in fixtures_gen.build_suite(2, 42)}
        import random
        for task in suite.values():
            rng = random.Random(1)
            good = task["sample_answer"](1.0, rng)
            if task["class"] == "code-generation":
                continue  # requires code execution; covered by integration run
            _, score, _ = CHECKERS[task["class"]](good, task["gold"])
            self.assertGreaterEqual(score, 0.9,
                                    f"{task['id']} high-quality answer scored {score}")


if __name__ == "__main__":
    unittest.main()
