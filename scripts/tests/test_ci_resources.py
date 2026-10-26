"""CI requires selected checks and preserves lint and test coverage."""

import copy
import importlib.util
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import tomllib
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch


ROOT = Path(__file__).resolve().parents[2]
JOBS = ("source", "lint", "shared", "desktop", "host", "mobile", "android", "services")
spec = importlib.util.spec_from_file_location("ci", ROOT / "scripts/ci.py")
ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci)


def workflow_source():
    workflow = (ROOT / ".github/workflows/ci.yml").read_text()

    def expand(match):
        commands = ci.check_commands(match.group(1), "1")
        return "\n".join(shlex.join(["python3" if value == sys.executable else value
                                       for value in command["argv"]]) for command in commands)

    return re.sub(r"python3 (?:(?:\.\./){2})?scripts/ci\.py check ([a-z-]+)", expand, workflow)


def job_section(workflow, name):
    return re.split(r"\n  [a-z_]+:\n", workflow.split(f"\n  {name}:\n", 1)[1], maxsplit=1)[0]


class CheckResults(unittest.TestCase):
    def results(self, selected):
        selection = ci.outputs(selected)
        return {
            "source": {"result": "success", "outputs": selection},
            **{name: {"result": "success" if lanes & selected else "skipped"}
               for name, lanes in ci.CHECK_JOBS.items()},
            "lint": {"result": "success" if ci.lint_packages(selected) else "skipped"},
        }

    def summarize(self, results, product=None):
        try:
            ci.require_results(results, product)
        except ValueError as error:
            return SimpleNamespace(returncode=1, stderr=str(error))
        return SimpleNamespace(returncode=0, stderr="")

    def test_report_cli(self):
        for product in (None, *ci.PRODUCTS):
            for results in (self.results(ci.LANES), {}):
                with self.subTest(product=product, results=bool(results)):
                    result = subprocess.run(
                        [sys.executable, str(ROOT / "scripts/ci.py"), "report",
                         *([product] if product else [])],
                        env={**os.environ, "CHECK_RESULTS": json.dumps(results)},
                        text=True, capture_output=True,
                    )
                    self.assertEqual(result.returncode == 0, bool(results), result.stderr)
                    if results:
                        self.assertIn(f"{product.title()} ready" if product else "Checks passed",
                                      result.stdout)
                    else:
                        self.assertIn("Missing check results", result.stderr)

    def test_requires_all_jobs(self):
        results = self.results(ci.LANES)
        result = self.summarize(results)
        self.assertEqual(result.returncode, 0, result.stderr)
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
                needed = {"source", "lint"} | {name for name, selected in ci.CHECK_JOBS.items() if lanes & selected}
                for name in set(JOBS) - needed:
                    results[name]["result"] = "failure"
                result = self.summarize(results, product)
                self.assertEqual(result.returncode, 0, result.stderr)
                for name in needed:
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
                    for name, selected in ci.CHECK_JOBS.items():
                        if missing in selected:
                            results[name]["result"] = "success"
                    self.assertNotEqual(self.summarize(results, product).returncode, 0)


class Workflow(unittest.TestCase):
    def test_gates_runtime_and_service_checks(self):
        workflow = workflow_source()
        source = workflow.split("\n  source:\n", 1)[1].split("\n  services:\n", 1)[0]
        self.assertNotIn("    if:", source.split("    steps:\n", 1)[0])
        self.assertIn("fetch-depth: 0", source)
        self.assertIn("run: python3 scripts/ci.py select", source)
        for name, lanes in ci.CHECK_JOBS.items():
            section = job_section(workflow, name).split("    steps:\n", 1)[0]
            self.assertIn("needs: source", section)
            for lane in lanes:
                self.assertIn(f"needs.source.outputs.{lane} == 'true'", section)
        self.assertIn("needs: [source, lint, shared, desktop, host, mobile, android, services]", workflow)
        self.assertIn("run: python3 scripts/ci.py report", workflow)

    def test_lints_every_workspace_member(self):
        workflow = workflow_source()
        workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["members"]
        packages = {tomllib.loads((ROOT / member / "Cargo.toml").read_text())["package"]["name"]
                    for member in workspace}
        self.assertEqual(set(ci.lint_packages(ci.LANES)), packages)
        lint = job_section(workflow, "lint")
        self.assertIn("run: python3 scripts/ci-run.py -- python3 scripts/ci.py lint", lint)
        self.assertIn("CHECK_SCOPE: ${{ toJSON(needs.source.outputs) }}", lint)
        self.assertIn("needs: source", lint)
        self.assertNotIn("cargo clippy", workflow)
        self.assertEqual(workflow.count("scripts/ci-run.py -- python3 scripts/ci.py lint"), 1)
        for name in ci.CHECK_JOBS:
            self.assertNotIn("needs: [source, lint]", job_section(workflow, name))

    def test_runs_shared_package_tests(self):
        workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["members"]
        packages = {tomllib.loads((ROOT / member / "Cargo.toml").read_text())["package"]["name"]
                    for member in workspace}
        products = {package for owner, names in ci.LINT_PACKAGES.items()
                    if owner != "shared" for package in names}
        command = ci.check_commands("backend")[0]["argv"]
        selected = {command[index + 1] for index, value in enumerate(command) if value == "-p"}
        self.assertEqual(selected, packages - products)
        self.assertNotIn("--run-ignored", command)

    def test_compiles_tests_once_and_keeps_timings(self):
        workflow = workflow_source()
        self.assertNotIn("--no-run", workflow)
        for command in re.findall(r"run: (?:python3 scripts/ci-run.py -- )?(cargo nextest run[^\n]+--profile ci[^\n]*)", workflow):
            self.assertIn("--timings", command)
        self.assertIn("--test dart --timings --", workflow)
        self.assertIn("run: cargo fmt --all -- --check", job_section(workflow, "lint"))

    def test_runs_plugin_contracts_in_shared(self):
        workflow = workflow_source()
        shared = job_section(workflow, "shared")
        self.assertIn("node-version: '22.22.1'", shared)
        self.assertIn("--example plugin-test-host --profile test", shared)
        self.assertIn("scripts/test-plugins.py --runtime-host", shared)
        self.assertLess(shared.index("--example plugin-test-host"), shared.index("scripts/test-plugins.py"))
        self.assertNotIn("scripts/test-plugins.py", job_section(workflow, "source"))

    def test_formats_once_for_selected_native_lanes(self):
        workflow = workflow_source()
        lint = job_section(workflow, "lint")
        header = lint.split("    steps:\n", 1)[0]
        self.assertEqual(workflow.count("run: cargo fmt --all -- --check"), 1)
        self.assertIn("run: cargo fmt --all -- --check", lint)
        for lane in ("shared", "desktop", "host", "mobile"):
            self.assertIn(f"needs.source.outputs.{lane} == 'true'", header)
        for lane in ("shared", "desktop", "host", "mobile", "android"):
            self.assertNotIn("name: Formatting", job_section(workflow, lane))

    def test_product_gates_require_real_jobs(self):
        workflow = workflow_source()
        for product, lanes in ci.PRODUCTS.items():
            with self.subTest(product=product):
                section = job_section(workflow, f"{product}_ready")
                self.assertIn(f"name: {product.title()} ready", section)
                self.assertIn("needs.source.result == 'success'", section)
                self.assertIn("needs.lint.result == 'success'", section)
                for lane in lanes:
                    self.assertIn(f"needs.source.outputs.{lane} == 'true'", section)
                for name, selected in ci.CHECK_JOBS.items():
                    if lanes & selected:
                        self.assertIn(f"needs.{name}.result == 'success'", section)
                self.assertIn(f"python3 scripts/ci.py report {product}", section)
                self.assertNotIn("continue-on-error", section)

    def test_merges_host_lifecycle_into_shared(self):
        workflow = workflow_source()
        shared = job_section(workflow, "shared")
        desktop = job_section(workflow, "desktop")
        host = job_section(workflow, "host")
        self.assertNotIn("-p sailry-host", desktop)
        self.assertIn("-p sailry-node-runtime", shared)
        self.assertIn("-p sailry-release", shared)
        self.assertIn("cargo check --locked -p sailry-desktop --jobs", desktop)
        for section in (host, shared):
            self.assertIn("cargo check --locked -p sailry-host --jobs", section)
        self.assertIn("runs-on: ubuntu-24.04", host)
        self.assertIn("bubblewrap libx11-dev libxi-dev libxtst-dev", host)
        self.assertIn("cargo nextest run --locked -p sailry-host --timings", host)
        self.assertIn("cargo nextest run --locked -p sailry-host --test lifecycle --timings", shared)
        for title, lane in (("Shared unit and local/remote integration", "shared"),
                            ("Isolated database engines", "shared"), ("Host macOS lifecycle", "host"),
                            ("Host without test features", "host")):
            step = shared.split(f"      - name: {title}\n", 1)[1].split("      - ", 1)[0]
            self.assertIn(f"if: needs.source.outputs.{lane} == 'true'", step)
        self.assertIn("bash scripts/check-mobile-boundary.sh aarch64-apple-ios sailry-mobile-bridge --dependencies-only", workflow)
        self.assertIn("bash scripts/check-mobile-android.sh --apk", workflow)

    def test_database_reuses_shared_package_graph(self):
        workflow = workflow_source()
        shared = job_section(workflow, "shared")
        commands = [line for line in shared.splitlines() if "cargo nextest run" in line and "-p sailry-node-runtime" in line]
        self.assertEqual(len(commands), 2)
        packages = [re.findall(r"-p ([\w-]+)", command) for command in commands]
        self.assertEqual(packages[0], packages[1])
        self.assertIn("sailry-release", packages[1])

    def test_source_fixtures_follow_product_owners(self):
        workflow = workflow_source()
        for owner in ci.FIXTURES:
            self.assertIn(f"python3 scripts/ci.py fixtures {owner}", workflow)
        shared = ci.fixture_names(ROOT, "shared")
        self.assertNotIn("test_android_package", shared)
        self.assertNotIn("test_host_install", shared)
        self.assertIn("test_release_versions", shared)
        self.assertIn("test_onnx_runtime", shared)
        self.assertIn("scripts/check-locales.py --product desktop", workflow)
        self.assertIn("scripts/check-locales.py --product mobile", workflow)

    def test_runs_selected_mobile_phases(self):
        workflow = workflow_source()
        mobile = job_section(workflow, "mobile")
        self.assertIn("flutter analyze --no-pub", mobile)
        self.assertIn("flutter test --no-pub", mobile)
        self.assertIn("flutter build ios --release --no-codesign", mobile)
        self.assertIn("runs-on: macos-15", mobile)
        self.assertIn("--test dart --timings", mobile)
        self.assertIn("--skip mcp_peer::stdio_peer --test-threads=4", mobile)
        for title, lane in (("Dart FFI contract fixtures", "mobile_ffi"),
                            ("Mobile source fixtures", "mobile"),
                            ("Flutter analysis and logic tests", "mobile"),
                            ("iOS Rust dependency boundary", "mobile"),
                            ("Unsigned iOS application", "mobile")):
            step = mobile.split(f"      - name: {title}\n", 1)[1].split("      - ", 1)[0]
            self.assertIn(f"needs.source.outputs.{lane} == 'true'", step)
        script = (ROOT / "scripts/check-mobile-contract.sh").read_text()
        self.assertIn("scripts/ci.py check mobile-contract", script)
        self.assertNotIn("cargo test", script)
        android = job_section(workflow, "android")
        self.assertIn("uses: ./.github/actions/rust-cache", android)
        self.assertLess(android.index("flutter build apk"), android.index("check-mobile-android.sh"))

    def test_shares_native_cache_and_limits_writes(self):
        workflow = workflow_source()
        action = (ROOT / ".github/actions/native-cache/action.yml").read_text()
        for path in ("target/sherpa-onnx-prebuilt", "target/llama-prebuilt"):
            self.assertIn(path, action)
        self.assertIn('native-key "$PACKAGE_TARGET" shared', action)
        for name in ("lint", "shared", "desktop", "mobile"):
            section = job_section(workflow, name)
            self.assertIn("uses: ./.github/actions/native-cache", section)
            self.assertIn("target: aarch64-apple-darwin", section)
            save = section.split("      - name: Save native libraries\n", 1)[1].split("      - ", 1)[0]
            self.assertIn("github.ref == 'refs/heads/main'", save)
            self.assertIn("steps.native-cache.outputs.hit != 'true'", save)
            self.assertNotIn("always()", save)
        for name in ("release", "release-host"):
            release = (ROOT / f".github/workflows/{name}.yml").read_text()
            self.assertIn("uses: ./.workflow-tools/.github/actions/native-cache", release)
            self.assertNotIn("Save speech native libraries", release)
            save = release.split("      - name: Save native libraries\n", 1)[1].split("      - ", 1)[0]
            self.assertIn("github.ref == 'refs/heads/main'", save)
            self.assertNotIn("always()", save)

    def test_omits_retired_source_cleanup(self):
        workflow = workflow_source()
        self.assertNotIn("prune-source-cache", workflow)
        self.assertNotIn("\n  source_cache:", workflow)
        self.assertFalse((ROOT / "scripts/prune-source-cache.py").exists())

    def test_always_builds_with_compiler_caching(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        build = job_section(workflow, "build")
        compilation = build.split("      - name: Build native Desktop\n", 1)[1].split("      - ", 1)[0]
        self.assertNotIn("if:", compilation)
        self.assertIn("cargo build --locked --release --timings", compilation)
        for legacy in ("reuse_build_run", "reusable-build", "key: packages-", "target/release\n"):
            self.assertNotIn(legacy, workflow)
        self.assertIn("uses: ./.workflow-tools/.github/actions/rust-cache", build)

    def test_reuses_sources_without_extracted_duplicates(self):
        action = (ROOT / ".github/actions/rust-cache/action.yml").read_text()
        workflows = [(ROOT / f".github/workflows/{name}.yml").read_text()
                     for name in ("ci", "release", "release-host")]
        self.assertIn("path: .cargo-sources", action)
        self.assertIn("source-cache restore", action)
        self.assertNotIn("~/.cargo/registry/src", action)
        self.assertNotIn("~/.cargo/git/checkouts", action)
        self.assertIn("enableCrossOsArchive: true", action)
        identity = action.split("    - name: Dependency source identity\n", 1)[1].split("    - name:", 1)[0]
        self.assertIn("continue-on-error: true", identity)
        self.assertIn('source-key "$GITHUB_WORKSPACE/Cargo.lock"', identity)
        self.assertIn('"$GITHUB_ACTION_PATH/../../../scripts/ci.py"', identity)
        self.assertNotIn("hashFiles('Cargo.lock')", action)
        sources = action.split("    - name: Compressed dependency sources\n", 1)[1].split("    - name:", 1)[0]
        self.assertIn("if: steps.source-key.outcome == 'success'", sources)
        self.assertIn("continue-on-error: true", sources)
        self.assertIn("key: ${{ steps.source-key.outputs.key }}", sources)
        self.assertIn("restore-keys: cargo-sources-v1-", sources)
        self.assertNotIn("runner.os", action)
        self.assertIn("version: v0.18.0", action)
        compiler = action.split("    - name: Compiler cache\n", 1)[1].split("    - name:", 1)[0]
        self.assertIn("continue-on-error: true", compiler)
        fallback = action.split("    - name: Fall back to uncached compilation\n", 1)[1].split("    - name:", 1)[0]
        self.assertIn("if: steps.compiler.outcome != 'success'", fallback)
        self.assertIn("echo 'RUSTC_WRAPPER='", fallback)
        self.assertIn("'refs/heads/main' && 'READ_WRITE' || 'READ_ONLY'", action)
        self.assertNotIn("SCCACHE_GHA_CACHE_TO", action)
        self.assertNotIn("SCCACHE_CACHE_SIZE", action)
        self.assertEqual(workflows[0].count("name: Save compressed dependency sources"), 1)
        self.assertIn("run: cargo fetch --locked", workflows[0])
        self.assertIn("steps.workspace-sources.outcome == 'success'", workflows[0])
        fetch = workflows[0].split("      - name: Fetch locked workspace sources\n", 1)[1].split("      - name:", 1)[0]
        self.assertIn("steps.rust-cache.outputs.source-key != ''", fetch)
        self.assertIn("continue-on-error: true", fetch)
        for workflow in workflows:
            self.assertIn("/.github/actions/rust-cache", workflow)
            self.assertNotIn("Swatinem/rust-cache", workflow)
            self.assertNotIn("target/release\n", workflow)
            self.assertNotIn("target/host-cross\n", workflow)
            self.assertIn("scripts/ci.py cache-usage", workflow)
        for workflow in workflows[1:]:
            self.assertNotIn("Save compilation before signing", workflow)
            self.assertIn("python3 .workflow-tools/scripts/ci-run.py --parsers -- cargo build --locked --release --timings", workflow)


class SourceCacheIdentity(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.lockfile = self.root / "Cargo.lock"
        self.original = (ROOT / "Cargo.lock").read_text()

    def key(self, contents):
        self.lockfile.write_text(contents)
        return ci.source_cache_key(self.lockfile)

    def package(self, values):
        return "[[package]]\n" + "".join(f"{name} = {json.dumps(value)}\n" for name, value in values.items())

    def test_ignores_workspace_version_changes(self):
        header, *packages = self.original.split("[[package]]")
        changed = 0
        for index, contents in enumerate(packages):
            package = tomllib.loads("[[package]]" + contents)["package"][0]
            if "source" not in package:
                packages[index] = re.sub(r'(?m)^version = "[^"]+"$', 'version = "99.0.0"', contents, count=1)
                changed += 1
        self.assertGreater(changed, 0)
        updated = header + "".join("[[package]]" + contents for contents in packages)
        self.assertEqual(self.key(updated), self.key(self.original))

    def test_tracks_external_identity(self):
        package = {"name": "example", "version": "1.0.0",
                   "source": "registry+https://registry.example/index", "checksum": "a" * 64}
        original = self.key(self.package(package))
        for name, value in (("name", "replacement"), ("version", "2.0.0"),
                            ("source", "registry+https://other.example/index"), ("checksum", "b" * 64)):
            with self.subTest(field=name):
                self.assertNotEqual(self.key(self.package({**package, name: value})), original)

    def test_ignores_package_order_and_format(self):
        header, *packages = self.original.split("[[package]]")
        reordered = header + "".join("[[package]]" + contents for contents in reversed(packages))
        self.assertEqual(self.key("# Same external sources\n" + reordered), self.key(self.original))

    def test_tracks_git_sources_without_checksums(self):
        package = {"name": "example", "version": "1.0.0", "source": "git+https://git.example/repo#" + "a" * 40}
        original = self.key(self.package(package))
        package["source"] = "git+https://git.example/repo#" + "b" * 40
        self.assertNotEqual(self.key(self.package(package)), original)

    def test_reads_product_lock_from_each_tool_location(self):
        action = (ROOT / ".github/actions/rust-cache/action.yml").read_text()
        identity = action.split("    - name: Dependency source identity\n", 1)[1].split("    - name:", 1)[0]
        command = identity.split("      run: ", 1)[1].strip()
        other = self.package({"name": "other", "version": "1.0.0", "source": "git+https://git.example/other#123"})
        expected = self.key(self.original)
        self.assertNotEqual(self.key(other), expected)
        for separate in (False, True):
            with self.subTest(separate_tooling=separate):
                product = self.root / str(separate) / "product checkout"
                product.mkdir(parents=True)
                (product / "Cargo.lock").write_text(self.original)
                tooling = product / ".workflow-tools" if separate else product
                script = tooling / "scripts/ci.py"
                script.parent.mkdir(parents=True)
                shutil.copyfile(ROOT / "scripts/ci.py", script)
                if separate:
                    (tooling / "Cargo.lock").write_text(other)
                action_path = tooling / ".github/actions/rust-cache"
                action_path.mkdir(parents=True)
                working = product / "different working directory"
                working.mkdir()
                (working / "Cargo.lock").write_text(other)
                output = product / "output"
                result = subprocess.run(
                    ["bash", "--noprofile", "--norc", "-e", "-c", command],
                    cwd=working, capture_output=True, text=True,
                    env={**os.environ, "GITHUB_WORKSPACE": str(product),
                         "GITHUB_ACTION_PATH": str(action_path), "GITHUB_OUTPUT": str(output)},
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, expected + "\n")
                self.assertEqual(output.read_text(), f"key={expected}\n")


class CacheUsage(unittest.TestCase):
    def test_rebuilds_readonly_stage_without_changing_sources(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            workspace = root / "workspace"
            workspace.mkdir()
            home = root / ".cargo"
            relative = Path("git/db/example/objects/pack/data.pack")
            source = home / relative
            source.parent.mkdir(parents=True)
            source.write_bytes(b"current pack")
            source.chmod(0o444)
            sentinel = home / "registry/src/keep.rs"
            sentinel.parent.mkdir(parents=True)
            sentinel.write_bytes(b"unpacked source")
            ci.cache_sources("save", workspace, home)
            archive = workspace / ".cargo-sources"
            stale = archive / "git/db/retired/objects/pack/old.pack"
            stale.parent.mkdir(parents=True)
            stale.write_bytes(b"obsolete pack")
            stale.chmod(0o444)
            source_mode = source.stat().st_mode
            ci.cache_sources("save", workspace, home)
            self.assertEqual((archive / relative).read_bytes(), b"current pack")
            self.assertFalse((archive / "git/db/retired").exists())
            self.assertEqual(source.read_bytes(), b"current pack")
            self.assertEqual(source.stat().st_mode, source_mode)
            self.assertEqual(sentinel.read_bytes(), b"unpacked source")

    def test_preserves_cargo_home_when_it_is_stage(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory)
            home = workspace / ".cargo-sources"
            source = home / "git/db/example/objects/pack/data.pack"
            source.parent.mkdir(parents=True)
            source.write_bytes(b"original pack")
            with self.assertRaises(ValueError):
                ci.cache_sources("save", workspace, home)
            self.assertEqual(source.read_bytes(), b"original pack")

    def test_restores_sources_across_runner_homes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first = root / "macos/.cargo"
            workspace = root / "workspace"
            workspace.mkdir()
            for relative in ("registry/index/crates/.cache/ex/am/example",
                             "registry/index/crates/config.json", "registry/cache/crates/example-1.0.crate",
                             "git/db/example/objects/pack/data.pack", "registry/src/example/src.rs",
                             "git/checkouts/example/src.rs"):
                path = first / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"fixture")
            metadata = b'\x03\x02\0\0\0etag: "fixture"\0' + b'{"name":"example","vers":"1.0.0"}\0'
            (first / "registry/index/crates/.cache/ex/am/example").write_bytes(metadata)
            ci.cache_sources("save", workspace, first)
            self.assertFalse((workspace / ".cargo-sources/registry/src").exists())
            self.assertFalse((workspace / ".cargo-sources/git/checkouts").exists())
            second = root / "linux/.cargo"
            sentinel = second / "registry/src/keep.rs"
            sentinel.parent.mkdir(parents=True)
            sentinel.write_bytes(b"unpacked source")
            ci.cache_sources("restore", workspace, second)
            self.assertEqual(sentinel.read_bytes(), b"unpacked source")
            self.assertEqual((second / "registry/index/crates/.cache/ex/am/example").read_bytes(), metadata)
            self.assertEqual((second / "registry/index/crates/config.json").read_bytes(), b"fixture")
            self.assertEqual((second / "registry/cache/crates/example-1.0.crate").read_bytes(), b"fixture")
            self.assertEqual((second / "git/db/example/objects/pack/data.pack").read_bytes(), b"fixture")

    def test_reports_usage_without_mutation(self):
        usage = {"active_caches_size_in_bytes": 10 * 2**30, "active_caches_count": 6966}
        with patch.dict(os.environ, {"GH_REPO": "fixture/product", "GITHUB_STEP_SUMMARY": ""}), \
                patch.object(ci.subprocess, "run", return_value=SimpleNamespace(stdout=json.dumps(usage))) as run, \
                patch("builtins.print") as output:
            ci.cache_usage()
        run.assert_called_once_with(
            ["gh", "api", "repos/fixture/product/actions/cache/usage"],
            check=True, text=True, capture_output=True,
        )
        output.assert_called_once_with("Repository cache: 10.000 GiB (6966 entries)")


class PhaseProgress(unittest.TestCase):
    def module(self):
        spec = importlib.util.spec_from_file_location("ci_run", ROOT / "scripts/ci-run.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def test_preserves_command_result_and_arguments(self):
        module = self.module()
        command = [sys.executable, "-c", "import sys; sys.exit(23)"]
        with patch.dict(os.environ, {"GITHUB_STEP_SUMMARY": ""}), patch("builtins.print"):
            self.assertEqual(module.run(command), 23)

    def test_observes_live_phase_without_restarting_it(self):
        module = self.module()
        process = Mock()
        process.wait.side_effect = [subprocess.TimeoutExpired(["cargo"], 60), 0]
        process.__enter__ = Mock(return_value=process)
        process.__exit__ = Mock(return_value=False)
        with patch.object(module.subprocess, "Popen", return_value=process) as spawn, \
                patch.object(module, "progress") as progress, \
                patch.dict(os.environ, {"GITHUB_STEP_SUMMARY": ""}), patch("builtins.print"):
            self.assertEqual(module.run(["cargo", "build", "--locked"]), 0)
        spawn.assert_called_once_with(["cargo", "build", "--locked"], env=None)
        progress.assert_called_once()


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

    def test_native_key_tracks_llama_lock(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            lock = root / "scripts/package/llama-runtime.lock.json"
            lock.parent.mkdir(parents=True)
            lock.write_text('{"revision":"first"}')
            first = ci.native_cache_key(root, "aarch64-apple-darwin", "shared")
            self.assertNotEqual(first, ci.native_cache_key(root, "x86_64-apple-darwin", "shared"))
            self.assertNotEqual(first, ci.native_cache_key(root, "aarch64-apple-darwin", "static"))
            lock.write_text('{"revision":"second"}')
            self.assertNotEqual(first, ci.native_cache_key(root, "aarch64-apple-darwin", "shared"))

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
