"""Focused tests for opt-in Phase 6 benchmark budget evaluation."""

import importlib.util
import unittest
from pathlib import Path


SPEC = importlib.util.spec_from_file_location(
    "benchmark_phase6", Path(__file__).with_name("benchmark-phase6.py")
)
assert SPEC is not None and SPEC.loader is not None
benchmark = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(benchmark)


class BudgetEvaluationTests(unittest.TestCase):
    def test_no_limits_are_reported_as_unconfigured(self):
        self.assertEqual(
            benchmark.evaluate_budgets({"elapsed_seconds": 1.0}),
            {"status": "not_configured", "checks": {}},
        )

    def test_supplied_limits_pass_or_fail_by_measurement(self):
        result = benchmark.evaluate_budgets(
            {"elapsed_seconds": 1.0, "peak_resident_memory_bytes": 1024}, 1.0, 1000
        )
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["checks"]["elapsed_seconds"]["status"], "passed")
        self.assertEqual(result["checks"]["peak_resident_memory_bytes"]["status"], "failed")

    def test_requested_limit_with_missing_measurement_is_unavailable(self):
        result = benchmark.evaluate_budgets({"peak_resident_memory_bytes": None}, None, 4096)
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["checks"]["peak_resident_memory_bytes"]["status"], "unavailable")

    def test_budget_limits_reject_non_finite_values(self):
        for value in ("nan", "inf", "-inf"):
            with self.subTest(value=value), self.assertRaises(benchmark.argparse.ArgumentTypeError):
                benchmark.positive_float(value)


if __name__ == "__main__":
    unittest.main()
