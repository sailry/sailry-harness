"""CI requires selected checks and preserves lint and test coverage."""

import copy
import importlib.util
import itertools
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib
import unittest


ROOT = Path(__file__).resolve().parents[2]
JOBS = ("source", "rust", "mobile", "android")
spec = importlib.util.spec_from_file_location("ci", ROOT / "scripts/ci.py")
ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci)


class CheckResults(unittest.TestCase):
    def results(self, selected):
        selection = ci.outputs(selected)
        selection["services_result"] = "success" if "services" in selected else "skipped"
        return {
            "source": {"result": "success", "outputs": selection},
            **{name: {"result": "success" if selection[name] == "true" else "skipped"}
               for name in JOBS[1:]},
        }

    def summarize(self, results):
        return subprocess.run(
            [sys.executable, str(ROOT / "scripts/ci.py"), "report"],
            env={**os.environ, "CHECK_RESULTS": json.dumps(results)},
            text=True,
            capture_output=True,
        )

    def test_requires_all_jobs(self):
        results = self.results(ci.LANES)
        result = self.summarize(results)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Checks passed", result.stdout)
        for name in JOBS:
            for status in ("failure", "cancelled", "skipped"):
                with self.subTest(job=name, status=status):
                    changed = copy.deepcopy(results)
                    changed[name]["result"] = status
                    self.assertNotEqual(self.summarize(changed).returncode, 0)

    def test_accepts_only_intentional_skips(self):
        for selected in (set(), {"desktop"}, {"backend"}, {"mobile"}, {"android"}, {"services"}):
            with self.subTest(selected=selected):
                results = self.results(selected)
                result = self.summarize(results)
                self.assertEqual(result.returncode, 0, result.stderr)
                for name in JOBS:
                    changed = copy.deepcopy(results)
                    changed[name]["result"] = "skipped"
                    if results[name]["result"] == "success":
                        self.assertNotEqual(self.summarize(changed).returncode, 0)
                    for status in ("failure", "cancelled", "unknown", None):
                        changed[name]["result"] = status
                        self.assertNotEqual(self.summarize(changed).returncode, 0)

    def test_rejects_missing_results(self):
        self.assertNotEqual(self.summarize({}).returncode, 0)
        for name in JOBS:
            with self.subTest(job=name):
                results = self.results(ci.LANES)
                del results[name]
                self.assertNotEqual(self.summarize(results).returncode, 0)

    def test_rejects_missing_selection(self):
        for key in (*ci.LANES, "rust", "rust_matrix", "services_result"):
            with self.subTest(key=key):
                results = self.results({"desktop"})
                del results["source"]["outputs"][key]
                self.assertNotEqual(self.summarize(results).returncode, 0)
        for invalid in (None, [], "false", {"desktop": True}):
            results = self.results({"desktop"})
            results["source"]["outputs"] = invalid
            self.assertNotEqual(self.summarize(results).returncode, 0)

    def test_requires_service_result(self):
        for selected in (set(), {"services"}):
            for status in ("success", "skipped", "failure", "cancelled", "unknown", None):
                with self.subTest(selected=selected, status=status):
                    results = self.results(selected)
                    results["source"]["outputs"]["services_result"] = status
                    passed = self.summarize(results).returncode == 0
                    self.assertEqual(passed, status == "success" or (status == "skipped" and not selected))

    def test_rejects_inconsistent_matrix(self):
        for key, value in (("rust", "false"), ("rust_matrix", '{"kind":["backend"]}'),
                           ("rust_matrix", '{"kind":[]}'), ("rust_matrix", "invalid")):
            with self.subTest(key=key, value=value):
                results = self.results(ci.LANES)
                results["source"]["outputs"][key] = value
                self.assertNotEqual(self.summarize(results).returncode, 0)


class Workflow(unittest.TestCase):
    def test_gates_runtime_and_service_checks(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        source = workflow.split("\n  source:\n", 1)[1].split("\n  rust:\n", 1)[0]
        self.assertNotIn("    if:", source.split("    steps:\n", 1)[0])
        self.assertIn("fetch-depth: 0", source)
        self.assertIn("run: python3 scripts/ci.py select", source)
        self.assertIn("if: steps.scope.outputs.services == 'true'", source)
        self.assertIn("services_result: ${{ steps.pairing.outcome }}", source)
        self.assertIn("- name: Pairing service\n        id: pairing", source)
        for lane in ("rust", "mobile", "android"):
            section = workflow.split(f"\n  {lane}:\n", 1)[1].split("    steps:\n", 1)[0]
            self.assertIn("needs: source", section)
            self.assertIn(f"if: needs.source.outputs.{lane} == 'true'", section)
        self.assertIn("matrix: ${{ fromJSON(needs.source.outputs.rust_matrix) }}", workflow)
        self.assertIn("needs: [source, rust, mobile, android]", workflow)
        self.assertIn("run: python3 scripts/ci.py report", workflow)

    def test_lints_every_workspace_member(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        commands = re.findall(r"run: (cargo clippy[^\n]+)", workflow)
        workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["members"]
        packages = {tomllib.loads((ROOT / member / "Cargo.toml").read_text())["package"]["name"]
                    for member in workspace}
        linted = set()
        for command in commands:
            self.assertIn("--all-targets", command)
            self.assertIn("-- -D warnings", command)
            self.assertNotIn("--workspace", command)
            group = set(re.findall(r"-p (\S+)", command))
            self.assertTrue(linted.isdisjoint(group))
            linted.update(group)
        self.assertEqual(linted, packages)

    def test_compiles_tests_once_and_keeps_timings(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertNotIn("--no-run", workflow)
        for command in re.findall(r"run: (cargo nextest run[^\n]+--profile ci[^\n]*)", workflow):
            self.assertIn("--timings", command)
        self.assertIn("--test dart --timings --", workflow)
        rust = workflow.split("\n  rust:\n", 1)[1].split("\n  mobile:\n", 1)[0]
        self.assertIn("run: cargo fmt --all -- --check", rust)
        mobile = workflow.split("\n  mobile:\n", 1)[1].split("\n  android:\n", 1)[0]
        self.assertIn("run: cargo fmt --all -- --check", mobile)

    def test_formats_once_for_selected_native_lanes(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        rust = workflow.split("\n  rust:\n", 1)[1].split("\n  mobile:\n", 1)[0]
        mobile = workflow.split("\n  mobile:\n", 1)[1].split("\n  android:\n", 1)[0]
        self.assertIn(
            "- name: Formatting\n"
            "        if: matrix.kind == 'backend' || needs.source.outputs.backend != 'true'\n"
            "        run: cargo fmt --all -- --check", rust,
        )
        self.assertIn(
            "- name: Formatting\n"
            "        if: needs.source.outputs.rust != 'true'\n"
            "        run: cargo fmt --all -- --check", mobile,
        )
        lanes = sorted(ci.LANES)
        for choices in itertools.product((False, True), repeat=len(lanes)):
            selected = {lane for lane, included in zip(lanes, choices) if included}
            with self.subTest(selected=selected):
                output = ci.outputs(selected)
                kinds = json.loads(output["rust_matrix"])["kind"] if output["rust"] == "true" else []
                count = sum(kind == "backend" or output["backend"] != "true" for kind in kinds)
                count += output["mobile"] == "true" and output["rust"] != "true"
                self.assertEqual(count, int(bool(selected & {"backend", "desktop", "mobile"})))


class Profiles(unittest.TestCase):
    def test_does_not_retry_or_hide_tests(self):
        config = tomllib.loads((ROOT / ".config/nextest.toml").read_text())
        for name in ("ci", "databases", "lifecycle"):
            with self.subTest(profile=name):
                profile = config["profile"][name]
                self.assertEqual(profile["retries"], 0)
                self.assertFalse(profile["fail-fast"])
                self.assertNotIn("default-filter", profile)
                self.assertEqual(
                    profile["slow-timeout"], {"period": "60s", "terminate-after": 5},
                )


if __name__ == "__main__":
    unittest.main()
