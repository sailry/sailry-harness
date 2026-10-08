"""CI reports success only when every required job passed."""

import json
import os
from pathlib import Path
import subprocess
import sys
import textwrap
import tomllib
import unittest


ROOT = Path(__file__).resolve().parents[2]
JOBS = ("source", "rust", "mobile", "android", "ios")


class CheckResults(unittest.TestCase):
    def summarize(self, results):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        summary = workflow.split("\n  checks:\n", 1)[1]
        script = textwrap.dedent(
            summary.split("          python3 - <<'PY'\n", 1)[1].split("          PY", 1)[0]
        )
        return subprocess.run(
            [sys.executable, "-c", script],
            env={**os.environ, "CHECK_RESULTS": json.dumps(results)},
            text=True,
            capture_output=True,
        )

    def test_requires_all_jobs(self):
        results = {name: {"result": "success"} for name in JOBS}
        result = self.summarize(results)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Checks passed", result.stdout)
        for name in JOBS:
            for status in ("failure", "cancelled", "skipped"):
                with self.subTest(job=name, status=status):
                    changed = {**results, name: {"result": status}}
                    self.assertNotEqual(self.summarize(changed).returncode, 0)

    def test_rejects_missing_results(self):
        self.assertNotEqual(self.summarize({}).returncode, 0)


class Profiles(unittest.TestCase):
    def test_does_not_retry_or_hide_tests(self):
        config = tomllib.loads((ROOT / ".config/nextest.toml").read_text())
        for name in ("ci", "databases", "lifecycle"):
            with self.subTest(profile=name):
                profile = config["profile"][name]
                self.assertEqual(profile["retries"], 0)
                self.assertFalse(profile["fail-fast"])
                self.assertNotIn("default-filter", profile)


if __name__ == "__main__":
    unittest.main()
