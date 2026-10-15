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
import tempfile
import tomllib
import unittest


ROOT = Path(__file__).resolve().parents[2]
JOBS = ("source", "shared", "desktop", "host", "mobile", "mobile_ffi", "android", "services")
spec = importlib.util.spec_from_file_location("ci", ROOT / "scripts/ci.py")
ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci)


def job_section(workflow, name):
    return re.split(r"\n  [a-z_]+:\n", workflow.split(f"\n  {name}:\n", 1)[1], maxsplit=1)[0]


class CheckResults(unittest.TestCase):
    def results(self, selected):
        selection = ci.outputs(selected)
        return {
            "source": {"result": "success", "outputs": selection},
            **{name: {"result": "success" if selection[name] == "true" else "skipped"}
               for name in JOBS[1:]},
        }

    def summarize(self, results, product=None):
        return subprocess.run(
            [sys.executable, str(ROOT / "scripts/ci.py"), "report", *([product] if product else [])],
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
        for selected in (
            set(), {"desktop"}, {"shared"}, {"host"}, {"mobile"}, {"mobile_ffi"}, {"android"}, {"services"},
        ):
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
        for key in ci.LANES:
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
                    results["services"]["result"] = status
                    passed = self.summarize(results).returncode == 0
                    self.assertEqual(passed, status == "success" or (status == "skipped" and not selected))

    def test_rejects_invalid_selection(self):
        for value in (True, False, "invalid", None):
            with self.subTest(value=value):
                results = self.results(ci.LANES)
                results["source"]["outputs"]["shared"] = value
                self.assertNotEqual(self.summarize(results).returncode, 0)

    def test_products_require_actual_success(self):
        for product, lanes in ci.PRODUCTS.items():
            with self.subTest(product=product):
                results = self.results(ci.LANES)
                unrelated = set(JOBS) - {"source"} - lanes
                for name in unrelated:
                    results[name]["result"] = "failure"
                result = self.summarize(results, product)
                self.assertEqual(result.returncode, 0, result.stderr)
                for name in {"source"} | lanes:
                    for status in ("failure", "cancelled", "skipped", None):
                        with self.subTest(job=name, status=status):
                            changed = copy.deepcopy(results)
                            changed[name]["result"] = status
                            self.assertNotEqual(self.summarize(changed, product).returncode, 0)

    def test_products_reject_partial_selection(self):
        for product, lanes in ci.PRODUCTS.items():
            for missing in lanes:
                with self.subTest(product=product, missing=missing):
                    results = self.results(ci.LANES - {missing})
                    results[missing]["result"] = "success"
                    self.assertNotEqual(self.summarize(results, product).returncode, 0)


class Workflow(unittest.TestCase):
    def test_gates_runtime_and_service_checks(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        source = workflow.split("\n  source:\n", 1)[1].split("\n  services:\n", 1)[0]
        self.assertNotIn("    if:", source.split("    steps:\n", 1)[0])
        self.assertIn("fetch-depth: 0", source)
        self.assertIn("run: python3 scripts/ci.py select", source)
        for lane in ci.LANES:
            section = workflow.split(f"\n  {lane}:\n", 1)[1].split("    steps:\n", 1)[0]
            self.assertIn("needs: source", section)
            self.assertIn(f"if: needs.source.outputs.{lane} == 'true'", section)
        self.assertIn("needs: [source, shared, desktop, host, mobile, mobile_ffi, android, services]", workflow)
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
        rust = workflow.split("\n  shared:\n", 1)[1].split("\n  mobile:\n", 1)[0]
        self.assertIn("run: cargo fmt --all -- --check", rust)
        mobile = workflow.split("\n  mobile:\n", 1)[1].split("\n  mobile_ffi:\n", 1)[0]
        self.assertIn("run: cargo fmt --all -- --check", mobile)

    def test_formats_once_for_selected_native_lanes(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        order = ("shared", "desktop", "host", "mobile")
        for index, lane in enumerate(order):
            section = job_section(workflow, lane)
            formatting = section.split("      - name: Formatting\n", 1)[1].split("      - ", 1)[0]
            self.assertIn("run: cargo fmt --all -- --check", formatting)
            condition = " && ".join(f"needs.source.outputs.{prior} != 'true'" for prior in order[:index])
            if condition:
                self.assertIn(f"if: {condition}", formatting)
            else:
                self.assertNotIn("if:", formatting)
        for choices in itertools.product((False, True), repeat=len(ci.LANES)):
            selected = {lane for lane, included in zip(sorted(ci.LANES), choices) if included}
            count = sum(lane in selected and not selected.intersection(order[:index])
                        for index, lane in enumerate(order))
            self.assertEqual(count, int(bool(selected.intersection(order))))

    def test_product_gates_require_real_jobs(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        for product, lanes in ci.PRODUCTS.items():
            with self.subTest(product=product):
                section = job_section(workflow, f"{product}_ready")
                self.assertIn(f"name: {product.title()} ready", section)
                self.assertIn("needs.source.result == 'success'", section)
                for lane in lanes:
                    self.assertIn(f"needs.{lane}.result == 'success'", section)
                self.assertIn(f"python3 scripts/ci.py report {product}", section)
                self.assertNotIn("continue-on-error", section)

    def test_keeps_host_out_of_desktop(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        shared = workflow.split("\n  shared:\n", 1)[1].split("\n  desktop:\n", 1)[0]
        desktop = workflow.split("\n  desktop:\n", 1)[1].split("\n  host:\n", 1)[0]
        host = workflow.split("\n  host:\n", 1)[1].split("\n  mobile:\n", 1)[0]
        self.assertNotIn("-p sailry-host", shared + desktop)
        self.assertIn("-p sailry-node-runtime", shared)
        self.assertIn("-p sailry-release", shared)
        self.assertIn("cargo check --locked -p sailry-desktop --jobs", desktop)
        self.assertIn("cargo check --locked -p sailry-host --jobs", host)
        self.assertIn("cargo nextest run --locked -p sailry-host", host)

    def test_source_fixtures_follow_product_owners(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        for owner in ci.FIXTURES:
            self.assertIn(f"python3 scripts/ci.py fixtures {owner}", workflow)
        shared = ci.fixture_names(ROOT, "shared")
        self.assertNotIn("test_android_package", shared)
        self.assertNotIn("test_host_install", shared)
        self.assertIn("test_release_versions", shared)
        self.assertIn("test_onnx_runtime", shared)
        self.assertIn("scripts/check-locales.py --product desktop", workflow)
        self.assertIn("scripts/check-locales.py --product mobile", workflow)

    def test_splits_dart_contracts_on_macos(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        mobile = workflow.split("\n  mobile:\n", 1)[1].split("\n  mobile_ffi:\n", 1)[0]
        ffi = workflow.split("\n  mobile_ffi:\n", 1)[1].split("\n  android:\n", 1)[0]
        self.assertNotIn("--test dart", mobile)
        self.assertIn("runs-on: macos-15", ffi)
        self.assertIn("DEVELOPER_DIR: /Applications/Xcode_26.3.app/Contents/Developer", ffi)
        self.assertIn("if: needs.source.outputs.mobile_ffi == 'true'", ffi)
        self.assertIn("mlugg/setup-zig@", ffi)
        self.assertIn("run: brew install ripgrep", ffi)
        self.assertNotIn("apt-get", ffi)
        self.assertIn("python3 scripts/ci.py speech-key aarch64-apple-darwin shared", ffi)
        bridge = "cargo build --locked -p sailry-mobile-bridge --lib --jobs 1"
        contract = (
            "brew install postgresql@17 mysql@8.4\n"
            "          export PATH=\"$(brew --prefix postgresql@17)/bin:"
            "$(brew --prefix mysql@8.4)/bin:$PATH\"\n"
            "          cd tests/mobile-contract\n"
            "          dart pub get --enforce-lockfile\n"
            "          dart analyze\n"
            "          cd ../..\n"
            "          cargo test --locked -p sailry-mobile-bridge --test dart --timings -- "
            "--ignored --skip mcp_peer::stdio_peer --test-threads=4 --nocapture"
        )
        self.assertIn(bridge, ffi)
        self.assertIn(contract, ffi)
        self.assertLess(ffi.index(bridge), ffi.index(contract))

    def test_keys_speech_cache_by_native_identity(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertNotIn("hashFiles('Cargo.lock')", workflow)
        self.assertEqual(workflow.count("key: ${{ steps.speech-key.outputs.key }}"), 4)
        self.assertEqual(
            workflow.count("python3 scripts/ci.py speech-key aarch64-apple-darwin shared"), 4,
        )
        self.assertNotIn("python3 scripts/ci.py speech-key x86_64-unknown-linux-gnu", workflow)


class SpeechCache(unittest.TestCase):
    def fixture(self, root, requirement="=1.2.3", system="1.2.3"):
        (root / "Cargo.toml").write_text(
            "[workspace]\n[workspace.dependencies]\n"
            f'sherpa-onnx = {{ version = "{requirement}", default-features = false }}\n',
            encoding="utf-8",
        )
        (root / "Cargo.lock").write_text(
            "version = 4\n\n"
            '[[package]]\nname = "sherpa-onnx"\nversion = "1.2.3"\n\n'
            f'[[package]]\nname = "sherpa-onnx-sys"\nversion = "{system}"\n',
            encoding="utf-8",
        )

    def test_uses_versions_target_and_link_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            self.assertEqual(
                ci.speech_cache_key(root, "aarch64-apple-darwin", "shared"),
                "speech-native-v1-sherpa-1.2.3-sys-1.2.3-aarch64-apple-darwin-shared",
            )
            before = ci.speech_cache_key(root, "aarch64-apple-darwin", "shared")
            with (root / "Cargo.lock").open("a", encoding="utf-8") as lock:
                lock.write('\n[[package]]\nname = "unrelated"\nversion = "9.9.9"\n')
            self.assertEqual(ci.speech_cache_key(root, "aarch64-apple-darwin", "shared"), before)
            self.fixture(root, system="1.2.4")
            self.assertIn("sys-1.2.4", ci.speech_cache_key(root, "x86_64-unknown-linux-gnu", "shared"))

    def test_requires_exact_workspace_pin(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root, requirement="1.2.3")
            with self.assertRaisesRegex(ValueError, "must remain pinned"):
                ci.speech_cache_key(root, "aarch64-apple-darwin", "shared")


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
