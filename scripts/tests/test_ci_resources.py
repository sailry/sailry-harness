"""Layered checks preserve candidate coverage and fit standard runner resources."""

import json
import os
from pathlib import Path
import subprocess
import sys
import textwrap
import unittest


ROOT = Path(__file__).resolve().parents[2]


class CheckScopes(unittest.TestCase):
    def setUp(self):
        self.workflow = (ROOT / ".github/workflows/ci.yml").read_text()

    def summarize(self, scope, **overrides):
        results = {
            name: {"result": "success" if scope == "candidate" or name == "source" else "skipped"}
            for name in ("source", "rust", "mobile", "android", "ios")
        }
        for name, result in overrides.items():
            results[name]["result"] = result
        summary = self.workflow.split("\n  checks:\n", 1)[1]
        script = textwrap.dedent(summary.split("          python3 - <<'PY'\n", 1)[1].split("          PY", 1)[0])
        return subprocess.run(
            [sys.executable, "-c", script],
            env={**os.environ, "CHECK_SCOPE": scope, "CHECK_RESULTS": json.dumps(results)},
            text=True, capture_output=True,
        )

    def test_defers_native_jobs(self):
        for name in ("rust", "mobile", "android", "ios"):
            job = self.workflow.split(f"\n  {name}:\n", 1)[1].split("\n  ", 1)[0]
            self.assertIn("if: github.event_name == 'workflow_dispatch'", job)
        self.assertIn("pull_request:", self.workflow)
        self.assertIn("workflow_dispatch:", self.workflow)
        group = next(line for line in self.workflow.splitlines() if "group: checks-" in line)
        self.assertIn("'candidate' || 'source'", group)

    def test_identifies_source_only_success(self):
        result = self.summarize("source")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("native validation was not run", result.stdout)
        self.assertNotIn("Candidate validation passed", result.stdout)

    def test_requires_complete_candidate(self):
        result = self.summarize("candidate")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Candidate validation passed", result.stdout)
        for status in ("failure", "cancelled", "skipped"):
            with self.subTest(status=status):
                self.assertNotEqual(self.summarize("candidate", rust=status).returncode, 0)

    def test_rejects_source_and_unexpected_native_failures(self):
        for overrides in ({"source": "failure"}, {"source": "skipped"}, {"rust": "failure"}):
            with self.subTest(overrides=overrides):
                self.assertNotEqual(self.summarize("source", **overrides).returncode, 0)


class RustResources(unittest.TestCase):
    def setUp(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        self.rust = workflow.split("\n  rust:\n", 1)[1].split("\n  mobile:\n", 1)[0]

    def test_caps_compilation(self):
        self.assertIn("CARGO_BUILD_JOBS: '2'", self.rust)
        self.assertIn(
            "BUILD_JOBS: ${{ matrix.kind == 'desktop' && 1 || matrix.platform.jobs }}",
            self.rust,
        )
        commands = [
            line for line in self.rust.splitlines()
            if "cargo " in line and "cargo clean " not in line and "cargo fmt " not in line
        ]
        self.assertEqual(len(commands), 10)
        self.assertTrue(all(
            '--jobs "$BUILD_JOBS"' in command or '--build-jobs "$BUILD_JOBS"' in command
            for command in commands
        ))
        cache_env = next(line for line in self.rust.splitlines() if "env-vars:" in line)
        self.assertNotIn("BUILD_JOBS", cache_env)

    def test_bounds_fixture_parallelism(self):
        matrix = self.rust.split("    runs-on:", 1)[0]
        arm, intel = matrix.split("          - runner: macos-15-intel", 1)
        self.assertIn("threads: 4", arm)
        self.assertIn("threads: 2", intel)
        self.assertIn("--test-threads=${{ matrix.platform.threads }}", self.rust)
        self.assertIn(
            'cargo nextest run --locked -p sailry-desktop --bin sailry-desktop --build-jobs "$BUILD_JOBS" --profile ci',
            self.rust,
        )

    def test_isolates_desktop_without_retries(self):
        import tomllib

        config = tomllib.loads((ROOT / ".config/nextest.toml").read_text())
        profile = config["profile"]["ci"]
        self.assertEqual(profile["test-threads"], 2)
        self.assertEqual(profile["retries"], 0)
        self.assertFalse(profile["fail-fast"])
        self.assertNotIn("default-filter", profile)
        self.assertIn("tool: cargo-nextest@0.9.143", self.rust)

    def test_avoids_duplicate_compiled_caches(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertIn("cache-targets: ${{ matrix.kind != 'lint' }}", self.rust)
        self.assertIn("'-lint-sources'", self.rust)
        self.assertIn("shared-key: aarch64-apple-darwin-rust", workflow)
        self.assertEqual(workflow.count("SCCACHE_IDLE_TIMEOUT: '0'"), 2)
        self.assertIn("target/nextest/ci/junit.xml", self.rust)

    def test_preserves_database_graph_and_doctests(self):
        self.assertIn("- name: Backend documentation tests", self.rust)
        self.assertIn('--doc --jobs "$BUILD_JOBS"', self.rust)
        command = next(line for line in self.rust.splitlines() if "binary(databases)" in line)
        for package in ("protocol", "link", "client", "node-runtime", "host"):
            self.assertIn(f"-p sailry-{package}", command)
        self.assertIn("--run-ignored only", command)
        self.assertIn("--test-threads=1", command)
        self.assertIn("--profile databases", command)
        self.assertIn("target/nextest/databases/junit.xml", self.rust)

    def test_keeps_lifecycle_after_binary_build(self):
        build = self.rust.index("- name: Desktop and Host binaries")
        lifecycle = self.rust.index("- name: Desktop process lifecycle")
        self.assertLess(build, lifecycle)
        self.assertIn("--test lifecycle", self.rust[lifecycle:])
        self.assertIn("--profile lifecycle", self.rust[lifecycle:])
        self.assertIn("target/nextest/lifecycle/junit.xml", self.rust)


if __name__ == "__main__":
    unittest.main()
